//! Tampon mémoire de pixels RGBA pour le rendu 2D logiciel / GPU blit.

use crate::error::RenderError;
use crate::limits::{MAX_BUFFER_DIMENSION, MAX_BUFFER_PIXELS};
use tracing::warn;

/// Division entière arrondie au plus proche (au lieu d'une troncature vers zéro).
///
/// Sans cet arrondi, chaque composition alpha perd jusqu'à un LSB par canal et
/// les blends répétés dérivent progressivement vers le noir.
const fn div_round(numerator: u32, denominator: u32) -> u32 {
    (numerator + denominator / 2) / denominator
}

/// Tampon de pixels RGBA avec gestion de composition alpha.
#[derive(Debug, Clone)]
pub struct PixelBuffer {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

impl PixelBuffer {
    /// Crée un nouveau tampon de pixels initialisé à transparent (0, 0, 0, 0).
    ///
    /// Les dimensions sont **bornées** par [`MAX_BUFFER_DIMENSION`] et
    /// [`MAX_BUFFER_PIXELS`] : une demande démesurée est rabotée (avec une trace
    /// d'avertissement) plutôt que de provoquer une allocation catastrophique.
    /// Utiliser [`PixelBuffer::try_new`] pour détecter le dépassement au lieu de le subir.
    #[must_use]
    pub fn new(width: u32, height: u32) -> Self {
        match Self::try_new(width, height) {
            Ok(buffer) => buffer,
            Err(err) => {
                let (w, h) = clamp_dimensions(width, height);
                warn!(
                    requested_width = width,
                    requested_height = height,
                    clamped_width = w,
                    clamped_height = h,
                    error = %err,
                    "Dimensions de PixelBuffer hors bornes : rabotées"
                );
                Self::allocate(w, h)
            }
        }
    }

    /// Crée un tampon en signalant explicitement un dépassement des bornes de sécurité.
    ///
    /// # Errors
    /// Renvoie `RenderError::InvalidManifestField` si les dimensions dépassent
    /// [`MAX_BUFFER_DIMENSION`] ou si le nombre total de pixels dépasse [`MAX_BUFFER_PIXELS`].
    pub fn try_new(width: u32, height: u32) -> Result<Self, RenderError> {
        if width > MAX_BUFFER_DIMENSION || height > MAX_BUFFER_DIMENSION {
            return Err(RenderError::invalid_field(
                "buffer_dimensions",
                format!("{width}x{height} dépasse la borne de {MAX_BUFFER_DIMENSION} px par axe"),
            ));
        }

        let pixel_count = u64::from(width) * u64::from(height);
        if pixel_count > MAX_BUFFER_PIXELS {
            return Err(RenderError::invalid_field(
                "buffer_dimensions",
                format!("{pixel_count} pixels dépasse la borne de {MAX_BUFFER_PIXELS}"),
            ));
        }

        Ok(Self::allocate(width, height))
    }

    fn allocate(width: u32, height: u32) -> Self {
        let size = (width as usize) * (height as usize) * 4;
        Self {
            width,
            height,
            pixels: vec![0; size],
        }
    }

    /// Largeur du buffer en pixels.
    #[must_use]
    pub const fn width(&self) -> u32 {
        self.width
    }

    /// Hauteur du buffer en pixels.
    #[must_use]
    pub const fn height(&self) -> u32 {
        self.height
    }

    /// Accès aux octets bruts RGBA.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.pixels
    }

    /// Accès mutable aux octets bruts RGBA.
    pub fn as_bytes_mut(&mut self) -> &mut [u8] {
        &mut self.pixels
    }

    /// Réinitialise l'ensemble du buffer à une couleur donnée (RGBA).
    pub fn clear(&mut self, r: u8, g: u8, b: u8, a: u8) {
        for chunk in self.pixels.chunks_exact_mut(4) {
            chunk[0] = r;
            chunk[1] = g;
            chunk[2] = b;
            chunk[3] = a;
        }
    }

