//! Découverte passive des dépôts Git sur le système de fichiers.

use crate::config::DEFAULT_MAX_SCAN_DEPTH;
use crate::git_path::GIT_DIR_NAME;
use directories::UserDirs;
use std::collections::HashSet;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use tracing::{debug, info, warn};
use walkdir::WalkDir;

/// Dossiers ignorés pour accélérer la recherche et éviter les boucles I/O.
///
/// La comparaison est insensible à la casse (`AppData` / `appdata`).
const IGNORED_DIRECTORIES: &[&str] = &[
    "node_modules",
    "target",
    ".cache",
    "vendor",
    ".cargo",
    "AppData",
    "Library",
    ".rustup",
    ".npm",
    ".nuget",
    "dist",
    "build",
    ".idea",
    ".vscode",
];

/// Dossiers dont le nom évoque un environnement virtuel Python.
///
/// Ils ne sont écartés que s'ils en sont réellement un (`pyvenv.cfg` présent) :
/// un dossier de sources légitimement nommé `env` doit rester scanné.
const VIRTUALENV_CANDIDATES: &[&str] = &["env", "venv", ".venv"];

/// Marqueur de présence d'un environnement virtuel Python.
const VIRTUALENV_MARKER: &str = "pyvenv.cfg";

/// Sous-dossiers conventionnels de développement recherchés dans le répertoire utilisateur.
const CANDIDATE_PROJECT_DIRS: &[&str] = &[
    "Projects",
    "Code",
    "dev",
    "src",
    "git",
    "_Git",
    "Workspace",
    "repos",
    "Development",
    "Documents/Projects",
    "Documents/Code",
    "Documents/GitHub",
];

/// Indique si un répertoire doit être écarté des parcours de découverte.
#[must_use]
pub fn is_ignored_directory(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };

    if VIRTUALENV_CANDIDATES
        .iter()
        .any(|candidate| candidate.eq_ignore_ascii_case(name))
    {
        return path.join(VIRTUALENV_MARKER).exists();
    }

    IGNORED_DIRECTORIES
        .iter()
        .any(|ignored| ignored.eq_ignore_ascii_case(name))
}

/// Scanner de répertoires à la recherche de dépôts Git.
pub struct GitScanner;

impl GitScanner {
    /// Détecte automatiquement les répertoires racines probables de développement selon l'OS.
    #[must_use]
    pub fn discover_default_roots() -> Vec<PathBuf> {
        let mut roots = Vec::new();

        if let Some(user_dirs) = UserDirs::new() {
            let home = user_dirs.home_dir();

            for candidate in CANDIDATE_PROJECT_DIRS {
                let candidate_path = home.join(candidate);
                if candidate_path.is_dir() {
                    roots.push(candidate_path);
                }
            }

            // Si aucun dossier conventionnel n'a été trouvé, ajouter le dossier personnel
            if roots.is_empty() {
                roots.push(home.to_path_buf());
            }
        }

        // Ajouter le répertoire de travail courant si pertinent
        if let Ok(current) = std::env::current_dir() {
            if !roots.contains(&current) {
                roots.push(current);
            }
        }

        roots
    }

    /// Explore récursivement une liste de dossiers racines avec la profondeur par défaut.
    #[must_use]
    pub fn scan_roots<P: AsRef<Path>>(roots: &[P]) -> Vec<PathBuf> {
        Self::scan_roots_with_depth(roots, DEFAULT_MAX_SCAN_DEPTH)
    }

    /// Explore récursivement une liste de dossiers racines jusqu'à une profondeur maximale donnée.
    ///
    /// Les dépôts sont dédupliqués puis triés par ordre alphabétique.
    #[must_use]
    pub fn scan_roots_with_depth<P: AsRef<Path>>(roots: &[P], max_depth: usize) -> Vec<PathBuf> {
        let never_cancelled = AtomicBool::new(false);
        let mut discovered = Vec::new();
        Self::scan_roots_cancellable(roots, max_depth, &never_cancelled, |repo| {
            discovered.push(repo.to_path_buf());
        });
        discovered.sort();
        discovered
    }

