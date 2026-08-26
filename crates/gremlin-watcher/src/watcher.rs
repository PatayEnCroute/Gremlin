//! Surveillance active des dépôts Git explicitement déclarés, via `notify`.
//!
//! `RepoWatcher` est une **façade sans état** : toute la connaissance des dépôts
//! surveillés vit dans le worker d'arrière-plan (voir `crate::worker`). Les
//! opérations d'enregistrement sont confirmées par accusé de réception, si bien
//! qu'un échec réel (chemin inaccessible, quota du système atteint) remonte
//! directement à l'appelant au lieu d'être noyé dans les journaux.
//!
//! Aucune découverte automatique : la liste des dépôts vient de
//! [`WatcherConfig::tracked_repos`] et des appels explicites à
//! [`RepoWatcher::watch_repo`].

use crate::config::WatcherConfig;
use crate::error::WatcherError;
use crate::signals::{DevSignal, ToolingStateAck, WatcherStatus};
use crate::worker::{NotifyMessage, WatcherControl, WatcherWorker, NOTIFY_CHANNEL_CAPACITY};
use crossbeam_channel::{Receiver, RecvTimeoutError, Sender, TrySendError};
use notify::{Config, Event, RecommendedWatcher, Watcher};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;
use tracing::warn;

/// Délai maximal d'attente d'un accusé de réception du worker.
const CONTROL_ACK_TIMEOUT: Duration = Duration::from_secs(5);
/// Capacité du canal de contrôle du worker.
const CONTROL_CHANNEL_CAPACITY: usize = 256;
/// Délai maximal d'insertion dans le canal de contrôle borné.
const CONTROL_SEND_TIMEOUT: Duration = Duration::from_secs(1);

/// Gestionnaire de surveillance des dépôts Git explicitement déclarés.
pub struct RepoWatcher {
    control_tx: Sender<WatcherControl>,
    config: WatcherConfig,
    is_running: Arc<AtomicBool>,
    worker_handle: Option<JoinHandle<()>>,
}

impl RepoWatcher {
    /// Initialise un nouveau surveillant de dépôts Git avec la configuration par défaut.
    ///
    /// # Errors
    /// Renvoie `WatcherError::Notify` si le watcher système ne peut pas être initialisé.
    pub fn new(signal_sender: Sender<DevSignal>) -> Result<Self, WatcherError> {
        Self::new_with_config(signal_sender, &WatcherConfig::default())
    }

    /// Initialise un nouveau surveillant de dépôts Git avec une configuration personnalisée.
    ///
    /// La surveillance ne démarre sur aucun chemin : utiliser
    /// [`Self::arm_tracked_repos`] pour monter les dépôts de la configuration, ou
    /// [`Self::watch_repo`] pour en ajouter un à la volée.
    ///
    /// # Errors
    /// Renvoie `WatcherError::Notify` si le watcher système ne peut pas être initialisé,
    /// ou `WatcherError::Io` si le thread de surveillance ne peut pas être lancé.
    pub fn new_with_config(
        signal_sender: Sender<DevSignal>,
        config: &WatcherConfig,
    ) -> Result<Self, WatcherError> {
        // Canal borné : sous un déluge d'événements (build, `npm install`), les
        // événements excédentaires sont comptés puis compensés par une relecture
        // complète, au lieu de faire enfler la mémoire sans limite.
        let (notify_tx, notify_rx) = crossbeam_channel::bounded(NOTIFY_CHANNEL_CAPACITY);
        let (control_tx, control_rx) = crossbeam_channel::bounded(CONTROL_CHANNEL_CAPACITY);
        let is_running = Arc::new(AtomicBool::new(true));
        let dropped_events = Arc::new(AtomicU64::new(0));

        let dropped_for_handler = Arc::clone(&dropped_events);
        let watcher = RecommendedWatcher::new(
            move |res: notify::Result<Event>| {
                let message = match res {
                    Ok(event) => NotifyMessage::Event(Box::new(event)),
                    Err(e) => NotifyMessage::Error(e.to_string()),
                };
                if notify_tx.try_send(message).is_err() {
                    let _ = dropped_for_handler.fetch_add(1, Ordering::Relaxed);
                }
            },
            Config::default(),
        )?;

        let mut normalized_config = config.clone();
        let _ = normalized_config.normalize();
        let worker = WatcherWorker::new(
            watcher,
            signal_sender,
            &normalized_config,
            dropped_events,
            Arc::clone(&is_running),
        )?;

        let worker_handle = std::thread::Builder::new()
            .name("gremlin-repo-watcher".into())
            .spawn(move || worker.run(&notify_rx, &control_rx))
            .map_err(WatcherError::Io)?;

        Ok(Self {
            control_tx,
            config: normalized_config,
            is_running,
            worker_handle: Some(worker_handle),
        })
    }

