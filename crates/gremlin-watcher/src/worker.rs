//! Worker d'arrière-plan de la surveillance des dépôts Git.
//!
//! Le worker est l'**unique détenteur** de l'état de surveillance : liste des dépôts
//! actifs et mémoire de debouncing. `RepoWatcher` n'est qu'une façade qui lui
//! adresse des commandes, ce qui interdit toute divergence d'état.
//!
//! Tous les dépôts qu'il surveille lui ont été **explicitement** désignés : il ne
//! parcourt aucune arborescence, n'observe aucune racine de projets et
//! n'enregistre jamais un dépôt qu'on ne lui a pas demandé.
//!
//! Sa boucle sert systématiquement, dans cet ordre : les commandes de contrôle, un
//! lot **borné** d'événements de fichiers, puis les dépôts stabilisés. Aucune rafale
//! d'événements ne peut donc affamer un `Shutdown` ni retarder indéfiniment un flush.

use crate::config::WatcherConfig;
use crate::debouncer::EventDebouncer;
use crate::error::WatcherError;
use crate::git_parser::GitRefParser;
use crate::git_path::{analyze_git_path, is_git_repo, normalize_path, GitPathKind, GIT_DIR_NAME};
use crate::signals::{DevSignal, GitCommitStamp, ToolingStateAck, WatcherStatus};
use crate::tooling::{ToolingEvent, ToolingPipeline};
use crossbeam_channel::{Receiver, Select, Sender, TryRecvError, TrySendError};
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{debug, info, warn};

/// Capacité du canal interne d'événements `notify`.
///
/// Au-delà, les événements sont écartés et comptabilisés : le worker resynchronise
/// alors l'état complet des dépôts. La mémoire reste ainsi bornée même sous un
/// déluge d'événements (build, `npm install`, `cargo build`).
pub const NOTIFY_CHANNEL_CAPACITY: usize = 4096;

/// Nombre maximal d'événements traités avant de re-servir les commandes de contrôle.
const MAX_EVENTS_PER_ITERATION: usize = 256;

/// Période de vérification de l'existence des dépôts surveillés.
///
/// Filet de sécurité : la disparition d'un dépôt ne produit pas toujours
/// d'événement exploitable, le watch OS partant avec le répertoire supprimé.
const LIVENESS_SWEEP_INTERVAL: Duration = Duration::from_millis(2000);

/// Nombre maximal de relectures d'un dépôt dont les métadonnées sont illisibles.
const MAX_READ_RETRIES: u8 = 5;

/// Sous-dossiers de `.git` surveillés en plus du répertoire `.git` lui-même.
const GIT_WATCHED_SUBDIRS: [&str; 2] = ["refs", "logs"];

/// Réponse synchrone à une commande de contrôle.
pub type Ack = Option<Sender<Result<(), WatcherError>>>;

/// Motif du retrait d'un dépôt de la surveillance.
///
/// N'influence plus aucune décision — la liste des dépôts suivis est tenue par
/// l'appelant — mais distingue les deux situations dans le journal, où elles
/// n'appellent pas le même diagnostic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DetachReason {
    /// Désinscription demandée par l'appelant.
    UserRequest,
    /// Le répertoire `.git` du dépôt a disparu du disque.
    Vanished,
}

/// Message interne acheminant le flux `notify` vers le worker.
#[derive(Debug)]
pub enum NotifyMessage {
    /// Événement de système de fichiers.
    Event(Box<Event>),
    /// Erreur remontée par le backend (quota atteint, watch perdu...).
    Error(String),
}

/// Commandes internes envoyées au worker d'arrière-plan du watcher.
#[derive(Debug)]
pub enum WatcherControl {
    /// Surveiller un dépôt Git existant.
    WatchRepo(PathBuf, Ack),
    /// Cesser de surveiller un dépôt.
    UnwatchRepo(PathBuf, Ack),
    /// Installer le canal de remontée d'incidents.
    SetStatusSender(Sender<WatcherStatus>),
    /// Demander la liste des dépôts actuellement surveillés.
    ListRepos(Sender<Vec<PathBuf>>),
    /// Activer ou désactiver réellement les ancres de rapports.
    SetToolingEnabled(bool, Sender<ToolingStateAck>),
    /// Arrêter le worker.
    Shutdown,
}

