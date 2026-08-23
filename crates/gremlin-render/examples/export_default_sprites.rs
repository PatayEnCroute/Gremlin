//! Exporte les sprites procéduraux par défaut en fichiers PNG.
//!
//! Cet outil remplace un ancien test qui écrivait dans `assets/skins/default` à chaque
//! `cargo test` : la génération d'assets est une action explicite, pas un effet de bord
//! de la suite de tests.
//!
//! ```text
//! cargo run -p gremlin-render --example export_default_sprites [dossier_de_sortie]
//! ```
//!
//! Sans argument, les fichiers sont écrits dans `assets/skins/default` relativement au
//! répertoire courant (typiquement la racine du dépôt).

use gremlin_render::SpriteAtlas;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out_dir = std::env::args()
        .nth(1)
        .map_or_else(|| PathBuf::from("assets/skins/default"), PathBuf::from);

    println!("=== Export des sprites procéduraux Gremlin ===");
    println!("Dossier de sortie : {}", out_dir.display());

    std::fs::create_dir_all(&out_dir)?;

    let mut atlas = SpriteAtlas::new();
    atlas.load_default_procedural_sprites();

    for key in SpriteAtlas::DEFAULT_PROCEDURAL_KEYS {
        let out_path = out_dir.join(format!("{key}.png"));
        atlas.save_frame_to_png(key, &out_path)?;
        println!("  -> {}", out_path.display());
    }

    println!(
        "\n{} sprite(s) exporté(s) avec succès.",
        SpriteAtlas::DEFAULT_PROCEDURAL_KEYS.len()
    );
    Ok(())
}
