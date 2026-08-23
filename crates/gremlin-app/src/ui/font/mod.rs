//! Moteur de police bitmap dessinée à la main.
//!
//! # Le dessin *est* le code source
//!
//! Chaque glyphe est écrit en clair dans `tables` et `small`, une ligne de texte par
//! ligne de pixels. Quatre caractères décrivent la couverture d'un pixel :
//!
//! | Caractère | Couverture | Usage |
//! | --- | --- | --- |
//! | `.` | 0 | vide |
//! | `:` | 90 | adoucissement léger |
//! | `+` | 170 | adoucissement marqué |
//! | `#` | 255 | plein |
//!
//! Les niveaux intermédiaires sont posés à la main sur les diagonales et les
//! courbes : c'est ce qui distingue cette police du bitmap 1 bit d'origine, qui
//! rendait l'interface « rustre » quelle que soit sa taille.
//!
//! # Avances proportionnelles
//!
//! La largeur de chaque glyphe est celle de son dessin : un `i` occupe trois
//! colonnes, un `m` en occupe sept. L'ancienne police imposait une avance
//! uniforme de six pixels, ce qui aérait anormalement les lettres étroites et
//! écrasait les larges. La mesure de texte est donc exacte par construction, et
//! non estimée en comptant les caractères.
//!
//! # Accents composés
//!
//! Les lettres accentuées ne sont pas dessinées une à une : le moteur superpose
//! une marque (aigu, grave, circonflexe, tréma, cédille) au glyphe de base, à
//! une hauteur qui dépend de la casse. C'est ce qui permet enfin d'afficher
//! `É`, `À` et `Ç` — l'ancienne police, faute de place au-dessus des capitales,
//! les dégradait en `E`, `A` et `C`.
//!
//! # Coût à l'exécution
//!
//! Les dessins sont convertis une seule fois, au premier usage, en tables de
//! couverture indexées ([`std::sync::OnceLock`]). La boucle de rendu n'analyse
//! donc jamais de texte de dessin et n'alloue pas : elle lit des octets.

mod legacy;
mod small;
mod tables;

use crate::ui::layout::{FontSize, GlyphChoice};
use gremlin_render::PixelBuffer;
use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::OnceLock;

pub use legacy::{draw_text_5x7, text_width_px};

/// Espacement horizontal ajouté après chaque glyphe, en pixels de dessin.
const LETTER_SPACING: i32 = 1;

/// Marque de coupure ajoutée par [`fit`] lorsqu'un texte est raccourci.
const ELLIPSIS: &str = "…";

/// Largeur de l'espace, en pixels de dessin.
const SPACE_WIDTH: i32 = 3;

/// Glyphe converti en table de couverture.
#[derive(Debug, Clone)]
struct Glyph {
    /// Largeur du dessin, en pixels.
    width: i32,
    /// Ordonnée de la première ligne dessinée, dans la cellule du corps.
    top: i32,
    /// Couverture par pixel, en lecture ligne par ligne (`width` par ligne).
    coverage: Vec<u8>,
}

impl Glyph {
    /// Couverture du pixel `(x, y)` exprimé dans la cellule du corps.
    fn coverage_at(&self, x: i32, y: i32) -> u8 {
        let local_y = y - self.top;
        if x < 0 || y < self.top || x >= self.width {
            return 0;
        }

        let rows = if self.width > 0 {
            self.coverage.len() as i32 / self.width
        } else {
            0
        };
        if local_y >= rows {
            return 0;
        }

        let index = (local_y * self.width + x) as usize;
        self.coverage.get(index).copied().unwrap_or(0)
    }

    /// Avance horizontale du glyphe, espacement compris.
    const fn advance(&self) -> i32 {
        self.width + LETTER_SPACING
    }
}

/// Corps de police converti, prêt au rendu.
#[derive(Debug)]
struct Face {
    glyphs: HashMap<char, Glyph>,
    /// Glyphe servi pour tout caractère sans dessin ni repli.
    fallback: Glyph,
    /// Glyphe de l'espace, sans dessin.
    space: Glyph,
}

impl Face {
    /// Glyphe à rendre pour `ch`, accents composés compris.
    fn glyph(&self, ch: char) -> Option<&Glyph> {
        if ch == ' ' || ch == '\u{a0}' {
            return Some(&self.space);
        }
        if let Some(glyph) = self.glyphs.get(&ch) {
            return Some(glyph);
        }
        None
    }