/// État d'un dépôt surveillé.
#[derive(Debug)]
struct RepoEntry {
    /// Répertoire `.git` du dépôt.
    git_dir: PathBuf,
    /// Chemins effectivement enregistrés auprès de l'OS.
    watched: Vec<PathBuf>,
    /// `true` une fois l'état initial mémorisé (aucun signal métier avant cela).
    seeded: bool,
    /// `true` tant que l'historique des jours de commits reste à relire.
    ///
    /// Le balayage est différé d'un tour de boucle plutôt qu'exécuté dans
    /// `attach_repo` : une rafale de rattachements ne devient donc pas une
    /// séquence de lectures ininterruptible qui affamerait un `Shutdown`.
    history_pending: bool,
    /// Nombre de relectures consécutives infructueuses.
    read_retries: u8,
    /// Ancres de rapports enregistrées exclusivement pour ce dépôt.
    tooling_watched: Vec<PathBuf>,
}

/// Suite à donner après le traitement d'une commande.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Flow {
    /// Poursuivre la boucle.
    Continue,
    /// Terminer le worker.
    Stop,
}

/// Worker de surveillance : détenteur exclusif de l'état de suivi des dépôts.
pub struct WatcherWorker {
    watcher: RecommendedWatcher,
    signal_sender: Sender<DevSignal>,
    status_sender: Option<Sender<WatcherStatus>>,
    debouncer: EventDebouncer,
    repos: HashMap<PathBuf, RepoEntry>,
    /// Correspondance chemin livré par `notify` -> clé canonique, pour éviter de
    /// canoniser (appel système) à chaque événement.
    aliases: HashMap<PathBuf, PathBuf>,
    dropped_events: Arc<AtomicU64>,
    is_running: Arc<AtomicBool>,
    last_sweep: Instant,
    tooling: ToolingPipeline,
}

impl WatcherWorker {
    /// Construit le worker à partir des ressources partagées avec la façade.
    pub fn new(
        watcher: RecommendedWatcher,
        signal_sender: Sender<DevSignal>,
        config: &WatcherConfig,
        dropped_events: Arc<AtomicU64>,
        is_running: Arc<AtomicBool>,
    ) -> Result<Self, WatcherError> {
        Ok(Self {
            watcher,
            signal_sender,
            status_sender: None,
            debouncer: EventDebouncer::new(config.debounce_duration()),
            repos: HashMap::new(),
            aliases: HashMap::new(),
            dropped_events,
            is_running,
            last_sweep: Instant::now(),
            tooling: ToolingPipeline::new(config)?,
        })
    }

    /// Boucle principale du worker.
    pub fn run(
        mut self,
        notify_rx: &Receiver<NotifyMessage>,
        control_rx: &Receiver<WatcherControl>,
    ) {
        while self.is_running.load(Ordering::Relaxed) {
            let mut progressed = false;

            // 1. Les commandes de contrôle passent toujours en premier.
            match self.drain_control(control_rx, &mut progressed) {
                Flow::Stop => break,
                Flow::Continue => {}
            }

            // 2. Historique d'un seul dépôt, pour rester interruptible.
            self.scan_one_pending_history(&mut progressed);

            // 3. Lot borné d'événements de fichiers.
            self.drain_events(notify_rx, &mut progressed);

            // 4. Rapports stabilisés et résultats du parser dédié.
            self.tooling.submit_ready();
            self.drain_tooling(&mut progressed);

            // 5. Événements perdus : resynchronisation complète.
            self.absorb_dropped_events();

            // 6. Dépôts stabilisés.
            for repo in self.debouncer.poll_ready() {
                self.emit_repo_signals(&repo);
            }

            // 7. Contrôle périodique d'existence des dépôts.
            if self.last_sweep.elapsed() >= LIVENESS_SWEEP_INTERVAL {
                self.sweep_dead_repos();
            }

            // 8. Rien à faire : sommeil borné jusqu'au prochain réveil utile.
            if !progressed {
                self.idle_wait(notify_rx, control_rx);
            }
        }

        debug!("Worker de surveillance Git arrêté");
    }

    /// Consomme toutes les commandes de contrôle en attente.
    fn drain_control(
        &mut self,
        control_rx: &Receiver<WatcherControl>,
        progressed: &mut bool,
    ) -> Flow {
        loop {
            match control_rx.try_recv() {
                Ok(control) => {
                    *progressed = true;
                    if self.handle_control(control) == Flow::Stop {
                        return Flow::Stop;
                    }
                }
                Err(TryRecvError::Empty) => return Flow::Continue,
                Err(TryRecvError::Disconnected) => return Flow::Stop,
            }
        }
    }

