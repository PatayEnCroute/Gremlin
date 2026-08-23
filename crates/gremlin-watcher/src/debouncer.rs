//! Système de debouncing temporel et d'agrégation d'événements pour les dépôts Git.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Débounceur temporel agrégeant les rafales d'événements de fichiers au niveau d'un dépôt.
#[derive(Debug)]
pub struct EventDebouncer {
    debounce_duration: Duration,
    pending_repos: HashMap<PathBuf, Instant>,
    last_known_shas: HashMap<PathBuf, String>,
    last_known_branches: HashMap<PathBuf, String>,
}

impl EventDebouncer {
    /// Crée un nouveau débounceur avec la durée spécifiée (par défaut 200 ms).
    #[must_use]
    pub fn new(debounce_duration: Duration) -> Self {
        Self {
            debounce_duration,
            pending_repos: HashMap::new(),
            last_known_shas: HashMap::new(),
            last_known_branches: HashMap::new(),
        }
    }

    /// Enregistre une activité sur un dépôt Git donné en réinitialisant son délai d'agrégation.
    pub fn record_repo_activity(&mut self, repo_root: PathBuf) {
        let _ = self.pending_repos.insert(repo_root, Instant::now());
    }

    /// Récupère et vide la liste des dépôts dont le délai de debounce est écoulé.
    pub fn poll_ready(&mut self) -> Vec<PathBuf> {
        let now = Instant::now();
        let mut ready = Vec::new();

        self.pending_repos.retain(|repo, last_seen| {
            if now.duration_since(*last_seen) >= self.debounce_duration {
                ready.push(repo.clone());
                false
            } else {
                true
            }
        });

        ready
    }

    /// Délai restant avant que le prochain dépôt en attente ne soit prêt.
    ///
    /// Renvoie `None` si aucun dépôt n'est en attente : le worker peut alors se
    /// mettre en sommeil sans échéance de réveil liée au debouncing.
    #[must_use]
    pub fn time_until_next_ready(&self) -> Option<Duration> {
        let now = Instant::now();
        self.pending_repos
            .values()
            .map(|last_seen| {
                self.debounce_duration
                    .saturating_sub(now.duration_since(*last_seen))
            })
            .min()
    }

    /// Indique si un dépôt attend actuellement sa stabilisation.
    #[must_use]
    pub fn is_pending(&self, repo: &Path) -> bool {
        self.pending_repos.contains_key(repo)
    }

    /// Vérifie si le commit SHA pour un dépôt a changé et met à jour le cache si c'est le cas.
    /// Renvoie `true` si le SHA est nouveau ou différent.
    pub fn update_commit_sha_if_changed(&mut self, repo: &Path, new_sha: &str) -> bool {
        let entry = self.last_known_shas.get(repo);
        if entry.is_none_or(|old| old != new_sha) {
            let _ = self
                .last_known_shas
                .insert(repo.to_path_buf(), new_sha.to_string());
            true
        } else {
            false
        }
    }

    /// Met à jour la branche active mémorisée et renvoie l'ancienne branche si elle a changé.
    pub fn update_branch_if_changed(&mut self, repo: &Path, new_branch: &str) -> Option<String> {
        let old_branch = self.last_known_branches.get(repo).cloned();
        let _ = self
            .last_known_branches
            .insert(repo.to_path_buf(), new_branch.to_string());

        match old_branch {
            Some(old) if old != new_branch => Some(old),
            _ => None,
        }
    }

    /// Supprime un dépôt de la mémoire interne lors d'une désinscription.
    pub fn remove_repo(&mut self, repo: &Path) {
        let _ = self.pending_repos.remove(repo);
        let _ = self.last_known_shas.remove(repo);
        let _ = self.last_known_branches.remove(repo);
    }
}

#[cfg(test)]
mod tests {
    use super::EventDebouncer;
    use std::path::PathBuf;
    use std::thread::sleep;
    use std::time::Duration;

    fn repo(name: &str) -> PathBuf {
        PathBuf::from("/path/to").join(name)
    }

