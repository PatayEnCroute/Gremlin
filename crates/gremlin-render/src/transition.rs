//! Fondu borné entre deux scènes RGBA déjà composées.

use crate::PixelBuffer;
use std::time::Duration;

const TRANSITION_FRAME_INTERVAL: Duration = Duration::from_millis(16);

/// Timeline indépendante de toute humeur ou clé de sprite.
#[derive(Debug, Clone, Copy)]
pub struct TransitionController {
    elapsed: Duration,
    duration: Duration,
    active: bool,
}

impl Default for TransitionController {
    fn default() -> Self {
        Self::new(Duration::from_millis(180))
    }
}

impl TransitionController {
    /// Crée une timeline avec une durée bornée au moment de son utilisation.
    #[must_use]
    pub const fn new(duration: Duration) -> Self {
        Self {
            elapsed: Duration::ZERO,
            duration,
            active: false,
        }
    }

    /// Redémarre le fondu depuis la scène sortante capturée.
    pub fn start(&mut self) {
        self.elapsed = Duration::ZERO;
        self.active = !self.duration.is_zero();
    }

    /// Annule le fondu et affiche immédiatement la scène entrante.
    pub fn cancel(&mut self) {
        self.elapsed = self.duration;
        self.active = false;
    }

    /// Indique si le fondu doit encore être animé.
    #[must_use]
    pub const fn is_active(&self) -> bool {
        self.active
    }

    /// Avance la timeline et indique si l'image interpolée a changé.
    pub fn update(&mut self, delta: Duration) -> bool {
        if !self.active || delta.is_zero() {
            return false;
        }

        self.elapsed = self.elapsed.saturating_add(delta).min(self.duration);
        if self.elapsed >= self.duration {
            self.active = false;
        }
        true
    }

    /// Progression entière de zéro à 255.
    #[must_use]
    pub fn progress(&self) -> u8 {
        if self.duration.is_zero() || !self.active {
            return 255;
        }
        let numerator = self.elapsed.as_nanos().saturating_mul(255);
        let denominator = self.duration.as_nanos().max(1);
        (numerator / denominator).min(255) as u8
    }

    /// Petit rebond vertical de la scène entrante, sans redimensionnement.
    #[must_use]
    pub fn incoming_offset_y(&self) -> i32 {
        if !self.active {
            return 0;
        }
        let progress = i32::from(self.progress());
        let distance_from_middle = (progress - 128).unsigned_abs().min(127) as i32;
        -2 + (distance_from_middle * 2 / 127)
    }

    /// Prochaine échéance nécessaire au fondu.
    #[must_use]
    pub fn next_wake_delay(&self) -> Option<Duration> {
        if !self.active {
            return None;
        }
        Some(
            self.duration
                .saturating_sub(self.elapsed)
                .min(TRANSITION_FRAME_INTERVAL),
        )
    }
}

/// Opérations de composition sans état.
pub struct TransitionRenderer;

impl TransitionRenderer {
    /// Mélange `outgoing` et `incoming` dans `output`.
    ///
    /// Renvoie `false` lorsque les dimensions ne sont pas compatibles. Dans ce
    /// cas, la scène entrante est tout de même copiée si sa taille correspond à
    /// la sortie, afin de ne jamais laisser le compagnon invisible.
    pub fn blend(
        outgoing: &PixelBuffer,
        incoming: &PixelBuffer,
        output: &mut PixelBuffer,
        progress: u8,
        incoming_offset_y: i32,
    ) -> bool {
        let compatible = outgoing.width() == incoming.width()
            && outgoing.height() == incoming.height()
            && output.width() == incoming.width()
            && output.height() == incoming.height();

        if !compatible {
            copy_if_compatible(incoming, output);
            return false;
        }

        let width = incoming.width() as usize;
        let height = incoming.height() as usize;
        let outgoing_bytes = outgoing.as_bytes();
        let incoming_bytes = incoming.as_bytes();
        let output_bytes = output.as_bytes_mut();
        let incoming_weight = u32::from(progress);
        let outgoing_weight = 255 - incoming_weight;

        for y in 0..height {
            let incoming_y = i32::try_from(y)
                .ok()
                .and_then(|value| value.checked_sub(incoming_offset_y))
                .and_then(|value| usize::try_from(value).ok())
                .filter(|value| *value < height);

            for x in 0..width {
                let output_index = (y * width + x) * 4;
                let outgoing_pixel = &outgoing_bytes[output_index..output_index + 4];
                let incoming_pixel = incoming_y.map_or([0, 0, 0, 0], |source_y| {
                    let index = (source_y * width + x) * 4;
                    [
                        incoming_bytes[index],
                        incoming_bytes[index + 1],
                        incoming_bytes[index + 2],
                        incoming_bytes[index + 3],
                    ]
                });
                let blended = blend_pixel(
                    outgoing_pixel,
                    incoming_pixel,
                    outgoing_weight,
                    incoming_weight,
                );
                output_bytes[output_index..output_index + 4].copy_from_slice(&blended);
            }
        }
        true
    }
}

