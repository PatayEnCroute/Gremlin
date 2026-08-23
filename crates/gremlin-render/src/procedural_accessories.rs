//! Génération procédurale d'accessoires pixel art intégrés (chapeaux, lunettes, tenues, objets, auras).
//!
//! Chaque accessoire est peint sur un canevas pleine taille
//! ([`CANVAS_SIZE`] x [`CANVAS_SIZE`]) et **déjà positionné** à son emplacement nominal
//! sur le corps du familier, conformément à la convention de composition documentée
//! sur [`crate::layer::LayerCompositor`]. Aucun ancrage de manifest ne leur est ajouté.

use crate::accessory::{AccessoryCatalog, AccessoryCategory, AccessoryItem, AccessoryManifest};
use crate::draw::{blank_canvas, fill_rect, set_px};
use crate::limits::{CANVAS_SIZE, DEFAULT_FRAME_DURATION_MS};
use crate::manifest::AnchorPoint;
use crate::sprite::{SpriteAtlas, SpriteFrame};
use std::collections::BTreeMap;
use tracing::warn;

/// Décalages d'humeur partagés par tous les accessoires portés sur le corps.
///
/// Ils suivent le rebond vertical du sprite de base selon l'humeur courante.
fn body_mood_offsets() -> BTreeMap<String, AnchorPoint> {
    [
        ("happy", AnchorPoint { x: 0, y: -3 }),
        ("hungry", AnchorPoint { x: 0, y: 1 }),
        ("sleep", AnchorPoint { x: 0, y: 2 }),
        ("dragged", AnchorPoint { x: 0, y: -4 }),
    ]
    .into_iter()
    .map(|(mood, offset)| (mood.to_string(), offset))
    .collect()
}

/// Recette de génération d'un accessoire procédural intégré.
struct AccessorySpec {
    id: &'static str,
    name: &'static str,
    description: &'static str,
    category: AccessoryCategory,
    /// Routine de peinture sur le canevas pleine taille.
    paint: fn(&mut [u8], usize),
    /// Les auras flottent autour du familier et ne suivent pas son rebond.
    follows_body: bool,
}

impl AccessorySpec {
    /// Peint le sprite, l'insère dans l'atlas et enregistre le manifest au catalogue.
    fn register(&self, atlas: &mut SpriteAtlas, catalog: &mut AccessoryCatalog) {
        let mut pixels = blank_canvas();
        (self.paint)(&mut pixels, CANVAS_SIZE as usize);

        match SpriteFrame::from_raw(CANVAS_SIZE, CANVAS_SIZE, pixels) {
            Ok(frame) => atlas.insert(self.id, frame),
            Err(err) => {
                warn!(
                    accessory = self.id,
                    error = %err,
                    "Génération procédurale incohérente : accessoire non enregistré"
                );
                return;
            }
        }

        catalog.register(AccessoryItem::procedural(AccessoryManifest {
            id: self.id.to_string(),
            name: self.name.to_string(),
            author: String::from("Gremlin Studio"),
            version: String::from("1.0.0"),
            category: self.category,
            description: self.description.to_string(),
            frame_width: CANVAS_SIZE,
            frame_height: CANVAS_SIZE,
            frames: vec![self.id.to_string()],
            frame_duration_ms: DEFAULT_FRAME_DURATION_MS,
            offsets_per_mood: if self.follows_body {
                body_mood_offsets()
            } else {
                BTreeMap::new()
            },
        }));
    }
}