    /// Configuration active du surveillant.
    #[must_use]
    pub const fn config(&self) -> &WatcherConfig {
        &self.config
    }

    /// Dépôts déclarés dans la configuration, après normalisation.
    #[must_use]
    pub fn tracked_repos(&self) -> &[PathBuf] {
        &self.config.tracked_repos
    }

    /// Ajoute un dépôt Git existant à la liste des répertoires surveillés.
    ///
    /// L'appel est confirmé par le worker : un dépôt inexistant ou impossible à
    /// surveiller renvoie une erreur. L'opération est idempotente.
    ///
    /// # Errors
    /// Renvoie `WatcherError::ChannelClosed` si le worker s'est arrêté,
    /// `WatcherError::Timeout` s'il ne répond pas,
    /// `WatcherError::NotARepository` si le chemin n'est pas un dépôt Git, ou
    /// `WatcherError::Notify` si l'enregistrement auprès du système échoue.
    pub fn watch_repo(&mut self, repo_path: &Path) -> Result<(), WatcherError> {
        let path = repo_path.to_path_buf();
        self.send_and_wait(|ack| WatcherControl::WatchRepo(path, ack))
    }

    /// Retire un dépôt de la surveillance active.
    ///
    /// Un `RepoRemoved` est émis si le dépôt était effectivement surveillé.
    /// L'opération est idempotente : désinscrire un dépôt inconnu réussit sans
    /// rien émettre.
    ///
    /// # Errors
    /// Voir [`Self::watch_repo`].
    pub fn unwatch_repo(&mut self, repo_path: &Path) -> Result<(), WatcherError> {
        let path = repo_path.to_path_buf();
        self.send_and_wait(|ack| WatcherControl::UnwatchRepo(path, ack))
    }

    /// Liste les dépôts actuellement surveillés (chemins canoniques, triés).
    ///
    /// # Errors
    /// Voir [`Self::watch_repo`].
    pub fn watched_repos(&self) -> Result<Vec<PathBuf>, WatcherError> {
        let (tx, rx) = crossbeam_channel::bounded(1);
        self.send_control(WatcherControl::ListRepos(tx))?;
        rx.recv_timeout(CONTROL_ACK_TIMEOUT).map_err(map_recv_error)
    }

    /// Installe un canal de remontée des incidents de surveillance.
    ///
    /// Sans ce canal, les échecs d'enregistrement asynchrones et les pertes
    /// d'événements ne sont visibles que dans les journaux.
    ///
    /// # Errors
    /// Renvoie `WatcherError::ChannelClosed` si le worker s'est arrêté.
    pub fn set_status_sender(&mut self, sender: Sender<WatcherStatus>) -> Result<(), WatcherError> {
        self.send_control(WatcherControl::SetStatusSender(sender))
    }

    /// Demande sans bloquer l'activation ou la désactivation des rapports.
    ///
    /// Le récepteur retourné confirme l'état réel une fois les ancres
    /// enregistrées ou retirées par le worker. Le même changement est aussi
    /// publié via [`WatcherStatus::ToolingStateChanged`] pour l'observabilité.
    ///
    /// # Errors
    ///
    /// Renvoie une erreur si le canal borné est saturé ou si le worker est arrêté.
    pub fn request_tooling_enabled(
        &self,
        enabled: bool,
    ) -> Result<Receiver<ToolingStateAck>, WatcherError> {
        let (ack_tx, ack_rx) = crossbeam_channel::bounded(1);
        self.control_tx
            .try_send(WatcherControl::SetToolingEnabled(enabled, ack_tx))
            .map_err(|error| match error {
                TrySendError::Full(_) => WatcherError::ChannelFull,
                TrySendError::Disconnected(_) => WatcherError::ChannelClosed,
            })?;
        Ok(ack_rx)
    }