    /// Écrit un pixel avec composition alpha (source-over blending).
    ///
    /// Le résultat est stocké en alpha **non prémultiplié**. Les divisions sont
    /// arrondies au plus proche afin qu'une suite de compositions ne dérive pas
    /// vers le noir.
    pub fn blend_pixel(&mut self, x: u32, y: u32, src: [u8; 4]) {
        if x >= self.width || y >= self.height {
            return;
        }

        let idx = ((y as usize) * (self.width as usize) + (x as usize)) * 4;
        let src_a = u32::from(src[3]);

        if src_a == 0 {
            return;
        }

        let Some(dst) = self.pixels.get_mut(idx..idx + 4) else {
            return;
        };

        if src_a == 255 {
            dst.copy_from_slice(&[src[0], src[1], src[2], 255]);
            return;
        }

        let dst_r = u32::from(dst[0]);
        let dst_g = u32::from(dst[1]);
        let dst_b = u32::from(dst[2]);
        let dst_a = u32::from(dst[3]);

        let inv_a = 255 - src_a;
        // Contribution de la destination, pondérée par son propre alpha : c'est ce
        // terme qui « dé-prémultiplie » le résultat lorsque la destination est vide.
        let weight = div_round(dst_a * inv_a, 255);
        let out_a = src_a + weight;

        if out_a == 0 {
            return;
        }

        // `min(255)` protège du cas limite où le cumul des arrondis ferait déborder
        // un canal d'une unité au-dessus de la pleine échelle.
        let channel = |src_c: u8, dst_c: u32| -> u8 {
            div_round(
                u32::from(src_c) * src_a + div_round(dst_c * dst_a * inv_a, 255),
                out_a,
            )
            .min(255) as u8
        };

        let (out_r, out_g, out_b) = (
            channel(src[0], dst_r),
            channel(src[1], dst_g),
            channel(src[2], dst_b),
        );

        dst[0] = out_r;
        dst[1] = out_g;
        dst[2] = out_b;
        dst[3] = out_a.min(255) as u8;
    }

    /// Dessine une sous-image (tableau d'octets RGBA) aux coordonnées `(offset_x, offset_y)`.
    ///
    /// # Contrat
    /// `src` doit contenir exactement `src_width * src_height * 4` octets. Un tampon
    /// plus court est toléré en release (les pixels manquants sont ignorés) mais
    /// déclenche une `debug_assert` en debug : c'est systématiquement le signe d'une
    /// invariante violée côté appelant.
    pub fn blit(
        &mut self,
        src: &[u8],
        src_width: u32,
        src_height: u32,
        offset_x: i32,
        offset_y: i32,
    ) {
        let expected_len = (src_width as usize)
            .saturating_mul(src_height as usize)
            .saturating_mul(4);
        debug_assert!(
            src.len() >= expected_len,
            "blit : tampon source sous-dimensionné ({} octets pour {}x{}, {} attendus)",
            src.len(),
            src_width,
            src_height,
            expected_len
        );

        for sy in 0..src_height {
            let dy = offset_y.saturating_add(sy as i32);
            if dy < 0 || dy >= (self.height as i32) {
                continue;
            }

            for sx in 0..src_width {
                let dx = offset_x.saturating_add(sx as i32);
                if dx < 0 || dx >= (self.width as i32) {
                    continue;
                }

                let src_idx = ((sy as usize) * (src_width as usize) + (sx as usize)) * 4;
                if let Some(pixel) = src.get(src_idx..src_idx + 4) {
                    let rgba = [pixel[0], pixel[1], pixel[2], pixel[3]];
                    self.blend_pixel(dx as u32, dy as u32, rgba);
                }
            }
        }
    }
}

