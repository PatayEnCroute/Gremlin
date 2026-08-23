//! Surveillance passive et rechargement à chaud des packs de skins et d'accessoires.

use crate::config::WatcherConfig;
use crate::error::WatcherError;
use crossbeam_channel::{select, Receiver, Sender};
use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use tracing::{debug, info, warn};

/// Capacité du canal interne d'événements d'assets.
const ASSET_CHANNEL_CAPACITY: usize = 1024;

/// Extensions de fichiers considérées comme des assets.
const ASSET_EXTENSIONS: [&str; 2] = ["png", "json"];

/// Signaux émis lors de modifications détectées dans les dossiers de skins et d'accessoires.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssetSignal {
    /// Un fichier de skin (PNG ou manifest) a été modifié ou ajouté.
    SkinChanged { skin_name: String, path: PathBuf },
    /// Un fichier d'accessoire a été modifié ou ajouté.
    AccessoryChanged { accessory_id: String, path: PathBuf },
    /// Demande de rechargement complet de l'ensemble des assets du catalogue.
    AssetsReloadRequested,
}

/// Message interne acheminant le flux `notify` vers le thread de debouncing.
#[derive(Debug)]
enum AssetMessage {
    /// Événement de système de fichiers.
    Event(Box<Event>),
    /// Erreur backend : impose un rechargement complet par précaution.
    Error(String),
}

/// Réveil du thread de debouncing.
enum Wake {
    /// Un message est disponible.
    Message(AssetMessage),
    /// L'échéance de stabilisation est atteinte.
    Flush,
    /// Arrêt demandé (ou canal fermé).
    Stop,
}

/// Surveillance passive des dossiers d'assets utilisateur avec debouncing.
pub struct AssetWatcher {
    watcher: RecommendedWatcher,
    join_handle: Option<JoinHandle<()>>,
    shutdown_tx: Sender<()>,
    is_running: Arc<AtomicBool>,
    watched_paths: HashSet<PathBuf>,
}

impl AssetWatcher {
    /// Initialise la surveillance des dossiers de skins et accessoires.
    ///
    /// # Errors
    /// Renvoie `WatcherError` si l'initialisation de l'observateur système de fichiers échoue.
    pub fn new(
        signal_sender: Sender<AssetSignal>,
        debounce_duration: Duration,
    ) -> Result<Self, WatcherError> {
        let is_running = Arc::new(AtomicBool::new(true));
        let running_clone = Arc::clone(&is_running);

        let (raw_tx, raw_rx) = crossbeam_channel::bounded::<AssetMessage>(ASSET_CHANNEL_CAPACITY);
        let (shutdown_tx, shutdown_rx) = crossbeam_channel::bounded::<()>(1);
        let dropped = Arc::new(AtomicU64::new(0));
        let dropped_for_handler = Arc::clone(&dropped);

        // Création du watcher `notify`
        let watcher = RecommendedWatcher::new(
            move |res: notify::Result<Event>| {
                let message = match res {
                    Ok(event) => AssetMessage::Event(Box::new(event)),
                    Err(e) => AssetMessage::Error(e.to_string()),
                };
                if raw_tx.try_send(message).is_err() {
                    // Canal saturé : l'événement est écarté, un rechargement complet
                    // sera demandé — la mémoire reste bornée.
                    let _ = dropped_for_handler.fetch_add(1, Ordering::Relaxed);
                }
            },
            Config::default(),
        )?;

        // Thread de debouncing et traitement des événements d'assets
        let handle = thread::Builder::new()
            .name("gremlin-asset-watcher".into())
            .spawn(move || {
                Self::debounce_loop(
                    &raw_rx,
                    &shutdown_rx,
                    &signal_sender,
                    debounce_duration,
                    &dropped,
                    &running_clone,
                );
            })
            .map_err(WatcherError::Io)?;

        Ok(Self {
            watcher,
            join_handle: Some(handle),
            shutdown_tx,
            is_running,
            watched_paths: HashSet::new(),
        })
    }

