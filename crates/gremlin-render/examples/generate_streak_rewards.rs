//! Dessine les trois cosmétiques de série de la phase 8 et écrit leurs assets.
//!
//! Les accessoires officiels sont des PNG embarqués dans l'exécutable. Les
//! produire ici plutôt qu'à la main donne trois choses : un dessin reproductible
//! octet pour octet, un placement calé sur les ancres réelles des skins, et un
//! diff lisible quand un trait change.
//!
//! ```text
//! cargo run -p gremlin-render --example generate_streak_rewards
//! ```
//!
//! ## Convention de placement
//!
//! Une frame d'accessoire est dessinée sur la toile pleine, **déjà en place sur
//! la morphologie par défaut**. Le compositeur la recale ensuite sur le skin
//! actif en soustrayant l'ancre déclarée de l'ancre du skin. Les trois
//! récompenses déclarent donc un seul dessin et une seule ancre — celle du skin
//! par défaut — et les variantes bébé et évoluée se contentent d'exister : leur
//! décalage vient des ancres du skin, pas d'un second dessin.
//!
//! Le résultat se **regarde**, il ne se lit pas : après génération, produire la
//! planche de contrôle avec `cargo run -p gremlin-render --example
//! accessory_proof_sheet`.

use gremlin_render::{PixelBuffer, SpriteAtlas, SpriteFrame, CANVAS_SIZE};
use std::path::{Path, PathBuf};

/// Côté de la toile, en entier signé, pour les calculs de dessin.
const SIDE: i32 = CANVAS_SIZE as i32;

/// Centre horizontal du familier sur la toile.
const CENTER_X: i32 = 32;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = std::env::args().nth(1).map_or_else(
        || PathBuf::from("assets/accessories/builtin"),
        PathBuf::from,
    );

    println!("=== Cosmétiques de série (phase 8) ===");
    println!("Dossier de sortie : {}", root.display());

    write_accessory(&root, "streak_leaf_pin", LEAF_PIN_MANIFEST, draw_leaf_pin)?;
    write_accessory(
        &root,
        "focus_headphones",
        FOCUS_HEADPHONES_MANIFEST,
        draw_focus_headphones,
    )?;
    write_accessory(&root, "aurora_aura", AURORA_AURA_MANIFEST, draw_aurora_aura)?;

    println!("\nTrois cosmétiques écrits. Regardez la planche de contrôle.");
    Ok(())
}

/// Écrit le manifeste et l'unique frame d'un accessoire.
fn write_accessory(
    root: &Path,
    id: &str,
    manifest_json: &str,
    draw: fn(&mut PixelBuffer),
) -> Result<(), Box<dyn std::error::Error>> {
    let dir = root.join(id);
    std::fs::create_dir_all(&dir)?;
    std::fs::write(dir.join("manifest.json"), manifest_json)?;

    let mut buffer = PixelBuffer::new(CANVAS_SIZE, CANVAS_SIZE);
    buffer.clear(0, 0, 0, 0);
    draw(&mut buffer);

    let key = format!("{id}_0");
    let frame = SpriteFrame::from_raw(CANVAS_SIZE, CANVAS_SIZE, buffer.as_bytes().to_vec())?;
    let mut atlas = SpriteAtlas::new();
    atlas.insert(key.clone(), frame);
    let path = dir.join(format!("{key}.png"));
    atlas.save_frame_to_png(&key, &path)?;
    println!("  -> {}", path.display());
    Ok(())
}

/// Peint un rectangle plein, en ignorant ce qui sort de la toile.
fn rect(buffer: &mut PixelBuffer, x: i32, y: i32, w: i32, h: i32, color: [u8; 4]) {
    for dy in 0..h {
        for dx in 0..w {
            let (px, py) = (x + dx, y + dy);
            if px >= 0 && py >= 0 && px < SIDE && py < SIDE {
                buffer.blend_pixel(px as u32, py as u32, color);
            }
        }
    }
}

/// Peint un masque ASCII à partir de son coin supérieur gauche.
///
/// Le dessin d'un petit sprite se relit ligne par ligne dans le code : une
/// dizaine d'appels de rectangle décriraient la même chose sans qu'on puisse
/// voir la forme.
fn stamp(buffer: &mut PixelBuffer, x: i32, y: i32, mask: &[&str], key: &[(char, [u8; 4])]) {
    for (row, line) in mask.iter().enumerate() {
        for (column, symbol) in line.chars().enumerate() {
            let Some((_, color)) = key.iter().find(|(candidate, _)| *candidate == symbol) else {
                continue;
            };
            rect(buffer, x + column as i32, y + row as i32, 1, 1, *color);
        }
    }
}

/// Feuille porte-bonheur : une pousse inclinée qui sort de la touffe du crâne.
///
/// Dessinée à la main : une feuille construite par formules sort symétrique, et
/// une feuille symétrique se lit comme un losange.
fn draw_leaf_pin(buffer: &mut PixelBuffer) {
    const MASK: [&str; 12] = [
        "......ll...",
        "....lllll..",
        "...lldlll..",
        "..lldddll..",
        "..lddddl...",
        ".llddddl...",
        ".lldddl....",
        ".llddl.....",
        "..lll......",
        "..ss.......",
        ".ss........",
        ".s.........",
    ];
    const KEY: [(char, [u8; 4]); 3] = [
        ('l', [124, 214, 128, 255]),
        ('d', [58, 138, 76, 255]),
        ('s', [126, 96, 56, 255]),
    ];

    // Posée sur le sommet du crâne, légèrement à droite de l'axe : centrée, elle
    // entrerait en concurrence avec la touffe de cheveux.
    stamp(buffer, CENTER_X - 1, 0, &MASK, &KEY);
}