    /// Explore les racines en signalant chaque dépôt découvert au fil de l'eau.
    ///
    /// Le parcours s'interrompt dès que `cancelled` passe à `true`, ce qui permet
    /// d'abandonner immédiatement un scan profond lorsque le watcher est détruit.
    /// Les dépôts sont dédupliqués entre racines et renvoyés dans l'ordre de découverte.
    pub fn scan_roots_cancellable<P: AsRef<Path>, F: FnMut(&Path)>(
        roots: &[P],
        max_depth: usize,
        cancelled: &AtomicBool,
        mut on_repo: F,
    ) {
        let mut seen: HashSet<PathBuf> = HashSet::new();

        for root in roots {
            if cancelled.load(Ordering::Relaxed) {
                debug!("Scan de dépôts interrompu à la demande");
                return;
            }

            let root_path = root.as_ref();
            if !root_path.is_dir() {
                continue;
            }

            info!(root = %root_path.display(), max_depth, "Scan des dépôts Git");

            // Si la racine passée est elle-même un dépôt Git
            if Self::is_git_repo(root_path) && seen.insert(root_path.to_path_buf()) {
                on_repo(root_path);
            }

            let walker = WalkDir::new(root_path)
                .max_depth(max_depth)
                .follow_links(false)
                .into_iter()
                .filter_entry(|e| {
                    e.depth() == 0 || !(e.file_type().is_dir() && is_ignored_directory(e.path()))
                });

            for entry in walker {
                if cancelled.load(Ordering::Relaxed) {
                    debug!("Scan de dépôts interrompu à la demande");
                    return;
                }

                let entry = match entry {
                    Ok(entry) => entry,
                    Err(e) => {
                        // Droits insuffisants, lien cassé, chemin trop long... : le scan
                        // continue mais l'incident ne doit pas rester invisible.
                        warn!(path = ?e.path(), "Dossier ignoré pendant le scan Git : {e}");
                        continue;
                    }
                };

                if entry.file_type().is_dir() && entry.file_name() == OsStr::new(GIT_DIR_NAME) {
                    if let Some(parent) = entry.path().parent() {
                        let repo_dir = parent.to_path_buf();
                        if seen.insert(repo_dir.clone()) {
                            debug!(repo = %repo_dir.display(), "Dépôt Git découvert");
                            on_repo(&repo_dir);
                        }
                    }
                }
            }
        }
    }

    /// Vérifie si un chemin donné constitue la racine d'un dépôt Git valide.
    #[must_use]
    pub fn is_git_repo(path: &Path) -> bool {
        path.join(GIT_DIR_NAME).exists()
    }
}

#[cfg(test)]
mod tests {
    use super::{is_ignored_directory, GitScanner};
    use crate::test_support::{create_dir, write_file, TempDirGuard};
    use std::path::Path;
    use std::sync::atomic::{AtomicBool, Ordering};

    #[test]
    fn test_scan_roots_discovers_nested_git() {
        let guard = TempDirGuard::new("scanner");
        let repo_a = guard.child("project_a");
        let repo_b = guard.child("subfolder/project_b");
        let ignored_repo = guard.child("node_modules/ignored_project");

        create_dir(&repo_a.join(".git"));
        create_dir(&repo_b.join(".git"));
        create_dir(&ignored_repo.join(".git"));

        let found = GitScanner::scan_roots_with_depth(&[guard.path()], 4);

        assert!(found.contains(&repo_a));
        assert!(found.contains(&repo_b));
        assert!(!found.contains(&ignored_repo));
    }

    #[test]
    fn test_scan_root_that_is_itself_a_repo() {
        let guard = TempDirGuard::new("scanner_self");
        create_dir(&guard.path().join(".git"));

        let found = GitScanner::scan_roots_with_depth(&[guard.path()], 2);
        assert_eq!(found, vec![guard.path().to_path_buf()]);
    }

