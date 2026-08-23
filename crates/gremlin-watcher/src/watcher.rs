//! Surveillance active des dépôts Git et détection à chaud via `notify`.
//!
//! `RepoWatcher` est une **façade sans état** : toute la connaissance des dépôts
//! surveillés vit dans le worker d'arrière-plan (voir [`crate::worker`]). Les
//! opérations d'enregistrement sont confirmées par accusé de réception, si bien
//! qu'un échec réel (chemin inaccessible, quota du système atteint) remonte
//! directement à l'appelant au lieu d'être noyé dans les journaux.

use crate::config::WatcherConfig;
use crate::error::WatcherError;
use crate::scanner::GitScanner;
use crate::signals::{DevSignal, WatcherStatus};
use crate::worker::{
    NotifyMessage, WatchOrigin, WatcherControl, WatcherWorker, NOTIFY_CHANNEL_CAPACITY,
};
use crossbeam_channel::{RecvTimeoutError, Sender};
use notify::{Config, Event, RecommendedWatcher, Watcher};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;
use tracing::{debug, warn};

/// Délai maximal d'attente d'un accusé de réception du worker.
const CONTROL_ACK_TIMEOUT: Duration = Duration::from_secs(5);

/// Gestionnaire de surveillance des dépôts Git et détection à chaud.
pub struct RepoWatcher {
    control_tx: Sender<WatcherControl>,
    config: WatcherConfig,
    is_running: Arc<AtomicBool>,
    worker_handle: Option<JoinHandle<()>>,
    scan_handle: Option<JoinHandle<()>>,
    scan_cancelled: Arc<AtomicBool>,
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
    /// La surveillance ne démarre sur aucun chemin : utiliser [`Self::watch_repo`],
    /// [`Self::watch_workspace_root`] ou [`Self::start_auto_discovery`].
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
        let (control_tx, control_rx) = crossbeam_channel::unbounded();
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

        let worker = WatcherWorker::new(
            watcher,
            signal_sender,
            config.debounce_duration(),
            dropped_events,
            Arc::clone(&is_running),
        );

        let worker_handle = std::thread::Builder::new()
            .name("gremlin-repo-watcher".into())
            .spawn(move || worker.run(&notify_rx, &control_rx))
            .map_err(WatcherError::Io)?;

