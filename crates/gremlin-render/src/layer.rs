//! Composition ordonnée des calques cosmétiques et du corps du Gremlin.
//!
//! # Convention de composition : canevas pleine taille
//!
//! **Tous les sprites de calque sont dessinés sur un canevas pleine taille**
//! ([`crate::limits::CANVAS_SIZE`], ou la taille de frame déclarée par le skin) et
//! sont dessinés à un emplacement de référence sur le Gremlin classique : un chapeau
//! en haut de son canevas, un objet à hauteur de main, etc.
//!
//! Il en découle trois règles, valables aussi bien pour les accessoires procéduraux que
//! pour les packs de skins chargés depuis le disque :
//!
//! 1. Les calques génériques de [`LayerCompositor::compose`] restent des blits directs,
//!    car [`ActiveLayer`] porte déjà leur décalage final.
//! 2. La composition habillée aligne l'ancre source de chaque accessoire sur l'ancre
//!    du skin actif, puis ajoute les corrections de pose du skin et les éventuels
//!    ajustements propres à l'accessoire.
//! 3. Les tenues sont découpées par l'alpha du corps courant. Elles épousent ainsi la
//!    silhouette de chaque skin au lieu de former un rectangle flottant.
//!
//! Le manifest reste utile à la composition : il sert à valider que les sprites de
//! l'atlas ont bien la taille de frame annoncée, et à tracer les divergences.

use crate::accessory::{AccessoryCatalog, WardrobeEquipment};
use crate::buffer::PixelBuffer;
use crate::manifest::{AnchorPoint, SkinManifest};
use crate::sprite::{SpriteAtlas, SpriteFrame};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tracing::warn;

/// Type de calque dans la pile de rendu.
/// L'ordre de déclaration définit l'ordre d'empilement strict (z-index croissant).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum LayerType {
    /// Aura / Effet d'arrière-plan (z-index 0).
    Aura,
    /// Corps / Sprite de base émotionnel (z-index 1).
    Base,
    /// Vêtements / Tenue (z-index 2).
    Outfit,
    /// Lunettes / Visière (z-index 3).
    Glasses,
    /// Chapeau / Casque (z-index 4).
    Hat,
    /// Objet tenu en main (clavier, tasse de café) (z-index 5).
    Held,
}

impl LayerType {
    /// Tous les calques, dans l'ordre d'empilement (z-index croissant).
    pub const ALL: [Self; 6] = [
        Self::Aura,
        Self::Base,
        Self::Outfit,
        Self::Glasses,
        Self::Hat,
        Self::Held,
    ];

    /// Nom de la clé d'ancrage correspondante dans le `manifest.json` d'un skin.
    ///
    /// Source unique de vérité pour la correspondance calque -> nom d'ancrage :
    /// [`AccessoryCategory::default_anchor_name`] y délègue.
    ///
    /// [`AccessoryCategory::default_anchor_name`]:
    ///     crate::accessory::AccessoryCategory::default_anchor_name
    #[must_use]
    pub const fn anchor_name(self) -> &'static str {
        match self {
            Self::Aura => "aura",
            Self::Base => "base",
            Self::Outfit => "outfit",
            Self::Glasses => "glasses",
            Self::Hat => "hat",
            Self::Held => "held",
        }
    }
}

/// Description d'un calque actif à dessiner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveLayer {
    /// Type de calque, qui détermine son z-index.
    pub layer_type: LayerType,
    /// Clé du sprite à dessiner dans le [`SpriteAtlas`].
    pub sprite_key: String,
    /// Décalage dynamique horizontal (humeur, animation) appliqué au blit.
    pub offset_x: i32,
    /// Décalage dynamique vertical (humeur, animation) appliqué au blit.
    pub offset_y: i32,
}

impl ActiveLayer {
    /// Crée un calque avec décalage nul.
    #[must_use]
    pub fn new(layer_type: LayerType, sprite_key: impl Into<String>) -> Self {
        Self {
            layer_type,
            sprite_key: sprite_key.into(),
            offset_x: 0,
            offset_y: 0,
        }
    }