    #[test]
    fn test_scan_deduplicates_overlapping_roots() {
        let guard = TempDirGuard::new("scanner_dedup");
        let repo = guard.child("shared_project");
        create_dir(&repo.join(".git"));

        let found = GitScanner::scan_roots_with_depth(&[guard.path(), guard.path()], 3);
        assert_eq!(found, vec![repo]);
    }

    #[test]
    fn test_scan_respects_max_depth() {
        let guard = TempDirGuard::new("scanner_depth");
        let deep_repo = guard.child("a/b/c/deep_project");
        create_dir(&deep_repo.join(".git"));

        assert!(GitScanner::scan_roots_with_depth(&[guard.path()], 2).is_empty());
        assert!(GitScanner::scan_roots_with_depth(&[guard.path()], 6).contains(&deep_repo));
    }

    #[test]
    fn test_scan_is_cancellable() {
        let guard = TempDirGuard::new("scanner_cancel");
        create_dir(&guard.child("project").join(".git"));

        let cancelled = AtomicBool::new(true);
        let mut discovered = Vec::new();
        GitScanner::scan_roots_cancellable(&[guard.path()], 4, &cancelled, |repo| {
            discovered.push(repo.to_path_buf());
        });

        assert!(
            discovered.is_empty(),
            "un scan déjà annulé ne doit rien parcourir"
        );
        cancelled.store(false, Ordering::SeqCst);
    }

    #[test]
    fn test_missing_root_is_skipped() {
        let guard = TempDirGuard::new("scanner_missing");
        let missing = guard.path().join("does_not_exist");
        assert!(GitScanner::scan_roots_with_depth(&[&missing], 3).is_empty());
    }

    #[test]
    fn test_ignored_directories_are_case_insensitive() {
        let guard = TempDirGuard::new("scanner_case");
        assert!(is_ignored_directory(&guard.child("NODE_MODULES")));
        assert!(is_ignored_directory(&guard.child("Target")));
        assert!(is_ignored_directory(&guard.child("appdata")));
        assert!(!is_ignored_directory(&guard.child("src")));
    }

    #[test]
    fn test_env_directory_only_ignored_when_really_a_virtualenv() {
        let guard = TempDirGuard::new("scanner_venv");

        // Un dossier de sources légitimement nommé "env" doit rester scanné.
        let source_env = guard.child("env");
        assert!(!is_ignored_directory(&source_env));

        // Un vrai environnement virtuel est écarté.
        let real_venv = guard.child("venv");
        write_file(&real_venv.join("pyvenv.cfg"), "home = /usr\n");
        assert!(is_ignored_directory(&real_venv));

        // Vérification de bout en bout sur le scan.
        create_dir(&source_env.join("nested_repo").join(".git"));
        create_dir(&real_venv.join("hidden_repo").join(".git"));
        let found = GitScanner::scan_roots_with_depth(&[guard.path()], 4);
        assert!(found.contains(&source_env.join("nested_repo")));
        assert!(!found.contains(&real_venv.join("hidden_repo")));
    }

    #[test]
    fn test_is_git_repo() {
        let guard = TempDirGuard::new("is_repo");
        create_dir(&guard.path().join(".git"));

        assert!(GitScanner::is_git_repo(guard.path()));
        assert!(!GitScanner::is_git_repo(&guard.path().join("nonexistent")));
    }

    #[test]
    fn test_discover_default_roots_returns_existing_directories() {
        let roots = GitScanner::discover_default_roots();

        assert!(
            !roots.is_empty(),
            "au moins le répertoire courant doit être proposé"
        );
        for root in &roots {
            assert!(
                Path::new(root).is_dir(),
                "{} doit être un répertoire existant",
                root.display()
            );
        }

        // Le répertoire de travail courant fait toujours partie des racines.
        if let Ok(current) = std::env::current_dir() {
            assert!(roots.contains(&current));
        }

        // Pas de doublon.
        let mut sorted = roots.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), roots.len());
    }
}
