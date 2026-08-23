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
pub mod error;
pub mod persistence;
pub mod renderer;
pub mod ui;

pub use app::{AppOptions, CustomAppEvent, GremlinApp, NATIVE_HEIGHT, NATIVE_WIDTH};
pub use config::AppConfig;
pub use error::AppError;
pub use persistence::{LoadOutcome, PersistenceManager, PetSaveData, SAVE_ENVELOPE_VERSION};
