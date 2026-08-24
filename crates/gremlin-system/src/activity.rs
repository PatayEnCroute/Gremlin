//! Échantillonnage passif de l'inactivité utilisateur.

use std::thread::{self, JoinHandle};
use std::time::Duration;

use crossbeam_channel::{bounded, Receiver, RecvTimeoutError, Sender, TrySendError};

use crate::error::SystemError;
use crate::platform::activity::{default_backend, IdleBackend};

const EVENT_CHANNEL_CAPACITY: usize = 4;
const CONTROL_CHANNEL_CAPACITY: usize = 1;
const DEFAULT_SAMPLE_INTERVAL: Duration = Duration::from_secs(1);

/// Mesure ponctuelle de l'inactivité globale de la session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActivitySample {
    idle_for: Duration,
}

impl ActivitySample {
    /// Durée écoulée depuis la dernière interaction clavier ou souris.
    #[must_use]
    pub const fn idle_for(self) -> Duration {
        self.idle_for
    }
}

/// Événement produit par le thread de surveillance d'activité.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActivityEvent {
    /// Une nouvelle mesure est disponible.
    Sample(ActivitySample),
    /// Aucun backend fiable n'est disponible pour la session courante.
    Unavailable(String),
    /// Le backend natif a démarré, mais une lecture a échoué.
    ReadFailed(String),
}

#[derive(Debug, Clone, Copy)]
enum ActivityControl {
    Shutdown,
}

/// Surveille l'inactivité sur un thread dédié, sans bloquer la boucle UI.
pub struct ActivityMonitor {
    event_rx: Receiver<ActivityEvent>,
    control_tx: Sender<ActivityControl>,
    handle: Option<JoinHandle<()>>,
}

impl ActivityMonitor {
    /// Démarre la surveillance native avec un échantillon par seconde.
    ///
    /// # Errors
    ///
    /// Renvoie une erreur si le thread dédié ne peut pas être créé. Une absence
    /// de backend pour la session graphique est remontée par [`ActivityEvent::Unavailable`].
    pub fn start() -> Result<Self, SystemError> {
        Self::start_with(DEFAULT_SAMPLE_INTERVAL, default_backend)
    }

    fn start_with<F>(interval: Duration, factory: F) -> Result<Self, SystemError>
    where
        F: FnOnce() -> Result<Box<dyn IdleBackend>, SystemError> + Send + 'static,
    {
        let (event_tx, event_rx) = bounded(EVENT_CHANNEL_CAPACITY);
        let worker_event_rx = event_rx.clone();
        let (control_tx, control_rx) = bounded(CONTROL_CHANNEL_CAPACITY);
        let handle = thread::Builder::new()
            .name("gremlin-activity".to_owned())
            .spawn(move || {
                run_monitor(
                    interval.max(Duration::from_millis(1)),
                    factory,
                    &event_tx,
                    &worker_event_rx,
                    &control_rx,
                );
            })?;

        Ok(Self {
            event_rx,
            control_tx,
            handle: Some(handle),
        })
    }

    /// Récepteur borné des mesures. Le producteur conserve la mesure la plus
    /// récente si l'interface tarde à consommer les événements.
    #[must_use]
    pub fn events(&self) -> &Receiver<ActivityEvent> {
        &self.event_rx
    }
}

impl Drop for ActivityMonitor {
    fn drop(&mut self) {
        let _send_result = self.control_tx.try_send(ActivityControl::Shutdown);
        if let Some(handle) = self.handle.take() {
            let _join_result = handle.join();
        }
    }
}

fn run_monitor<F>(
    interval: Duration,
    factory: F,
    event_tx: &Sender<ActivityEvent>,
    event_rx: &Receiver<ActivityEvent>,
    control_rx: &Receiver<ActivityControl>,
) where
    F: FnOnce() -> Result<Box<dyn IdleBackend>, SystemError>,
{
    let mut backend = match factory() {
        Ok(backend) => backend,
        Err(error) => {
            send_latest(
                event_tx,
                event_rx,
                ActivityEvent::Unavailable(error.to_string()),
            );
            let _control = control_rx.recv();
            return;
        }
    };

    loop {
        match control_rx.recv_timeout(interval) {
            Ok(ActivityControl::Shutdown) | Err(RecvTimeoutError::Disconnected) => return,
            Err(RecvTimeoutError::Timeout) => {
                let event = match backend.idle_for() {
                    Ok(idle_for) => ActivityEvent::Sample(ActivitySample { idle_for }),
                    Err(error) => ActivityEvent::ReadFailed(error.to_string()),
                };
                send_latest(event_tx, event_rx, event);
            }
        }
    }
}

fn send_latest(
    event_tx: &Sender<ActivityEvent>,
    event_rx: &Receiver<ActivityEvent>,
    event: ActivityEvent,
) {
    match event_tx.try_send(event) {
        Ok(()) | Err(TrySendError::Disconnected(_)) => {}
        Err(TrySendError::Full(event)) => {
            let _stale_event = event_rx.try_recv();
            let _retry_result = event_tx.try_send(event);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FixedBackend;

    impl IdleBackend for FixedBackend {
        fn idle_for(&mut self) -> Result<Duration, SystemError> {
            Ok(Duration::from_secs(42))
        }
    }

    #[test]
    fn monitor_emits_samples_and_stops_deterministically() {
        let monitor =
            ActivityMonitor::start_with(Duration::from_millis(1), || Ok(Box::new(FixedBackend)));
        let monitor = match monitor {
            Ok(monitor) => monitor,
            Err(error) => panic!("le thread de test doit démarrer : {error}"),
        };

        let event = monitor.events().recv_timeout(Duration::from_secs(1));
        assert_eq!(
            event,
            Ok(ActivityEvent::Sample(ActivitySample {
                idle_for: Duration::from_secs(42)
            }))
        );
        drop(monitor);
    }

    #[test]
    fn backend_creation_failure_is_reported() {
        let monitor = ActivityMonitor::start_with(Duration::from_millis(1), || {
            Err(SystemError::ActivityUnavailable("test".to_owned()))
        });
        let monitor = match monitor {
            Ok(monitor) => monitor,
            Err(error) => panic!("le thread de test doit démarrer : {error}"),
        };

        let event = monitor.events().recv_timeout(Duration::from_secs(1));
        assert!(
            matches!(event, Ok(ActivityEvent::Unavailable(message)) if message.contains("test"))
        );
    }
}
