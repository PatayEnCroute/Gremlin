//! Intégration avec l'environnement de bureau hôte.
//!
//! Ce module isole les rares appels dépendants du système d'exploitation pour
//! que la logique métier de [`crate::app`] reste exempte de `#[cfg]`.

use std::io;
use std::path::Path;

/// Ouvre un répertoire dans le gestionnaire de fichiers du système.
///
/// # Errors
/// * `NotFound` si `path` ne désigne pas un répertoire existant.
/// * L'erreur d'entrée/sortie sous-jacente si le gestionnaire de fichiers ne
///   peut pas être lancé.
#[cfg(any(target_os = "windows", target_os = "macos", target_os = "linux"))]
pub fn open_directory(path: &Path) -> io::Result<()> {
    #[cfg(target_os = "windows")]
    const FILE_MANAGER: &str = "explorer";
    #[cfg(target_os = "macos")]
    const FILE_MANAGER: &str = "open";
    #[cfg(target_os = "linux")]
    const FILE_MANAGER: &str = "xdg-open";

    // Un dépôt découvert au démarrage peut avoir été déplacé ou supprimé depuis.
    // Sans cette vérification, `spawn` réussit — le binaire existe, lui — et
    // l'appelant conclut à un succès alors que rien ne s'est ouvert : le
    // gestionnaire de fichiers signale l'erreur dans son coin, ou pas du tout.
    if !path.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "le répertoire à ouvrir n'existe pas",
        ));
    }

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
        // paniquer, et sans lancer le moindre processus — sur les trois
        // systèmes, `spawn` aurait réussi et masqué l'échec.
        let result = open_directory(Path::new(""));
        assert!(
            matches!(&result, Err(error) if error.kind() == io::ErrorKind::NotFound),
            "un répertoire absent doit être signalé, obtenu : {result:?}"
        );
    }
}