/// Catalogue déclaratif des accessoires intégrés.
const BUILTIN_ACCESSORIES: [AccessorySpec; 10] = [
    AccessorySpec {
        id: "wizard_hat",
        name: "Chapeau de Mage",
        description: "Confère +10 en concentration lors du débogage Rust.",
        category: AccessoryCategory::Hat,
        paint: paint_wizard_hat,
        follows_body: true,
    },
    AccessorySpec {
        id: "royal_crown",
        name: "Couronne Royale",
        description: "Symbole suprême d'une architecture sans warning.",
        category: AccessoryCategory::Hat,
        paint: paint_royal_crown,
        follows_body: true,
    },
    AccessorySpec {
        id: "dev_cap",
        name: "Casquette Dev",
        description: "Casquette portée à l'envers, pour coder à 200 commits/h.",
        category: AccessoryCategory::Hat,
        paint: paint_dev_cap,
        follows_body: true,
    },
    AccessorySpec {
        id: "vr_visor",
        name: "Visière VR Cyberpunk",
        description: "Immersion totale dans le graphe Git.",
        category: AccessoryCategory::Glasses,
        paint: paint_vr_visor,
        follows_body: true,
    },
    AccessorySpec {
        id: "cool_shades",
        name: "Lunettes Noires Deal With It",
        description: "Quand la CI passe au vert du premier coup.",
        category: AccessoryCategory::Glasses,
        paint: paint_cool_shades,
        follows_body: true,
    },
    AccessorySpec {
        id: "cozy_hoodie",
        name: "Sweat à Capuche Noir",
        description: "L'uniforme officiel du développeur nocturne.",
        category: AccessoryCategory::Outfit,
        paint: paint_cozy_hoodie,
        follows_body: true,
    },
    AccessorySpec {
        id: "coffee_mug",
        name: "Tasse de Café Fumante",
        description: "Carburant universel à conversion d'idées en code.",
        category: AccessoryCategory::Held,
        paint: paint_coffee_mug,
        follows_body: true,
    },
    AccessorySpec {
        id: "dev_keyboard",
        name: "Clavier Mécanique RGB",
        description: "Switches tactiles lubrifiés pour cliquetis satisfaisant.",
        category: AccessoryCategory::Held,
        paint: paint_dev_keyboard,
        follows_body: true,
    },
    AccessorySpec {
        id: "fire_aura",
        name: "Aura Enflammée Super Saiyan",
        description: "Se manifeste lors des sessions de rush intensif.",
        category: AccessoryCategory::Aura,
        paint: paint_fire_aura,
        follows_body: false,
    },
    AccessorySpec {
        id: "matrix_aura",
        name: "Pluie Numérique Matrix",
        description: "Symboles et glyphes fluorescents flottant autour de Gremlin.",
        category: AccessoryCategory::Aura,
        paint: paint_matrix_aura,
        follows_body: false,
    },
];

/// Enregistre tous les accessoires procéduraux intégrés dans l'atlas et le catalogue.
pub fn register_default_procedural_accessories(
    atlas: &mut SpriteAtlas,
    catalog: &mut AccessoryCatalog,
) {
    for spec in &BUILTIN_ACCESSORIES {
        spec.register(atlas, catalog);
    }
}

// ==========================================
// 1. CHAPEAUX (HATS)
// ==========================================

/// Chapeau de Mage : cône bleu nuit à large bord, orné d'une étoile dorée.
fn paint_wizard_hat(buf: &mut [u8], size: usize) {
    let c_blue = [38, 50, 96, 255];
    let c_dark = [26, 35, 70, 255];
    let c_gold = [255, 215, 0, 255];

    // Bord large du chapeau
    fill_rect(buf, size, 14, 20, 36, 3, c_dark);
    fill_rect(buf, size, 16, 19, 32, 2, c_blue);
    // Cône pointu
    fill_rect(buf, size, 22, 14, 20, 5, c_blue);
    fill_rect(buf, size, 26, 9, 12, 5, c_blue);
    fill_rect(buf, size, 29, 5, 6, 4, c_blue);
    fill_rect(buf, size, 33, 2, 4, 3, c_dark);
    // Étoile dorée
    fill_rect(buf, size, 30, 12, 4, 4, c_gold);
    set_px(buf, size, 31, 11, c_gold);
    set_px(buf, size, 31, 16, c_gold);
    set_px(buf, size, 29, 13, c_gold);
    set_px(buf, size, 34, 13, c_gold);
}

/// Couronne Royale : bandeau doré à trois pointes serties de rubis.
fn paint_royal_crown(buf: &mut [u8], size: usize) {
    let c_gold = [255, 193, 7, 255];
    let c_gold_dark = [218, 165, 32, 255];
    let c_ruby = [229, 57, 53, 255];

    fill_rect(buf, size, 22, 17, 20, 3, c_gold_dark);
    fill_rect(buf, size, 22, 16, 20, 2, c_gold);
    // Pointes de la couronne
    fill_rect(buf, size, 22, 12, 4, 4, c_gold);
    fill_rect(buf, size, 30, 10, 4, 6, c_gold);
    fill_rect(buf, size, 38, 12, 4, 4, c_gold);
    // Joyaux rubis
    set_px(buf, size, 23, 13, c_ruby);
    set_px(buf, size, 31, 12, c_ruby);
    set_px(buf, size, 39, 13, c_ruby);
}

