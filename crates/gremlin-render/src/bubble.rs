//! Rendu sans état des phylactères pixel-art du compagnon.

use crate::PixelBuffer;

const MAX_LINES: usize = 2;
const GLYPHS_PER_LINE: usize = 9;
const CELL_COUNT: usize = MAX_LINES * GLYPHS_PER_LINE;
const GLYPH_WIDTH: i32 = 5;
const GLYPH_HEIGHT: usize = 7;
const GLYPH_ADVANCE: i32 = 6;
const LINE_ADVANCE: i32 = 8;
const TAIL_HEIGHT: i32 = 4;

/// Rectangle entier exprimé dans les coordonnées natives du framebuffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BubbleRect {
    /// Abscisse du bord gauche.
    pub x: i32,
    /// Ordonnée du bord haut.
    pub y: i32,
    /// Largeur totale, queue exclue.
    pub width: u32,
    /// Hauteur totale, queue incluse.
    pub height: u32,
}

impl BubbleRect {
    /// Rectangle par défaut du compagnon 64×64.
    #[must_use]
    pub const fn companion_default() -> Self {
        Self {
            x: 2,
            y: 1,
            width: 60,
            height: 23,
        }
    }
}

/// Vue immuable d'une bulle préparée par l'orchestrateur.
#[derive(Debug, Clone, Copy)]
pub struct SpeechBubbleView<'a> {
    /// Texte court à afficher.
    pub text: &'a str,
    /// Opacité globale de zéro à 255.
    pub opacity: u8,
    /// Rectangle souhaité, automatiquement borné au framebuffer.
    pub bounds: BubbleRect,
    /// Point de la tête visé par la queue.
    pub target_anchor: (i32, i32),
}

/// Compositeur de bulle pixel-art.
pub struct SpeechBubbleRenderer;

impl SpeechBubbleRenderer {
    /// Indique si le texte tient dans les deux lignes sans troncature.
    ///
    /// Cette méthode partage exactement l'algorithme du rendu : les appelants
    /// peuvent valider leurs libellés sans dupliquer une approximation.
    #[must_use]
    pub fn text_fits(text: &str) -> bool {
        !layout_text(text).1
    }

    /// Dessine une bulle entièrement clippée dans le framebuffer.
    pub fn render(buffer: &mut PixelBuffer, view: SpeechBubbleView<'_>) {
        if view.opacity == 0 || buffer.width() == 0 || buffer.height() == 0 {
            return;
        }

        let bounds = clamp_bounds(buffer, view.bounds);
        let body_height = (bounds.height as i32 - TAIL_HEIGHT).max(3);
        let border = [28, 24, 38, scaled_alpha(245, view.opacity)];
        let fill = [255, 248, 219, scaled_alpha(238, view.opacity)];
        let text_color = [42, 35, 52, view.opacity];

        for offset_y in 0..body_height {
            for offset_x in 0..bounds.width as i32 {
                let is_border = offset_x == 0
                    || offset_y == 0
                    || offset_x == bounds.width as i32 - 1
                    || offset_y == body_height - 1;
                blend_at(
                    buffer,
                    bounds.x + offset_x,
                    bounds.y + offset_y,
                    if is_border { border } else { fill },
                );
            }
        }

        render_tail(
            buffer,
            bounds,
            body_height,
            view.target_anchor.0,
            border,
            fill,
        );

        let (cells, _) = layout_text(view.text);
        let text_x = bounds.x + 2;
        let text_y = bounds.y + 2;
        for line in 0..MAX_LINES {
            for column in 0..GLYPHS_PER_LINE {
                let glyph = cells[line * GLYPHS_PER_LINE + column];
                if glyph == '\0' {
                    continue;
                }
                draw_glyph(
                    buffer,
                    text_x + column as i32 * GLYPH_ADVANCE,
                    text_y + line as i32 * LINE_ADVANCE,
                    glyph,
                    text_color,
                );
            }
        }
    }
}

fn clamp_bounds(buffer: &PixelBuffer, requested: BubbleRect) -> BubbleRect {
    let width = requested.width.clamp(1, buffer.width());
    let height = requested.height.clamp(1, buffer.height());
    let max_x = (buffer.width() - width) as i32;
    let max_y = (buffer.height() - height) as i32;
    BubbleRect {
        x: requested.x.clamp(0, max_x),
        y: requested.y.clamp(0, max_y),
        width,
        height,
    }
}

