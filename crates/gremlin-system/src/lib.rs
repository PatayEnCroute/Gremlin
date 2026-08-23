//! # `gremlin-system`
//!
//! Intégrations avec le système d'exploitation hôte (Windows, macOS, Linux).
//! Gère la création de fenêtres transparentes sans bordure, le mode click-through,
//! le menu de la barre des tâches (systray), l'autostart et la persistance atomique sécurisée.
//!
//! ## Coutures de plateforme
//!
//! Toute spécificité OS est isolée dans [`platform`], derrière deux traits :
//! [`PlatformWindowExt`] pour les capacités de fenêtre et
//! [`platform::AutostartBackend`] pour le démarrage automatique. Le reste de la
//! caisse (et l'application appelante) reste agnostique de l'OS.

pub mod autostart;
pub mod error;
pub mod paths;
pub mod platform;
pub mod storage;
pub mod tray;
pub mod window;

#[cfg(test)]
mod test_support;

pub use autostart::AutostartManager;
pub use error::SystemError;
pub use paths::AppPaths;
pub use platform::{
    AutostartBackend, AutostartTarget, BoxedAutostartBackend, LaunchAgentBackend, PlatformImpl,
    PlatformWindowExt, UnsupportedBackend, XdgAutostartBackend,
};
pub use storage::AtomicStorage;
pub use tray::{action_for_tray_click, SystemTrayManager, TrayActionMap, TrayMenuAction};
pub use window::{load_app_icon, WindowConfig, EMBEDDED_APP_ICON_PNG};

#[cfg(target_os = "windows")]
pub use platform::RegistryRunBackend;
