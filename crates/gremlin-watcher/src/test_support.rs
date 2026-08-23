//! Utilitaires partagés par les tests unitaires du crate.
//!
//! Chaque test dispose d'un répertoire temporaire **unique** (compteur atomique +
//! identifiant de processus) nettoyé à la fin du test : aucune collision possible
//! entre exécutions parallèles ou après une exécution interrompue.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Répertoire temporaire unique supprimé automatiquement en fin de test.
pub struct TempDirGuard {
    path: PathBuf,
}

impl TempDirGuard {
    /// Crée un répertoire temporaire unique préfixé par `prefix`.
    pub fn new(prefix: &str) -> Self {
        let unique = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
        let pid = std::process::id();
        let path = std::env::temp_dir().join(format!("gremlin_test_{prefix}_{pid}_{unique}"));
        let _ = std::fs::remove_dir_all(&path);
        create_dir(&path);
        Self {
            path: crate::git_path::normalize_path(&path),
        }
    }

    /// Chemin racine du répertoire temporaire (normalisé).
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Crée (si nécessaire) et renvoie un sous-répertoire.
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

/// Crée un répertoire et toute son arborescence parente, en échouant bruyamment.
pub fn create_dir(path: &Path) {
    if let Err(e) = std::fs::create_dir_all(path) {
        panic!("préparation du test impossible ({}) : {e}", path.display());
    }
}

/// Écrit un fichier de test en créant son arborescence parente.
///
/// Toute erreur de préparation fait échouer le test immédiatement, au lieu de se
/// manifester plus tard sous la forme d'une assertion trompeuse.
pub fn write_file(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        create_dir(parent);
    }
    if let Err(e) = std::fs::write(path, content) {
        panic!("écriture de test impossible ({}) : {e}", path.display());
    }
}
