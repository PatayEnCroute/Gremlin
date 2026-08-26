//! Intégration avec l'environnement de bureau hôte.
//!
//! Ce module isole les rares appels dépendants du système d'exploitation pour
//! que la logique métier de [`crate::app`] reste exempte de `#[cfg]`.

use std::io;
use std::path::{Path, PathBuf};

/// Indique si un sélecteur de dossier natif est utilisable sur cette plateforme.
///
/// L'interface s'en sert pour ne proposer l'entrée « Parcourir… » que là où elle
/// aboutit réellement : une commande qui échoue systématiquement vaut moins
/// qu'une commande absente, la saisie du chemin restant disponible partout.
#[must_use]
pub const fn folder_picker_available() -> bool {
    cfg!(all(
        feature = "folder_dialog",
        any(target_os = "windows", target_os = "linux")
    ))
}

/// Ouvre le sélecteur de dossier du système et renvoie le dossier choisi.
///
/// Renvoie `Ok(None)` si l'utilisateur a annulé.
///
/// # Contrainte d'exécution
///
/// **À n'appeler que depuis un fil dédié**, jamais depuis la boucle
/// d'événements : la boîte de dialogue est modale et bloque son appelant
/// jusqu'à la réponse de l'utilisateur. Le familier se figerait pendant tout ce
/// temps, et la boucle `winit` ne servirait plus aucun événement.
///
/// # Pourquoi macOS en est exclu
///
/// `NSOpenPanel` doit s'exécuter sur le fil principal. Appelé depuis un fil
/// secondaire, il est renvoyé vers la file principale et n'aboutit que si
/// celle-ci le sert — un interblocage y reste possible selon l'état de la
/// boucle. Plutôt que d'embarquer ce risque dans un démon résident, la
/// plateforme renvoie une erreur explicite et l'interface n'y propose pas
/// l'entrée : la saisie du chemin, elle, fonctionne partout.
///
/// # Errors
/// Renvoie `Unsupported` si la feature `folder_dialog` est absente ou si la
/// plateforme n'est pas prise en charge.
#[cfg(all(
    feature = "folder_dialog",
    any(target_os = "windows", target_os = "linux")
))]
pub fn pick_repository_folder() -> io::Result<Option<PathBuf>> {
    Ok(rfd::FileDialog::new()
        .set_title("Choisir un dépôt Git à surveiller")
        .pick_folder())
}

/// Ouvre le sélecteur de dossier du système et renvoie le dossier choisi.
///
/// # Errors
/// Renvoie systématiquement `Unsupported` : voir la variante prise en charge
/// pour le détail des raisons.
#[cfg(not(all(
    feature = "folder_dialog",
    any(target_os = "windows", target_os = "linux")
)))]
pub fn pick_repository_folder() -> io::Result<Option<PathBuf>> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "aucun sélecteur de dossier disponible : saisissez le chemin du dépôt",
    ))
}

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
    fn test_folder_picker_reports_its_own_unavailability() {
        // Jamais de faux succès : là où le sélecteur n'existe pas, l'appel
        // échoue explicitement au lieu de renvoyer « aucun dossier choisi »,
        // qui se confondrait avec une annulation de l'utilisateur.
        if !folder_picker_available() {
            let result = pick_repository_folder();
            assert!(
                matches!(&result, Err(error) if error.kind() == io::ErrorKind::Unsupported),
                "l'indisponibilité doit être signalée, obtenu : {result:?}"
            );
        }
    }

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
