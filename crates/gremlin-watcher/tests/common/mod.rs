//! Outillage partagé par les tests d'intégration de `gremlin-watcher`.

use crossbeam_channel::{Receiver, RecvTimeoutError};
use gremlin_watcher::{normalize_path, DevSignal, WatcherConfig};
use std::fmt::Debug;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// Délai d'attente d'un signal attendu.
///
/// Volontairement généreux : les tests attendent un **signal précis** et non
/// l'écoulement d'une durée, ils ne peuvent donc pas devenir instables sur une
/// machine chargée — seul un vrai blocage les fait échouer.
pub const SIGNAL_TIMEOUT: Duration = Duration::from_secs(20);

/// Durée d'observation utilisée pour vérifier qu'un signal n'arrive **pas**.
pub const QUIET_PERIOD: Duration = Duration::from_millis(1500);

static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Répertoire temporaire unique, supprimé à la fin du test.
pub struct TempDirGuard {
    path: PathBuf,
}

impl TempDirGuard {
    /// Crée un répertoire temporaire unique (préfixe + PID + compteur atomique).
    pub fn new(prefix: &str) -> Self {
        let count = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
        let pid = std::process::id();
        let path = std::env::temp_dir().join(format!("gremlin_int_{prefix}_{pid}_{count}"));
        let _ = std::fs::remove_dir_all(&path);
        create_dir(&path);
        Self {
            // Chemin canonique : `notify` livre lui aussi des chemins canoniques,
            // la comparaison directe des `PathBuf` reste donc valable sous Windows.
            path: normalize_path(&path),
        }
    }

    /// Racine du répertoire temporaire.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Crée et renvoie un sous-répertoire.
    pub fn child(&self, relative: &str) -> PathBuf {
        let child = self.path.join(relative);
        create_dir(&child);
        child
    }
}

impl Drop for TempDirGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// Crée une arborescence de répertoires, en échouant bruyamment.
pub fn create_dir(path: &Path) {
    if let Err(e) = std::fs::create_dir_all(path) {
        panic!("préparation du test impossible ({}) : {e}", path.display());
    }
}

/// Écrit un fichier de test ; toute erreur de préparation fait échouer le test tout de suite.
pub fn write_file(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        create_dir(parent);
    }
    if let Err(e) = std::fs::write(path, content) {
        panic!("écriture de test impossible ({}) : {e}", path.display());
    }
}

/// Supprime une arborescence, en échouant bruyamment.
pub fn remove_tree(path: &Path) {
    if let Err(e) = std::fs::remove_dir_all(path) {
        panic!("suppression de test impossible ({}) : {e}", path.display());
    }
}

/// Configuration de test : debounce court, aucun dépôt suivi au départ.
///
/// Le garde-fou est devenu structurel : la surveillance ne pouvant plus explorer
/// d'arborescence, aucun test ne *peut* parcourir le répertoire personnel de la
/// machine, quelle que soit la configuration.
pub fn test_config(debounce_ms: u64) -> WatcherConfig {
    WatcherConfig {
        debounce_duration_ms: debounce_ms,
        tooling_debounce_duration_ms: debounce_ms,
        ..WatcherConfig::default()
    }
}

/// Configuration de test montant d'emblée une liste de dépôts déclarés.
pub fn tracked_config(debounce_ms: u64, repos: &[&Path]) -> WatcherConfig {
    WatcherConfig {
        tracked_repos: repos.iter().map(|repo| repo.to_path_buf()).collect(),
        ..test_config(debounce_ms)
    }
}

/// Prépare un dépôt Git minimal (HEAD + référence loose).
pub fn init_repo(repo_root: &Path, branch: &str, sha: &str) {
    let git_dir = repo_root.join(".git");
    write_file(
        &git_dir.join("HEAD"),
        &format!("ref: refs/heads/{branch}\n"),
    );
    write_file(&ref_file(repo_root, branch), &format!("{sha}\n"));
}

/// Chemin de la référence loose d'une branche.
pub fn ref_file(repo_root: &Path, branch: &str) -> PathBuf {
    let mut path = repo_root.join(".git").join("refs").join("heads");
    for part in branch.split('/') {
        path.push(part);
    }
    path
}