    /// Consomme au plus [`MAX_EVENTS_PER_ITERATION`] événements de fichiers.
    fn drain_events(&mut self, notify_rx: &Receiver<NotifyMessage>, progressed: &mut bool) {
        for _ in 0..MAX_EVENTS_PER_ITERATION {
            match notify_rx.try_recv() {
                Ok(NotifyMessage::Event(event)) => {
                    *progressed = true;
                    self.handle_event(&event);
                }
                Ok(NotifyMessage::Error(reason)) => {
                    *progressed = true;
                    warn!("Erreur notify interceptée : {reason}");
                    self.on_events_lost(0, reason);
                }
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => return,
            }
        }
    }

    fn drain_tooling(&mut self, progressed: &mut bool) {
        let events = self.tooling.drain_results();
        *progressed |= !events.is_empty();
        for event in events {
            match event {
                ToolingEvent::Signal(signal) => self.emit(signal),
                ToolingEvent::Status(status) => self.report_status(status),
            }
        }
    }

    /// Attend passivement le prochain réveil utile (événement, commande ou échéance).
    ///
    /// Sans dépôt surveillé ni stabilisation en cours, l'attente est strictement
    /// bloquante : aucun réveil périodique pour une application résidente.
    fn idle_wait(
        &self,
        notify_rx: &Receiver<NotifyMessage>,
        control_rx: &Receiver<WatcherControl>,
    ) {
        let sweep_deadline = (!self.repos.is_empty())
            .then(|| LIVENESS_SWEEP_INTERVAL.saturating_sub(self.last_sweep.elapsed()));
        let deadline = [
            self.debouncer.time_until_next_ready(),
            self.tooling.time_until_next_ready(),
            sweep_deadline,
        ]
        .into_iter()
        .flatten()
        .min();

        let mut select = Select::new();
        let _ = select.recv(control_rx);
        let _ = select.recv(notify_rx);
        let _ = select.recv(self.tooling.result_receiver());

        // `ready`/`ready_timeout` ne consomment pas le message : il sera traité par
        // le début de la boucle, commandes de contrôle en premier.
        let Some(timeout) = deadline else {
            let _ = select.ready();
            return;
        };
        let _ = select.ready_timeout(timeout);
    }

    // -------------------------------------------------------------------------
    // Commandes de contrôle
    // -------------------------------------------------------------------------

    /// Exécute une commande de contrôle et répond à l'appelant si un accusé est attendu.
    fn handle_control(&mut self, control: WatcherControl) -> Flow {
        match control {
            WatcherControl::WatchRepo(path, ack) => {
                let result = self.attach_repo(&normalize_path(&path));
                reply(ack, result);
            }
            WatcherControl::UnwatchRepo(path, ack) => {
                self.detach_repo(&normalize_path(&path), DetachReason::UserRequest);
                reply(ack, Ok(()));
            }
            WatcherControl::SetStatusSender(sender) => self.status_sender = Some(sender),
            WatcherControl::ListRepos(sender) => {
                let mut repos: Vec<PathBuf> = self.repos.keys().cloned().collect();
                repos.sort();
                let _ = sender.send(repos);
            }
            WatcherControl::SetToolingEnabled(enabled, ack) => {
                let confirmation = self.set_tooling_enabled(enabled);
                let _ = ack.try_send(confirmation);
            }
            WatcherControl::Shutdown => {
                debug!("Signal d'arrêt reçu par le worker de surveillance");
                return Flow::Stop;
            }
        }

        if self.is_running.load(Ordering::Relaxed) {
            Flow::Continue
        } else {
            Flow::Stop
        }
    }