    /// Monte les dépôts déclarés dans [`WatcherConfig::tracked_repos`].
    ///
    /// Aucun parcours du système de fichiers : chaque chemin est enregistré tel
    /// qu'il a été déclaré. Un dépôt injoignable — disque débranché, dossier
    /// déplacé, chemin devenu invalide — **n'interrompt pas** l'armement des
    /// autres : son échec est renvoyé à l'appelant, qui peut en rendre compte
    /// dans l'interface, et le dépôt reste déclaré.
    ///
    /// Renvoie la liste des dépôts qui n'ont pas pu être montés, avec leur cause.
    ///
    /// # Errors
    /// Renvoie `WatcherError::ChannelClosed` ou `WatcherError::Timeout` si le
    /// worker ne répond plus : dans ce cas plus rien ne peut être monté, et
    /// poursuivre n'aurait aucun sens.
    pub fn arm_tracked_repos(&mut self) -> Result<Vec<(PathBuf, WatcherError)>, WatcherError> {
        let repos = self.config.tracked_repos.clone();
        let mut failures = Vec::new();

        for repo in repos {
            match self.watch_repo(&repo) {
                Ok(()) => {}
                Err(e @ (WatcherError::ChannelClosed | WatcherError::Timeout)) => return Err(e),
                Err(e) => {
                    warn!(repo = %repo.display(), "Dépôt suivi non surveillable : {e}");
                    failures.push((repo, e));
                }
            }
        }

        Ok(failures)
    }

    /// Envoie une commande et attend sa confirmation par le worker.
    fn send_and_wait<F>(&self, build: F) -> Result<(), WatcherError>
    where
        F: FnOnce(crate::worker::Ack) -> WatcherControl,
    {
        let (ack_tx, ack_rx) = crossbeam_channel::bounded(1);
        self.send_control(build(Some(ack_tx)))?;
        rx_result(ack_rx.recv_timeout(CONTROL_ACK_TIMEOUT))
    }

    fn send_control(&self, control: WatcherControl) -> Result<(), WatcherError> {
        self.control_tx
            .send_timeout(control, CONTROL_SEND_TIMEOUT)
            .map_err(|error| match error {
                crossbeam_channel::SendTimeoutError::Timeout(_) => WatcherError::ChannelFull,
                crossbeam_channel::SendTimeoutError::Disconnected(_) => WatcherError::ChannelClosed,
            })
    }
}

/// Convertit le résultat d'une attente d'accusé de réception.
fn rx_result(
    received: Result<Result<(), WatcherError>, RecvTimeoutError>,
) -> Result<(), WatcherError> {
    match received {
        Ok(result) => result,
        Err(e) => Err(map_recv_error(e)),
    }
}

/// Traduit une erreur de réception en erreur du watcher.
fn map_recv_error(error: RecvTimeoutError) -> WatcherError {
    match error {
        RecvTimeoutError::Timeout => WatcherError::Timeout,
        RecvTimeoutError::Disconnected => WatcherError::ChannelClosed,
    }
}

