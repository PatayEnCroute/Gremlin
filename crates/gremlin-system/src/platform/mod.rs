//! Coutures d'abstraction vers les capacités spécifiques au système d'exploitation.
//!
//! Deux coutures cohabitent, construites sur le même modèle (un trait + une
//! implémentation par mécanisme natif, jamais de `#[cfg]` dans la logique métier) :
//!
//! * [`PlatformWindowExt`] — capacités natives de fenêtre (mode click-through) ;
//! * [`AutostartBackend`] — enregistrement au démarrage de la session.

pub mod autostart;
mod click_through;

pub use autostart::{
    default_autostart_backend, AutostartBackend, AutostartTarget, BoxedAutostartBackend,
    LaunchAgentBackend, UnsupportedBackend, XdgAutostartBackend,
};
pub use click_through::PlatformImpl;

#[cfg(target_os = "windows")]
pub use autostart::RegistryRunBackend;

use crate::error::SystemError;
use winit::window::Window;

/// Trait d'extension de fenêtre pour les capacités natives OS (click-through).
pub trait PlatformWindowExt {
    /// Active ou désactive le mode *click-through* (la souris traverse la fenêtre sans interagir).
    ///
    /// # Errors
    /// * [`SystemError::ClickThroughUnsupported`] si le backend de fenêtrage ne
    ///   sait pas rendre une fenêtre traversante (iOS, Android, Web, Orbital,
    ///   et Wayland selon la configuration).
    /// * [`SystemError::WindowError`] si l'appel système sous-jacent échoue ou
    ///   est ignoré par le gestionnaire de fenêtres.
    ///
    /// Un succès (`Ok(())`) signifie toujours que l'état demandé a réellement
    /// été appliqué : l'appelant peut s'y fier pour cocher la case du menu.
    fn set_click_through(&self, window: &Window, enabled: bool) -> Result<(), SystemError>;
}
