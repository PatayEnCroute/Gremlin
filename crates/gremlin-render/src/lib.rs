//! # `gremlin-render`
//!
//! Moteur de rendu 2D logiciel / GPU, composition multi-calques, animations, accessoires et décodage des skins.
//!
//! ## Convention de composition
//!
//! Tous les sprites de calque (corps, tenue, accessoires, aura) sont peints sur un
//! canevas pleine taille de [`CANVAS_SIZE`] x [`CANVAS_SIZE`]. Les accessoires déclarent
//! leur point source ; le compositeur l'aligne sur les ancres du skin et de la pose
//! actifs. Voir [`layer::LayerCompositor`] pour le détail.
//!
//! ## Entrées non fiables
//!
//! Les manifests et images de skins proviennent de dossiers utilisateur. Le module
//! [`limits`] rassemble les bornes appliquées à toute valeur issue de ces sources
//! (dimensions, durées d'animation, budget de décodage).

mod draw;

pub mod accessory;
pub mod animation;
pub mod bubble;
pub mod buffer;
pub mod builtin_accessories;
pub mod error;
pub mod layer;
pub mod limits;
pub mod manifest;
pub mod particles;
pub mod sprite;
pub mod transition;

pub use accessory::{
    AccessoryCatalog, AccessoryCategory, AccessoryItem, AccessoryManifest, AccessorySource,
    AccessoryVariant, WardrobeEquipment,
};
pub use animation::{AnimationController, AnimationFrame, PlayMode, SpriteAnimation};
pub use bubble::{BubbleRect, SpeechBubbleRenderer, SpeechBubbleView};
pub use buffer::PixelBuffer;
pub use builtin_accessories::register_default_accessories;
pub use error::RenderError;
pub use layer::{ActiveLayer, LayerCompositor, LayerType};
pub use limits::{
    CANVAS_SIZE, DEFAULT_FRAME_DURATION_MS, MAX_FRAME_DIMENSION, MAX_FRAME_DURATION_MS,
    MIN_FRAME_DURATION_MS,
};
pub use manifest::{AnchorPoint, AnimationDef, SkinManifest};
pub use particles::{ParticleEngine, ParticlePreset, ParticleShape, MAX_PARTICLES};
pub use sprite::{SpriteAtlas, SpriteBounds, SpriteFrame};
pub use transition::{TransitionController, TransitionRenderer};
