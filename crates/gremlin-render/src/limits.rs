//! Constantes de dimensionnement et bornes de sécurité du moteur de rendu.
//!
//! Les skins, accessoires et images sont chargés depuis le disque (dossiers de mods
//! utilisateur, packs téléchargés). Ils doivent donc être considérés comme des **entrées
//! non fiables** : toute dimension, durée ou allocation dérivée d'un manifest ou d'un PNG
//! est validée contre les bornes ci-dessous avant d'être utilisée, afin d'éviter les
//! épuisements mémoire (OOM) et les boucles de rattrapage non bornées.

/// Taille canonique (largeur et hauteur, en pixels) du canevas de composition d'un Gremlin.
///
/// Tous les calques (corps, tenue, accessoires, aura) sont dessinés sur un canevas
/// de cette taille. Voir [`crate::layer::LayerCompositor`] pour la convention d'ancrage.
pub const CANVAS_SIZE: u32 = 64;

/// Dimension maximale (largeur ou hauteur) acceptée pour une frame déclarée dans un manifest.
///
/// Un manifest annonçant une frame plus grande est rejeté : composer un tel calque
/// allouerait des tampons démesurés pour un sprite de familier.
pub const MAX_FRAME_DIMENSION: u32 = 1024;

/// Dimension maximale (largeur ou hauteur) d'un [`crate::buffer::PixelBuffer`].
pub const MAX_BUFFER_DIMENSION: u32 = 8192;

/// Nombre maximal de pixels d'un [`crate::buffer::PixelBuffer`] (soit 64 Mio de RGBA8).
pub const MAX_BUFFER_PIXELS: u64 = 16 * 1024 * 1024;

/// Dimension maximale (largeur ou hauteur) acceptée pour une image décodée depuis un PNG/JPEG.
pub const MAX_IMAGE_DIMENSION: u32 = 8192;

/// Budget mémoire maximal accordé au décodeur d'images pour une seule image.
pub const MAX_IMAGE_ALLOC_BYTES: u64 = 256 * 1024 * 1024;

/// Décalage absolu maximal accepté pour une coordonnée d'ancrage de manifest.
pub const MAX_ANCHOR_OFFSET: i32 = 4096;

/// Durée d'affichage par défaut d'une frame d'animation, en millisecondes.
pub const DEFAULT_FRAME_DURATION_MS: u64 = 200;

/// Durée d'affichage minimale d'une frame, en millisecondes.
///
/// Une durée nulle rendrait le rattrapage temporel de
/// [`crate::animation::AnimationController::update`] non convergent : elle est donc
/// systématiquement relevée à cette valeur.
pub const MIN_FRAME_DURATION_MS: u64 = 1;

/// Durée d'affichage maximale d'une frame, en millisecondes (une minute).
pub const MAX_FRAME_DURATION_MS: u64 = 60_000;

/// Ramène une durée de frame issue d'une source non fiable dans l'intervalle autorisé.
///
/// Renvoie la durée bornée ainsi qu'un booléen indiquant si un ajustement a eu lieu.
#[must_use]
pub const fn clamp_frame_duration_ms(raw: u64) -> (u64, bool) {
    if raw < MIN_FRAME_DURATION_MS {
        (MIN_FRAME_DURATION_MS, true)
    } else if raw > MAX_FRAME_DURATION_MS {
        (MAX_FRAME_DURATION_MS, true)
    } else {
        (raw, false)
    }
}

/// Construit les limites de décodage appliquées à toute image chargée depuis le disque.
#[must_use]
pub fn decode_limits() -> image::Limits {
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_IMAGE_DIMENSION);
    limits.max_image_height = Some(MAX_IMAGE_DIMENSION);
    limits.max_alloc = Some(MAX_IMAGE_ALLOC_BYTES);
    limits
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clamp_frame_duration_rejette_zero() {
        assert_eq!(clamp_frame_duration_ms(0), (MIN_FRAME_DURATION_MS, true));
        assert_eq!(
            clamp_frame_duration_ms(u64::MAX),
            (MAX_FRAME_DURATION_MS, true)
        );
        assert_eq!(clamp_frame_duration_ms(200), (200, false));
    }

    #[test]
    fn test_decode_limits_sont_explicites() {
        let limits = decode_limits();
        assert_eq!(limits.max_image_width, Some(MAX_IMAGE_DIMENSION));
        assert_eq!(limits.max_image_height, Some(MAX_IMAGE_DIMENSION));
        assert_eq!(limits.max_alloc, Some(MAX_IMAGE_ALLOC_BYTES));
    }
}
