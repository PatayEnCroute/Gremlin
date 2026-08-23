//! Backends de démarrage automatique, un par mécanisme natif.
//!
//! Comme pour le mode click-through, la variation par OS est isolée derrière un
//! trait ([`AutostartBackend`]) plutôt que dispersée en `#[cfg]` au milieu de la
//! logique métier. Les backends « fichier » (`LaunchAgent` macOS, entrée XDG
//! Linux) sont compilés sur **toutes** les plateformes : ce ne sont que des
//! écritures de fichiers, ce qui les rend testables partout. Seule la
//! *sélection* du backend par défaut est conditionnée à l'OS cible.

mod launch_agent;
mod unsupported;
mod xdg;

#[cfg(target_os = "windows")]
mod registry;

pub use launch_agent::LaunchAgentBackend;
pub use unsupported::UnsupportedBackend;
pub use xdg::XdgAutostartBackend;

#[cfg(target_os = "windows")]
pub use registry::RegistryRunBackend;

use crate::error::SystemError;
use std::path::{Path, PathBuf};

/// Application à enregistrer auprès du mécanisme de démarrage automatique.
///
/// Sépare *quoi* enregistrer (cette structure) de *comment* l'enregistrer
/// (le [`AutostartBackend`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutostartTarget {
    app_name: String,
    executable_path: PathBuf,
}

impl AutostartTarget {
    /// Construit une cible d'autostart.
    #[must_use]
    pub fn new(app_name: impl Into<String>, executable_path: PathBuf) -> Self {
        Self {
            app_name: app_name.into(),
            executable_path,
        }
    }

    /// Nom lisible de l'application (utilisé tel quel dans les libellés).
    #[must_use]
    pub fn app_name(&self) -> &str {
        &self.app_name
    }

    /// Chemin absolu de l'exécutable à lancer au démarrage.
    #[must_use]
    pub fn executable_path(&self) -> &Path {
        &self.executable_path
    }

    /// Fragment de nom de fichier sûr dérivé du nom de l'application.
    ///
    /// Tout caractère hors `[a-z0-9_-]` est remplacé par `-` afin qu'un nom
    /// d'application fantaisiste (`../../evil`) ne puisse pas s'échapper du
    /// répertoire d'autostart.
    #[must_use]
    pub fn file_slug(&self) -> String {
        let slug: String = self
            .app_name
            .to_lowercase()
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '-'
                }
            })
            .collect();

        if slug.is_empty() {
            String::from("application")
        } else {
            slug
        }
    }

    /// Identifiant inverse-DNS attendu par `launchd` (`com.gremlin.desktop`).
    #[must_use]
    pub fn reverse_dns_label(&self) -> String {
        format!("com.{}.desktop", self.file_slug())
    }

    /// Chemin de l'exécutable en texte (conversion permissive pour les
    /// chemins non-UTF-8, rares mais légaux sur Unix comme sur Windows).
    #[must_use]
    pub fn executable_string(&self) -> String {
        self.executable_path.to_string_lossy().into_owned()
    }
}

/// Mécanisme natif d'enregistrement au démarrage de la session.
pub trait AutostartBackend {
    /// Indique si l'application est actuellement enregistrée.
    fn is_enabled(&self, target: &AutostartTarget) -> bool;

    /// Enregistre l'application au démarrage.
    ///
    /// # Errors
    /// Renvoie `SystemError` si l'écriture (registre ou système de fichiers)
    /// échoue, ou `SystemError::AutostartUnsupported` si la plateforme ne
    /// propose aucun mécanisme connu.
    fn enable(&self, target: &AutostartTarget) -> Result<(), SystemError>;

    /// Retire l'enregistrement au démarrage.
    ///
    /// # Errors
    /// Renvoie `SystemError` si la suppression échoue. Une application déjà
    /// non enregistrée est un succès (opération idempotente).
    fn disable(&self, target: &AutostartTarget) -> Result<(), SystemError>;
}

/// Alias du type de backend stocké par le gestionnaire public.
///
/// `Send + Sync` préserve les traits automatiques historiques de
/// `AutostartManager`, qui n'était qu'un couple `String` / `PathBuf`.
pub type BoxedAutostartBackend = Box<dyn AutostartBackend + Send + Sync>;

/// Sélectionne le backend natif correspondant à l'OS de compilation.
///
/// # Errors
/// Renvoie `SystemError::PathResolutionFailed` si les répertoires standards de
/// l'utilisateur (`$HOME`, `$XDG_CONFIG_HOME`) ne peuvent pas être résolus.
#[cfg(target_os = "windows")]
pub fn default_autostart_backend() -> Result<BoxedAutostartBackend, SystemError> {
    Ok(Box::new(RegistryRunBackend))
}

/// Sélectionne le backend natif correspondant à l'OS de compilation.
///
/// # Errors
/// Renvoie `SystemError::PathResolutionFailed` si le répertoire personnel de
/// l'utilisateur ne peut pas être résolu.
#[cfg(target_os = "macos")]
pub fn default_autostart_backend() -> Result<BoxedAutostartBackend, SystemError> {
    let dirs = directories::BaseDirs::new().ok_or(SystemError::PathResolutionFailed)?;
    Ok(Box::new(LaunchAgentBackend::from_home(dirs.home_dir())))
}

/// Sélectionne le backend natif correspondant à l'OS de compilation.
///
/// # Errors
/// Renvoie `SystemError::PathResolutionFailed` si le répertoire de
/// configuration XDG ne peut pas être résolu.
#[cfg(target_os = "linux")]
pub fn default_autostart_backend() -> Result<BoxedAutostartBackend, SystemError> {
    let dirs = directories::BaseDirs::new().ok_or(SystemError::PathResolutionFailed)?;
    Ok(Box::new(XdgAutostartBackend::from_config_dir(
        dirs.config_dir(),
    )))
}

/// Sélectionne le backend natif correspondant à l'OS de compilation.
///
/// # Errors
/// N'échoue jamais sur les plateformes non supportées : le backend inerte est
/// toujours disponible et signalera l'absence de support à l'appel d'`enable`.
#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
pub fn default_autostart_backend() -> Result<BoxedAutostartBackend, SystemError> {
    Ok(Box::new(UnsupportedBackend))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(name: &str) -> AutostartTarget {
        AutostartTarget::new(name, PathBuf::from("/usr/local/bin/gremlin"))
    }

    #[test]
    fn test_file_slug_is_lowercased() {
        assert_eq!(target("Gremlin").file_slug(), "gremlin");
        assert_eq!(target("Gremlin").reverse_dns_label(), "com.gremlin.desktop");
    }

    #[test]
    fn test_file_slug_neutralises_path_traversal() {
        let slug = target("../../etc/cron.d/evil").file_slug();
        assert!(
            !slug.contains('/') && !slug.contains('.') && !slug.contains('\\'),
            "le fragment de nom de fichier doit être inoffensif, obtenu : {slug}"
        );
        assert_eq!(slug, "------etc-cron-d-evil");
    }

    #[test]
    fn test_file_slug_falls_back_when_name_is_empty() {
        assert_eq!(target("").file_slug(), "application");
    }
}