/// Écrit une entrée de reflog `logs/HEAD`.
pub fn write_reflog(repo_root: &Path, old_sha: &str, new_sha: &str, action: &str) {
    write_file(
        &repo_root.join(".git").join("logs").join("HEAD"),
        &format!("{old_sha} {new_sha} Dev <dev@gremlin.rs> 1700000000 +0000\t{action}\n"),
    );
}

/// Simule un commit : mise à jour de la référence et du reflog.
pub fn simulate_commit(repo_root: &Path, branch: &str, old_sha: &str, new_sha: &str, msg: &str) {
    write_reflog(repo_root, old_sha, new_sha, &format!("commit: {msg}"));
    write_file(&ref_file(repo_root, branch), &format!("{new_sha}\n"));
}

/// Attend un signal satisfaisant le prédicat, en ignorant les autres.
pub fn wait_for<T, F>(rx: &Receiver<T>, what: &str, predicate: F) -> T
where
    T: Debug,
    F: Fn(&T) -> bool,
{
    let deadline = Instant::now() + SIGNAL_TIMEOUT;
    let mut seen: Vec<T> = Vec::new();

    while Instant::now() < deadline {
        match rx.recv_timeout(Duration::from_millis(50)) {
            Ok(signal) => {
                if predicate(&signal) {
                    return signal;
                }
                seen.push(signal);
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                panic!("canal fermé avant réception de : {what} (reçus : {seen:?})")
            }
        }
    }

    panic!("{what} non reçu en {SIGNAL_TIMEOUT:?} (signaux reçus : {seen:?})")
}

/// Vérifie qu'aucun signal satisfaisant le prédicat n'arrive pendant la période d'observation.
pub fn assert_no_signal<T, F>(rx: &Receiver<T>, what: &str, predicate: F)
where
    T: Debug,
    F: Fn(&T) -> bool,
{
    let deadline = Instant::now() + QUIET_PERIOD;
    while Instant::now() < deadline {
        match rx.recv_timeout(Duration::from_millis(50)) {
            Ok(signal) => assert!(
                !predicate(&signal),
                "signal interdit reçu ({what}) : {signal:?}"
            ),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
    }
}

/// Attend plusieurs signaux distincts, **dans un ordre quelconque**.
///
/// Indispensable : rien ne garantit l'ordre relatif des signaux de deux dépôts
/// différents, et attendre le second après le premier perdrait celui reçu en avance.
pub fn wait_for_all<T, F>(rx: &Receiver<T>, what: &str, predicates: Vec<(String, F)>)
where
    T: Debug,
    F: Fn(&T) -> bool,
{
    let mut remaining = predicates;
    let deadline = Instant::now() + SIGNAL_TIMEOUT;

    while !remaining.is_empty() && Instant::now() < deadline {
        match rx.recv_timeout(Duration::from_millis(50)) {
            Ok(signal) => {
                if let Some(index) = remaining.iter().position(|(_, p)| p(&signal)) {
                    let _ = remaining.remove(index);
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                panic!("canal fermé avant réception de : {what}")
            }
        }
    }

    let missing: Vec<&String> = remaining.iter().map(|(label, _)| label).collect();
    assert!(
        missing.is_empty(),
        "{what} — signaux manquants : {missing:?}"
    );
}

/// Attend la découverte d'un dépôt précis.
pub fn wait_for_discovery(rx: &Receiver<DevSignal>, repo: &Path) {
    let _ = wait_for(
        rx,
        &format!("RepoDiscovered({})", repo.display()),
        |signal| matches!(signal, DevSignal::RepoDiscovered { path, .. } if path == repo),
    );
}

/// Attend la découverte de plusieurs dépôts, dans un ordre quelconque.
pub fn wait_for_discoveries(rx: &Receiver<DevSignal>, repos: &[&Path]) {
    let predicates: Vec<(String, _)> = repos
        .iter()
        .map(|repo| {
            let expected = repo.to_path_buf();
            (
                format!("RepoDiscovered({})", repo.display()),
                move |signal: &DevSignal| {
                    matches!(signal, DevSignal::RepoDiscovered { path, .. } if *path == expected)
                },
            )
        })
        .collect();
    wait_for_all(rx, "découverte des dépôts", predicates);
}
