//! Composition ordonnée des calques cosmétiques et du corps du Gremlin.
//!
//! # Convention de composition : canevas pleine taille
//!
//! **Tous les sprites de calque sont dessinés sur un canevas pleine taille**
//! ([`crate::limits::CANVAS_SIZE`], ou la taille de frame déclarée par le skin) et
//! sont **déjà positionnés** à leur emplacement nominal sur le corps du familier : un
//! chapeau est peint en haut de son canevas, un objet tenu à hauteur de main, etc.
//!
//! Il en découle deux règles, valables aussi bien pour les accessoires procéduraux que
//! pour les packs de skins chargés depuis le disque :
//!
//! 1. Composer un calque revient à un simple `blit` en `(0, 0)`, auquel s'ajoute
//!    uniquement le décalage dynamique du calque ([`ActiveLayer::offset_x`] /
//!    [`ActiveLayer::offset_y`], alimenté par les décalages d'humeur et d'animation).
//! 2. Le champ `anchors` du [`SkinManifest`] est une **métadonnée descriptive** — le
//!    point d'attache de référence documenté par l'auteur du skin, exploité par
//!    l'outillage d'édition. Il n'est **jamais** ajouté comme translation, sans quoi
//!    tout skin déclarant un ancrage non nul décalerait deux fois les accessoires
//!    déjà positionnés.
//!
//! Le manifest reste utile à la composition : il sert à valider que les sprites de
//! l'atlas ont bien la taille de frame annoncée, et à tracer les divergences.

use crate::accessory::{AccessoryCatalog, WardrobeEquipment};
use crate::buffer::PixelBuffer;
use crate::manifest::SkinManifest;
use crate::sprite::SpriteAtlas;
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
    /// Chaque calque est dessiné à son décalage dynamique
    /// ([`ActiveLayer::offset_x`] / [`ActiveLayer::offset_y`]) : les sprites étant
    /// dessinés sur un canevas pleine taille et déjà positionnés, aucun ancrage de
    /// manifest n'est ajouté (voir la convention en tête de module).
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
        // Trier les calques par ordre d'affichage (z-index)
        let mut sorted_layers = layers.to_vec();
        sorted_layers.sort_by_key(|l| l.layer_type);

        for layer in sorted_layers {
            let Some(sprite) = atlas.get(&layer.sprite_key) else {
                warn!(
                    sprite_key = %layer.sprite_key,
                    layer = ?layer.layer_type,
                    "Sprite absent de l'atlas : calque ignoré"
                );
                continue;
            };

            if let Some(m) = manifest {
                if let Err(err) = m.validate_frame_size(sprite.width, sprite.height) {
                    warn!(
                        sprite_key = %layer.sprite_key,
                        layer = ?layer.layer_type,
                        error = %err,
                        "Dimensions de sprite incohérentes avec le manifest du skin"
                    );
                }
            }

            buffer.blit(
                &sprite.rgba,
                sprite.width,
                sprite.height,
                layer.offset_x,
                layer.offset_y,
            );
        }
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
        let mut layers = Vec::with_capacity(LayerType::ALL.len());

        // Corps du familier : toujours présent, z-index intermédiaire.
        layers.push(ActiveLayer::new(LayerType::Base, base_frame_key));

        // Accessoires équipés, parcourus dans l'ordre d'empilement des catégories.
        for (category, accessory_id) in equipment.equipped_slots() {
            let Some(item) = catalog.get(accessory_id) else {
                warn!(
                    accessory_id,
                    category = ?category,
                    "Accessoire équipé absent du catalogue : calque ignoré"
                );
                continue;
            };

            let Some(frame) = item.manifest.frame_key_at(elapsed) else {
                warn!(
                    accessory_id,
                    category = ?category,
                    "Accessoire sans frame : calque ignoré"
                );
                continue;
            };

            let (mx, my) = item.manifest.mood_offset(mood_key);
            layers.push(ActiveLayer::with_offset(
                category.to_layer_type(),
                frame,
                mx,
                my,
            ));
        }

        Self::compose(buffer, &layers, atlas, manifest);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accessory::AccessoryCategory;
    use crate::limits::CANVAS_SIZE;
    use crate::manifest::AnchorPoint;
    use crate::procedural_accessories::register_default_procedural_accessories;
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
            frame_width: CANVAS_SIZE,
            frame_height: CANVAS_SIZE,
            anchors,
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
        register_default_procedural_accessories(&mut atlas, &mut catalog);

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
    fn test_ancrages_non_nuls_ne_deplacent_pas_les_calques() {
        // Régression : les ancrages du manifest étaient additionnés au décalage de
        // chaque calque, ce qui décalait les accessoires déjà positionnés dès qu'un
        // skin réel déclarait un ancrage non nul.
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
            "les ancrages du manifest sont descriptifs et ne doivent rien translater"
        );
        assert_eq!(&sans_manifest.as_bytes()[0..4], &[10, 20, 30, 255]);
    }

    #[test]
    fn test_pet_complet_identique_avec_et_sans_ancrages() {
        let mut atlas = SpriteAtlas::new();
        atlas.load_default_procedural_sprites();
        let mut catalog = AccessoryCatalog::new();
        register_default_procedural_accessories(&mut atlas, &mut catalog);

        let mut wardrobe = WardrobeEquipment::new();
        wardrobe.equip(AccessoryCategory::Hat, "wizard_hat");
        wardrobe.equip(AccessoryCategory::Held, "coffee_mug");
        wardrobe.equip(AccessoryCategory::Aura, "fire_aura");

        let manifest = manifest_with_anchors(12);

        let mut sans = PixelBuffer::new(CANVAS_SIZE, CANVAS_SIZE);
        LayerCompositor::compose_layered_pet(
            &mut sans, &wardrobe, &atlas, None, &catalog, "idle_0", "idle",
        );

        let mut avec = PixelBuffer::new(CANVAS_SIZE, CANVAS_SIZE);
        LayerCompositor::compose_layered_pet(
            &mut avec,
            &wardrobe,
            &atlas,
            Some(&manifest),
            &catalog,
            "idle_0",
            "idle",
        );

        assert_eq!(sans.as_bytes(), avec.as_bytes());
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
        catalog.register(crate::accessory::AccessoryItem::procedural(
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