    #[test]
    fn test_debouncer_aggregation_single_repo_burst() {
        let mut debouncer = EventDebouncer::new(Duration::from_millis(60));
        let repo = repo("my_project");

        // Simuler plusieurs écritures rapprochées
        debouncer.record_repo_activity(repo.clone());
        debouncer.record_repo_activity(repo.clone());
        debouncer.record_repo_activity(repo.clone());

        assert!(debouncer.poll_ready().is_empty());

        sleep(Duration::from_millis(80));

        let ready = debouncer.poll_ready();
        assert_eq!(ready.len(), 1);
        assert_eq!(ready.first(), Some(&repo));
        assert!(debouncer.poll_ready().is_empty());
    }

    #[test]
    fn test_activity_resets_the_window() {
        let mut debouncer = EventDebouncer::new(Duration::from_millis(120));
        let repo = repo("reset_window");

        debouncer.record_repo_activity(repo.clone());
        sleep(Duration::from_millis(70));
        // Nouvelle activité avant l'échéance : le compte à rebours repart de zéro.
        debouncer.record_repo_activity(repo.clone());
        assert!(
            debouncer.poll_ready().is_empty(),
            "le dépôt ne doit pas être prêt tant que l'activité continue"
        );
        assert!(debouncer.is_pending(&repo));

        sleep(Duration::from_millis(150));
        assert_eq!(debouncer.poll_ready(), vec![repo]);
    }

    #[test]
    fn test_multiple_repos_are_tracked_independently() {
        let mut debouncer = EventDebouncer::new(Duration::from_millis(60));
        let first = repo("alpha");
        let second = repo("beta");

        debouncer.record_repo_activity(first.clone());
        sleep(Duration::from_millis(80));
        debouncer.record_repo_activity(second.clone());

        let ready = debouncer.poll_ready();
        assert_eq!(ready, vec![first], "seul le dépôt stabilisé sort");
        assert!(debouncer.is_pending(&second));

        sleep(Duration::from_millis(80));
        assert_eq!(debouncer.poll_ready(), vec![second]);
    }

    #[test]
    fn test_time_until_next_ready() {
        let mut debouncer = EventDebouncer::new(Duration::from_millis(200));
        assert_eq!(debouncer.time_until_next_ready(), None);

        debouncer.record_repo_activity(repo("deadline"));
        let remaining = debouncer.time_until_next_ready();
        assert!(remaining.is_some_and(|d| d <= Duration::from_millis(200)));

        sleep(Duration::from_millis(220));
        assert_eq!(
            debouncer.time_until_next_ready(),
            Some(Duration::from_millis(0))
        );
    }

    #[test]
    fn test_sha_deduplication() {
        let mut debouncer = EventDebouncer::new(Duration::from_millis(50));
        let repo = repo("repo");

        assert!(debouncer.update_commit_sha_if_changed(&repo, "sha_1"));
        assert!(!debouncer.update_commit_sha_if_changed(&repo, "sha_1"));
        assert!(debouncer.update_commit_sha_if_changed(&repo, "sha_2"));
    }

    #[test]
    fn test_branch_change_detection() {
        let mut debouncer = EventDebouncer::new(Duration::from_millis(50));
        let repo = repo("repo");

        // Premier enregistrement de branche -> renvoie None (état initial)
        assert_eq!(debouncer.update_branch_if_changed(&repo, "main"), None);
        // Même branche -> None
        assert_eq!(debouncer.update_branch_if_changed(&repo, "main"), None);
        // Changement vers develop -> Some("main")
        assert_eq!(
            debouncer.update_branch_if_changed(&repo, "develop"),
            Some("main".to_string())
        );
    }

    #[test]
    fn test_remove_repo_clears_all_state() {
        let mut debouncer = EventDebouncer::new(Duration::from_millis(50));
        let repo = repo("gone");

        debouncer.record_repo_activity(repo.clone());
        assert!(debouncer.update_commit_sha_if_changed(&repo, "sha_1"));
        assert_eq!(debouncer.update_branch_if_changed(&repo, "main"), None);

        debouncer.remove_repo(&repo);

        assert!(!debouncer.is_pending(&repo));
        // L'état mémorisé a bien disparu : tout est de nouveau "inédit".
        assert!(debouncer.update_commit_sha_if_changed(&repo, "sha_1"));
        assert_eq!(debouncer.update_branch_if_changed(&repo, "main"), None);
    }
}
