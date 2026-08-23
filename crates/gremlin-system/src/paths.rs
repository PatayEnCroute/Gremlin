//! Résolution des chemins de fichiers standards multi-OS (XDG / `AppData` / Library).

use crate::error::SystemError;
use directories::ProjectDirs;
use std::fs;
use std::path::{Path, PathBuf};

const QUALIFIER: &str = "com";
const ORGANIZATION: &str = "Gremlin";
const APPLICATION: &str = "Gremlin";

/// Nom du fichier de sauvegarde d'état.
const SAVE_FILE_NAME: &str = "save.json";

/// Nom du sous-répertoire contenant les skins installés.
const SKINS_DIR_NAME: &str = "skins";

/// Gestionnaire des chemins de configuration, de cache et de données de Gremlin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppPaths {
    config: PathBuf,
    data: PathBuf,
    cache: PathBuf,
}

impl AppPaths {
    /// Initialise les chemins standards pour le système courant.
    ///
    /// # Errors
    /// Renvoie `SystemError::PathResolutionFailed` si les répertoires OS ne peuvent pas être déterminés.
    pub fn new() -> Result<Self, SystemError> {
        let proj_dirs = ProjectDirs::from(QUALIFIER, ORGANIZATION, APPLICATION)
            .ok_or(SystemError::PathResolutionFailed)?;

        Ok(Self {
            config: proj_dirs.config_dir().to_path_buf(),
            data: proj_dirs.data_dir().to_path_buf(),
            cache: proj_dirs.cache_dir().to_path_buf(),
        })
    }

    /// Construit un jeu de chemins explicite.
    ///
    /// Utile pour les tests et les déploiements portables (clé USB, bac à
    /// sable) où l'arborescence ne doit pas dépendre du profil utilisateur.
    #[must_use]
    pub fn from_dirs(config: PathBuf, data: PathBuf, cache: PathBuf) -> Self {
        Self {
            config,
            data,
            cache,
        }
    }

    /// Répertoire de configuration (`~/.config/gremlin` ou `%APPDATA%\Gremlin\config`).
    #[must_use]
    pub fn config_dir(&self) -> &Path {
        &self.config
    }

    /// Répertoire des données applicatives (sauvegardes, mods installés).
    #[must_use]
    pub fn data_dir(&self) -> &Path {
        &self.data
    }

    /// Répertoire de cache (contenu régénérable, purgeable sans perte).
    #[must_use]
    pub fn cache_dir(&self) -> &Path {
        &self.cache
    }

    /// Répertoire des skins (`~/.config/gremlin/skins`).
    #[must_use]
    pub fn skins_dir(&self) -> PathBuf {
        self.config.join(SKINS_DIR_NAME)
    }

    /// Chemin du fichier de sauvegarde d'état (`save.json`).
    #[must_use]
    pub fn save_file(&self) -> PathBuf {
        self.data.join(SAVE_FILE_NAME)
    }

    /// S'assure que tous les dossiers nécessaires existent sur le disque.
    ///
    /// # Errors
    /// Renvoie `SystemError::Io` si la création des dossiers échoue (droits
    /// insuffisants, ou chemin déjà occupé par un fichier régulier).
    pub fn ensure_directories_exist(&self) -> Result<(), SystemError> {
        fs::create_dir_all(&self.config)?;
        fs::create_dir_all(&self.data)?;
        fs::create_dir_all(&self.cache)?;
        fs::create_dir_all(self.skins_dir())?;
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use crate::test_support::TempDir;

    fn sandboxed(dir: &TempDir) -> AppPaths {
        AppPaths::from_dirs(
            dir.path().join("config"),
            dir.path().join("data"),
            dir.path().join("cache"),
        )
    }

    #[test]
    fn test_accessors_return_the_injected_directories() {
        let dir = TempDir::new("paths_accessors");
        let paths = sandboxed(&dir);

        assert_eq!(paths.config_dir(), dir.path().join("config"));
        assert_eq!(paths.data_dir(), dir.path().join("data"));
        assert_eq!(paths.cache_dir(), dir.path().join("cache"));
    }

    #[test]
    fn test_save_file_lives_in_the_data_directory() {
        let dir = TempDir::new("paths_save_file");
        let paths = sandboxed(&dir);
        let save = paths.save_file();

        assert_eq!(save.parent(), Some(paths.data_dir()));
        assert_eq!(save.file_name().and_then(|n| n.to_str()), Some("save.json"));
        assert_eq!(
            save.extension().and_then(|e| e.to_str()),
            Some("json"),
            "l'extension conditionne la désérialisation côté application"
        );
    }

    #[test]
    fn test_skins_dir_lives_in_the_config_directory() {
        let dir = TempDir::new("paths_skins_dir");
        let paths = sandboxed(&dir);
        let skins = paths.skins_dir();

        assert_eq!(skins.parent(), Some(paths.config_dir()));
        assert_eq!(skins.file_name().and_then(|n| n.to_str()), Some("skins"));
        assert_ne!(
            skins,
            paths.data_dir().join("skins"),
            "les skins sont de la configuration, pas des données générées"
        );
    }

    #[test]
    fn test_ensure_directories_exist_creates_every_directory() {
        let dir = TempDir::new("paths_ensure");
        let paths = sandboxed(&dir);

        assert!(!paths.config_dir().exists());
        paths
            .ensure_directories_exist()
            .expect("Création de l'arborescence");

        assert!(paths.config_dir().is_dir());
        assert!(paths.data_dir().is_dir());
        assert!(paths.cache_dir().is_dir());
        assert!(paths.skins_dir().is_dir());
    }

    #[test]
    fn test_ensure_directories_exist_is_idempotent() {
        let dir = TempDir::new("paths_ensure_twice");
        let paths = sandboxed(&dir);
        let marker = paths.skins_dir().join("mon_skin.json");

        paths
            .ensure_directories_exist()
            .expect("Première création réussie");
        fs::write(&marker, b"{}").expect("Écriture du marqueur");

        paths
            .ensure_directories_exist()
            .expect("Seconde création réussie");
        assert!(
            marker.is_file(),
            "un second appel ne doit rien détruire dans l'arborescence existante"
        );
    }

    #[test]
    fn test_ensure_directories_exist_reports_io_errors() {
        let dir = TempDir::new("paths_ensure_conflict");
        let blocker = dir.path().join("config");
        fs::write(&blocker, b"je suis un fichier").expect("Création du bloqueur");

        let paths = sandboxed(&dir);
        let result = paths.ensure_directories_exist();

        assert!(
            matches!(result, Err(SystemError::Io(_))),
            "un chemin occupé par un fichier doit remonter une erreur, obtenu : {result:?}"
        );
    }

    #[test]
    fn test_new_resolves_platform_directories_for_gremlin() {
        let paths = AppPaths::new().expect("Résolution des répertoires standards");

        assert!(paths.config_dir().is_absolute());
        assert!(paths.data_dir().is_absolute());
        assert!(paths.cache_dir().is_absolute());
        assert!(
            paths
                .config_dir()
                .to_string_lossy()
                .to_lowercase()
                .contains("gremlin"),
            "le répertoire de configuration doit être propre à Gremlin : {}",
            paths.config_dir().display()
        );
        assert_eq!(
            paths.save_file().file_name().and_then(|n| n.to_str()),
            Some("save.json")
        );
    }
}