    /// Crée un calque avec coordonnées de décalage explicites.
    #[must_use]
    pub fn with_offset(
        layer_type: LayerType,
        sprite_key: impl Into<String>,
        offset_x: i32,
        offset_y: i32,
    ) -> Self {
        Self {
            layer_type,
            sprite_key: sprite_key.into(),
            offset_x,
            offset_y,
        }
    }
}

/// Compositeur multi-calques appliquant les décalages dynamiques.
///
/// Voir la documentation du module pour la convention de positionnement.
pub struct LayerCompositor;

impl LayerCompositor {
    /// Compose les calques actifs sur le tampon de pixels.
    ///
    /// Chaque calque générique est dessiné à son décalage dynamique
    /// ([`ActiveLayer::offset_x`] / [`ActiveLayer::offset_y`]). La résolution des
    /// ancres du catalogue est réservée aux méthodes `compose_layered_pet*`.
    ///
    /// Lorsqu'un `manifest` est fourni, il sert de contrôle de cohérence : tout sprite
    /// dont les dimensions diffèrent de la frame déclarée est tracé en avertissement.
    /// Une clé de sprite absente de l'atlas est également tracée plutôt qu'ignorée
    /// silencieusement.
    pub fn compose(
        buffer: &mut PixelBuffer,
        layers: &[ActiveLayer],
        atlas: &SpriteAtlas,
        manifest: Option<&SkinManifest>,
    ) {
        // L'ordre est imposé par la petite table statique plutôt que par un
        // clone et un tri du slice à chaque frame.
        for layer_type in LayerType::ALL {
            for layer in layers.iter().filter(|layer| layer.layer_type == layer_type) {
                Self::compose_sprite(
                    buffer,
                    atlas,
                    manifest,
                    layer.sprite_key.as_str(),
                    layer.layer_type,
                    layer.offset_x,
                    layer.offset_y,
                );
            }
        }
    }

    /// Dessine un sprite de calque déjà résolu, sans allocation intermédiaire.
    fn compose_sprite(
        buffer: &mut PixelBuffer,
        atlas: &SpriteAtlas,
        manifest: Option<&SkinManifest>,
        sprite_key: &str,
        layer_type: LayerType,
        offset_x: i32,
        offset_y: i32,
    ) {
        let Some(sprite) = atlas.get(sprite_key) else {
            warn!(
                sprite_key,
                layer = ?layer_type,
                "Sprite absent de l'atlas : calque ignoré"
            );
            return;
        };

        if let Some(m) = manifest {
            if let Err(err) = m.validate_frame_size(sprite.width, sprite.height) {
                warn!(
                    sprite_key,
                    layer = ?layer_type,
                    error = %err,
                    "Dimensions de sprite incohérentes avec le manifest du skin"
                );
            }
        }

        buffer.blit(
            &sprite.rgba,
            sprite.width,
            sprite.height,
            offset_x,
            offset_y,
        );
    }

    /// Compose entièrement le familier avec sa tenue, ses accessoires équipés et son humeur.
    ///
    /// Chaque accessoire est rendu sur sa frame principale
    /// ([`crate::AccessoryManifest::primary_frame_key`]). Pour animer les accessoires
    /// multi-frames, utiliser [`LayerCompositor::compose_layered_pet_animated`].
    pub fn compose_layered_pet(
        buffer: &mut PixelBuffer,
        equipment: &WardrobeEquipment,
        atlas: &SpriteAtlas,
        manifest: Option<&SkinManifest>,
        catalog: &AccessoryCatalog,
        base_frame_key: &str,
        mood_key: &str,
    ) {
        Self::compose_layered_pet_animated(
            buffer,
            equipment,
            atlas,
            manifest,
            catalog,
            base_frame_key,
            mood_key,
            Duration::ZERO,
        );
    }

