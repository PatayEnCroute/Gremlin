//! Mode *click-through* portable, bâti sur `winit::window::Window::set_cursor_hittest`.
//!
//! `winit` implémente déjà le test de collision du curseur nativement sur les
//! trois plateformes visées :
//! * **Windows** — `SetWindowLongPtrW(GWL_EXSTYLE, WS_EX_TRANSPARENT | WS_EX_LAYERED)`
//!   suivi du rafraîchissement de cadre qui va bien ;
//! * **macOS** — `NSWindow::setIgnoresMouseEvents:` ;
//! * **X11** — région d'entrée vide via l'extension `XShape`.
//!
//! S'appuyer dessus évite de réimplémenter (mal) trois FFI natives et supprime
//! tout bloc `unsafe` de cette caisse. Les backends qui ne savent pas le faire
//! (iOS, Android, Web, Orbital, et Wayland selon la configuration) renvoient
//! `ExternalError::NotSupported`, que l'on remonte comme une vraie erreur.

use super::PlatformWindowExt;
use crate::error::SystemError;
use tracing::debug;
use winit::error::ExternalError;
use winit::window::Window;

/// Implémentation de plateforme unique pour les capacités natives de fenêtre.
///
/// Elle est volontairement portable : la variation par OS est entièrement
/// déléguée au backend `winit` sous-jacent.
#[derive(Debug, Default, Clone, Copy)]
pub struct PlatformImpl;

impl PlatformWindowExt for PlatformImpl {
    fn set_click_through(&self, window: &Window, enabled: bool) -> Result<(), SystemError> {
        // `hittest = true` signifie « la fenêtre capte les événements souris ».
        // Le mode traversant est donc exactement l'inverse.
        match window.set_cursor_hittest(!enabled) {
            Ok(()) => {
                debug!(enabled, "Mode click-through appliqué à la fenêtre");
                Ok(())
            }
            Err(ExternalError::NotSupported(_)) => Err(SystemError::ClickThroughUnsupported),
            Err(e @ (ExternalError::Ignored | ExternalError::Os(_))) => {
                Err(SystemError::WindowError(format!(
                    "échec de configuration du mode click-through (enabled={enabled}) : {e}"
                )))
            }
        }
    }
}
