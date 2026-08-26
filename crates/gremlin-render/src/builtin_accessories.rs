//! Chargement des accessoires officiels depuis les PNG embarqués.
//!
//! Les ressources sont décodées une seule fois au démarrage. Un pack intégré
//! incohérent est ignoré en bloc afin de ne jamais enregistrer un accessoire
//! dont une animation ou une variante serait partiellement disponible.

use crate::accessory::{AccessoryCatalog, AccessoryItem, AccessoryManifest};
use crate::sprite::{SpriteAtlas, SpriteFrame};
use std::collections::BTreeSet;
use tracing::warn;

struct EmbeddedFrame {
    key: &'static str,
    png: &'static [u8],
}

struct EmbeddedAccessory {
    id: &'static str,
    manifest_json: &'static str,
    frames: &'static [EmbeddedFrame],
}

macro_rules! embedded_frame {
    ($id:literal, $key:literal) => {
        EmbeddedFrame {
            key: $key,
            png: include_bytes!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../assets/accessories/builtin/",
                $id,
                "/",
                $key,
                ".png"
            )),
        }
    };
}

const WIZARD_HAT_FRAMES: &[EmbeddedFrame] = &[
    embedded_frame!("wizard_hat", "wizard_hat_default_0"),
    embedded_frame!("wizard_hat", "wizard_hat_default_1"),
    embedded_frame!("wizard_hat", "wizard_hat_default_2"),
    embedded_frame!("wizard_hat", "wizard_hat_baby_0"),
    embedded_frame!("wizard_hat", "wizard_hat_baby_1"),
    embedded_frame!("wizard_hat", "wizard_hat_baby_2"),
    embedded_frame!("wizard_hat", "wizard_hat_evolved_0"),
    embedded_frame!("wizard_hat", "wizard_hat_evolved_1"),
    embedded_frame!("wizard_hat", "wizard_hat_evolved_2"),
];

const ROYAL_CROWN_FRAMES: &[EmbeddedFrame] = &[
    embedded_frame!("royal_crown", "royal_crown_default_0"),
    embedded_frame!("royal_crown", "royal_crown_baby_0"),
    embedded_frame!("royal_crown", "royal_crown_evolved_0"),
];

const DEV_CAP_FRAMES: &[EmbeddedFrame] = &[
    embedded_frame!("dev_cap", "dev_cap_default_0"),
    embedded_frame!("dev_cap", "dev_cap_baby_0"),
    embedded_frame!("dev_cap", "dev_cap_evolved_0"),
];

const VR_VISOR_FRAMES: &[EmbeddedFrame] = &[
    embedded_frame!("vr_visor", "vr_visor_default_0"),
    embedded_frame!("vr_visor", "vr_visor_default_1"),
    embedded_frame!("vr_visor", "vr_visor_baby_0"),
    embedded_frame!("vr_visor", "vr_visor_baby_1"),
    embedded_frame!("vr_visor", "vr_visor_evolved_0"),
    embedded_frame!("vr_visor", "vr_visor_evolved_1"),
];

const COOL_SHADES_FRAMES: &[EmbeddedFrame] = &[
    embedded_frame!("cool_shades", "cool_shades_default_0"),
    embedded_frame!("cool_shades", "cool_shades_baby_0"),
    embedded_frame!("cool_shades", "cool_shades_evolved_0"),
];

const COZY_HOODIE_FRAMES: &[EmbeddedFrame] = &[
    embedded_frame!("cozy_hoodie", "cozy_hoodie_default_0"),
    embedded_frame!("cozy_hoodie", "cozy_hoodie_baby_0"),
    embedded_frame!("cozy_hoodie", "cozy_hoodie_evolved_0"),
];

const COFFEE_MUG_FRAMES: &[EmbeddedFrame] = &[
    embedded_frame!("coffee_mug", "coffee_mug_default_0"),
    embedded_frame!("coffee_mug", "coffee_mug_default_1"),
    embedded_frame!("coffee_mug", "coffee_mug_default_2"),
    embedded_frame!("coffee_mug", "coffee_mug_baby_0"),
    embedded_frame!("coffee_mug", "coffee_mug_baby_1"),
    embedded_frame!("coffee_mug", "coffee_mug_baby_2"),
    embedded_frame!("coffee_mug", "coffee_mug_evolved_0"),
    embedded_frame!("coffee_mug", "coffee_mug_evolved_1"),
    embedded_frame!("coffee_mug", "coffee_mug_evolved_2"),
];

const DEV_KEYBOARD_FRAMES: &[EmbeddedFrame] = &[
    embedded_frame!("dev_keyboard", "dev_keyboard_default_0"),
    embedded_frame!("dev_keyboard", "dev_keyboard_baby_0"),
    embedded_frame!("dev_keyboard", "dev_keyboard_evolved_0"),
];