    /// Variante animée de [`LayerCompositor::compose_layered_pet`].
    ///
    /// `elapsed` est le temps de lecture cumulé de la scène : il sélectionne la frame
    /// courante de chaque accessoire multi-frames via
    /// [`crate::AccessoryManifest::frame_key_at`], en respectant la durée de frame
    /// déclarée par son manifest.
    #[allow(clippy::too_many_arguments)]
    pub fn compose_layered_pet_animated(
        buffer: &mut PixelBuffer,
        equipment: &WardrobeEquipment,
        atlas: &SpriteAtlas,
        manifest: Option<&SkinManifest>,
        catalog: &AccessoryCatalog,
        base_frame_key: &str,
        mood_key: &str,
        elapsed: Duration,
    ) {
        let base_sprite = atlas.get(base_frame_key);
        let accessory_style = manifest.map_or("default", |skin| skin.accessory_style.as_str());
        for layer_type in LayerType::ALL {
            if layer_type == LayerType::Base {
                Self::compose_sprite(
                    buffer,
                    atlas,
                    manifest,
                    base_frame_key,
                    LayerType::Base,
                    0,
                    0,
                );
                continue;
            }

            let category = match layer_type {
                LayerType::Aura => crate::AccessoryCategory::Aura,
                LayerType::Outfit => crate::AccessoryCategory::Outfit,
                LayerType::Glasses => crate::AccessoryCategory::Glasses,
                LayerType::Hat => crate::AccessoryCategory::Hat,
                LayerType::Held => crate::AccessoryCategory::Held,
                LayerType::Base => continue,
            };
            let Some(accessory_id) = equipment.get_equipped(category) else {
                continue;
            };
            let Some(item) = catalog.get(accessory_id) else {
                warn!(
                    accessory_id,
                    category = ?category,
                    "Accessoire équipé absent du catalogue : calque ignoré"
                );
                continue;
            };
            let Some(frame) = item
                .manifest
                .frame_key_at_for_style(accessory_style, elapsed)
            else {
                warn!(
                    accessory_id,
                    category = ?category,
                    "Accessoire sans frame : calque ignoré"
                );
                continue;
            };
            let (offset_x, offset_y) =
                Self::accessory_offset(item, manifest, mood_key, accessory_style);
            if layer_type == LayerType::Outfit && item.manifest.clip_to_body {
                Self::compose_outfit(
                    buffer,
                    atlas,
                    manifest,
                    frame,
                    offset_x,
                    offset_y,
                    base_sprite,
                    manifest.and_then(|skin| skin.anchor_for_mood("head", mood_key)),
                );
            } else {
                Self::compose_sprite(
                    buffer, atlas, manifest, frame, layer_type, offset_x, offset_y,
                );
            }
        }
    }

    /// Calcule le déplacement final d'un accessoire sans allocation.
    fn accessory_offset(
        item: &crate::AccessoryItem,
        manifest: Option<&SkinManifest>,
        mood_key: &str,
        accessory_style: &str,
    ) -> (i32, i32) {
        let source = item.manifest.reference_anchor_for_style(accessory_style);
        let target = manifest
            .and_then(|skin| skin.anchor_for_mood(item.category().default_anchor_name(), mood_key))
            .unwrap_or(source);
        let (mood_x, mood_y) = item.manifest.mood_offset(mood_key);

        (
            target.x.saturating_sub(source.x).saturating_add(mood_x),
            target.y.saturating_sub(source.y).saturating_add(mood_y),
        )
    }

