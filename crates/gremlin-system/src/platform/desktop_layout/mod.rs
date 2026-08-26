//! Sources natives de la zone de travail, une par système.
//!
//! Chaque backend répond à une seule question : « pour ce moniteur, quelle
//! portion reste libre des barres système ? ». Les limites du moniteur lui sont
//! fournies — `winit` les connaît déjà sur les trois plateformes — ce qui réduit
//! la FFI au strict nécessaire.
//!
//! Les plateformes sans réponse possible renvoient une **erreur explicite**,
//! jamais un écran fabriqué : `gremlin-app` désactive alors le magnétisme et le
//! panneau l'annonce.

use crate::desktop_layout::WorkAreaSource;
use crate::error::SystemError;

#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "macos")]
mod macos;

#[cfg(target_os = "linux")]
mod x11;

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
mod unsupported;

/// Source de zone de travail de la plateforme courante.
///
/// # Errors
/// Renvoie [`SystemError::DesktopLayoutUnavailable`] lorsque la plateforme ne
/// permet pas d'interroger la topologie — Wayland au premier chef.
pub fn default_work_area_source() -> Result<Box<dyn WorkAreaSource + Send + Sync>, SystemError> {
    #[cfg(target_os = "windows")]
    {
        windows::source()
    }
    #[cfg(target_os = "macos")]
    {
        macos::source()
    }
    #[cfg(target_os = "linux")]
    {
        x11::source()
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        unsupported::source()
    }
}