    fn set_tooling_enabled(&mut self, enabled: bool) -> ToolingStateAck {
        if self.tooling.is_enabled() == enabled {
            self.report_status(WatcherStatus::ToolingStateChanged {
                enabled,
                error: None,
            });
            return ToolingStateAck {
                enabled,
                error: None,
            };
        }

        self.tooling.set_enabled(enabled);
        let repos: Vec<PathBuf> = self.repos.keys().cloned().collect();
        if enabled {
            for repo in repos {
                self.ensure_tooling_watches(&repo);
                self.tooling.seed_repo(&repo);
            }
        } else {
            for repo in repos {
                let watched = self
                    .repos
                    .get_mut(&repo)
                    .map(|entry| std::mem::take(&mut entry.tooling_watched))
                    .unwrap_or_default();
                for path in watched {
                    let _ = self.watcher.unwatch(&path);
                }
                self.tooling.remove_repo(&repo);
            }
        }
        self.report_status(WatcherStatus::ToolingStateChanged {
            enabled,
            error: None,
        });
        ToolingStateAck {
            enabled,
            error: None,
        }
    }

    // -------------------------------------------------------------------------
    // Gestion des dépôts
    // -------------------------------------------------------------------------

    /// Enregistre la surveillance ciblée d'un dépôt Git.
    ///
    /// Seules les métadonnées utiles sont surveillées (`.git`, `.git/refs`,
    /// `.git/logs`) : ni `.git/objects`, ni l'arborescence de travail. Le volume
    /// d'événements pendant un build ou un `git gc` s'en trouve réduit d'autant.
    fn attach_repo(&mut self, repo_root: &Path) -> Result<(), WatcherError> {
        if self.repos.contains_key(repo_root) {
            return Ok(());
        }

        // Vérification explicite avant l'enregistrement : sans elle, un dossier
        // qui n'est pas un dépôt ne se signale que par une erreur `notify` au
        // message système opaque, impossible à traduire pour l'utilisateur.
        //
        // Le refus emprunte le canal de statut au même titre qu'un échec système :
        // un consommateur qui l'a installé doit voir *toutes* les raisons pour
        // lesquelles un chemin n'est pas surveillé, pas seulement certaines.
        if !is_git_repo(repo_root) {
            let error = WatcherError::NotARepository(repo_root.to_path_buf());
            debug!(repo = %repo_root.display(), "Chemin refusé : ce n'est pas un dépôt Git");
            self.report_status(WatcherStatus::WatchFailed {
                path: repo_root.to_path_buf(),
                reason: error.to_string(),
            });
            return Err(error);
        }

        let git_dir = repo_root.join(GIT_DIR_NAME);
        if let Err(e) = self.watcher.watch(&git_dir, RecursiveMode::NonRecursive) {
            let reason = e.to_string();
            warn!(repo = %repo_root.display(), "Échec d'enregistrement watch Git : {reason}");
            self.report_status(WatcherStatus::WatchFailed {
                path: git_dir.clone(),
                reason,
            });
            return Err(WatcherError::Notify(e));
        }

        let entry = RepoEntry {
            watched: vec![git_dir.clone()],
            git_dir,
            seeded: false,
            history_pending: true,
            read_retries: 0,
            tooling_watched: Vec::new(),
        };
        let _ = self.repos.insert(repo_root.to_path_buf(), entry);
        self.ensure_sub_watches(repo_root);
        self.ensure_tooling_watches(repo_root);
        self.seed_repo(repo_root);
        self.tooling.seed_repo(repo_root);

        info!(repo = %repo_root.display(), "Dépôt enregistré sous surveillance active");
        self.emit(DevSignal::RepoDiscovered {
            repo_name: GitRefParser::extract_repo_name(repo_root),
            path: repo_root.to_path_buf(),
        });
        Ok(())
    }

    /// Complète la surveillance des sous-dossiers `.git/refs` et `.git/logs`.
    ///
    /// Ces dossiers n'existent pas toujours au moment de l'attachement (`git init`
    /// sans commit, clone en cours) : la surveillance est complétée dès qu'ils
    /// apparaissent.
    fn ensure_sub_watches(&mut self, repo_root: &Path) {
        let Some(entry) = self.repos.get(repo_root) else {
            return;
        };
        if entry.watched.len() > GIT_WATCHED_SUBDIRS.len() {
            return;
        }

        let git_dir = entry.git_dir.clone();
        let already: Vec<PathBuf> = entry.watched.clone();
        let mut added = Vec::new();

        for subdir in GIT_WATCHED_SUBDIRS {
            let path = git_dir.join(subdir);
            // `.git/logs` n'apparaît qu'au premier commit, `.git/refs` peut manquer
            // pendant un clone : le test d'existence évite un enregistrement voué à
            // l'échec à chaque événement.
            if already.contains(&path) || !path.is_dir() {
                continue;
            }
            if self.watcher.watch(&path, RecursiveMode::Recursive).is_ok() {
                added.push(path);
            }
        }

        if let Some(entry) = self.repos.get_mut(repo_root) {
            entry.watched.extend(added);
        }
    }

