//! Interface utilisateur moderne et palette de commandes inspirée de Raycast.

pub mod command_palette;
pub mod font;
pub mod preview;
pub mod renderer;
pub mod text;
pub mod theme;

pub use command_palette::{
    CommandPalette, PaletteAction, PaletteContext, PaletteExecutionResult, PaletteItem,
    PaletteSection, RepoDisplayInfo,
};
pub use preview::LivePetPreview;
pub use renderer::RaycastRenderer;
pub use text::{truncate_chars, truncate_with_ellipsis};
pub use theme::{RaycastLayout, RaycastTheme};
