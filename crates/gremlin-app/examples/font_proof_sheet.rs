//! Planche de contrôle de la police dessinée à la main.
//!
//! Une police bitmap ne se juge pas dans son code source : il faut la regarder.
//! Cet exemple rend l'intégralité des glyphes, les capitales accentuées et des
//! libellés réels du panneau, à chaque agrandissement, puis écrit une image PNG.
//!
//! ```bash
//! cargo run -p gremlin-app --example font_proof_sheet
//! ```
//!
//! L'image est écrite dans `target/font_proof_sheet.png`. Après chaque retouche
//! d'un dessin dans `ui/font/tables.rs`, relancer cette commande et comparer.

use gremlin_app::ui::font;
use gremlin_app::ui::layout::{FontSize, GlyphChoice};
use gremlin_app::ui::theme::Theme;
use gremlin_render::PixelBuffer;
use std::process::ExitCode;

/// Largeur de la planche, en pixels.
const SHEET_WIDTH: u32 = 900;

/// Hauteur de la planche, en pixels.
const SHEET_HEIGHT: u32 = 720;

/// Palette employée par la planche.
const THEME: Theme = Theme::DARK;

/// Marge intérieure de la planche.
const MARGIN: i32 = 16;

/// Chemin de sortie, sous le répertoire de compilation (ignoré par Git).
const OUTPUT_PATH: &str = "target/font_proof_sheet.png";

/// Lignes de la planche : intitulé, texte à rendre, agrandissement.
const SPECIMENS: &[(&str, &str, u32)] = &[
    ("Capitales", "ABCDEFGHIJKLMNOPQRSTUVWXYZ", 1),
    ("Bas de casse", "abcdefghijklmnopqrstuvwxyz", 1),
    ("Chiffres", "0123456789", 1),
    (
        "Ponctuation",
        "., : ; ! ? ' \" ( ) [ ] { } - _ + = * / \\ | < >",
        1,
    ),
    ("Signes", "% & $ # @ ^ ~ ` ° • › ‹ « » — … ↑ ↓ → ←", 1),
    ("Capitales accentuees", "É È Ê Ë À Â Ä Î Ï Ô Ö Ù Û Ü Ç", 1),
    (
        "Bas de casse accentue",
        "é è ê ë à â ä î ï ô ö ù û ü ç œ æ ñ",
        1,
    ),
    ("Repli inconnu", "漢字 🐉 \u{2603}", 1),
    ("Libelle x1", "Échelle de zoom : 3x — Dépôt Gremlin", 1),
    ("Libelle x2", "Échelle de zoom : 3x", 2),
    ("Libelle x3", "Réanimer", 3),
    (
        "Phrase longue",
        "Ajuste le zoom de 1x à 5x sans flou pixel-art, et gère « l'unicode ».",
        1,
    ),
];

fn main() -> ExitCode {
    let mut buffer = PixelBuffer::new(SHEET_WIDTH, SHEET_HEIGHT);
    fill(
        &mut buffer,
        0,
        0,
        SHEET_WIDTH as i32,
        SHEET_HEIGHT as i32,
        THEME.bg_primary,
    );

    let label_choice = GlyphChoice {
        face: FontSize::Medium,
        upscale: 1,
    };

    let mut y = MARGIN;
    for (label, specimen, upscale) in SPECIMENS {
        let choice = GlyphChoice {
            face: FontSize::Medium,
            upscale: *upscale,
        };

        // Intitulé de la ligne, en gris atténué.
        font::draw(
            &mut buffer,
            label,
            MARGIN,
            y,
            THEME.text_muted,
            label_choice,
        );

        // Repère de la largeur mesurée : il doit affleurer la fin du texte.
        let specimen_x = MARGIN + 170;
        let measured = font::measure(specimen, choice);
        fill(
            &mut buffer,
            specimen_x + measured,
            y,
            1,
            FontSize::Medium.cell_height() * (*upscale as i32),
            THEME.accent,
        );

        font::draw(
            &mut buffer,
            specimen,
            specimen_x,
            y,
            THEME.text_primary,
            choice,
        );

        y += (FontSize::Medium.cell_height() + 7) * (*upscale as i32);
    }

    // Bloc de retour à la ligne automatique, encadré à sa largeur exacte.
    y += 12;
    let wrap_width = 300;
    let paragraph = "Le panneau expose enfin les descriptions completes : \
                     plus de coupure a vingt-huit caracteres, et les mots ne \
                     sont plus tranches en leur milieu.";

    font::draw(
        &mut buffer,
        "Retour a la ligne",
        MARGIN,
        y,
        THEME.text_muted,
        label_choice,
    );

    let box_x = MARGIN + 170;
    fill(&mut buffer, box_x + wrap_width, y, 1, 140, THEME.border);

    for (line_index, line) in font::wrap(paragraph, wrap_width, label_choice)
        .iter()
        .enumerate()
    {
        font::draw(
            &mut buffer,
            line,
            box_x,
            y + (line_index as i32) * (FontSize::Medium.cell_height() + 3),
            THEME.text_primary,
            label_choice,
        );
    }

    match write_png(&buffer) {
        Ok(()) => {
            println!("Planche ecrite dans {OUTPUT_PATH}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("Echec d'ecriture de la planche : {e}");
            ExitCode::FAILURE
        }
    }
}

/// Remplit un rectangle de couleur opaque.
fn fill(buffer: &mut PixelBuffer, x: i32, y: i32, width: i32, height: i32, color: [u8; 4]) {
    for dy in 0..height {
        for dx in 0..width {
            let px = x + dx;
            let py = y + dy;
            if px >= 0 && py >= 0 {
                buffer.blend_pixel(px as u32, py as u32, color);
            }
        }
    }
}

/// Encode la planche en PNG.
fn write_png(buffer: &PixelBuffer) -> Result<(), Box<dyn std::error::Error>> {
    let image =
        image::RgbaImage::from_raw(buffer.width(), buffer.height(), buffer.as_bytes().to_vec())
            .ok_or("dimensions du tampon incompatibles avec l'image")?;
    image.save(OUTPUT_PATH)?;
    Ok(())
}
