//! # `gremlin-core`
//!
//! Logique métier pure et agnostique du système pour le familier de bureau Gremlin.
//! Ce module gère les statistiques, les humeurs, les gains d'XP, le cycle de vie
//! et les simulations du compagnon.
//!
//! ## Invariants garantis
//!
//! [`PetState`] est un agrégat encapsulé : ses champs sont privés et toutes les
//! mutations passent par ses méthodes. Il en découle trois garanties que
//! l'appelant n'a pas à revérifier :
//!
//! * les jauges restent finies et bornées dans `[0, 100]`, y compris après
//!   chargement d'une sauvegarde éditée à la main ([`PetState::normalize`]) ;
//! * l'humeur est toujours cohérente avec les statistiques ;
//! * `tick` termine toujours, quelle que soit la durée écoulée fournie.

pub mod action;
pub mod config;
pub mod error;
pub mod events;
pub mod focus;
pub mod mood;
pub mod progression;
pub mod state;
pub mod stats;
pub mod tooling;

pub use action::ActionKind;
pub use config::{
    ActionConfig, CoreConfig, DecayConfig, FocusConfig, MoodConfig, ToolingRewardsConfig,
    DEFAULT_CATCHUP_STEP_SECS, MAX_CATCHUP_DURATION_SECS, MAX_CATCHUP_STEP_SECS,
    MIN_CATCHUP_STEP_SECS,
};
pub use error::CoreError;
pub use events::CoreEvent;
pub use focus::ActivityState;
pub use mood::PetMood;
pub use progression::{EvolutionStage, PetProgression, MIN_LEVEL};
pub use state::{PetState, SAVE_FORMAT_VERSION};
pub use stats::{PetStats, MAX_STAT_VALUE, MIN_STAT_VALUE};
pub use tooling::{
    BreakReason, BuildSummary, BuildTool, RepositoryId, TestFramework, TestSummary, MAX_TEST_COUNT,
    MAX_TOOLING_DURATION,
};