        Ok(Self {
            control_tx,
            config: config.clone(),
            is_running,
            worker_handle: Some(worker_handle),
            scan_handle: None,
            scan_cancelled: Arc::new(AtomicBool::new(false)),
        })
    }

    /// Configuration active du surveillant.
    #[must_use]
    pub const fn config(&self) -> &WatcherConfig {
        &self.config
    }

    /// Ajoute un dépôt Git existant à la liste des répertoires surveillés.
    ///
    /// L'appel est confirmé par le worker : un dépôt inexistant ou impossible à
    /// surveiller renvoie une erreur. L'opération est idempotente.
    ///
    /// # Errors
    /// Renvoie `WatcherError::ChannelClosed` si le worker s'est arrêté,
    /// `WatcherError::Timeout` s'il ne répond pas, ou `WatcherError::Notify` si
    /// l'enregistrement auprès du système échoue.
    pub fn watch_repo(&mut self, repo_path: &Path) -> Result<(), WatcherError> {
        let path = repo_path.to_path_buf();
        self.send_and_wait(|ack| WatcherControl::WatchRepo(path, WatchOrigin::Explicit, ack))
    }

    /// Ajoute un répertoire racine de projets (ex: `~/Projects`) pour la détection à chaud.
    ///
    /// La racine n'est **pas** surveillée récursivement dans son intégralité : seuls
    /// ses sous-dossiers proches, hors dossiers ignorés et hors dépôts déjà connus,
    /// sont observés.
    ///
    /// # Errors
    /// Voir [`Self::watch_repo`].
    pub fn watch_workspace_root(&mut self, root_path: &Path) -> Result<(), WatcherError> {
        let path = root_path.to_path_buf();
        self.send_and_wait(|ack| WatcherControl::WatchRoot(path, ack))
    }

    /// Retire un dépôt de la surveillance active, quelle que soit son origine.
    ///
    /// Fonctionne aussi bien pour un dépôt ajouté explicitement que pour un dépôt
    /// découvert automatiquement (scan ou détection à chaud). Un `RepoRemoved` est
    /// émis si le dépôt était effectivement surveillé.
    ///
    /// Le dépôt est ensuite **exclu de la découverte automatique** : sans cela, le
    /// premier événement venu le ré-enregistrerait aussitôt. Un nouvel appel à
    /// [`Self::watch_repo`] lève l'exclusion.
    ///
    /// # Errors
    /// Voir [`Self::watch_repo`].
    pub fn unwatch_repo(&mut self, repo_path: &Path) -> Result<(), WatcherError> {
        let path = repo_path.to_path_buf();
        self.send_and_wait(|ack| WatcherControl::UnwatchRepo(path, ack))
    }

    /// Retire une racine de projets de la détection à chaud.
    ///
    /// Les dépôts déjà découverts sous cette racine restent surveillés.
    ///
    /// # Errors
    /// Voir [`Self::watch_repo`].
    pub fn unwatch_workspace_root(&mut self, root_path: &Path) -> Result<(), WatcherError> {
        let path = root_path.to_path_buf();
        self.send_and_wait(|ack| WatcherControl::UnwatchRoot(path, ack))
    }

    /// Liste les dépôts actuellement surveillés (chemins canoniques, triés).
    ///
    /// # Errors
    /// Voir [`Self::watch_repo`].
    pub fn watched_repos(&self) -> Result<Vec<PathBuf>, WatcherError> {
        let (tx, rx) = crossbeam_channel::bounded(1);
        self.control_tx
            .send(WatcherControl::ListRepos(tx))
            .map_err(|_| WatcherError::ChannelClosed)?;
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
        self.control_tx
            .send(WatcherControl::SetStatusSender(sender))
            .map_err(|_| WatcherError::ChannelClosed)
    }

    /// Applique la configuration de découverte : racines surveillées puis scan de fond.
    ///
    /// Consomme `auto_discovery`, `custom_roots` et `max_scan_depth` de la
    /// [`WatcherConfig`] fournie à la construction.
    ///
    /// # Errors
    /// Renvoie `WatcherError::ChannelClosed` ou `WatcherError::Timeout` si le worker
    /// ne répond plus. Un échec de surveillance d'une racine isolée est journalisé
    /// et remonté sur le canal de statut, sans interrompre les autres racines.
    pub fn start_auto_discovery(&mut self) -> Result<(), WatcherError> {
        let mut roots: Vec<PathBuf> = Vec::new();
        if self.config.auto_discovery {
            roots.extend(GitScanner::discover_default_roots());
        }
        for custom in &self.config.custom_roots {
            if !roots.contains(custom) {
                roots.push(custom.clone());
            }
        }

        for root in &roots {
            match self.watch_workspace_root(root) {
                Ok(()) => {}
                Err(e @ (WatcherError::ChannelClosed | WatcherError::Timeout)) => return Err(e),
                Err(e) => {
                    warn!(root = %root.display(), "Racine de projets ignorée : {e}");
                }
            }
        }

        let max_depth = self.config.max_scan_depth;
        self.start_background_scan(roots, max_depth);
        Ok(())
    }

    /// Lance un scan asynchrone des racines et enregistre les dépôts découverts au fil de l'eau.
    ///
    /// Un seul scan peut être actif à la fois : un appel concurrent est ignoré.
    /// Le scan est annulé et attendu lors de la destruction du `RepoWatcher`, ce qui
    /// évite qu'un parcours profond ne survive à son propriétaire.
    pub fn start_background_scan(&mut self, roots: Vec<PathBuf>, max_depth: usize) {
        if self
            .scan_handle
            .as_ref()
            .is_some_and(|handle| !handle.is_finished())
        {
            debug!("Scan de dépôts déjà en cours — nouvelle demande ignorée");
            return;
        }
        // Récupérer le thread précédent, déjà terminé.
        if let Some(handle) = self.scan_handle.take() {
            let _ = handle.join();
        }
        if roots.is_empty() {
            return;
        }

        self.scan_cancelled.store(false, Ordering::Relaxed);
        let cancelled = Arc::clone(&self.scan_cancelled);
        let control_tx = self.control_tx.clone();

        match std::thread::Builder::new()
            .name("gremlin-git-scan".into())
            .spawn(move || {
                GitScanner::scan_roots_cancellable(&roots, max_depth, &cancelled, |repo| {
                    let _ = control_tx.send(WatcherControl::WatchRepo(
                        repo.to_path_buf(),
                        WatchOrigin::Discovery,
                        None,
                    ));
                });
            }) {
            Ok(handle) => self.scan_handle = Some(handle),
            Err(e) => warn!("Impossible de lancer le scan de dépôts : {e}"),
        }
    }

    /// Envoie une commande et attend sa confirmation par le worker.
    fn send_and_wait<F>(&self, build: F) -> Result<(), WatcherError>
    where
        F: FnOnce(crate::worker::Ack) -> WatcherControl,
    {
        let (ack_tx, ack_rx) = crossbeam_channel::bounded(1);
        self.control_tx
            .send(build(Some(ack_tx)))
            .map_err(|_| WatcherError::ChannelClosed)?;
        rx_result(ack_rx.recv_timeout(CONTROL_ACK_TIMEOUT))
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
        self.scan_cancelled.store(true, Ordering::Relaxed);
        self.is_running.store(false, Ordering::Relaxed);
        let _ = self.control_tx.send(WatcherControl::Shutdown);

        if let Some(handle) = self.worker_handle.take() {
            let _ = handle.join();
        }
        if let Some(handle) = self.scan_handle.take() {
            let _ = handle.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::RepoWatcher;
    use crate::config::WatcherConfig;
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
            auto_discovery: false,
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