const FIRE_AURA_FRAMES: &[EmbeddedFrame] = &[
    embedded_frame!("fire_aura", "fire_aura_default_0"),
    embedded_frame!("fire_aura", "fire_aura_baby_0"),
    embedded_frame!("fire_aura", "fire_aura_evolved_0"),
];

/// Récompenses de série de la phase 8.
///
/// Le catalogue les connaît comme n'importe quel accessoire : c'est
/// `gremlin-app` qui refuse de les équiper tant que le palier n'est pas atteint.
/// Mettre la règle de déblocage ici mélangerait le dessin et le jeu.
/// Un seul dessin par récompense : les variantes ne redessinent rien, leur
/// décalage vient des ancres du skin. Déclarer trois frames identiques aurait
/// triplé le poids embarqué pour un résultat au pixel près identique.
const STREAK_LEAF_PIN_FRAMES: &[EmbeddedFrame] =
    &[embedded_frame!("streak_leaf_pin", "streak_leaf_pin_0")];

const FOCUS_HEADPHONES_FRAMES: &[EmbeddedFrame] =
    &[embedded_frame!("focus_headphones", "focus_headphones_0")];

const AURORA_AURA_FRAMES: &[EmbeddedFrame] = &[embedded_frame!("aurora_aura", "aurora_aura_0")];

const MATRIX_AURA_FRAMES: &[EmbeddedFrame] = &[
    embedded_frame!("matrix_aura", "matrix_aura_default_0"),
    embedded_frame!("matrix_aura", "matrix_aura_default_1"),
    embedded_frame!("matrix_aura", "matrix_aura_default_2"),
    embedded_frame!("matrix_aura", "matrix_aura_default_3"),
    embedded_frame!("matrix_aura", "matrix_aura_baby_0"),
    embedded_frame!("matrix_aura", "matrix_aura_baby_1"),
    embedded_frame!("matrix_aura", "matrix_aura_baby_2"),
    embedded_frame!("matrix_aura", "matrix_aura_baby_3"),
    embedded_frame!("matrix_aura", "matrix_aura_evolved_0"),
    embedded_frame!("matrix_aura", "matrix_aura_evolved_1"),
    embedded_frame!("matrix_aura", "matrix_aura_evolved_2"),
    embedded_frame!("matrix_aura", "matrix_aura_evolved_3"),
];

const BUILTIN_ACCESSORIES: &[EmbeddedAccessory] = &[
    EmbeddedAccessory {
        id: "wizard_hat",
        manifest_json: include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/accessories/builtin/wizard_hat/manifest.json"
        )),
        frames: WIZARD_HAT_FRAMES,
    },
    EmbeddedAccessory {
        id: "royal_crown",
        manifest_json: include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/accessories/builtin/royal_crown/manifest.json"
        )),
        frames: ROYAL_CROWN_FRAMES,
    },
    EmbeddedAccessory {
        id: "dev_cap",
        manifest_json: include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/accessories/builtin/dev_cap/manifest.json"
        )),
        frames: DEV_CAP_FRAMES,
    },
    EmbeddedAccessory {
        id: "vr_visor",
        manifest_json: include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/accessories/builtin/vr_visor/manifest.json"
        )),
        frames: VR_VISOR_FRAMES,
    },
    EmbeddedAccessory {
        id: "cool_shades",
        manifest_json: include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/accessories/builtin/cool_shades/manifest.json"
        )),
        frames: COOL_SHADES_FRAMES,
    },
    EmbeddedAccessory {
        id: "cozy_hoodie",
        manifest_json: include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/accessories/builtin/cozy_hoodie/manifest.json"
        )),
        frames: COZY_HOODIE_FRAMES,
    },
    EmbeddedAccessory {
        id: "coffee_mug",
        manifest_json: include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/accessories/builtin/coffee_mug/manifest.json"
        )),
        frames: COFFEE_MUG_FRAMES,
    },
    EmbeddedAccessory {
        id: "dev_keyboard",
        manifest_json: include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/accessories/builtin/dev_keyboard/manifest.json"
        )),
        frames: DEV_KEYBOARD_FRAMES,
    },
    EmbeddedAccessory {
        id: "fire_aura",
        manifest_json: include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/accessories/builtin/fire_aura/manifest.json"
        )),
        frames: FIRE_AURA_FRAMES,
    },
    EmbeddedAccessory {
        id: "matrix_aura",
        manifest_json: include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/accessories/builtin/matrix_aura/manifest.json"
        )),
        frames: MATRIX_AURA_FRAMES,
    },
    EmbeddedAccessory {
        id: "streak_leaf_pin",
        manifest_json: include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/accessories/builtin/streak_leaf_pin/manifest.json"
        )),
        frames: STREAK_LEAF_PIN_FRAMES,
    },
    EmbeddedAccessory {
        id: "focus_headphones",
        manifest_json: include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/accessories/builtin/focus_headphones/manifest.json"
        )),
        frames: FOCUS_HEADPHONES_FRAMES,
    },
    EmbeddedAccessory {
        id: "aurora_aura",
        manifest_json: include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/accessories/builtin/aurora_aura/manifest.json"
        )),
        frames: AURORA_AURA_FRAMES,
    },
];