/// Casquette Dev : casquette rouge à visière tournée vers la droite.
fn paint_dev_cap(buf: &mut [u8], size: usize) {
    let c_red = [211, 47, 47, 255];
    let c_white = [250, 250, 250, 255];

    fill_rect(buf, size, 20, 16, 24, 5, c_red);
    fill_rect(buf, size, 40, 19, 8, 2, c_white); // Visière
    fill_rect(buf, size, 28, 17, 8, 3, c_white); // Logo
}

// ==========================================
// 2. LUNETTES (GLASSES)
// ==========================================

/// Visière VR Cyberpunk : bandeau noir aux verres cyan et magenta.
fn paint_vr_visor(buf: &mut [u8], size: usize) {
    let c_frame = [33, 33, 33, 255];
    let c_cyan = [0, 229, 255, 255];
    let c_magenta = [255, 0, 128, 255];

    fill_rect(buf, size, 18, 27, 28, 8, c_frame);
    fill_rect(buf, size, 20, 29, 10, 4, c_cyan);
    fill_rect(buf, size, 34, 29, 10, 4, c_magenta);
}

/// Lunettes de soleil pixel : verres noirs avec reflets en escalier.
fn paint_cool_shades(buf: &mut [u8], size: usize) {
    let c_dark_glass = [18, 18, 18, 255];
    let c_glint = [255, 255, 255, 230];

    fill_rect(buf, size, 18, 29, 11, 7, c_dark_glass);
    fill_rect(buf, size, 35, 29, 11, 7, c_dark_glass);
    fill_rect(buf, size, 29, 31, 6, 2, c_dark_glass); // Pont
    set_px(buf, size, 19, 30, c_glint);
    set_px(buf, size, 20, 31, c_glint);
    set_px(buf, size, 36, 30, c_glint);
    set_px(buf, size, 37, 31, c_glint);
}

// ==========================================
// 3. TENUES (OUTFITS)
// ==========================================

/// Sweat à capuche : corps sombre, poche kangourou et cordons clairs.
fn paint_cozy_hoodie(buf: &mut [u8], size: usize) {
    let c_hoodie = [45, 52, 54, 255];
    let c_cord = [223, 230, 233, 255];

    fill_rect(buf, size, 16, 40, 32, 14, c_hoodie);
    fill_rect(buf, size, 22, 44, 20, 6, [38, 43, 45, 255]); // Poche kangourou
    fill_rect(buf, size, 27, 38, 2, 6, c_cord); // Cordon gauche
    fill_rect(buf, size, 35, 38, 2, 6, c_cord); // Cordon droit
}

// ==========================================
// 4. OBJETS TENUS (HELD ITEMS)
// ==========================================

/// Tasse de café fumante tenue à droite du familier.
fn paint_coffee_mug(buf: &mut [u8], size: usize) {
    let c_mug = [236, 240, 241, 255];
    let c_coffee = [109, 76, 65, 255];
    let c_steam = [200, 200, 200, 180];

    // Corps de la tasse
    fill_rect(buf, size, 42, 40, 10, 12, c_mug);
    fill_rect(buf, size, 44, 40, 6, 2, c_coffee);
    // Anse (le rectangle transparent creuse l'intérieur)
    fill_rect(buf, size, 52, 42, 3, 7, c_mug);
    fill_rect(buf, size, 52, 44, 1, 3, [0, 0, 0, 0]);
    // Vapeur
    set_px(buf, size, 45, 37, c_steam);
    set_px(buf, size, 46, 36, c_steam);
    set_px(buf, size, 48, 35, c_steam);
}

/// Clavier mécanique aux touches RGB.
fn paint_dev_keyboard(buf: &mut [u8], size: usize) {
    let c_body = [40, 40, 45, 255];

    fill_rect(buf, size, 38, 44, 20, 10, c_body);
    // Touches RGB
    fill_rect(buf, size, 40, 46, 3, 3, [255, 64, 129, 255]); // Rose
    fill_rect(buf, size, 44, 46, 3, 3, [0, 229, 255, 255]); // Cyan
    fill_rect(buf, size, 48, 46, 3, 3, [118, 255, 3, 255]); // Vert
    fill_rect(buf, size, 52, 46, 3, 3, [255, 215, 0, 255]); // Jaune
}

// ==========================================
// 5. AURAS
// ==========================================