    /// Initialise la surveillance d'assets à partir de la configuration commune.
    ///
    /// # Errors
    /// Voir [`Self::new`].
    pub fn new_with_config(
        signal_sender: Sender<AssetSignal>,
        config: &WatcherConfig,
    ) -> Result<Self, WatcherError> {
        Self::new(signal_sender, config.asset_debounce_duration())
    }

    /// Ajoute un répertoire à surveiller (ex: `~/.config/gremlin/skins/`).
    ///
    /// # Errors
    /// Renvoie `WatcherError` si l'enregistrement auprès de l'OS échoue.
    pub fn watch_directory<P: AsRef<Path>>(&mut self, path: P) -> Result<(), WatcherError> {
        let p = path.as_ref();
        if self.watched_paths.contains(p) {
            return Ok(());
        }

        // `create_dir_all` est idempotent : inutile de tester l'existence au préalable.
        if let Err(e) = std::fs::create_dir_all(p) {
            debug!(path = %p.display(), "Création du dossier d'assets impossible : {e}");
        }

        self.watcher
            .watch(p, RecursiveMode::Recursive)
            .map_err(WatcherError::Notify)?;
        let _ = self.watched_paths.insert(p.to_path_buf());

        debug!(path = %p.display(), "Dossier d'assets sous surveillance active");
        Ok(())
    }

    /// Répertoires actuellement surveillés.
    #[must_use]
    pub fn watched_directories(&self) -> Vec<PathBuf> {
        let mut paths: Vec<PathBuf> = self.watched_paths.iter().cloned().collect();
        paths.sort();
        paths
    }

    /// Boucle de debouncing : agrège les rafales puis émet un signal par asset.
    fn debounce_loop(
        raw_rx: &Receiver<AssetMessage>,
        shutdown_rx: &Receiver<()>,
        signal_sender: &Sender<AssetSignal>,
        debounce_duration: Duration,
        dropped: &AtomicU64,
        is_running: &AtomicBool,
    ) {
        let mut pending_paths: HashSet<PathBuf> = HashSet::new();
        let mut window_started: Option<Instant> = None;

        while is_running.load(Ordering::Relaxed) {
            // Sans travail en attente, l'attente est purement bloquante : aucun
            // réveil périodique inutile pour une application résidente.
            let timeout =
                window_started.map(|start| debounce_duration.saturating_sub(start.elapsed()));

            match Self::wait_next(raw_rx, shutdown_rx, timeout) {
                Wake::Stop => return,
                Wake::Message(AssetMessage::Event(event)) => {
                    if Self::is_relevant_asset_event(&event) {
                        pending_paths.extend(event.paths.iter().cloned());
                        window_started = Some(Instant::now());
                    }
                }
                Wake::Message(AssetMessage::Error(reason)) => {
                    warn!("Erreur reçue de notify dans AssetWatcher : {reason}");
                    if signal_sender
                        .send(AssetSignal::AssetsReloadRequested)
                        .is_err()
                    {
                        return;
                    }
                }
                Wake::Flush => {
                    window_started = None;
                    for path in pending_paths.drain() {
                        let Some(signal) = Self::path_to_signal(&path) else {
                            continue;
                        };
                        info!(path = %path.display(), "Modification d'asset détectée — rechargement à chaud");
                        if signal_sender.send(signal).is_err() {
                            // Plus personne n'écoute : inutile de continuer à travailler.
                            debug!("Consommateur d'assets fermé — arrêt de la surveillance");
                            return;
                        }
                    }
                }
            }

            if dropped.swap(0, Ordering::Relaxed) > 0
                && signal_sender
                    .send(AssetSignal::AssetsReloadRequested)
                    .is_err()
            {
                return;
            }
        }
    }

    /// Attend le prochain réveil : message, échéance de debounce ou arrêt.
    fn wait_next(
        raw_rx: &Receiver<AssetMessage>,
        shutdown_rx: &Receiver<()>,
        timeout: Option<Duration>,
    ) -> Wake {
        let Some(timeout) = timeout else {
            return Self::wait_idle(raw_rx, shutdown_rx);
        };
        select! {
            recv(raw_rx) -> msg => msg.map_or(Wake::Stop, Wake::Message),
            recv(shutdown_rx) -> _ => Wake::Stop,
            default(timeout) => Wake::Flush,
        }
    }