    /// Compose une tenue en la découpant sur la silhouette alpha du corps courant.
    #[allow(clippy::too_many_arguments)]
    fn compose_outfit(
        buffer: &mut PixelBuffer,
        atlas: &SpriteAtlas,
        manifest: Option<&SkinManifest>,
        sprite_key: &str,
        offset_x: i32,
        offset_y: i32,
        base_sprite: Option<&SpriteFrame>,
        head_anchor: Option<AnchorPoint>,
    ) {
        let Some(sprite) = atlas.get(sprite_key) else {
            warn!(
                sprite_key,
                layer = ?LayerType::Outfit,
                "Sprite absent de l'atlas : calque ignoré"
            );
            return;
        };

        if let Some(m) = manifest {
            if let Err(err) = m.validate_frame_size(sprite.width, sprite.height) {
                warn!(
                    sprite_key,
                    layer = ?LayerType::Outfit,
                    error = %err,
                    "Dimensions de sprite incohérentes avec le manifest du skin"
                );
            }
        }

        let Some(mask) = base_sprite else {
            buffer.blit(
                &sprite.rgba,
                sprite.width,
                sprite.height,
                offset_x,
                offset_y,
            );
            return;
        };

        for source_y in 0..sprite.height {
            let target_y = offset_y.saturating_add(source_y as i32);
            if target_y < 0 || target_y >= buffer.height() as i32 {
                continue;
            }
            for source_x in 0..sprite.width {
                let target_x = offset_x.saturating_add(source_x as i32);
                if target_x < 0 || target_x >= buffer.width() as i32 {
                    continue;
                }

                let source_index =
                    ((source_y as usize) * (sprite.width as usize) + source_x as usize) * 4;
                let mask_index =
                    ((target_y as usize) * (mask.width as usize) + target_x as usize) * 4;
                let Some(source_pixel) = sprite.rgba.get(source_index..source_index + 4) else {
                    continue;
                };
                let Some(mask_alpha) = mask.rgba.get(mask_index + 3) else {
                    continue;
                };
                if *mask_alpha == 0 {
                    continue;
                }
                if head_anchor
                    .is_some_and(|head| Self::inside_head_exclusion(target_x, target_y, head))
                {
                    continue;
                }

                buffer.blend_pixel(
                    target_x as u32,
                    target_y as u32,
                    [
                        source_pixel[0],
                        source_pixel[1],
                        source_pixel[2],
                        source_pixel[3],
                    ],
                );
            }
        }
    }

