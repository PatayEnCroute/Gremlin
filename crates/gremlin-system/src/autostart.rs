//! Gestion de l'enregistrement du lancement au démarrage du système hôte (Autostart).
//!
//! Ce module n'est qu'une façade : le mécanisme natif (registre Windows,
//! `LaunchAgent` macOS, entrée `.desktop` XDG) vit derrière la couture
//! [`crate::platform::AutostartBackend`], au même titre que le mode
//! click-through. Cela rend chaque backend testable indépendamment de l'OS
//! d'exécution.

use crate::error::SystemError;
use crate::platform::{default_autostart_backend, AutostartTarget, BoxedAutostartBackend};
use std::path::{Path, PathBuf};

/// Gestionnaire d'activation et désactivation du démarrage automatique avec l'OS.
pub struct AutostartManager {
    target: AutostartTarget,
    backend: BoxedAutostartBackend,
}

impl AutostartManager {
    /// Crée un nouveau gestionnaire d'autostart pour l'application courante.
    ///
    /// Le backend natif est choisi selon l'OS de compilation.
    ///
    /// # Errors
    /// * `SystemError::Io` si le chemin vers l'exécutable courant ne peut être résolu ;
    /// * `SystemError::PathResolutionFailed` si les répertoires standards de
    ///   l'utilisateur, nécessaires aux backends macOS et Linux, sont introuvables.
    pub fn new(app_name: impl Into<String>) -> Result<Self, SystemError> {
        let executable_path = std::env::current_exe()?;
        Ok(Self {
            target: AutostartTarget::new(app_name, executable_path),
            backend: default_autostart_backend()?,
        })
    }

    /// Crée un gestionnaire avec un backend et un exécutable explicites.
    ///
    /// Point d'injection destiné aux tests et aux déploiements portables : il
    /// permet d'enraciner les backends « fichier » ailleurs que dans le
    /// répertoire personnel réel de l'utilisateur.
    #[must_use]
    pub fn with_backend(
        app_name: impl Into<String>,
        executable_path: PathBuf,
        backend: BoxedAutostartBackend,
    ) -> Self {
        Self {
            target: AutostartTarget::new(app_name, executable_path),
            backend,
        }
    }

    /// Nom de l'application tel qu'enregistré auprès de l'OS.
    #[must_use]
    pub fn app_name(&self) -> &str {
        self.target.app_name()
    }

    /// Chemin de l'exécutable lancé au démarrage.
    #[must_use]
    pub fn executable_path(&self) -> &Path {
        self.target.executable_path()
    }

    /// Indique si l'autostart est actuellement activé pour l'application.
    #[must_use]
    pub fn is_enabled(&self) -> bool {
        self.backend.is_enabled(&self.target)
    }

    /// Active le démarrage automatique au boot de l'OS.
    ///
    /// # Errors
    /// Renvoie `SystemError` en cas d'échec d'écriture dans le registre ou le
    /// système de fichiers, ou `SystemError::AutostartUnsupported` si la
    /// plateforme ne propose aucun mécanisme connu.
    pub fn enable(&self) -> Result<(), SystemError> {
        self.backend.enable(&self.target)
    }

    /// Désactive le démarrage automatique.
    ///
    /// L'opération est idempotente : désactiver une application déjà
    /// désenregistrée est un succès.
    ///
    /// # Errors
    /// Renvoie `SystemError` si la suppression échoue réellement (droits
    /// insuffisants, valeur de registre verrouillée, …).
    pub fn disable(&self) -> Result<(), SystemError> {
        self.backend.disable(&self.target)
    }
}

impl std::fmt::Debug for AutostartManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AutostartManager")
            .field("target", &self.target)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use crate::platform::{LaunchAgentBackend, UnsupportedBackend, XdgAutostartBackend};
    use crate::test_support::TempDir;

    #[test]
    fn test_autostart_manager_targets_the_current_executable() {
        let manager = AutostartManager::new("GremlinTest").expect("AutostartManager valide");
        let current_exe = std::env::current_exe().expect("Exécutable courant résolu");

        assert_eq!(manager.app_name(), "GremlinTest");
        assert_eq!(manager.executable_path(), current_exe.as_path());
        assert!(
            manager.executable_path().is_file(),
            "le binaire de test doit exister sur le disque : {}",
            manager.executable_path().display()
        );
    }

    #[test]
    fn test_manager_delegates_to_the_launch_agent_backend() {
        let dir = TempDir::new("manager_launch_agent");
        let backend = LaunchAgentBackend::from_home(dir.path());
        let plist = dir
            .path()
            .join("Library")
            .join("LaunchAgents")
            .join("com.gremlin.desktop.plist");

        let manager = AutostartManager::with_backend(
            "Gremlin",
            PathBuf::from("/opt/gremlin"),
            Box::new(backend),
        );

        assert!(!manager.is_enabled());
        manager.enable().expect("Activation réussie");
        assert!(manager.is_enabled());
        assert!(plist.is_file());

        manager.disable().expect("Désactivation réussie");
        assert!(!manager.is_enabled());
        assert!(!plist.exists());
    }

    #[test]
    fn test_manager_delegates_to_the_xdg_backend() {
        let dir = TempDir::new("manager_xdg");
        let backend = XdgAutostartBackend::from_config_dir(dir.path());
        let entry = dir.path().join("autostart").join("gremlin.desktop");

        let manager = AutostartManager::with_backend(
            "Gremlin",
            PathBuf::from("/usr/bin/gremlin"),
            Box::new(backend),
        );

        assert!(!manager.is_enabled());
        manager.enable().expect("Activation réussie");
        assert!(manager.is_enabled());

        let content = std::fs::read_to_string(&entry).expect("Lecture de l'entrée .desktop");
        assert!(content.contains(r#"Exec="/usr/bin/gremlin""#));

        manager.disable().expect("Désactivation réussie");
        assert!(!manager.is_enabled());
        assert!(!entry.exists());
    }

    #[test]
    fn test_manager_surfaces_unsupported_platforms() {
        let manager = AutostartManager::with_backend(
            "Gremlin",
            PathBuf::from("/opt/gremlin"),
            Box::new(UnsupportedBackend),
        );

        assert!(!manager.is_enabled());
        assert!(matches!(
            manager.enable(),
            Err(SystemError::AutostartUnsupported)
        ));
    }
}