fn blend_pixel(
    outgoing: &[u8],
    incoming: [u8; 4],
    outgoing_weight: u32,
    incoming_weight: u32,
) -> [u8; 4] {
    let outgoing_alpha = u32::from(outgoing[3]);
    let incoming_alpha = u32::from(incoming[3]);
    let weighted_alpha = outgoing_alpha * outgoing_weight + incoming_alpha * incoming_weight;
    if weighted_alpha == 0 {
        return [0; 4];
    }

    let channel = |outgoing_channel: u8, incoming_channel: u8| -> u8 {
        let numerator = u32::from(outgoing_channel) * outgoing_alpha * outgoing_weight
            + u32::from(incoming_channel) * incoming_alpha * incoming_weight;
        ((numerator + weighted_alpha / 2) / weighted_alpha).min(255) as u8
    };

    [
        channel(outgoing[0], incoming[0]),
        channel(outgoing[1], incoming[1]),
        channel(outgoing[2], incoming[2]),
        ((weighted_alpha + 127) / 255).min(255) as u8,
    ]
}

fn copy_if_compatible(source: &PixelBuffer, destination: &mut PixelBuffer) {
    if source.width() == destination.width() && source.height() == destination.height() {
        destination
            .as_bytes_mut()
            .copy_from_slice(source.as_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opaque_buffer(color: [u8; 4]) -> PixelBuffer {
        let mut buffer = PixelBuffer::new(2, 2);
        buffer.clear(color[0], color[1], color[2], color[3]);
        buffer
    }

    #[test]
    fn test_progression_est_bornee() {
        let mut controller = TransitionController::new(Duration::from_millis(100));
        controller.start();
        assert_eq!(controller.progress(), 0);
        assert!(controller.update(Duration::from_millis(50)));
        assert!((127..=128).contains(&controller.progress()));
        assert!(controller.update(Duration::from_secs(10)));
        assert_eq!(controller.progress(), 255);
        assert!(!controller.is_active());
    }

    #[test]
    fn test_fondu_respecte_les_extremites() {
        let outgoing = opaque_buffer([255, 0, 0, 255]);
        let incoming = opaque_buffer([0, 0, 255, 255]);
        let mut output = PixelBuffer::new(2, 2);

        assert!(TransitionRenderer::blend(
            &outgoing,
            &incoming,
            &mut output,
            0,
            0
        ));
        assert_eq!(&output.as_bytes()[..4], &[255, 0, 0, 255]);
        assert!(TransitionRenderer::blend(
            &outgoing,
            &incoming,
            &mut output,
            255,
            0
        ));
        assert_eq!(&output.as_bytes()[..4], &[0, 0, 255, 255]);
    }

    #[test]
    fn test_dimensions_incompatibles_affichent_la_scene_entrante() {
        let outgoing = opaque_buffer([255, 0, 0, 255]);
        let mut incoming = PixelBuffer::new(3, 3);
        incoming.clear(0, 255, 0, 255);
        let mut output = PixelBuffer::new(3, 3);
        assert!(!TransitionRenderer::blend(
            &outgoing,
            &incoming,
            &mut output,
            128,
            0
        ));
        assert_eq!(output.as_bytes(), incoming.as_bytes());
    }

    #[test]
    fn test_duree_nulle_est_instantanee() {
        let mut controller = TransitionController::new(Duration::ZERO);
        controller.start();
        assert!(!controller.is_active());
        assert_eq!(controller.progress(), 255);
        assert_eq!(controller.next_wake_delay(), None);
    }
}