impl Drop for RepoWatcher {
    fn drop(&mut self) {
        self.is_running.store(false, Ordering::Relaxed);
        let _ = self.control_tx.try_send(WatcherControl::Shutdown);

        if let Some(handle) = self.worker_handle.take() {
            let _ = handle.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::RepoWatcher;
    use crate::config::WatcherConfig;
    use crate::error::WatcherError;
    use crate::signals::DevSignal;
    use crate::test_support::{write_file, TempDirGuard};
    use crossbeam_channel::Receiver;
    use std::path::Path;
    use std::time::{Duration, Instant};

    /// Délai généreux : les tests attendent un signal précis, jamais une durée fixe.
    const SIGNAL_TIMEOUT: Duration = Duration::from_secs(15);

    fn config(debounce_ms: u64) -> WatcherConfig {
        WatcherConfig {
            debounce_duration_ms: debounce_ms,
            ..WatcherConfig::default()
        }
    }

    fn wait_for<F>(rx: &Receiver<DevSignal>, predicate: F) -> DevSignal
    where
        F: Fn(&DevSignal) -> bool,
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
                    panic!("canal de signaux fermé prématurément")
                }
            }
        }
        panic!("signal attendu non reçu dans le délai imparti");
    }

    fn init_repo(root: &Path, branch: &str, sha: &str) {
        write_file(
            &root.join(".git").join("HEAD"),
            &format!("ref: refs/heads/{branch}\n"),
        );
        write_file(
            &root.join(".git").join("refs").join("heads").join(branch),
            &format!("{sha}\n"),
        );
    }

    #[test]
    fn test_repo_watcher_lifecycle_and_signals() {
        let guard = TempDirGuard::new("watcher_lifecycle");
        let repo = guard.path().to_path_buf();
        init_repo(&repo, "main", &"1".repeat(40));

        let (tx, rx) = crossbeam_channel::unbounded();
        let mut watcher = match RepoWatcher::new_with_config(tx, &config(100)) {
            Ok(w) => w,
            Err(e) => panic!("Échec création watcher : {e}"),
        };

        if let Err(e) = watcher.watch_repo(&repo) {
            panic!("l'enregistrement du dépôt doit réussir : {e}");
        }
        wait_for(
            &rx,
            |signal| matches!(signal, DevSignal::RepoDiscovered { path, .. } if *path == repo),
        );

        // Le dépôt est bien référencé par l'unique source de vérité : le worker.
        match watcher.watched_repos() {
            Ok(repos) => assert_eq!(repos, vec![repo.clone()]),
            Err(e) => panic!("interrogation du worker impossible : {e}"),
        }

        // Simuler un commit (ref + reflog).
        let sha = "2".repeat(40);
        write_file(
            &repo.join(".git").join("logs").join("HEAD"),
            &format!(
                "{old} {sha} User <u@example.com> 1700000000 +0000\tcommit: test commit\n",
                old = "1".repeat(40)
            ),
        );
        write_file(
            &repo.join(".git").join("refs").join("heads").join("main"),
            &format!("{sha}\n"),
        );

        let signal = wait_for(&rx, |signal| {
            matches!(signal, DevSignal::CommitCreated { .. })
        });
        match signal {
            DevSignal::CommitCreated {
                commit_sha,
                message,
                branch,
                ..
            } => {
                assert_eq!(commit_sha, Some(sha));
                assert_eq!(message, Some("test commit".to_string()));
                assert_eq!(branch, "main");
            }
            other => panic!("signal inattendu : {other:?}"),
        }
    }

    #[test]
    fn test_watch_repo_reports_real_failure() {
        let guard = TempDirGuard::new("watcher_failure");
        let (tx, _rx) = crossbeam_channel::unbounded();
        let mut watcher = match RepoWatcher::new_with_config(tx, &config(100)) {
            Ok(w) => w,
            Err(e) => panic!("Échec création watcher : {e}"),
        };

        // Aucun `.git` : l'échec doit remonter à l'appelant, pas seulement dans les logs.
        let missing = guard.path().join("pas_un_depot");
        assert!(
            watcher.watch_repo(&missing).is_err(),
            "l'enregistrement d'un dépôt inexistant doit échouer"
        );
        assert_eq!(watcher.watched_repos().ok(), Some(Vec::new()));
    }

    #[test]
    fn test_watch_repo_rejects_a_non_git_directory_with_a_clear_error() {
        let guard = TempDirGuard::new("watcher_not_a_repo");
        let ordinary = guard.child("dossier_ordinaire");

        let (tx, _rx) = crossbeam_channel::unbounded();
        let mut watcher = match RepoWatcher::new_with_config(tx, &config(100)) {
            Ok(w) => w,
            Err(e) => panic!("Échec création watcher : {e}"),
        };

        // Le motif du refus doit être exploitable par l'interface : un message
        // `notify` opaque ne permettrait pas d'expliquer quoi que ce soit.
        match watcher.watch_repo(&ordinary) {
            Err(WatcherError::NotARepository(path)) => assert_eq!(path, ordinary),
            other => panic!("erreur attendue NotARepository, obtenu : {other:?}"),
        }
    }

    #[test]
    fn test_arming_skips_missing_repo_without_dropping_the_others() {
        let guard = TempDirGuard::new("watcher_arm");
        let first = guard.child("premier");
        let missing = guard.path().join("disparu");
        let third = guard.child("troisieme");
        init_repo(&first, "main", &"a".repeat(40));
        init_repo(&third, "main", &"b".repeat(40));

        let armed = WatcherConfig {
            tracked_repos: vec![first.clone(), missing.clone(), third.clone()],
            ..config(100)
        };

        let (tx, _rx) = crossbeam_channel::unbounded();
        let mut watcher = match RepoWatcher::new_with_config(tx, &armed) {
            Ok(w) => w,
            Err(e) => panic!("Échec création watcher : {e}"),
        };

        let failures = match watcher.arm_tracked_repos() {
            Ok(failures) => failures,
            Err(e) => panic!("l'armement ne doit pas échouer globalement : {e}"),
        };

        assert_eq!(failures.len(), 1, "un seul dépôt doit être en échec");
        assert_eq!(failures[0].0, missing);

        // Les deux dépôts valides sont bien montés malgré le voisin invalide.
        match watcher.watched_repos() {
            Ok(repos) => {
                assert!(repos.contains(&first));
                assert!(repos.contains(&third));
                assert_eq!(repos.len(), 2);
            }
            Err(e) => panic!("interrogation du worker impossible : {e}"),
        }
    }

    #[test]
    fn test_no_repo_is_attached_without_an_explicit_request() {
        // Anti-régression du retrait du scanner : un dépôt créé à côté d'un
        // dépôt suivi ne doit jamais s'enregistrer de lui-même.
        let guard = TempDirGuard::new("watcher_no_discovery");
        let tracked = guard.child("suivi");
        init_repo(&tracked, "main", &"1".repeat(40));

        let (tx, rx) = crossbeam_channel::unbounded();
        let mut watcher = match RepoWatcher::new_with_config(tx, &config(50)) {
            Ok(w) => w,
            Err(e) => panic!("Échec création watcher : {e}"),
        };
        if let Err(e) = watcher.watch_repo(&tracked) {
            panic!("l'enregistrement du dépôt doit réussir : {e}");
        }
        wait_for(
            &rx,
            |signal| matches!(signal, DevSignal::RepoDiscovered { path, .. } if *path == tracked),
        );

        // `git init` dans le voisinage immédiat, y compris à l'intérieur du dépôt suivi.
        let sibling = guard.path().join("intrus");
        let nested = tracked.join("sous_projet");
        init_repo(&sibling, "main", &"2".repeat(40));
        init_repo(&nested, "main", &"3".repeat(40));

        let deadline = Instant::now() + Duration::from_millis(1500);
        while Instant::now() < deadline {
            match rx.recv_timeout(Duration::from_millis(50)) {
                Ok(DevSignal::RepoDiscovered { path, .. }) => {
                    panic!(
                        "dépôt enregistré sans demande explicite : {}",
                        path.display()
                    )
                }
                Ok(_) | Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
                Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
            }
        }

        match watcher.watched_repos() {
            Ok(repos) => assert_eq!(repos, vec![tracked]),
            Err(e) => panic!("interrogation du worker impossible : {e}"),
        }
    }

    #[test]
    fn test_drop_joins_worker_even_under_load() {
        let guard = TempDirGuard::new("watcher_drop");
        let repo = guard.path().to_path_buf();
        init_repo(&repo, "main", &"3".repeat(40));

        let (tx, rx) = crossbeam_channel::unbounded();
        let mut watcher = match RepoWatcher::new_with_config(tx, &config(50)) {
            Ok(w) => w,
            Err(e) => panic!("Échec création watcher : {e}"),
        };
        if let Err(e) = watcher.watch_repo(&repo) {
            panic!("l'enregistrement du dépôt doit réussir : {e}");
        }
        wait_for(&rx, |signal| {
            matches!(signal, DevSignal::RepoDiscovered { .. })
        });

        // Rafale soutenue d'écritures pendant la destruction du watcher.
        let git_dir = repo.join(".git");
        for i in 0..400 {
            write_file(&git_dir.join("COMMIT_EDITMSG"), &format!("wip {i}\n"));
        }

        let started = Instant::now();
        drop(watcher);
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "la destruction ne doit jamais être affamée par le flux d'événements"
        );
    }
}
