//! Plateformes sans topologie de bureau interrogeable.
//!
//! Renvoie une erreur explicite plutôt qu'un écran synthétique : l'interface doit
//! désactiver le magnétisme et le dire, pas afficher un réglage sans effet.

use crate::desktop_layout::WorkAreaSource;
use crate::error::SystemError;

/// Refuse la source sur une plateforme non couverte.
///
/// # Errors
/// Renvoie toujours [`SystemError::DesktopLayoutUnavailable`].
pub(super) fn source() -> Result<Box<dyn WorkAreaSource + Send + Sync>, SystemError> {
    Err(SystemError::DesktopLayoutUnavailable(String::from(
        "cette plateforme n'expose pas la topologie de ses écrans",
    )))
}