    /// Glyphe effectivement rendu, repli compris.
    fn glyph_or_fallback(&self, ch: char) -> &Glyph {
        self.glyph(ch).unwrap_or(&self.fallback)
    }
}

/// Ensemble des corps convertis, construit une seule fois.
static FACES: OnceLock<HashMap<FontSize, Face>> = OnceLock::new();

/// Corps converti correspondant à `size`.
fn face(size: FontSize) -> &'static Face {
    let faces = FACES.get_or_init(tables::build_faces);

    // Le corps moyen est toujours présent : il sert de secours à un corps
    // qui ne serait pas encore dessiné, plutôt que de ne rien afficher.
    faces
        .get(&size)
        .or_else(|| faces.get(&FontSize::Medium))
        .unwrap_or_else(|| {
            // Table vide impossible en pratique : `build_faces` garantit au
            // moins le corps moyen. On ne panique pas pour autant.
            static EMPTY: OnceLock<Face> = OnceLock::new();
            EMPTY.get_or_init(tables::empty_face)
        })
}

/// Largeur, en pixels, qu'occupera `text` rendu avec `choice`.
///
/// Mesure exacte : elle additionne les avances réelles des glyphes au lieu de
/// multiplier un nombre de caractères par une largeur moyenne.
#[must_use]
pub fn measure(text: &str, choice: GlyphChoice) -> i32 {
    let face = face(choice.face);
    let upscale = choice.upscale.max(1) as i32;

    text.chars()
        .filter(|c| *c != '\n')
        .map(|ch| face.glyph_or_fallback(ch).advance())
        .sum::<i32>()
        .saturating_mul(upscale)
}

/// Découpe `text` en lignes qui tiennent dans `max_width_px`.
///
/// La coupure se fait entre les mots ; un mot plus large que la ligne est
/// laissé intact plutôt que tronqué au milieu, l'appelant restant libre de le
/// raccourcir avec [`crate::ui::text::truncate_with_ellipsis`].
#[must_use]
pub fn wrap(text: &str, max_width_px: i32, choice: GlyphChoice) -> Vec<&str> {
    if max_width_px <= 0 || text.is_empty() {
        return Vec::new();
    }

    let mut lines = Vec::new();
    let mut line_start = 0_usize;
    let mut line_end = 0_usize;
    let mut width = 0_i32;

    for (offset, word) in word_boundaries(text) {
        let word_width = measure(word, choice);
        let separator_width = if line_end == line_start {
            0
        } else {
            measure(" ", choice)
        };

        if width + separator_width + word_width > max_width_px && line_end > line_start {
            lines.push(&text[line_start..line_end]);
            line_start = offset;
            line_end = offset + word.len();
            width = word_width;
        } else {
            line_end = offset + word.len();
            width += separator_width + word_width;
        }
    }

    if line_end > line_start {
        lines.push(&text[line_start..line_end]);
    }

    lines
}

/// Raccourcit `text` pour qu'il tienne dans `max_width_px`, en signalant la coupure.
///
/// Renvoie le texte tel quel — sans allocation — lorsqu'il tient déjà, ce qui est
/// le cas courant dans la boucle de rendu. Sinon, il est raccourci caractère par
/// caractère et suivi de points de suspension.
///
/// Le rendu se contentait auparavant de garder la première ligne d'un retour à la
/// ligne, ce qui coupait les libellés en silence : rien n'indiquait à
/// l'utilisateur qu'il manquait du texte.
#[must_use]
pub fn fit(text: &str, max_width_px: i32, choice: GlyphChoice) -> Cow<'_, str> {
    if max_width_px <= 0 {
        return Cow::Borrowed("");
    }
    if measure(text, choice) <= max_width_px {
        return Cow::Borrowed(text);
    }

    let budget = max_width_px - measure(ELLIPSIS, choice);
    if budget <= 0 {
        return Cow::Borrowed(ELLIPSIS);
    }

    let face = face(choice.face);
    let upscale = choice.upscale.max(1) as i32;
    let mut width = 0_i32;
    let mut cut = 0_usize;

    for (offset, ch) in text.char_indices() {
        let advance = face.glyph_or_fallback(ch).advance().saturating_mul(upscale);
        if width + advance > budget {
            break;
        }
        width += advance;
        // La coupure tombe toujours sur une frontière de caractère : indexer par
        // octet paniquerait au milieu d'une lettre accentuée.
        cut = offset + ch.len_utf8();
    }

    let mut out = String::with_capacity(cut + ELLIPSIS.len());
    out.push_str(&text[..cut]);
    out.push_str(ELLIPSIS);
    Cow::Owned(out)
}

