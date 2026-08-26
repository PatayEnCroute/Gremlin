//! # `gremlin-app`
//!
//! Orchestrateur du compagnon de bureau Gremlin : il assemble le moteur de jeu
//! ([`gremlin_core`]), la surveillance Git ([`gremlin_watcher`]), le rendu
//! ([`gremlin_render`]) et l'intégration système ([`gremlin_system`]).
//!
//! La logique vit dans cette bibliothèque plutôt que dans le binaire : c'est ce
//! qui rend l'orchestrateur testable, notamment via
//! [`AppOptions::headless`](app::AppOptions::headless) qui construit une
//! application sans surveillance disque ni icône dans la zone de notification.

pub mod app;
pub mod config;
pub mod desktop;
pub mod desktop_motion;
mod dialogue;
pub mod error;
pub mod persistence;
pub mod pet_gesture;
pub mod renderer;
pub mod ui;
mod visual_feedback;

pub use app::{AppOptions, CustomAppEvent, GremlinApp, NATIVE_HEIGHT, NATIVE_WIDTH};
pub use config::AppConfig;
pub use desktop_motion::{
    DesktopMotion, MotionConfig, MotionPhase, MotionUpdate, PlacementIntent, ScreenAnchor,
};
pub use error::AppError;
pub use persistence::{LoadOutcome, PersistenceManager, PetSaveData, SAVE_ENVELOPE_VERSION};
pub use pet_gesture::{GestureConfig, GestureOutcome, PetGesture};
