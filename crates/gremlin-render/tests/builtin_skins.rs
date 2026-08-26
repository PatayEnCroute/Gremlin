//! Validation des skins distribués avec l'application.

#![allow(clippy::expect_used)]

use gremlin_render::{SkinManifest, SpriteFrame, CANVAS_SIZE};
use std::path::{Path, PathBuf};

const BUILTIN_SKINS: [&str; 3] = ["baby", "default", "evolved"];
const REQUIRED_ANIMATIONS: [&str; 10] = [
    "angry", "coding", "dead", "dragged", "focus", "happy", "hungry", "idle", "sleep", "sick",
];
const ACCESSORY_ANCHORS: [&str; 5] = ["hat", "glasses", "outfit", "held", "aura"];
const MOVING_HEAD_POSES: [&str; 5] = ["happy", "sleep", "dead", "coding", "dragged"];

fn skins_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/skins")
}

#[test]
fn test_builtin_skins_expose_all_phase_six_features() {
    for skin_id in BUILTIN_SKINS {
        let skin_dir = skins_root().join(skin_id);
        let json = std::fs::read_to_string(skin_dir.join("manifest.json"))
            .expect("manifest intégré lisible");
        let manifest = SkinManifest::from_json(&json).expect("manifest intégré valide");

        assert_eq!(manifest.version, "2.0.0", "skin {skin_id}");
        // Chaque skin intégré déclare sa famille visuelle : sans elle, les
        // variantes d'accessoires dessinées pour lui ne seraient jamais servies.
        assert_eq!(
            manifest.accessory_style, skin_id,
            "style d'accessoire du skin {skin_id}"
        );
        assert_eq!(manifest.frame_width, CANVAS_SIZE, "skin {skin_id}");
        assert_eq!(manifest.frame_height, CANVAS_SIZE, "skin {skin_id}");

        for anchor_name in [
            "head",
            "effect_origin",
            "hat",
            "glasses",
            "outfit",
            "held",
            "aura",
        ] {
            let anchor = manifest
                .anchors
                .get(anchor_name)
                .expect("ancrage d'effet présent");
            assert!(
                (0..CANVAS_SIZE as i32).contains(&anchor.x)
                    && (0..CANVAS_SIZE as i32).contains(&anchor.y),
                "ancrage {anchor_name} hors canevas pour {skin_id}"
            );
        }

        for mood in MOVING_HEAD_POSES {
            assert!(
                manifest
                    .anchor_offsets_per_mood
                    .get(mood)
                    .is_some_and(|offsets| offsets.contains_key("head")),
                "correction de tête absente pour {skin_id}/{mood}"
            );
        }

        for mood in REQUIRED_ANIMATIONS {
            for anchor_name in ACCESSORY_ANCHORS {
                assert!(
                    manifest.anchor_for_mood(anchor_name, mood).is_some(),
                    "ancrage {anchor_name} non résolu pour {skin_id}/{mood}"
                );
            }
        }

        for animation_name in REQUIRED_ANIMATIONS {
            let animation = manifest
                .animations
                .get(animation_name)
                .expect("animation requise présente");
            assert!(
                !animation.frames.is_empty(),
                "animation {animation_name} vide pour {skin_id}"
            );

            for frame_key in &animation.frames {
                assert!(
                    !frame_key.contains('/') && !frame_key.contains('\\'),
                    "clé de frame non sûre pour {skin_id}: {frame_key}"
                );
                let frame_path = skin_dir.join(format!("{frame_key}.png"));
                let frame = SpriteFrame::from_png_file(&frame_path)
                    .expect("frame intégrée présente et décodable");
                manifest
                    .validate_frame_size(frame.width, frame.height)
                    .expect("dimensions conformes au manifest");
            }
        }

        // La posture studieuse réutilise les frames de `coding` : elle
        // n'introduit aucun PNG, et les skins n'ont pas eu à être redessinés.
        let focus = manifest
            .animations
            .get("focus")
            .expect("animation focus présente");
        let coding = manifest
            .animations
            .get("coding")
            .expect("animation coding présente");
        assert_eq!(
            focus.frames, coding.frames,
            "focus doit reprendre les frames de coding pour {skin_id}"
        );
    }
}

/// Un pack antérieur à la phase 8 ne déclare pas `focus` : il doit rester
/// chargeable, l'application retombant alors sur `coding` puis `idle`.
#[test]
fn test_a_skin_without_focus_animation_stays_valid() {
    let json = r#"{
        "id": "legacy",
        "name": "Pack ancien",
        "author": "Communauté",
        "version": "2.0.0",
        "frame_width": 64,
        "frame_height": 64,
        "animations": {
            "idle": { "frames": ["idle_0"], "frame_duration_ms": 200 },
            "coding": { "frames": ["coding_0"], "frame_duration_ms": 200 }
        }
    }"#;

    let manifest = SkinManifest::from_json(json).expect("skin sans focus accepté");
    assert!(!manifest.animations.contains_key("focus"));
    assert!(manifest.animations.contains_key("coding"));
}