fn render_tail(
    buffer: &mut PixelBuffer,
    bounds: BubbleRect,
    body_height: i32,
    target_x: i32,
    border: [u8; 4],
    fill: [u8; 4],
) {
    if bounds.height as i32 <= body_height {
        return;
    }

    let left = bounds.x + 3;
    let right = bounds.x + bounds.width as i32 - 4;
    let center = target_x.clamp(left.min(right), left.max(right));
    let top = bounds.y + body_height - 1;
    for (offset_y, half_width) in [(0_i32, 3_i32), (1, 2), (2, 1), (3, 0)] {
        for offset_x in -half_width..=half_width {
            let color = if offset_x.unsigned_abs() == half_width as u32 {
                border
            } else {
                fill
            };
            blend_at(buffer, center + offset_x, top + offset_y, color);
        }
    }
}

fn layout_text(text: &str) -> ([char; CELL_COUNT], bool) {
    let mut cells = ['\0'; CELL_COUNT];
    let mut line = 0usize;
    let mut column = 0usize;
    let mut truncated = false;

    'words: for word in text.split_whitespace() {
        let word_len = word.chars().count();
        if column > 0 {
            if word_len + 1 > GLYPHS_PER_LINE - column {
                line += 1;
                column = 0;
            } else {
                cells[line * GLYPHS_PER_LINE + column] = ' ';
                column += 1;
            }
        }

        for character in word.chars() {
            if column >= GLYPHS_PER_LINE {
                line += 1;
                column = 0;
            }
            if line >= MAX_LINES {
                truncated = true;
                break 'words;
            }
            cells[line * GLYPHS_PER_LINE + column] = character;
            column += 1;
        }
    }

    if truncated {
        for cell in &mut cells[CELL_COUNT - 3..] {
            *cell = '.';
        }
    }
    (cells, truncated)
}

fn scaled_alpha(base: u8, opacity: u8) -> u8 {
    ((u16::from(base) * u16::from(opacity) + 127) / 255) as u8
}

fn blend_at(buffer: &mut PixelBuffer, x: i32, y: i32, color: [u8; 4]) {
    let (Ok(x), Ok(y)) = (u32::try_from(x), u32::try_from(y)) else {
        return;
    };
    buffer.blend_pixel(x, y, color);
}

fn draw_glyph(buffer: &mut PixelBuffer, x: i32, y: i32, character: char, color: [u8; 4]) {
    let rows = glyph_rows(character);
    for (row_index, row) in rows.into_iter().enumerate() {
        for column in 0..GLYPH_WIDTH {
            let mask = 1 << (GLYPH_WIDTH - 1 - column);
            if row & mask != 0 {
                blend_at(buffer, x + column, y + row_index as i32, color);
            }
        }
    }
}