/// Rabote une paire de dimensions dans les bornes de sécurité du tampon.
fn clamp_dimensions(width: u32, height: u32) -> (u32, u32) {
    let mut w = width.min(MAX_BUFFER_DIMENSION);
    let mut h = height.min(MAX_BUFFER_DIMENSION);

    while u64::from(w) * u64::from(h) > MAX_BUFFER_PIXELS {
        if w >= h {
            w /= 2;
        } else {
            h /= 2;
        }
        if w == 0 || h == 0 {
            return (w.max(1), h.max(1));
        }
    }

    (w, h)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Lit le pixel `(x, y)` du tampon.
    fn px(buffer: &PixelBuffer, x: u32, y: u32) -> [u8; 4] {
        let idx = ((y as usize) * (buffer.width() as usize) + (x as usize)) * 4;
        let bytes = &buffer.as_bytes()[idx..idx + 4];
        [bytes[0], bytes[1], bytes[2], bytes[3]]
    }

    #[test]
    fn test_pixel_buffer_creation_and_clear() {
        let mut buffer = PixelBuffer::new(2, 2);
        assert_eq!(buffer.as_bytes().len(), 16);

        buffer.clear(255, 0, 0, 255);
        assert_eq!(&buffer.as_bytes()[0..4], &[255, 0, 0, 255]);
    }

    #[test]
    fn test_pixel_blit() {
        let mut buffer = PixelBuffer::new(4, 4);
        let src_pixel = [0, 255, 0, 255]; // Vert opaque 1x1
        buffer.blit(&src_pixel, 1, 1, 1, 1);

        let expected_idx = 20; // position (1, 1) sur une largeur de 4 -> (1*4 + 1)*4 = 20
        assert_eq!(
            &buffer.as_bytes()[expected_idx..expected_idx + 4],
            &[0, 255, 0, 255]
        );
    }

    #[test]
    fn test_try_new_rejette_dimensions_demesurees() {
        assert!(PixelBuffer::try_new(64, 64).is_ok());
        assert!(PixelBuffer::try_new(MAX_BUFFER_DIMENSION + 1, 1).is_err());
        assert!(PixelBuffer::try_new(1, MAX_BUFFER_DIMENSION + 1).is_err());
        // 8192 x 8192 = 64 Mpx > MAX_BUFFER_PIXELS
        assert!(PixelBuffer::try_new(MAX_BUFFER_DIMENSION, MAX_BUFFER_DIMENSION).is_err());
    }

    #[test]
    fn test_new_rabote_au_lieu_dexploser() {
        let buffer = PixelBuffer::new(u32::MAX, u32::MAX);
        assert!(buffer.width() <= MAX_BUFFER_DIMENSION);
        assert!(buffer.height() <= MAX_BUFFER_DIMENSION);
        assert!(u64::from(buffer.width()) * u64::from(buffer.height()) <= MAX_BUFFER_PIXELS);
        assert_eq!(
            buffer.as_bytes().len(),
            (buffer.width() as usize) * (buffer.height() as usize) * 4
        );
    }

    // ---------------------------------------------------------------------
    // Composition alpha (source-over)
    // ---------------------------------------------------------------------

    #[test]
    fn test_blend_alpha_zero_ne_touche_rien() {
        let mut buffer = PixelBuffer::new(1, 1);
        buffer.clear(10, 20, 30, 255);
        buffer.blend_pixel(0, 0, [255, 255, 255, 0]);
        assert_eq!(px(&buffer, 0, 0), [10, 20, 30, 255]);
    }

    #[test]
    fn test_blend_alpha_opaque_remplace_la_destination() {
        let mut buffer = PixelBuffer::new(1, 1);
        buffer.clear(10, 20, 30, 128);
        buffer.blend_pixel(0, 0, [200, 100, 50, 255]);
        assert_eq!(px(&buffer, 0, 0), [200, 100, 50, 255]);
    }

    #[test]
    fn test_blend_semi_transparent_sur_opaque() {
        let mut buffer = PixelBuffer::new(1, 1);
        // Destination rouge opaque, source verte à 50 %.
        buffer.clear(255, 0, 0, 255);
        buffer.blend_pixel(0, 0, [0, 255, 0, 128]);

        let out = px(&buffer, 0, 0);
        assert_eq!(
            out[3], 255,
            "une source semi-opaque sur un fond opaque reste opaque"
        );
        // 128/255 de vert et 127/255 de rouge, arrondi au plus proche.
        assert_eq!(out[0], 127);
        assert_eq!(out[1], 128);
        assert_eq!(out[2], 0);
    }

    #[test]
    fn test_blend_sur_destination_totalement_transparente_preserve_la_couleur() {
        let mut buffer = PixelBuffer::new(1, 1);
        // Destination vide : le chemin de « dé-prémultiplication » doit restituer
        // exactement la couleur source, sans l'assombrir vers le noir du tampon.
        buffer.blend_pixel(0, 0, [200, 100, 50, 64]);
        assert_eq!(px(&buffer, 0, 0), [200, 100, 50, 64]);
    }

    #[test]
    fn test_blend_alpha_se_cumule_sans_depasser_255() {
        let mut buffer = PixelBuffer::new(1, 1);
        for _ in 0..8 {
            buffer.blend_pixel(0, 0, [255, 255, 255, 128]);
        }
        let out = px(&buffer, 0, 0);
        assert_eq!(out[3], 255, "l'alpha doit converger vers l'opacité totale");
        assert_eq!([out[0], out[1], out[2]], [255, 255, 255]);
    }

    #[test]
    fn test_blend_repete_ne_derive_pas_vers_le_noir() {
        let mut buffer = PixelBuffer::new(1, 1);
        // Fond gris opaque, puis 50 blends du même gris à 50 % : sans arrondi la
        // troncature ferait perdre un LSB à presque chaque passe.
        buffer.clear(128, 128, 128, 255);
        for _ in 0..50 {
            buffer.blend_pixel(0, 0, [128, 128, 128, 128]);
        }
        assert_eq!(px(&buffer, 0, 0), [128, 128, 128, 255]);
    }

    #[test]
    fn test_blend_hors_limites_est_ignore() {
        let mut buffer = PixelBuffer::new(2, 2);
        buffer.blend_pixel(2, 0, [255, 255, 255, 255]);
        buffer.blend_pixel(0, 2, [255, 255, 255, 255]);
        assert!(buffer.as_bytes().iter().all(|&b| b == 0));
    }

    #[test]
    fn test_blit_avec_offset_negatif_clippe() {
        let mut buffer = PixelBuffer::new(2, 2);
        let src = vec![255u8; 2 * 2 * 4];
        buffer.blit(&src, 2, 2, -1, -1);
        // Seul le pixel (0, 0) reçoit le coin bas-droit de la source.
        assert_eq!(px(&buffer, 0, 0), [255, 255, 255, 255]);
        assert_eq!(px(&buffer, 1, 1), [0, 0, 0, 0]);
    }
}
