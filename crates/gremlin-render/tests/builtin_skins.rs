//! Validation des skins distribués avec l'application.

#![allow(clippy::expect_used)]

use gremlin_render::{SkinManifest, SpriteFrame, CANVAS_SIZE};
use std::path::{Path, PathBuf};

const BUILTIN_SKINS: [&str; 3] = ["baby", "default", "evolved"];
const REQUIRED_ANIMATIONS: [&str; 9] = [
    "angry", "coding", "dead", "dragged", "happy", "hungry", "idle", "sleep", "sick",
];

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
        assert_eq!(manifest.frame_width, CANVAS_SIZE, "skin {skin_id}");
        assert_eq!(manifest.frame_height, CANVAS_SIZE, "skin {skin_id}");

        for anchor_name in ["head", "effect_origin"] {
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
    }
}