#[allow(clippy::too_many_lines)]
const fn glyph_rows(character: char) -> [u8; GLYPH_HEIGHT] {
    match character {
        'a' | 'A' | 'à' | 'À' | 'â' | 'Â' => [0x0E, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11],
        'b' | 'B' => [0x1E, 0x11, 0x11, 0x1E, 0x11, 0x11, 0x1E],
        'c' | 'C' | 'ç' | 'Ç' => [0x0E, 0x11, 0x10, 0x10, 0x10, 0x11, 0x0E],
        'd' | 'D' => [0x1E, 0x11, 0x11, 0x11, 0x11, 0x11, 0x1E],
        'e' | 'E' | 'é' | 'É' | 'è' | 'È' | 'ê' | 'Ê' => {
            [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x1F]
        }
        'f' | 'F' => [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x10],
        'g' | 'G' => [0x0E, 0x11, 0x10, 0x17, 0x11, 0x11, 0x0F],
        'h' | 'H' => [0x11, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11],
        'i' | 'I' | 'î' | 'Î' | 'ï' | 'Ï' => [0x0E, 0x04, 0x04, 0x04, 0x04, 0x04, 0x0E],
        'j' | 'J' => [0x07, 0x02, 0x02, 0x02, 0x12, 0x12, 0x0C],
        'k' | 'K' => [0x11, 0x12, 0x14, 0x18, 0x14, 0x12, 0x11],
        'l' | 'L' => [0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x1F],
        'm' | 'M' => [0x11, 0x1B, 0x15, 0x15, 0x11, 0x11, 0x11],
        'n' | 'N' => [0x11, 0x19, 0x15, 0x13, 0x11, 0x11, 0x11],
        'o' | 'O' | 'ô' | 'Ô' => [0x0E, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E],
        'p' | 'P' => [0x1E, 0x11, 0x11, 0x1E, 0x10, 0x10, 0x10],
        'q' | 'Q' => [0x0E, 0x11, 0x11, 0x11, 0x15, 0x12, 0x0D],
        'r' | 'R' => [0x1E, 0x11, 0x11, 0x1E, 0x14, 0x12, 0x11],
        's' | 'S' => [0x0F, 0x10, 0x10, 0x0E, 0x01, 0x01, 0x1E],
        't' | 'T' => [0x1F, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04],
        'u' | 'U' | 'ù' | 'Ù' | 'û' | 'Û' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E],
        'v' | 'V' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x0A, 0x04],
        'w' | 'W' => [0x11, 0x11, 0x11, 0x15, 0x15, 0x15, 0x0A],
        'x' | 'X' => [0x11, 0x11, 0x0A, 0x04, 0x0A, 0x11, 0x11],
        'y' | 'Y' => [0x11, 0x11, 0x0A, 0x04, 0x04, 0x04, 0x04],
        'z' | 'Z' => [0x1F, 0x01, 0x02, 0x04, 0x08, 0x10, 0x1F],
        '0' => [0x0E, 0x11, 0x13, 0x15, 0x19, 0x11, 0x0E],
        '1' => [0x04, 0x0C, 0x14, 0x04, 0x04, 0x04, 0x1F],
        '2' => [0x0E, 0x11, 0x01, 0x02, 0x04, 0x08, 0x1F],
        '3' => [0x1E, 0x01, 0x01, 0x0E, 0x01, 0x01, 0x1E],
        '4' => [0x02, 0x06, 0x0A, 0x12, 0x1F, 0x02, 0x02],
        '5' => [0x1F, 0x10, 0x10, 0x1E, 0x01, 0x01, 0x1E],
        '6' => [0x0E, 0x10, 0x10, 0x1E, 0x11, 0x11, 0x0E],
        '7' => [0x1F, 0x01, 0x02, 0x04, 0x08, 0x08, 0x08],
        '8' => [0x0E, 0x11, 0x11, 0x0E, 0x11, 0x11, 0x0E],
        '9' => [0x0E, 0x11, 0x11, 0x0F, 0x01, 0x01, 0x0E],
        ' ' => [0; GLYPH_HEIGHT],
        '.' => [0, 0, 0, 0, 0, 0x0C, 0x0C],
        '!' => [0x04, 0x04, 0x04, 0x04, 0x04, 0, 0x04],
        '+' => [0, 0x04, 0x04, 0x1F, 0x04, 0x04, 0],
        '-' => [0, 0, 0, 0x1F, 0, 0, 0],
        '\'' | '’' => [0x04, 0x04, 0x08, 0, 0, 0, 0],
        '<' => [0x02, 0x04, 0x08, 0x10, 0x08, 0x04, 0x02],
        '>' => [0x08, 0x04, 0x02, 0x01, 0x02, 0x04, 0x08],
        _ => [0x0E, 0x11, 0x01, 0x02, 0x04, 0, 0x04],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_layout_court_tient_sur_deux_lignes() {
        let (cells, truncated) = layout_text("Bon commit");
        assert!(!truncated);
        assert_eq!(cells[0], 'B');
        assert_eq!(cells[GLYPHS_PER_LINE], 'c');
    }

    #[test]
    fn test_layout_tronque_sans_couper_unicode() {
        let (cells, truncated) = layout_text("Évolution extraordinaire 🐉 et sûre");
        assert!(truncated);
        assert_eq!(&cells[CELL_COUNT - 3..], &['.', '.', '.']);
    }

    #[test]
    fn test_bulle_reste_dans_le_canevas() {
        let mut buffer = PixelBuffer::new(64, 64);
        SpeechBubbleRenderer::render(
            &mut buffer,
            SpeechBubbleView {
                text: "Niveau +1",
                opacity: 255,
                bounds: BubbleRect {
                    x: i32::MAX,
                    y: i32::MIN,
                    width: u32::MAX,
                    height: u32::MAX,
                },
                target_anchor: (i32::MAX, i32::MIN),
            },
        );
        assert!(buffer.as_bytes().iter().any(|channel| *channel != 0));
        assert_eq!(buffer.as_bytes().len(), 64 * 64 * 4);
    }

    #[test]
    fn test_opacite_nulle_ne_modifie_pas_le_tampon() {
        let mut buffer = PixelBuffer::new(64, 64);
        SpeechBubbleRenderer::render(
            &mut buffer,
            SpeechBubbleView {
                text: "Invisible",
                opacity: 0,
                bounds: BubbleRect::companion_default(),
                target_anchor: (32, 20),
            },
        );
        assert!(buffer.as_bytes().iter().all(|channel| *channel == 0));
    }

    #[test]
    fn test_glyphe_inconnu_utilise_le_point_interrogation() {
        assert_eq!(glyph_rows('🐉'), glyph_rows('?'));
    }
}