    /// Approxime la zone de tête protégée des tenues par une ellipse.
    ///
    /// L'ancre `head` vise le haut du visage pour les bulles et couvre-chefs ; le
    /// centre de l'ellipse est donc légèrement abaissé pour préserver yeux et bouche.
    fn inside_head_exclusion(x: i32, y: i32, head: AnchorPoint) -> bool {
        const RADIUS_X: i64 = 15;
        const RADIUS_Y: i64 = 14;
        const CENTER_Y_OFFSET: i32 = 6;

        let dx = i64::from(x.saturating_sub(head.x));
        let dy = i64::from(y.saturating_sub(head.y.saturating_add(CENTER_Y_OFFSET)));
        dx.saturating_mul(dx)
            .saturating_mul(RADIUS_Y.saturating_mul(RADIUS_Y))
            .saturating_add(
                dy.saturating_mul(dy)
                    .saturating_mul(RADIUS_X.saturating_mul(RADIUS_X)),
            )
            <= RADIUS_X
                .saturating_mul(RADIUS_X)
                .saturating_mul(RADIUS_Y.saturating_mul(RADIUS_Y))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accessory::AccessoryCategory;
    use crate::limits::CANVAS_SIZE;
    use crate::manifest::AnchorPoint;
    use crate::register_default_accessories;
    use crate::sprite::SpriteFrame;
    use std::collections::BTreeMap;

    /// Construit un manifest de skin dont tous les ancrages sont non nuls.
    fn manifest_with_anchors(offset: i32) -> SkinManifest {
        let mut anchors = BTreeMap::new();
        for layer in LayerType::ALL {
            anchors.insert(
                layer.anchor_name().to_string(),
                AnchorPoint {
                    x: offset,
                    y: offset * 2,
                },
            );
        }

        SkinManifest {
            name: "Test".into(),
            author: "Test".into(),
            version: "1.0.0".into(),
            accessory_style: String::from("default"),
            frame_width: CANVAS_SIZE,
            frame_height: CANVAS_SIZE,
            anchors,
            anchor_offsets_per_mood: BTreeMap::new(),
            animations: BTreeMap::new(),
        }
    }

    /// Atlas d'un seul sprite plein, de la taille du canevas.
    fn solid_atlas(key: &str, color: [u8; 4]) -> SpriteAtlas {
        let side = CANVAS_SIZE as usize;
        let mut rgba = Vec::with_capacity(side * side * 4);
        for _ in 0..(side * side) {
            rgba.extend_from_slice(&color);
        }

        let mut atlas = SpriteAtlas::new();
        match SpriteFrame::from_raw(CANVAS_SIZE, CANVAS_SIZE, rgba) {
            Ok(frame) => atlas.insert(key, frame),
            Err(e) => panic!("frame de test invalide : {e}"),
        }
        atlas
    }

    #[test]
    fn test_layer_ordering_and_stacking() {
        let mut buffer = PixelBuffer::new(CANVAS_SIZE, CANVAS_SIZE);
        let mut atlas = SpriteAtlas::new();
        atlas.load_default_procedural_sprites();

        let mut catalog = AccessoryCatalog::new();
        register_default_accessories(&mut atlas, &mut catalog);

        let mut wardrobe = WardrobeEquipment::new();
        wardrobe.equip(AccessoryCategory::Hat, "wizard_hat");
        wardrobe.equip(AccessoryCategory::Glasses, "cool_shades");
        wardrobe.equip(AccessoryCategory::Held, "coffee_mug");

        LayerCompositor::compose_layered_pet(
            &mut buffer,
            &wardrobe,
            &atlas,
            None,
            &catalog,
            "idle_0",
            "idle",
        );

        // Vérifier que le buffer n'est plus vide
        assert!(buffer.as_bytes().iter().any(|&b| b > 0));
    }

    #[test]
    fn test_compose_generique_ne_resout_pas_les_ancrages_du_catalogue() {
        // `ActiveLayer` porte déjà son décalage final. La primitive générique ne
        // connaît ni le catalogue ni l'ancre source qui permettraient un alignement.
        let atlas = solid_atlas("base", [10, 20, 30, 255]);
        let layers = [ActiveLayer::new(LayerType::Base, "base")];

        let mut sans_manifest = PixelBuffer::new(CANVAS_SIZE, CANVAS_SIZE);
        LayerCompositor::compose(&mut sans_manifest, &layers, &atlas, None);

        let manifest = manifest_with_anchors(16);
        let mut avec_ancrages = PixelBuffer::new(CANVAS_SIZE, CANVAS_SIZE);
        LayerCompositor::compose(&mut avec_ancrages, &layers, &atlas, Some(&manifest));

        assert_eq!(
            sans_manifest.as_bytes(),
            avec_ancrages.as_bytes(),
            "la primitive générique ne doit pas inventer une ancre source"
        );
        assert_eq!(&sans_manifest.as_bytes()[0..4], &[10, 20, 30, 255]);
    }

    #[test]
    fn test_accessoire_suit_ancre_du_skin_et_de_la_pose() {
        let mut atlas = SpriteAtlas::new();
        atlas.load_default_procedural_sprites();
        let mut catalog = AccessoryCatalog::new();
        register_default_accessories(&mut atlas, &mut catalog);
        let Some(hat) = catalog.get("wizard_hat") else {
            panic!("chapeau procédural manquant");
        };

        let mut manifest = manifest_with_anchors(0);
        manifest
            .anchors
            .insert("hat".into(), AnchorPoint { x: 20, y: 9 });
        manifest.anchor_offsets_per_mood.insert(
            "happy".into(),
            BTreeMap::from([("head".into(), AnchorPoint { x: 2, y: 3 })]),
        );

        assert_eq!(
            LayerCompositor::accessory_offset(hat, Some(&manifest), "idle", "default"),
            (4, 5)
        );
        assert_eq!(
            LayerCompositor::accessory_offset(hat, Some(&manifest), "happy", "default"),
            (6, 8)
        );
    }

    #[test]
    fn test_decalage_dhumeur_est_bien_applique() {
        // Le seul décalage légitime est celui porté par le calque lui-même.
        let atlas = solid_atlas("base", [255, 0, 0, 255]);
        let mut buffer = PixelBuffer::new(CANVAS_SIZE, CANVAS_SIZE);
        let layers = [ActiveLayer::with_offset(LayerType::Base, "base", 2, 3)];
        LayerCompositor::compose(&mut buffer, &layers, &atlas, None);

        let idx_vide = 0;
        assert_eq!(&buffer.as_bytes()[idx_vide..idx_vide + 4], &[0, 0, 0, 0]);

        let side = CANVAS_SIZE as usize;
        let idx_decale = (3 * side + 2) * 4;
        assert_eq!(
            &buffer.as_bytes()[idx_decale..idx_decale + 4],
            &[255, 0, 0, 255]
        );
    }

    #[test]
    fn test_zone_de_tete_protege_le_visage_des_tenues() {
        let head = AnchorPoint { x: 32, y: 17 };
        assert!(LayerCompositor::inside_head_exclusion(32, 23, head));
        assert!(LayerCompositor::inside_head_exclusion(20, 23, head));
        assert!(!LayerCompositor::inside_head_exclusion(5, 23, head));
        assert!(!LayerCompositor::inside_head_exclusion(32, 50, head));
    }

    #[test]
    fn test_sprite_manquant_ne_casse_pas_la_composition() {
        let atlas = solid_atlas("base", [1, 2, 3, 255]);
        let mut buffer = PixelBuffer::new(CANVAS_SIZE, CANVAS_SIZE);
        let layers = [
            ActiveLayer::new(LayerType::Base, "base"),
            ActiveLayer::new(LayerType::Hat, "cle_inexistante"),
        ];
        LayerCompositor::compose(&mut buffer, &layers, &atlas, None);
        assert_eq!(&buffer.as_bytes()[0..4], &[1, 2, 3, 255]);
    }

    #[test]
    fn test_composition_animee_change_de_frame() {
        let side = CANVAS_SIZE as usize;
        let mut atlas = SpriteAtlas::new();
        for (key, color) in [("anim_0", [255u8, 0, 0, 255]), ("anim_1", [0, 255, 0, 255])] {
            let mut rgba = Vec::with_capacity(side * side * 4);
            for _ in 0..(side * side) {
                rgba.extend_from_slice(&color);
            }
            match SpriteFrame::from_raw(CANVAS_SIZE, CANVAS_SIZE, rgba) {
                Ok(frame) => atlas.insert(key, frame),
                Err(e) => panic!("frame de test invalide : {e}"),
            }
        }
        // Frame de base transparente pour ne pas masquer l'accessoire.
        match SpriteFrame::from_raw(CANVAS_SIZE, CANVAS_SIZE, vec![0; side * side * 4]) {
            Ok(frame) => atlas.insert("base", frame),
            Err(e) => panic!("frame de test invalide : {e}"),
        }

        let mut catalog = AccessoryCatalog::new();
        catalog.register(crate::accessory::AccessoryItem::built_in(
            crate::accessory::AccessoryManifest {
                id: "anim_hat".into(),
                name: "Chapeau animé".into(),
                author: String::new(),
                version: String::new(),
                category: AccessoryCategory::Hat,
                description: String::new(),
                frame_width: CANVAS_SIZE,
                frame_height: CANVAS_SIZE,
                frames: vec!["anim_0".into(), "anim_1".into()],
                frame_duration_ms: 100,
                offsets_per_mood: BTreeMap::new(),
                anchor: None,
                variants: BTreeMap::new(),
                clip_to_body: false,
            },
        ));

        let mut wardrobe = WardrobeEquipment::new();
        wardrobe.equip(AccessoryCategory::Hat, "anim_hat");

        let mut premiere = PixelBuffer::new(CANVAS_SIZE, CANVAS_SIZE);
        LayerCompositor::compose_layered_pet_animated(
            &mut premiere,
            &wardrobe,
            &atlas,
            None,
            &catalog,
            "base",
            "idle",
            Duration::ZERO,
        );
        assert_eq!(&premiere.as_bytes()[0..4], &[255, 0, 0, 255]);

        let mut seconde = PixelBuffer::new(CANVAS_SIZE, CANVAS_SIZE);
        LayerCompositor::compose_layered_pet_animated(
            &mut seconde,
            &wardrobe,
            &atlas,
            None,
            &catalog,
            "base",
            "idle",
            Duration::from_millis(150),
        );
        assert_eq!(&seconde.as_bytes()[0..4], &[0, 255, 0, 255]);
    }
}