/// Casque de concentration : arceau fin par-dessus le crâne et deux coussinets.
///
/// Le calque « lunettes » passe au-dessus du corps : l'arceau peut donc couvrir
/// le sommet du crâne sans être masqué. L'arc est **mince** — un bandeau plein
/// se lirait comme un bloc gris posé sur la tête.
fn draw_focus_headphones(buffer: &mut PixelBuffer) {
    const BAND: [u8; 4] = [66, 72, 90, 255];
    const BAND_LIGHT: [u8; 4] = [128, 138, 162, 255];
    const PAD: [u8; 4] = [34, 38, 50, 255];
    const PAD_EDGE: [u8; 4] = [92, 100, 122, 255];
    const LED: [u8; 4] = [132, 210, 255, 255];

    // Demi-ellipse supérieure : demi-axes 15 × 10, centrée à hauteur d'oreille.
    // Un arc plus large et plus haut flotterait au-dessus du crâne au lieu de
    // l'épouser.
    const ARC_CENTER_Y: i32 = 18;
    const SEMI_X: i32 = 15;
    const SEMI_Y: i32 = 10;

    for x in -SEMI_X..=SEMI_X {
        // Ordonnée exacte de l'ellipse pour cette abscisse, arrondie à l'entier.
        let ratio = 1.0 - f64::from(x * x) / f64::from(SEMI_X * SEMI_X);
        if ratio <= 0.0 {
            continue;
        }
        let dy = (f64::from(SEMI_Y) * ratio.sqrt()).round() as i32;
        let top = ARC_CENTER_Y - dy;
        // Deux pixels d'épaisseur, plus un reflet sur la crête au sommet.
        rect(buffer, CENTER_X + x, top, 1, 2, BAND);
        if x.abs() <= 5 {
            rect(buffer, CENTER_X + x, top, 1, 1, BAND_LIGHT);
        }
    }

    // Coussinets posés sur les oreilles, de part et d'autre du crâne.
    for x in [CENTER_X - 17, CENTER_X + 13] {
        rect(buffer, x, 15, 4, 11, PAD);
        rect(buffer, x, 15, 4, 1, PAD_EDGE);
        rect(buffer, x, 25, 4, 1, PAD_EDGE);
    }
    // Témoin lumineux : le seul point clair, il signale la session en cours.
    rect(buffer, CENTER_X + 14, 18, 2, 2, LED);
}

/// Aura aurorale : halo annulaire translucide autour de la silhouette.
///
/// Le calque « aura » est dessine **derriere** le corps : un voile plein serait
/// entierement masque. Seul un anneau, qui deborde de la silhouette, se voit.
fn draw_aurora_aura(buffer: &mut PixelBuffer) {
    // Centre de gravite visuel du familier, legerement sous le milieu de toile.
    const CENTER_Y: i32 = 34;
    const INNER: i32 = 23;
    const OUTER: i32 = 29;

    for y in 0..SIDE {
        for x in 0..SIDE {
            let dx = x - CENTER_X;
            // L'anneau est aplati verticalement : la silhouette est plus haute
            // que large, un cercle parfait laisserait des trous sur les cotes.
            let dy = (y - CENTER_Y) * 4 / 5;
            let distance_squared = dx * dx + dy * dy;
            if !(INNER * INNER..=OUTER * OUTER).contains(&distance_squared) {
                continue;
            }

            // Teinte parcourue du violet au vert selon la hauteur, sans degrade
            // par pixel : trois paliers suffisent a l'echelle du sprite.
            let color = match (y - CENTER_Y + OUTER) / 20 {
                0 => [188, 140, 255, 150],
                1 => [126, 176, 255, 165],
                _ => [104, 226, 190, 150],
            };
            rect(buffer, x, y, 1, 1, color);
        }
    }
}

const LEAF_PIN_MANIFEST: &str = r#"{
  "id": "streak_leaf_pin",
  "name": "Feuille porte-bonheur",
  "author": "Gremlin Studio",
  "version": "1.0.0",
  "category": "Hat",
  "description": "Récompense des trois premiers jours de commits consécutifs.",
  "frame_width": 64,
  "frame_height": 64,
  "frames": [
    "streak_leaf_pin_0"
  ],
  "frame_duration_ms": 200,
  "offsets_per_mood": {},
  "anchor": {
    "x": 16,
    "y": 4
  },
  "variants": {
    "baby": {},
    "evolved": {}
  },
  "clip_to_body": false
}
"#;

const FOCUS_HEADPHONES_MANIFEST: &str = r#"{
  "id": "focus_headphones",
  "name": "Casque de concentration",
  "author": "Gremlin Studio",
  "version": "1.0.0",
  "category": "Glasses",
  "description": "Récompense de sept jours de commits consécutifs.",
  "frame_width": 64,
  "frame_height": 64,
  "frames": [
    "focus_headphones_0"
  ],
  "frame_duration_ms": 200,
  "offsets_per_mood": {},
  "anchor": {
    "x": 16,
    "y": 20
  },
  "variants": {
    "baby": {},
    "evolved": {}
  },
  "clip_to_body": false
}
"#;

const AURORA_AURA_MANIFEST: &str = r#"{
  "id": "aurora_aura",
  "name": "Aura aurorale",
  "author": "Gremlin Studio",
  "version": "1.0.0",
  "category": "Aura",
  "description": "Récompense de trente jours de commits consécutifs.",
  "frame_width": 64,
  "frame_height": 64,
  "frames": [
    "aurora_aura_0"
  ],
  "frame_duration_ms": 200,
  "offsets_per_mood": {},
  "anchor": {
    "x": 0,
    "y": 0
  },
  "variants": {
    "baby": {},
    "evolved": {}
  },
  "clip_to_body": false
}
"#;
