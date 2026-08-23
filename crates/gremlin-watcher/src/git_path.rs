//! Analyse partagée des chemins Git : normalisation et classification.
//!
//! Toute la logique de reconnaissance des chemins internes à un dépôt (`.git/HEAD`,
//! `.git/refs/heads/...`) est centralisée ici et raisonne sur les **composants** de
//! chemin, jamais sur des sous-chaînes : sous Windows `notify` livre
//! `refs\heads\main`, une comparaison textuelle avec `"refs/heads"` ne matcherait
//! jamais, et `contains(".git")` capturerait à tort `.github/` ou `.gitignore`.

use std::ffi::OsStr;
use std::path::{Component, Path, PathBuf};

/// Nom du répertoire de métadonnées d'un dépôt Git.
pub const GIT_DIR_NAME: &str = ".git";

/// Catégorie d'un chemin situé à l'intérieur (ou sur) un répertoire `.git`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitPathKind {
    /// Le répertoire `.git` lui-même (création ou suppression d'un dépôt).
    GitDir,
    /// Métadonnée pertinente : `HEAD`, `refs/heads/**`, `logs/HEAD`, `packed-refs`...
    Metadata,
    /// Contenu interne sans intérêt métier (objets, index, verrous transitoires...).
    Internal,
}

/// Découpe un chemin autour du premier composant `.git` rencontré.
///
/// Renvoie la racine du dépôt et les composants situés sous le répertoire `.git`.
fn split_at_git_dir(path: &Path) -> Option<(PathBuf, Vec<&OsStr>)> {
    let mut repo_root = PathBuf::new();
    let mut components = path.components();
    let mut found = false;

    for component in components.by_ref() {
        if matches!(component, Component::Normal(name) if name == OsStr::new(GIT_DIR_NAME)) {
            found = true;
            break;
        }
        repo_root.push(component.as_os_str());
    }

    if !found {
        return None;
    }

    let rest = components
        .filter_map(|c| match c {
            Component::Normal(name) => Some(name),
            _ => None,
        })
        .collect();

    Some((repo_root, rest))
}

/// Retrouve la racine du dépôt à partir d'un chemin pointant dans (ou sur) `.git`.
#[must_use]
pub fn find_repo_root(path: &Path) -> Option<PathBuf> {
    split_at_git_dir(path).map(|(root, _)| root)
}

/// Classe un chemin situé dans un répertoire `.git`.
///
/// Renvoie `None` si le chemin ne concerne aucun dépôt Git.
#[must_use]
pub fn classify_git_path(path: &Path) -> Option<GitPathKind> {
    let (_, rest) = split_at_git_dir(path)?;
    Some(classify_components(&rest))
}

/// Décompose un chemin en racine de dépôt et catégorie, en une seule passe.
#[must_use]
pub fn analyze_git_path(path: &Path) -> Option<(PathBuf, GitPathKind)> {
    let (root, rest) = split_at_git_dir(path)?;
    let kind = classify_components(&rest);
    Some((root, kind))
}

/// Indique si un chemin doit déclencher une relecture des métadonnées du dépôt.
#[must_use]
pub fn is_relevant_git_path(path: &Path) -> bool {
    matches!(
        classify_git_path(path),
        Some(GitPathKind::Metadata | GitPathKind::GitDir)
    )
}

/// Classe les composants relatifs au répertoire `.git`.
fn classify_components(rest: &[&OsStr]) -> GitPathKind {
    let Some(first) = rest.first() else {
        return GitPathKind::GitDir;
    };

    // Les fichiers de verrouillage transitoires (`HEAD.lock`, `main.lock`) sont du bruit.
    if rest
        .last()
        .is_some_and(|name| name.to_string_lossy().ends_with(".lock"))
    {
        return GitPathKind::Internal;
    }

    if rest.len() == 1 {
        let is_metadata = matches!(
            first.to_string_lossy().as_ref(),
            "HEAD" | "ORIG_HEAD" | "COMMIT_EDITMSG" | "packed-refs"
        );
        return if is_metadata {
            GitPathKind::Metadata
        } else {
            GitPathKind::Internal
        };
    }

    let second = rest.get(1).map(|c| c.to_string_lossy().into_owned());
    match (first.to_string_lossy().as_ref(), second.as_deref()) {
        // Références locales : `refs/heads/**` (y compris `feature/xxx`).
        ("refs", Some("heads")) => GitPathKind::Metadata,
        // Journal de HEAD : `logs/HEAD` et `logs/refs/heads/**`.
        ("logs", Some("HEAD")) if rest.len() == 2 => GitPathKind::Metadata,
        ("logs", Some("refs")) if rest.get(2).is_some_and(|c| *c == OsStr::new("heads")) => {
            GitPathKind::Metadata
        }
        _ => GitPathKind::Internal,
    }
}