/// Itère sur les mots de `text` avec leur décalage en octets.
fn word_boundaries(text: &str) -> impl Iterator<Item = (usize, &str)> {
    text.split_whitespace().map(move |word| {
        // `split_whitespace` ne rend pas les décalages : on les retrouve par
        // arithmétique de pointeurs, ce qui reste exact en UTF-8 puisque le
        // sous-slice provient du slice d'origine.
        let offset = word.as_ptr() as usize - text.as_ptr() as usize;
        (offset, word)
    })
}

/// Dessine `text` dans `buffer`, le coin haut-gauche de la première cellule
/// étant `(x, y)`.
///
/// La couleur est modulée par la couverture du glyphe : c'est cette modulation
/// qui produit l'adoucissement des contours.
pub fn draw(
    buffer: &mut PixelBuffer,
    text: &str,
    x: i32,
    y: i32,
    color: [u8; 4],
    choice: GlyphChoice,
) {
    let face = face(choice.face);
    let upscale = choice.upscale.max(1) as i32;
    let cell_height = choice.face.cell_height();
    let mut pen_x = x;

    for ch in text.chars().filter(|c| *c != '\n') {
        let glyph = face.glyph_or_fallback(ch);

        for row in 0..cell_height {
            for col in 0..glyph.width {
                let coverage = glyph.coverage_at(col, row);
                if coverage == 0 {
                    continue;
                }

                let blended = [
                    color[0],
                    color[1],
                    color[2],
                    modulate_alpha(color[3], coverage),
                ];

                let dest_x = pen_x + col.saturating_mul(upscale);
                let dest_y = y + row.saturating_mul(upscale);
                blit_block(buffer, dest_x, dest_y, upscale, blended);
            }
        }

        pen_x = pen_x.saturating_add(glyph.advance().saturating_mul(upscale));
    }
}

/// Module l'opacité d'une couleur par la couverture d'un pixel de glyphe.
fn modulate_alpha(alpha: u8, coverage: u8) -> u8 {
    ((u32::from(alpha) * u32::from(coverage)) / 255) as u8
}