    /// Relit l'historique de commits d'**un seul** dépôt en attente.
    ///
    /// La lecture reste sur le worker, jamais dans le callback `notify` ni dans
    /// l'orchestrateur. Une seule par tour de boucle : les commandes de contrôle
    /// repassent en premier avant la suivante.
    fn scan_one_pending_history(&mut self, progressed: &mut bool) {
        let Some((repo_root, git_dir)) = self
            .repos
            .iter()
            .find(|(_, entry)| entry.history_pending)
            .map(|(root, entry)| (root.clone(), entry.git_dir.clone()))
        else {
            return;
        };
        *progressed = true;
        if let Some(entry) = self.repos.get_mut(&repo_root) {
            entry.history_pending = false;
        }

        let repo_name = GitRefParser::extract_repo_name(&repo_root);
        let Some(history) = GitRefParser::read_commit_day_history(&git_dir) else {
            warn!(repo = %repo_name, "Journal de références illisible : série non reconstituée");
            self.report_status(WatcherStatus::HistoryUnreadable {
                path: git_dir,
                reason: String::from("lecture du journal .git/logs/HEAD refusée"),
            });
            return;
        };

        debug!(
            repo = %repo_name,
            days = history.stamps.len(),
            truncated = history.truncated,
            "Historique de jours de commits relu"
        );
        self.emit(DevSignal::CommitHistorySeeded {
            repo_name,
            repo_path: repo_root,
            stamps: history.stamps,
            truncated: history.truncated,
        });
    }

    /// Mémorise silencieusement l'état initial d'un dépôt (aucun signal métier).
    fn seed_repo(&mut self, repo_root: &Path) {
        let Some(entry) = self.repos.get(repo_root) else {
            return;
        };
        let snapshot = GitRefParser::read_snapshot(&entry.git_dir);
        let Some(branch) = snapshot.branch else {
            return;
        };

        let _ = self.debouncer.update_branch_if_changed(repo_root, &branch);
        if let Some(sha) = snapshot.commit_sha {
            let _ = self.debouncer.update_commit_sha_if_changed(repo_root, &sha);
        }
        if let Some(entry) = self.repos.get_mut(repo_root) {
            entry.seeded = true;
        }
    }

    /// Retire un dépôt de la surveillance et libère les watches associés.
    fn detach_repo(&mut self, repo_root: &Path, reason: DetachReason) {
        let Some(entry) = self.repos.remove(repo_root) else {
            return;
        };

        for path in entry.watched {
            let _ = self.watcher.unwatch(&path);
        }
        for path in entry.tooling_watched {
            let _ = self.watcher.unwatch(&path);
        }
        self.tooling.remove_repo(repo_root);
        self.debouncer.remove_repo(repo_root);
        self.aliases.retain(|_, canonical| canonical != repo_root);

        info!(repo = %repo_root.display(), reason = ?reason, "Dépôt retiré de la surveillance");
        self.emit(DevSignal::RepoRemoved {
            repo_name: GitRefParser::extract_repo_name(repo_root),
            path: repo_root.to_path_buf(),
        });
    }

    /// Retire le dépôt si son répertoire `.git` a disparu du disque.
    fn check_repo_alive(&mut self, repo_root: &Path) {
        let Some(entry) = self.repos.get(repo_root) else {
            return;
        };
        if !entry.git_dir.exists() {
            self.detach_repo(repo_root, DetachReason::Vanished);
        }
    }

    /// Vérifie périodiquement l'existence de tous les dépôts surveillés.
    ///
    /// Filet de sécurité indispensable : la suppression d'un dépôt ne génère pas
    /// toujours d'événement exploitable (watch OS perdu avec le répertoire).
    fn sweep_dead_repos(&mut self) {
        self.last_sweep = Instant::now();
        let dead: Vec<PathBuf> = self
            .repos
            .iter()
            .filter(|(_, entry)| !entry.git_dir.exists())
            .map(|(repo, _)| repo.clone())
            .collect();

        for repo in dead {
            self.detach_repo(&repo, DetachReason::Vanished);
        }
    }

