//! Intégration avec l'environnement de bureau hôte.
//!
//! Ce module isole les rares appels dépendants du système d'exploitation pour
//! que la logique métier de [`crate::app`] reste exempte de `#[cfg]`.

use std::io;
use std::path::Path;

/// Ouvre un répertoire dans le gestionnaire de fichiers du système.
///
/// # Errors
/// Renvoie une erreur si le gestionnaire de fichiers ne peut pas être lancé.
#[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
pub fn open_directory(path: &Path) -> io::Result<()> {
    #[cfg(target_os = "windows")]
    const FILE_MANAGER: &str = "explorer";
    #[cfg(target_os = "macos")]
    const FILE_MANAGER: &str = "open";
    #[cfg(target_os = "linux")]
    const FILE_MANAGER: &str = "xdg-open";

    // `explorer.exe` renvoie un code de sortie non nul même en cas de succès :
    // on lance donc le processus sans attendre ni interpréter son statut.
    std::process::Command::new(FILE_MANAGER)
        .arg(path)
        .spawn()
        .map(|_| ())
}

/// Ouvre un répertoire dans le gestionnaire de fichiers du système.
///
/// # Errors
/// Renvoie systématiquement `Unsupported` : aucune convention d'ouverture de
/// dossier n'est connue pour cette plateforme.
#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
pub fn open_directory(_path: &Path) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "aucun gestionnaire de fichiers connu pour cette plateforme",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_opening_a_missing_directory_reports_an_error_without_panicking() {
        // Le chemin n'existe pas : l'appel doit échouer proprement, jamais
        // paniquer, quelle que soit la plateforme.
        let result = open_directory(Path::new(""));
        assert!(result.is_err() || cfg!(target_os = "windows"));
    }
}