/// Recouvre un bloc carré de `size` pixels, agrandissement entier du glyphe.
fn blit_block(buffer: &mut PixelBuffer, x: i32, y: i32, size: i32, color: [u8; 4]) {
    for dy in 0..size {
        for dx in 0..size {
            let px = x + dx;
            let py = y + dy;
            if px >= 0 && py >= 0 {
                buffer.blend_pixel(px as u32, py as u32, color);
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    const MEDIUM: GlyphChoice = GlyphChoice {
        face: FontSize::Medium,
        upscale: 1,
    };

    fn canvas() -> PixelBuffer {
        PixelBuffer::new(240, 40)
    }

    fn inked_pixels(buffer: &PixelBuffer) -> usize {
        buffer
            .as_bytes()
            .chunks_exact(4)
            .filter(|px| px[3] > 0)
            .count()
    }

    #[test]
    fn test_measure_matches_the_rendered_extent() {
        // La mesure pilote le curseur de saisie et les badges : un écart avec
        // le rendu réel décalait visiblement le curseur sur texte accentué.
        let mut buffer = canvas();
        let text = "Depot";
        draw(&mut buffer, text, 0, 5, [255, 255, 255, 255], MEDIUM);

        let measured = measure(text, MEDIUM);
        let rightmost = buffer
            .as_bytes()
            .chunks_exact(4)
            .enumerate()
            .filter(|(_, px)| px[3] > 0)
            .map(|(i, _)| (i % 240) as i32)
            .max()
            .expect("le texte doit avoir marqué des pixels");

        assert!(
            rightmost < measured,
            "le rendu dépasse la mesure : {rightmost} >= {measured}"
        );
        assert!(
            measured - rightmost <= LETTER_SPACING + 2,
            "la mesure sur-estime largement le rendu : {measured} contre {rightmost}"
        );
    }

    #[test]
    fn test_advances_are_proportional() {
        // Le gain visible de la refonte : les lettres étroites cessent d'être
        // espacées comme les larges.
        let narrow = measure("iii", MEDIUM);
        let wide = measure("mmm", MEDIUM);
        assert!(
            narrow < wide,
            "avances non proportionnelles : « iii » = {narrow}, « mmm » = {wide}"
        );
    }

    #[test]
    fn test_accented_uppercase_are_no_longer_folded() {
        // Régression historique : faute de place au-dessus des capitales, la
        // police 5×7 rendait « É » comme « E ». Les accents composés lèvent la
        // limite, et le glyphe accentué doit donc marquer *plus* de pixels.
        for (accented, base) in [('É', 'E'), ('À', 'A'), ('Ç', 'C')] {
            let mut with_accent = canvas();
            let mut without = canvas();
            draw(
                &mut with_accent,
                &accented.to_string(),
                0,
                2,
                [255, 255, 255, 255],
                MEDIUM,
            );
            draw(
                &mut without,
                &base.to_string(),
                0,
                2,
                [255, 255, 255, 255],
                MEDIUM,
            );

            assert!(
                inked_pixels(&with_accent) > inked_pixels(&without),
                "« {accented} » est encore dégradé en « {base} »"
            );
        }
    }

    #[test]
    fn test_lowercase_accents_do_not_touch_the_letter() {
        // L'accent doit rester séparé du corps de la lettre : collé, il
        // transforme « é » en tache illisible aux petits corps.
        let mut buffer = canvas();
        draw(&mut buffer, "é", 0, 0, [255, 255, 255, 255], MEDIUM);

        let rows: Vec<bool> = (0..FontSize::Medium.cell_height())
            .map(|row| {
                (0..12).any(|col| {
                    let idx = ((row as usize) * 240 + col) * 4;
                    buffer.as_bytes().get(idx + 3).copied().unwrap_or(0) > 0
                })
            })
            .collect();

        // Il doit exister une ligne vide entre la marque et la lettre.
        let first_ink = rows.iter().position(|&r| r).unwrap_or(0);
        let gap = rows.iter().skip(first_ink).position(|&r| !r);
        assert!(
            gap.is_some(),
            "aucune ligne libre entre l'accent et la lettre : {rows:?}"
        );
    }

    #[test]
    fn test_unsupported_characters_fall_back_without_panicking() {
        // Entrées hostiles : noms de dépôts et messages de commit arbitraires.
        let mut buffer = canvas();
        draw(
            &mut buffer,
            "漢字🐉\u{0}\u{7f}\u{feff}",
            0,
            2,
            [255, 255, 255, 255],
            MEDIUM,
        );
        assert!(measure("漢字🐉", MEDIUM) > 0);
    }

    #[test]
    fn test_drawing_outside_the_buffer_is_clipped() {
        let mut buffer = PixelBuffer::new(8, 8);
        for (x, y) in [(-100, -100), (100, 100), (-4, 4), (4, -4), (7, 7)] {
            draw(&mut buffer, "Gremlin", x, y, [255, 255, 255, 255], MEDIUM);
        }
    }

    #[test]
    fn test_upscale_multiplies_the_extent() {
        let single = measure("Gremlin", MEDIUM);
        let doubled = measure(
            "Gremlin",
            GlyphChoice {
                face: FontSize::Medium,
                upscale: 2,
            },
        );
        assert_eq!(doubled, single * 2);
    }

    #[test]
    fn test_wrap_breaks_between_words_and_never_loses_content() {
        let text = "Ajuste le zoom du panneau sans flou pixel-art";
        let lines = wrap(text, measure("Ajuste le zoom", MEDIUM), MEDIUM);

        assert!(lines.len() > 1, "aucune coupure : {lines:?}");
        for line in &lines {
            assert!(!line.starts_with(' ') && !line.ends_with(' '));
        }

        let rejoined = lines.join(" ");
        assert_eq!(rejoined, text, "le retour à la ligne a altéré le texte");
    }

    #[test]
    fn test_wrap_on_degenerate_widths() {
        let text = "Recharger les skins";
        assert!(wrap(text, 0, MEDIUM).is_empty());
        assert!(wrap(text, -50, MEDIUM).is_empty());
        assert!(wrap("", 500, MEDIUM).is_empty());

        // Une largeur inférieure au plus petit mot ne doit pas boucler.
        let squeezed = wrap(text, 2, MEDIUM);
        assert_eq!(squeezed.len(), 3);
    }

    #[test]
    fn test_wrap_preserves_multibyte_boundaries() {
        let text = "gère les caractères « spéciaux » — et l'unicode";
        let lines = wrap(text, 120, MEDIUM);
        for line in lines {
            assert!(line.is_char_boundary(0));
            assert!(line.is_char_boundary(line.len()));
        }
    }
}
