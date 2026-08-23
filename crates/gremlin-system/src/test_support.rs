//! Utilitaires partagés par les tests unitaires de la caisse.
//!
//! Fournit un répertoire temporaire **unique par test** (et par exécution) afin
//! d'éviter les collisions entre tests parallèles, entre exécutions
//! concurrentes de `cargo test` et entre plusieurs postes partageant un
//! `TMPDIR` réseau.

#![allow(clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// Compteur garantissant l'unicité entre deux appels du même processus.
static SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Répertoire temporaire supprimé automatiquement à la fin du test.
pub struct TempDir {
    path: PathBuf,
}

impl TempDir {
    /// Crée un répertoire temporaire dédié, préfixé par `label`.
    ///
    /// # Panics
    /// Panique si le répertoire ne peut pas être créé : un test ne peut pas
    /// s'exécuter sans son bac à sable.
    pub fn new(label: &str) -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);

        let path = std::env::temp_dir().join(format!(
            "gremlin-system-{label}-{}-{sequence}-{nanos}",
            std::process::id()
        ));

        std::fs::create_dir_all(&path).expect("création du répertoire temporaire de test");
        Self { path }
    }

    /// Chemin racine du bac à sable.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Raccourci pour construire un chemin enfant.
    pub fn join(&self, child: &str) -> PathBuf {
        self.path.join(child)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        // Nettoyage au mieux : un échec ne doit pas masquer l'échec du test lui-même.
        let _ = std::fs::remove_dir_all(&self.path);
    }
}