/// Normalise un chemin pour qu'il serve de clé stable.
///
/// La canonisation aligne la casse et la forme des chemins fournis par l'appelant
/// avec ceux livrés par `notify` (indispensable sous Windows), et le préfixe
/// verbatim `\\?\` est retiré pour rester lisible et comparable côté appelant.
/// En cas d'échec (chemin inexistant, droits insuffisants) le chemin d'origine est
/// conservé tel quel.
#[must_use]
pub fn normalize_path(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).map_or_else(|_| path.to_path_buf(), |p| strip_verbatim_prefix(&p))
}

/// Retire le préfixe verbatim Windows (`\\?\`) d'un chemin canonisé.
#[must_use]
pub fn strip_verbatim_prefix(path: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        if let Some(text) = path.to_str() {
            if let Some(rest) = text.strip_prefix(r"\\?\") {
                // Les chemins UNC (`\\?\UNC\serveur\part`) doivent conserver leur préfixe.
                if !rest.starts_with("UNC\\") {
                    return PathBuf::from(rest);
                }
            }
        }
    }
    path.to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::{
        analyze_git_path, classify_git_path, find_repo_root, is_relevant_git_path, GitPathKind,
    };
    use std::path::{Path, PathBuf};

    fn repo() -> PathBuf {
        PathBuf::from(if cfg!(windows) {
            r"C:\dev\my_repo"
        } else {
            "/dev/my_repo"
        })
    }

    #[test]
    fn test_find_repo_root_from_nested_git_path() {
        let path = repo().join(".git").join("refs").join("heads").join("main");
        assert_eq!(find_repo_root(&path), Some(repo()));
    }

    #[test]
    fn test_git_dir_itself_is_detected() {
        let path = repo().join(".git");
        assert_eq!(classify_git_path(&path), Some(GitPathKind::GitDir));
        assert_eq!(find_repo_root(&path), Some(repo()));
    }

    #[test]
    fn test_metadata_paths_are_relevant() {
        let git = repo().join(".git");
        for relative in [
            vec!["HEAD"],
            vec!["ORIG_HEAD"],
            vec!["COMMIT_EDITMSG"],
            vec!["packed-refs"],
            vec!["refs", "heads", "main"],
            vec!["refs", "heads", "feature", "tamagotchi"],
            vec!["logs", "HEAD"],
            vec!["logs", "refs", "heads", "main"],
        ] {
            let mut path = git.clone();
            for part in &relative {
                path.push(part);
            }
            assert_eq!(
                classify_git_path(&path),
                Some(GitPathKind::Metadata),
                "{} doit être pertinent",
                path.display()
            );
        }
    }

    #[test]
    fn test_internal_paths_are_filtered() {
        let git = repo().join(".git");
        for relative in [
            vec!["index"],
            vec!["FETCH_HEAD"],
            vec!["objects", "ab", "cdef"],
            vec!["refs", "remotes", "origin", "main"],
            vec!["refs", "heads", "main.lock"],
            vec!["HEAD.lock"],
            vec!["logs", "refs", "remotes", "origin", "main"],
        ] {
            let mut path = git.clone();
            for part in &relative {
                path.push(part);
            }
            assert_eq!(
                classify_git_path(&path),
                Some(GitPathKind::Internal),
                "{} doit être filtré",
                path.display()
            );
        }
    }

    #[test]
    fn test_non_git_paths_are_ignored() {
        for path in [
            repo().join(".github").join("workflows").join("ci.yml"),
            repo().join(".gitignore"),
            repo().join(".gitlab-ci.yml"),
            repo().join("src").join("main.rs"),
        ] {
            assert_eq!(classify_git_path(&path), None, "{}", path.display());
            assert!(!is_relevant_git_path(&path));
            assert_eq!(find_repo_root(&path), None);
        }
    }

    #[test]
    fn test_analyze_returns_root_and_kind() {
        let path = repo().join(".git").join("logs").join("HEAD");
        assert_eq!(
            analyze_git_path(&path),
            Some((repo(), GitPathKind::Metadata))
        );
    }

    #[cfg(windows)]
    #[test]
    fn test_windows_backslash_separators_are_handled() {
        // Cas exact remonté par `notify` sous Windows.
        assert!(is_relevant_git_path(Path::new(
            r"C:\dev\my_repo\.git\refs\heads\main"
        )));
        assert!(is_relevant_git_path(Path::new(
            r"C:\dev\my_repo\.git\logs\HEAD"
        )));
        assert!(!is_relevant_git_path(Path::new(
            r"C:\dev\my_repo\.github\workflows\ci.yml"
        )));
        assert_eq!(
            find_repo_root(Path::new(r"C:\dev\my_repo\.git\refs\heads\main")),
            Some(PathBuf::from(r"C:\dev\my_repo"))
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn test_unix_separators_are_handled() {
        assert!(is_relevant_git_path(Path::new(
            "/dev/my_repo/.git/refs/heads/main"
        )));
        assert!(!is_relevant_git_path(Path::new("/dev/my_repo/.gitignore")));
    }
}