/// Aura de flammes montantes entourant le familier.
fn paint_fire_aura(buf: &mut [u8], size: usize) {
    let c_fire1 = [255, 87, 34, 180];
    let c_fire2 = [255, 193, 7, 220];

    fill_rect(buf, size, 12, 30, 40, 24, [255, 112, 67, 70]);
    // Flammes qui montent
    fill_rect(buf, size, 10, 24, 6, 16, c_fire1);
    fill_rect(buf, size, 12, 18, 4, 8, c_fire2);
    fill_rect(buf, size, 48, 22, 6, 18, c_fire1);
    fill_rect(buf, size, 48, 16, 4, 8, c_fire2);
    fill_rect(buf, size, 28, 10, 8, 10, c_fire2);
}

/// Pluie numérique : particules de code vert flottant autour du familier.
fn paint_matrix_aura(buf: &mut [u8], size: usize) {
    let c_matrix = [0, 230, 118, 190];
    let c_bright = [185, 246, 202, 230];

    for (x, y, color) in [
        (8, 12, c_matrix),
        (8, 14, c_bright),
        (12, 28, c_matrix),
        (12, 30, c_bright),
        (52, 14, c_bright),
        (52, 16, c_matrix),
        (56, 26, c_matrix),
        (24, 6, c_bright),
        (40, 8, c_matrix),
    ] {
        set_px(buf, size, x, y, color);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_register_procedural_accessories() {
        let mut atlas = SpriteAtlas::new();
        let mut catalog = AccessoryCatalog::new();

        register_default_procedural_accessories(&mut atlas, &mut catalog);

        assert_eq!(catalog.len(), BUILTIN_ACCESSORIES.len());
        for spec in &BUILTIN_ACCESSORIES {
            assert!(atlas.contains_key(spec.id), "sprite manquant : {}", spec.id);
            assert!(
                catalog.get(spec.id).is_some(),
                "item manquant : {}",
                spec.id
            );
        }

        assert_eq!(catalog.items_by_category(AccessoryCategory::Hat).len(), 3);
        assert_eq!(
            catalog.items_by_category(AccessoryCategory::Glasses).len(),
            2
        );
        assert_eq!(catalog.items_by_category(AccessoryCategory::Held).len(), 2);
        assert_eq!(catalog.items_by_category(AccessoryCategory::Aura).len(), 2);
        assert_eq!(
            catalog.items_by_category(AccessoryCategory::Outfit).len(),
            1
        );
    }

    #[test]
    fn test_les_ids_sont_uniques() {
        let mut ids: Vec<&str> = BUILTIN_ACCESSORIES.iter().map(|s| s.id).collect();
        let total = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), total, "identifiants d'accessoires dupliqués");
    }

    #[test]
    fn test_frames_pleine_taille_et_non_vides() {
        let mut atlas = SpriteAtlas::new();
        let mut catalog = AccessoryCatalog::new();
        register_default_procedural_accessories(&mut atlas, &mut catalog);

        for spec in &BUILTIN_ACCESSORIES {
            let Some(frame) = atlas.get(spec.id) else {
                panic!("sprite manquant : {}", spec.id);
            };
            assert_eq!(frame.width, CANVAS_SIZE);
            assert_eq!(frame.height, CANVAS_SIZE);
            assert!(
                frame.rgba.chunks_exact(4).any(|px| px[3] > 0),
                "sprite entièrement transparent : {}",
                spec.id
            );
        }
    }

    #[test]
    fn test_les_auras_ne_suivent_pas_le_rebond_du_corps() {
        let mut atlas = SpriteAtlas::new();
        let mut catalog = AccessoryCatalog::new();
        register_default_procedural_accessories(&mut atlas, &mut catalog);

        let Some(aura) = catalog.get("fire_aura") else {
            panic!("aura manquante");
        };
        assert_eq!(aura.manifest.mood_offset("happy"), (0, 0));

        let Some(hat) = catalog.get("wizard_hat") else {
            panic!("chapeau manquant");
        };
        assert_eq!(hat.manifest.mood_offset("happy"), (0, -3));
        assert_eq!(hat.manifest.mood_offset("inconnu"), (0, 0));
    }

    #[test]
    fn test_manifests_integres_sont_valides() {
        let mut atlas = SpriteAtlas::new();
        let mut catalog = AccessoryCatalog::new();
        register_default_procedural_accessories(&mut atlas, &mut catalog);

        for item in catalog.all_items() {
            assert!(
                item.manifest.validate().is_ok(),
                "manifest intégré invalide : {}",
                item.id()
            );
            assert!(item.manifest.primary_frame_key().is_some());
        }
    }
}