    // -------------------------------------------------------------------------
    // Traitement des événements
    // -------------------------------------------------------------------------

    /// Analyse et achemine un événement de modification de fichier `notify`.
    fn handle_event(&mut self, event: &Event) {
        if event.need_rescan() {
            self.on_events_lost(
                0,
                String::from("le backend a signalé une perte d'événements"),
            );
        }

        if !matches!(
            event.kind,
            EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)
        ) {
            return;
        }

        let is_removal = matches!(event.kind, EventKind::Remove(_));
        for path in &event.paths {
            self.handle_event_path(path, is_removal);
        }
    }

    /// Traite un chemin issu d'un événement de fichier.
    fn handle_event_path(&mut self, path: &Path, is_removal: bool) {
        self.handle_tooling_path(path, is_removal);
        let Some((raw_root, kind)) = analyze_git_path(path) else {
            self.handle_workspace_path(path, is_removal);
            return;
        };

        if let Some(repo_root) = self.resolve_known_repo(&raw_root) {
            if is_removal && kind == GitPathKind::GitDir {
                self.check_repo_alive(&repo_root);
                return;
            }
            // Appelé pour tout événement du dépôt : la création de `.git/logs` ou
            // `.git/refs` est elle-même un événement « interne » qu'il faut saisir
            // pour compléter la surveillance.
            self.ensure_sub_watches(&repo_root);
            if matches!(kind, GitPathKind::Metadata | GitPathKind::GitDir) {
                self.debouncer.record_repo_activity(repo_root);
            }
            return;
        }

        // Dépôt inconnu : ignoré. Un `git init` ou un `git clone` sous un dépôt
        // surveillé ne s'enregistre pas de lui-même — c'est à l'utilisateur de
        // déclarer les espaces de travail qu'il confie à son Gremlin.
        debug!(path = %raw_root.display(), "Chemin Git hors des dépôts suivis — ignoré");
    }

    fn handle_tooling_path(&mut self, path: &Path, is_removal: bool) {
        if !self.tooling.is_enabled() {
            return;
        }
        let repo_root = self
            .repos
            .keys()
            .filter(|repo| path.starts_with(repo))
            .max_by_key(|repo| repo.components().count())
            .cloned();
        let Some(repo_root) = repo_root else {
            return;
        };

        if is_removal {
            self.tooling.forget_path(path);
            return;
        }
        if path.is_dir() {
            self.ensure_tooling_watches(&repo_root);
            for status in self.tooling.record_directory(&repo_root, path) {
                self.report_status(status);
            }
            return;
        }
        if let Some(status) = self.tooling.record_path(&repo_root, path) {
            self.report_status(status);
        }
    }

    /// Enregistre uniquement les ancres nécessaires aux rapports configurés.
    fn ensure_tooling_watches(&mut self, repo_root: &Path) {
        if !self.tooling.is_enabled() {
            return;
        }
        let sources = self.tooling.sources().to_vec();
        let already = self
            .repos
            .get(repo_root)
            .map(|entry| entry.tooling_watched.clone())
            .unwrap_or_default();
        let mut added = Vec::new();

        for source in sources {
            let target = repo_root.join(source.relative_path);
            let target_is_file = target.extension().is_some();
            let mut candidate = if target_is_file {
                target.parent().map(Path::to_path_buf)
            } else if target.is_dir() {
                Some(target.clone())
            } else {
                target.parent().map(Path::to_path_buf)
            };

            while candidate.as_ref().is_some_and(|path| !path.is_dir()) {
                candidate = candidate.and_then(|path| path.parent().map(Path::to_path_buf));
            }
            let Some(candidate) = candidate else {
                continue;
            };
            if !candidate.starts_with(repo_root)
                || already.contains(&candidate)
                || added.contains(&candidate)
            {
                continue;
            }
            let recursive = !target_is_file && candidate == target;
            let mode = if recursive {
                RecursiveMode::Recursive
            } else {
                RecursiveMode::NonRecursive
            };
            match self.watcher.watch(&candidate, mode) {
                Ok(()) => added.push(candidate),
                Err(error) => self.report_status(WatcherStatus::WatchFailed {
                    path: candidate,
                    reason: error.to_string(),
                }),
            }
        }

        if let Some(entry) = self.repos.get_mut(repo_root) {
            entry.tooling_watched.extend(added);
        }
    }

    /// Traite un chemin situé hors de tout répertoire `.git`.
    ///
    /// Seules les ancres de rapports produisent encore de tels chemins : aucune
    /// racine de projets n'est surveillée. La suppression reste à traiter, car
    /// effacer le répertoire d'un dépôt suivi n'émet pas toujours d'événement sur
    /// son `.git` — le watch système part avec le répertoire.
    fn handle_workspace_path(&mut self, path: &Path, is_removal: bool) {
        if is_removal {
            self.check_repos_under(path);
        }
    }

    /// Vérifie les dépôts susceptibles d'avoir disparu avec un chemin supprimé.
    fn check_repos_under(&mut self, removed: &Path) {
        let candidates: Vec<PathBuf> = self
            .repos
            .keys()
            .filter(|repo| repo.starts_with(removed))
            .cloned()
            .collect();
        for repo in candidates {
            self.check_repo_alive(&repo);
        }
    }

    /// Retrouve la clé canonique d'un dépôt connu à partir d'un chemin d'événement.
    ///
    /// Sous Windows, `notify` peut livrer une casse ou une forme de chemin différente
    /// de celle fournie par l'appelant : la correspondance est mise en cache pour ne
    /// canoniser qu'une seule fois par variante rencontrée.
    fn resolve_known_repo(&mut self, raw_root: &Path) -> Option<PathBuf> {
        if self.repos.contains_key(raw_root) {
            return Some(raw_root.to_path_buf());
        }
        if let Some(canonical) = self.aliases.get(raw_root) {
            if self.repos.contains_key(canonical) {
                return Some(canonical.clone());
            }
        }

        let canonical = normalize_path(raw_root);
        if self.repos.contains_key(&canonical) {
            let _ = self
                .aliases
                .insert(raw_root.to_path_buf(), canonical.clone());
            return Some(canonical);
        }
        None
    }

    // -------------------------------------------------------------------------
    // Émission des signaux métier
    // -------------------------------------------------------------------------

    /// Émet les signaux `DevSignal` pour un dépôt dont les modifications se sont stabilisées.
    fn emit_repo_signals(&mut self, repo_root: &Path) {
        let Some(entry) = self.repos.get(repo_root) else {
            return;
        };
        let git_dir = entry.git_dir.clone();
        let was_seeded = entry.seeded;

        let mut snapshot = GitRefParser::read_snapshot(&git_dir);
        let Some(branch) = snapshot.branch.take() else {
            self.handle_unreadable_head(repo_root, &git_dir);
            return;
        };

        if let Some(entry) = self.repos.get_mut(repo_root) {
            entry.read_retries = 0;
            entry.seeded = true;
        }

        let repo_name = GitRefParser::extract_repo_name(repo_root);
        let old_branch = self.debouncer.update_branch_if_changed(repo_root, &branch);
        let sha_is_new = snapshot
            .commit_sha
            .as_ref()
            .is_some_and(|sha| self.debouncer.update_commit_sha_if_changed(repo_root, sha));

        if !was_seeded {
            // Premier état lisible du dépôt : il sert de référence, pas d'événement.
            debug!(repo = %repo_name, branch = %branch, "État initial du dépôt mémorisé");
            return;
        }

        if let Some(old_branch) = old_branch.clone() {
            info!(repo = %repo_name, from = %old_branch, to = %branch, "Changement de branche Git détecté");
            self.emit(DevSignal::BranchChanged {
                repo_name: repo_name.clone(),
                old_branch,
                new_branch: branch.clone(),
                repo_path: repo_root.to_path_buf(),
            });
        }

        if sha_is_new && is_real_commit(&snapshot, old_branch.is_some()) {
            info!(repo = %repo_name, branch = %branch, sha = ?snapshot.commit_sha, "Nouveau commit Git détecté");
            self.emit(DevSignal::CommitCreated {
                repo_name,
                branch,
                stamp: authoritative_stamp(&snapshot),
                commit_sha: snapshot.commit_sha,
                message: snapshot.message,
                repo_path: repo_root.to_path_buf(),
            });
        }
    }

    /// Réagit à un `HEAD` illisible : suppression du dépôt ou état transitoire.
    ///
    /// Git remplace brièvement `HEAD` pendant un `checkout` : plutôt que d'inventer
    /// une branche par défaut (source de faux `BranchChanged`), la lecture est
    /// simplement reprogrammée.
    fn handle_unreadable_head(&mut self, repo_root: &Path, git_dir: &Path) {
        if !git_dir.exists() {
            self.detach_repo(repo_root, DetachReason::Vanished);
            return;
        }

        let Some(entry) = self.repos.get_mut(repo_root) else {
            return;
        };
        entry.read_retries = entry.read_retries.saturating_add(1);
        if entry.read_retries <= MAX_READ_RETRIES {
            debug!(repo = %repo_root.display(), "HEAD temporairement illisible — nouvelle tentative");
            self.debouncer.record_repo_activity(repo_root.to_path_buf());
        } else {
            warn!(repo = %repo_root.display(), "HEAD durablement illisible — dépôt ignoré jusqu'à la prochaine activité");
        }
    }

    // -------------------------------------------------------------------------
    // Santé de la surveillance
    // -------------------------------------------------------------------------

    /// Prend en compte les événements écartés faute de place dans le canal.
    fn absorb_dropped_events(&mut self) {
        let dropped = self.dropped_events.swap(0, Ordering::Relaxed);
        if dropped > 0 {
            self.on_events_lost(
                dropped,
                String::from("saturation du canal d'événements de fichiers"),
            );
        }
    }

    /// Resynchronise l'état de tous les dépôts après une perte d'événements.
    fn on_events_lost(&mut self, dropped: u64, reason: String) {
        warn!(
            dropped,
            "Perte d'événements : resynchronisation des dépôts ({reason})"
        );
        self.report_status(WatcherStatus::EventsLost { dropped, reason });

        let repos: Vec<PathBuf> = self.repos.keys().cloned().collect();
        for repo in repos {
            self.debouncer.record_repo_activity(repo);
        }
    }

    /// Transmet un incident sur le canal de statut s'il est installé.
    fn report_status(&mut self, status: WatcherStatus) {
        if let Some(sender) = &self.status_sender {
            match sender.try_send(status) {
                Ok(()) => {}
                Err(TrySendError::Full(_)) => {
                    debug!("Canal de statut saturé — incident écarté");
                }
                Err(TrySendError::Disconnected(_)) => {
                    debug!("Canal de statut fermé — remontée d'incidents désactivée");
                    self.status_sender = None;
                }
            }
        }
    }

    /// Émet un signal métier et détecte la fermeture du consommateur.
    fn emit(&self, signal: DevSignal) {
        match self.signal_sender.try_send(signal) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => {
                let _ = self.dropped_events.fetch_add(1, Ordering::Relaxed);
            }
            Err(TrySendError::Disconnected(_)) => {
                warn!("Consommateur de signaux fermé — arrêt de la surveillance Git");
                self.is_running.store(false, Ordering::Relaxed);
            }
        }
    }
}