/// Charge les accessoires officiels dans l'atlas et le catalogue.
pub fn register_default_accessories(atlas: &mut SpriteAtlas, catalog: &mut AccessoryCatalog) {
    for embedded in BUILTIN_ACCESSORIES {
        if let Err(error) = register_embedded_accessory(embedded, atlas, catalog) {
            warn!(
                accessory = embedded.id,
                error, "Accessoire intégré invalide : pack ignoré"
            );
        }
    }
}

fn register_embedded_accessory(
    embedded: &EmbeddedAccessory,
    atlas: &mut SpriteAtlas,
    catalog: &mut AccessoryCatalog,
) -> Result<(), String> {
    let manifest = AccessoryManifest::from_json(embedded.manifest_json)
        .map_err(|error| format!("manifest illisible : {error}"))?;

    let referenced: BTreeSet<&str> = manifest
        .frames
        .iter()
        .chain(
            manifest
                .variants
                .values()
                .flat_map(|variant| variant.frames.iter()),
        )
        .map(String::as_str)
        .collect();
    let embedded_keys: BTreeSet<&str> = embedded.frames.iter().map(|frame| frame.key).collect();
    if referenced != embedded_keys {
        return Err(String::from(
            "liste de frames différente des ressources embarquées",
        ));
    }

    let mut decoded = Vec::with_capacity(embedded.frames.len());
    for source in embedded.frames {
        let frame = SpriteFrame::from_png_bytes(source.png)
            .map_err(|error| format!("frame {} illisible : {error}", source.key))?;
        if frame.width != manifest.frame_width || frame.height != manifest.frame_height {
            return Err(format!(
                "frame {} en {}x{}, manifest en {}x{}",
                source.key, frame.width, frame.height, manifest.frame_width, manifest.frame_height
            ));
        }
        if !frame.rgba.chunks_exact(4).any(|pixel| pixel[3] != 0) {
            return Err(format!("frame {} entièrement transparente", source.key));
        }
        decoded.push((source.key, frame));
    }

    for (key, frame) in decoded {
        atlas.insert(key, frame);
    }
    catalog.register(AccessoryItem::built_in(manifest));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accessory::{AccessoryCategory, WardrobeEquipment};
    use crate::limits::CANVAS_SIZE;

    /// Contrat de persistance : identifiants sauvegardés et calque de destination.
    ///
    /// Ces identifiants sont écrits dans la configuration des joueurs ; les
    /// renommer casserait toutes les garde-robes existantes.
    const EXPECTED_CATALOG: [(&str, AccessoryCategory); 13] = [
        ("wizard_hat", AccessoryCategory::Hat),
        ("royal_crown", AccessoryCategory::Hat),
        ("dev_cap", AccessoryCategory::Hat),
        ("vr_visor", AccessoryCategory::Glasses),
        ("cool_shades", AccessoryCategory::Glasses),
        ("cozy_hoodie", AccessoryCategory::Outfit),
        ("coffee_mug", AccessoryCategory::Held),
        ("dev_keyboard", AccessoryCategory::Held),
        ("fire_aura", AccessoryCategory::Aura),
        ("matrix_aura", AccessoryCategory::Aura),
        // Récompenses de série : le catalogue les porte, la règle de déblocage
        // vit dans `gremlin-app`.
        ("streak_leaf_pin", AccessoryCategory::Hat),
        ("focus_headphones", AccessoryCategory::Glasses),
        ("aurora_aura", AccessoryCategory::Aura),
    ];

    /// Familles visuelles servies par les skins intégrés.
    const STYLES: [&str; 3] = ["default", "baby", "evolved"];

    /// Cadence attendue de chaque accessoire, dans chacune des trois familles.
    const EXPECTED_FRAME_COUNT: [(&str, usize); 13] = [
        ("wizard_hat", 3),
        ("royal_crown", 1),
        ("dev_cap", 1),
        ("vr_visor", 2),
        ("cool_shades", 1),
        ("cozy_hoodie", 1),
        ("coffee_mug", 3),
        ("dev_keyboard", 1),
        ("fire_aura", 1),
        ("matrix_aura", 4),
        ("streak_leaf_pin", 1),
        ("focus_headphones", 1),
        ("aurora_aura", 1),
    ];

    fn loaded() -> (SpriteAtlas, AccessoryCatalog) {
        let mut atlas = SpriteAtlas::new();
        let mut catalog = AccessoryCatalog::new();
        register_default_accessories(&mut atlas, &mut catalog);
        (atlas, catalog)
    }

    #[test]
    fn les_accessoires_embarques_sont_complets() {
        let (atlas, catalog) = loaded();

        assert_eq!(catalog.len(), EXPECTED_CATALOG.len());
        // Trois familles visuelles pour les dix accessoires historiques, et un
        // dessin unique pour chacune des trois récompenses de série.
        assert_eq!(atlas.len(), 57);
        for item in catalog.all_items() {
            assert!(item.is_built_in());
            assert!(item.manifest.variants.contains_key("baby"));
            assert!(item.manifest.variants.contains_key("evolved"));
            let name = item.manifest.name.to_lowercase();
            assert!(!name.contains("matrix"));
            assert!(!name.contains("super saiyan"));
            assert!(!name.contains("deal with it"));
        }
    }

    #[test]
    fn les_identifiants_et_categories_sont_preserves() {
        let (_, catalog) = loaded();

        for (id, category) in EXPECTED_CATALOG {
            let Some(item) = catalog.get(id) else {
                panic!("accessoire {id} absent du catalogue intégré")
            };
            assert_eq!(item.category(), category, "calque de {id}");
        }
        assert_eq!(catalog.len(), EXPECTED_CATALOG.len());
    }

    #[test]
    fn un_equipement_sauvegarde_reste_resolu() {
        let (_, catalog) = loaded();

        // Configuration telle qu'écrite sur disque avant la refonte.
        let saved = r#"{
            "skin_id": "baby",
            "slots": {
                "Hat": "wizard_hat",
                "Glasses": "cool_shades",
                "Outfit": "cozy_hoodie",
                "Held": "coffee_mug",
                "Aura": "matrix_aura"
            }
        }"#;
        let wardrobe: WardrobeEquipment = match serde_json::from_str(saved) {
            Ok(w) => w,
            Err(e) => panic!("garde-robe sauvegardée illisible : {e}"),
        };

        assert_eq!(wardrobe.slots.len(), 5);
        for (category, id) in wardrobe.equipped_slots() {
            let Some(item) = catalog.get(id) else {
                panic!("accessoire équipé {id} introuvable après migration")
            };
            assert_eq!(item.category(), category, "calque de {id}");
        }
    }

    #[test]
    fn chaque_variante_expose_ses_frames_dans_l_atlas() {
        let (atlas, catalog) = loaded();
        let expected_counts: std::collections::BTreeMap<&str, usize> =
            EXPECTED_FRAME_COUNT.into_iter().collect();

        for (id, _) in EXPECTED_CATALOG {
            let Some(item) = catalog.get(id) else {
                panic!("accessoire {id} absent du catalogue intégré")
            };
            let manifest = &item.manifest;
            assert_eq!(manifest.frame_width, CANVAS_SIZE, "largeur de {id}");
            assert_eq!(manifest.frame_height, CANVAS_SIZE, "hauteur de {id}");

            for style in STYLES {
                let frames = manifest.frames_for_style(style);
                assert_eq!(
                    Some(&frames.len()),
                    expected_counts.get(id),
                    "cadence de {id} en {style}"
                );

                for key in frames {
                    let Some(frame) = atlas.get(key) else {
                        panic!("frame {key} de {id} absente de l'atlas")
                    };
                    assert_eq!(frame.width, CANVAS_SIZE, "largeur de {key}");
                    assert_eq!(frame.height, CANVAS_SIZE, "hauteur de {key}");
                    assert_eq!(
                        frame.rgba.len(),
                        (CANVAS_SIZE as usize) * (CANVAS_SIZE as usize) * 4,
                        "canal alpha de {key}"
                    );
                    assert!(
                        frame.opaque_bounds().is_some(),
                        "frame {key} entièrement transparente"
                    );
                }

                // Le point source doit rester dans la toile, sans quoi le
                // compositeur calerait l'accessoire hors du familier.
                let anchor = manifest.reference_anchor_for_style(style);
                assert!(
                    (0..CANVAS_SIZE as i32).contains(&anchor.x)
                        && (0..CANVAS_SIZE as i32).contains(&anchor.y),
                    "point source hors toile pour {id} en {style} : {anchor:?}"
                );
            }
        }
    }
}
