//! Interface utilisateur moderne et palette de commandes inspirée de Raycast.

#[cfg(feature = "a11y")]
pub mod a11y;
pub mod command_palette;
pub mod font;
pub mod layout;
pub mod preferences;
pub mod preview;
pub mod renderer;
pub mod search;
pub mod settings_window;
pub mod text;
pub mod theme;

pub use command_palette::{
    CommandPalette, PaletteAction, PaletteContext, PaletteExecutionResult, PaletteGroup,
    PaletteItem, PaletteSection, PaletteView, RepoDisplayInfo,
};
pub use layout::{FontSize, GlyphChoice, PanelDp, TextSize, UiMetrics};
pub use preferences::UiPreferences;
pub use preview::LivePetPreview;
pub use renderer::{PanelInteraction, PanelScene, PanelStyle, RaycastRenderer};
pub use settings_window::SettingsWindow;
pub use text::{truncate_chars, truncate_with_ellipsis};
pub use theme::{contrast_ratio, RaycastLayout, SystemTheme, Theme, ThemePreference};