/// Répond à une commande de contrôle si un accusé de réception est attendu.
fn reply(ack: Ack, result: Result<(), WatcherError>) {
    if let Some(sender) = ack {
        let _ = sender.send(result);
    }
}

/// Détermine si un changement de SHA correspond à un véritable commit.
///
/// Le reflog distingue un `commit` d'un `checkout`, d'un `clone` ou d'un `reset`,
/// qui déplacent eux aussi `HEAD` sans qu'aucun commit n'ait été créé.
fn is_real_commit(snapshot: &crate::git_parser::RepoSnapshot, branch_changed: bool) -> bool {
    match (&snapshot.last_reflog, &snapshot.commit_sha) {
        // Reflog à jour : il fait autorité.
        (Some(entry), Some(sha)) if &entry.new_sha == sha => entry.is_commit_action(),
        // Sans reflog exploitable, un simple changement de branche n'est pas un commit.
        _ => !branch_changed,
    }
}

/// Horodatage à joindre à un commit live, uniquement si le reflog fait autorité.
///
/// Le chemin de repli — un SHA qui a changé sans entrée de reflog correspondante
/// — fait toujours réagir le familier, mais ne date rien : attribuer une journée
/// de série sans preuve temporelle reviendrait à la deviner.
fn authoritative_stamp(snapshot: &crate::git_parser::RepoSnapshot) -> Option<GitCommitStamp> {
    let entry = snapshot.last_reflog.as_ref()?;
    let sha = snapshot.commit_sha.as_ref()?;
    if &entry.new_sha != sha || !entry.is_commit_action() {
        return None;
    }
    entry.stamp
}