    /// Attente strictement bloquante : aucun réveil périodique lorsqu'il n'y a rien à faire.
    fn wait_idle(raw_rx: &Receiver<AssetMessage>, shutdown_rx: &Receiver<()>) -> Wake {
        select! {
            recv(raw_rx) -> msg => msg.map_or(Wake::Stop, Wake::Message),
            recv(shutdown_rx) -> _ => Wake::Stop,
        }
    }

    fn is_relevant_asset_event(event: &Event) -> bool {
        match event.kind {
            EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_) => {
                event.paths.iter().any(|p| {
                    p.extension().and_then(|e| e.to_str()).is_some_and(|ext| {
                        ASSET_EXTENSIONS
                            .iter()
                            .any(|known| known.eq_ignore_ascii_case(ext))
                    })
                })
            }
            _ => false,
        }
    }

    /// Classe un chemin d'asset modifié en signal métier.
    fn path_to_signal(path: &Path) -> Option<AssetSignal> {
        let parent = path.parent()?;
        let folder_name = parent.file_name()?.to_str()?.to_string();

        // Si le fichier est directement dans "skins" ou dans un sous-dossier de skins
        if parent.ends_with("skins") {
            let stem = path.file_stem()?.to_str()?.to_string();
            return Some(AssetSignal::SkinChanged {
                skin_name: stem,
                path: path.to_path_buf(),
            });
        }
        if parent.ends_with("accessories") {
            let id = path.file_stem()?.to_str()?.to_string();
            return Some(AssetSignal::AccessoryChanged {
                accessory_id: id,
                path: path.to_path_buf(),
            });
        }

        let Some(grandparent) = parent.parent() else {
            return Some(AssetSignal::AssetsReloadRequested);
        };

        if grandparent.ends_with("skins") {
            Some(AssetSignal::SkinChanged {
                skin_name: folder_name,
                path: path.to_path_buf(),
            })
        } else if grandparent.ends_with("accessories") {
            let id = path.file_stem()?.to_str()?.to_string();
            Some(AssetSignal::AccessoryChanged {
                accessory_id: id,
                path: path.to_path_buf(),
            })
        } else {
            Some(AssetSignal::AssetsReloadRequested)
        }
    }
}

