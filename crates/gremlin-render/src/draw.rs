//! Primitives de dessin pixel art partagées par les générateurs procéduraux.
//!
//! Ces helpers travaillent sur un tampon RGBA8 carré de `size * size` pixels et
//! ignorent silencieusement tout ce qui sort du canevas (clipping), ce qui permet
//! aux routines de dessin d'utiliser des coordonnées signées sans précaution.

use crate::limits::CANVAS_SIZE;

/// Alloue un canevas RGBA entièrement transparent de [`CANVAS_SIZE`] x [`CANVAS_SIZE`].
#[must_use]
pub fn blank_canvas() -> Vec<u8> {
    let side = CANVAS_SIZE as usize;
    vec![0u8; side * side * 4]
}

/// Écrit un pixel opaque (remplacement, sans composition alpha) dans le canevas.
///
/// Les coordonnées hors canevas sont ignorées.
pub fn set_px(buf: &mut [u8], size: usize, x: i32, y: i32, color: [u8; 4]) {
    if x < 0 || y < 0 || (x as usize) >= size || (y as usize) >= size {
        return;
    }
    let idx = ((y as usize) * size + (x as usize)) * 4;
    // Garde-fou : un tampon sous-dimensionné ne doit jamais provoquer de panique.
    let Some(slot) = buf.get_mut(idx..idx + 4) else {
        return;
    };
    slot.copy_from_slice(&color);
}

/// Remplit un rectangle plein, en clippant sur les bords du canevas.
pub fn fill_rect(buf: &mut [u8], size: usize, x: i32, y: i32, w: i32, h: i32, color: [u8; 4]) {
    for dy in 0..h {
        for dx in 0..w {
            set_px(buf, size, x + dx, y + dy, color);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_set_px_clippe_hors_canevas() {
        let mut buf = blank_canvas();
        set_px(&mut buf, CANVAS_SIZE as usize, -1, 0, [255, 0, 0, 255]);
        set_px(&mut buf, CANVAS_SIZE as usize, 0, 64, [255, 0, 0, 255]);
        assert!(buf.iter().all(|&b| b == 0));
    }

    #[test]
    fn test_set_px_tolere_tampon_sous_dimensionne() {
        let mut buf = vec![0u8; 8];
        set_px(&mut buf, 64, 10, 10, [255, 0, 0, 255]);
        assert!(buf.iter().all(|&b| b == 0));
    }

    #[test]
    fn test_fill_rect_remplit_la_zone_demandee() {
        let side = CANVAS_SIZE as usize;
        let mut buf = blank_canvas();
        fill_rect(&mut buf, side, 2, 3, 4, 5, [1, 2, 3, 4]);

        let idx = (3 * side + 2) * 4;
        assert_eq!(&buf[idx..idx + 4], &[1, 2, 3, 4]);
        let outside = (2 * side + 2) * 4;
        assert_eq!(&buf[outside..outside + 4], &[0, 0, 0, 0]);
    }
}