impl Drop for AssetWatcher {
    fn drop(&mut self) {
        self.is_running.store(false, Ordering::Relaxed);
        // Réveille le thread bloqué en attente d'événements avant de l'attendre.
        let _ = self.shutdown_tx.send(());
        if let Some(handle) = self.join_handle.take() {
            let _ = handle.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{AssetSignal, AssetWatcher};
    use crate::test_support::{write_file, TempDirGuard};
    use crossbeam_channel::Receiver;
    use std::path::{Path, PathBuf};
    use std::time::{Duration, Instant};

    const SIGNAL_TIMEOUT: Duration = Duration::from_secs(15);

    fn wait_for<F>(rx: &Receiver<AssetSignal>, predicate: F) -> AssetSignal
    where
        F: Fn(&AssetSignal) -> bool,
    {
        let deadline = Instant::now() + SIGNAL_TIMEOUT;
        while Instant::now() < deadline {
            match rx.recv_timeout(Duration::from_millis(50)) {
                Ok(signal) => {
                    if predicate(&signal) {
                        return signal;
                    }
                }
                Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
                Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                    panic!("canal d'assets fermé prématurément")
                }
            }
        }
        panic!("signal d'asset attendu non reçu dans le délai imparti");
    }

    fn signal_for(relative: &str) -> Option<AssetSignal> {
        let base = PathBuf::from(if cfg!(windows) {
            r"C:\config\gremlin"
        } else {
            "/config/gremlin"
        });
        let mut path = base;
        for part in relative.split('/') {
            path.push(part);
        }
        AssetWatcher::path_to_signal(&path)
    }

    #[test]
    fn test_asset_watcher_detects_skin_file() {
        let guard = TempDirGuard::new("asset_watch");
        let skins_dir = guard.child("skins");

        let (tx, rx) = crossbeam_channel::unbounded();
        let mut watcher = match AssetWatcher::new(tx, Duration::from_millis(50)) {
            Ok(w) => w,
            Err(e) => panic!("Échec de création de l'AssetWatcher : {e}"),
        };

        if let Err(e) = watcher.watch_directory(&skins_dir) {
            panic!("la surveillance du dossier doit réussir : {e}");
        }
        assert_eq!(watcher.watched_directories(), vec![skins_dir.clone()]);

        write_file(&skins_dir.join("manifest.json"), r#"{"name": "Test Skin"}"#);

        let signal = wait_for(
            &rx,
            |signal| matches!(signal, AssetSignal::SkinChanged { skin_name, .. } if skin_name == "manifest"),
        );
        match signal {
            AssetSignal::SkinChanged { path, .. } => {
                assert!(path.ends_with("manifest.json"));
            }
            other => panic!("signal inattendu : {other:?}"),
        }
    }

    #[test]
    fn test_watch_directory_is_idempotent_and_creates_missing_dir() {
        let guard = TempDirGuard::new("asset_idempotent");
        let missing = guard.path().join("skins_absent");

        let (tx, _rx) = crossbeam_channel::unbounded();
        let mut watcher = match AssetWatcher::new(tx, Duration::from_millis(50)) {
            Ok(w) => w,
            Err(e) => panic!("Échec de création de l'AssetWatcher : {e}"),
        };

        for _ in 0..3 {
            if let Err(e) = watcher.watch_directory(&missing) {
                panic!("le dossier manquant doit être créé puis surveillé : {e}");
            }
        }
        assert!(Path::new(&missing).is_dir());
        assert_eq!(watcher.watched_directories().len(), 1);
    }

    #[test]
    fn test_drop_joins_the_worker_thread() {
        let guard = TempDirGuard::new("asset_drop");
        let (tx, rx) = crossbeam_channel::unbounded();
        let mut watcher = match AssetWatcher::new(tx, Duration::from_millis(200)) {
            Ok(w) => w,
            Err(e) => panic!("Échec de création de l'AssetWatcher : {e}"),
        };
        if let Err(e) = watcher.watch_directory(guard.path()) {
            panic!("la surveillance du dossier doit réussir : {e}");
        }

        let started = Instant::now();
        drop(watcher);
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "le thread d'assets doit être joint sans délai notable"
        );
        // Le thread est bien terminé : plus aucun émetteur ne subsiste.
        assert!(rx.recv_timeout(Duration::from_millis(50)).is_err());
    }

    #[test]
    fn test_path_to_signal_classification() {
        assert_eq!(
            signal_for("skins/pixel_gremlin.png"),
            Some(AssetSignal::SkinChanged {
                skin_name: "pixel_gremlin".to_string(),
                path: skin_path("skins/pixel_gremlin.png"),
            })
        );
        assert_eq!(
            signal_for("skins/neon/manifest.json"),
            Some(AssetSignal::SkinChanged {
                skin_name: "neon".to_string(),
                path: skin_path("skins/neon/manifest.json"),
            })
        );
        assert_eq!(
            signal_for("accessories/hat_wizard.png"),
            Some(AssetSignal::AccessoryChanged {
                accessory_id: "hat_wizard".to_string(),
                path: skin_path("accessories/hat_wizard.png"),
            })
        );
        assert_eq!(
            signal_for("accessories/hats/crown.png"),
            Some(AssetSignal::AccessoryChanged {
                accessory_id: "crown".to_string(),
                path: skin_path("accessories/hats/crown.png"),
            })
        );
        assert_eq!(
            signal_for("themes/dark/palette.json"),
            Some(AssetSignal::AssetsReloadRequested)
        );
    }

    fn skin_path(relative: &str) -> PathBuf {
        let mut path = PathBuf::from(if cfg!(windows) {
            r"C:\config\gremlin"
        } else {
            "/config/gremlin"
        });
        for part in relative.split('/') {
            path.push(part);
        }
        path
    }
}
