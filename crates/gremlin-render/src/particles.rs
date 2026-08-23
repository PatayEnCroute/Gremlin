//! Micro-particules pixel-art à capacité fixe.
//!
//! Le moteur ne connaît aucun événement métier. L'orchestrateur choisit un
//! [`ParticlePreset`] puis fournit un point d'émission en pixels du canevas.

use crate::PixelBuffer;
use std::time::Duration;

/// Nombre maximal de particules simultanément actives.
pub const MAX_PARTICLES: usize = 96;

const SUBPIXEL_SCALE: i32 = 256;
const MAX_SIMULATION_STEP_MS: u128 = 100;
const PARTICLE_FRAME_INTERVAL: Duration = Duration::from_millis(16);
const DEFAULT_RNG_SEED: u64 = 0x9E37_79B9_7F4A_7C15;

/// Forme pixel-art d'une particule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParticleShape {
    /// Pixel isolé.
    Pixel,
    /// Carré plein de deux pixels de côté.
    Quad2x2,
    /// Croix de trois pixels de côté.
    Cross3x3,
    /// Étoile de cinq pixels de côté.
    Star5x5,
    /// Glyphe de sommeil « Z ».
    GlyphZ,
    /// Goutte de sueur.
    Drop,
    /// Petit cœur.
    Heart,
}

/// Recette visuelle générique choisie par l'orchestrateur.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParticlePreset {
    /// Gerbe brève d'étincelles.
    SparkBurst,
    /// Confettis multicolores plus prioritaires.
    ConfettiBurst,
    /// Glyphe montant lentement.
    RisingZ,
    /// Goutte tombante.
    FallingDrop,
    /// Cœurs flottants.
    FloatingHearts,
}

impl ParticlePreset {
    const fn count(self) -> usize {
        match self {
            Self::SparkBurst => 12,
            Self::ConfettiBurst => 24,
            Self::RisingZ => 1,
            Self::FallingDrop => 2,
            Self::FloatingHearts => 5,
        }
    }

    const fn priority(self) -> u8 {
        match self {
            Self::ConfettiBurst => 3,
            Self::SparkBurst | Self::FloatingHearts => 2,
            Self::RisingZ | Self::FallingDrop => 1,
        }
    }
}

/// Particule interne dont les invariants sont préservés par [`ParticleEngine`].
#[derive(Debug, Clone, Copy)]
struct Particle {
    x: i32,
    y: i32,
    velocity_x: i32,
    velocity_y: i32,
    acceleration_x: i32,
    acceleration_y: i32,
    color: [u8; 4],
    remaining: Duration,
    lifetime: Duration,
    shape: ParticleShape,
    priority: u8,
    phase: u8,
}

/// Pool borné de particules réutilisé pendant toute la vie de l'application.
#[derive(Debug, Clone)]
pub struct ParticleEngine {
    slots: [Option<Particle>; MAX_PARTICLES],
    active_count: usize,
    rng_state: u64,
}

impl Default for ParticleEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl ParticleEngine {
    /// Crée un moteur vide avec une graine déterministe non nulle.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            slots: [None; MAX_PARTICLES],
            active_count: 0,
            rng_state: DEFAULT_RNG_SEED,
        }
    }

    /// Crée un moteur avec une graine explicite, utile aux tests et aperçus.
    #[must_use]
    pub const fn with_seed(seed: u64) -> Self {
        Self {
            slots: [None; MAX_PARTICLES],
            active_count: 0,
            rng_state: if seed == 0 { DEFAULT_RNG_SEED } else { seed },
        }
    }

    /// Nombre de particules actuellement actives.
    #[must_use]
    pub const fn active_count(&self) -> usize {
        self.active_count
    }

    /// Indique si le pool contient au moins une particule active.
    #[must_use]
    pub const fn is_active(&self) -> bool {
        self.active_count > 0
    }

    /// Délai de rafraîchissement nécessaire tant que des particules bougent.
    #[must_use]
    pub const fn next_wake_delay(&self) -> Option<Duration> {
        if self.is_active() {
            Some(PARTICLE_FRAME_INTERVAL)
        } else {
            None
        }
    }

    /// Émet un preset autour d'une origine exprimée en pixels du canevas.
    ///
    /// Renvoie le nombre de particules effectivement insérées. En saturation,
    /// une particule moins prioritaire peut être remplacée ; les autres émissions
    /// excédentaires sont abandonnées.
    pub fn emit(&mut self, preset: ParticlePreset, origin: (i32, i32)) -> usize {
        let mut inserted = 0usize;
        for index in 0..preset.count() {
            let Some(slot_index) = self.slot_for_priority(preset.priority()) else {
                continue;
            };
            let was_empty = self.slots[slot_index].is_none();
            let particle = self.build_particle(preset, origin, index);
            self.slots[slot_index] = Some(particle);
            if was_empty {
                self.active_count += 1;
            }
            inserted += 1;
        }
        inserted
    }

    /// Avance la simulation et signale si l'image visible doit être recomposée.
    pub fn update(&mut self, delta: Duration) -> bool {
        if delta.is_zero() || self.active_count == 0 {
            return false;
        }

        let simulation_ms = delta.as_millis().min(MAX_SIMULATION_STEP_MS) as i64;
        let mut removed = 0usize;

        for slot in &mut self.slots {
            let Some(mut particle) = *slot else {
                continue;
            };

            if delta >= particle.remaining {
                *slot = None;
                removed += 1;
                continue;
            }

            particle.remaining = particle.remaining.saturating_sub(delta);
            let wobble = if particle.shape == ParticleShape::GlyphZ {
                i64::from(i32::from(particle.phase % 16) - 8) * 6
            } else {
                0
            };

            particle.velocity_x = bounded_i32(
                i64::from(particle.velocity_x)
                    + i64::from(particle.acceleration_x) * simulation_ms / 1_000,
            );
            particle.velocity_y = bounded_i32(
                i64::from(particle.velocity_y)
                    + i64::from(particle.acceleration_y) * simulation_ms / 1_000,
            );
            particle.x = bounded_i32(
                i64::from(particle.x)
                    + (i64::from(particle.velocity_x) + wobble) * simulation_ms / 1_000,
            );
            particle.y = bounded_i32(
                i64::from(particle.y) + i64::from(particle.velocity_y) * simulation_ms / 1_000,
            );
            particle.phase = particle.phase.wrapping_add(simulation_ms as u8);
            *slot = Some(particle);
        }

        self.active_count = self.active_count.saturating_sub(removed);
        true
    }

    /// Dessine les particules actives avec clipping dans le tampon cible.
    pub fn render(&self, buffer: &mut PixelBuffer) {
        for particle in self.slots.iter().flatten() {
            let x = particle.x / SUBPIXEL_SCALE;
            let y = particle.y / SUBPIXEL_SCALE;
            let remaining = particle.remaining.as_millis();
            let lifetime = particle.lifetime.as_millis().max(1);
            let alpha = (u128::from(particle.color[3]) * remaining / lifetime).min(255) as u8;
            let color = [
                particle.color[0],
                particle.color[1],
                particle.color[2],
                alpha,
            ];
            render_shape(buffer, x, y, particle.shape, color);
        }
    }

    fn slot_for_priority(&self, priority: u8) -> Option<usize> {
        self.slots.iter().position(Option::is_none).or_else(|| {
            self.slots
                .iter()
                .enumerate()
                .filter_map(|(index, particle)| {
                    let particle = particle.as_ref()?;
                    (particle.priority < priority).then_some((index, particle.remaining))
                })
                .min_by_key(|(_, remaining)| *remaining)
                .map(|(index, _)| index)
        })
    }

    fn build_particle(
        &mut self,
        preset: ParticlePreset,
        origin: (i32, i32),
        index: usize,
    ) -> Particle {
        let origin_x = origin.0.saturating_mul(SUBPIXEL_SCALE);
        let origin_y = origin.1.saturating_mul(SUBPIXEL_SCALE);
        let priority = preset.priority();

        match preset {
            ParticlePreset::SparkBurst => Particle {
                x: origin_x,
                y: origin_y,
                velocity_x: self.random_between(-7, 7) * SUBPIXEL_SCALE,
                velocity_y: self.random_between(-11, -4) * SUBPIXEL_SCALE,
                acceleration_x: 0,
                acceleration_y: 18 * SUBPIXEL_SCALE,
                color: if index.is_multiple_of(2) {
                    [255, 214, 64, 255]
                } else {
                    [64, 224, 255, 255]
                },
                remaining: Duration::from_millis(700),
                lifetime: Duration::from_millis(700),
                shape: if index.is_multiple_of(3) {
                    ParticleShape::Star5x5
                } else {
                    ParticleShape::Cross3x3
                },
                priority,
                phase: 0,
            },
            ParticlePreset::ConfettiBurst => {
                let colors = [
                    [255, 82, 82, 255],
                    [255, 214, 64, 255],
                    [64, 224, 255, 255],
                    [126, 255, 112, 255],
                    [218, 112, 255, 255],
                ];
                Particle {
                    x: origin_x.saturating_add(self.random_between(-18, 18) * SUBPIXEL_SCALE),
                    y: origin_y.saturating_add(self.random_between(-3, 3) * SUBPIXEL_SCALE),
                    velocity_x: self.random_between(-4, 4) * SUBPIXEL_SCALE,
                    velocity_y: self.random_between(-12, -5) * SUBPIXEL_SCALE,
                    acceleration_x: 0,
                    acceleration_y: 14 * SUBPIXEL_SCALE,
                    color: colors[index % colors.len()],
                    remaining: Duration::from_millis(1_200),
                    lifetime: Duration::from_millis(1_200),
                    shape: if index.is_multiple_of(2) {
                        ParticleShape::Pixel
                    } else {
                        ParticleShape::Quad2x2
                    },
                    priority,
                    phase: 0,
                }
            }
            ParticlePreset::RisingZ => Particle {
                x: origin_x.saturating_add(self.random_between(-2, 2) * SUBPIXEL_SCALE),
                y: origin_y,
                velocity_x: 0,
                velocity_y: -5 * SUBPIXEL_SCALE,
                acceleration_x: 0,
                acceleration_y: 0,
                color: [180, 210, 255, 230],
                remaining: Duration::from_millis(1_400),
                lifetime: Duration::from_millis(1_400),
                shape: ParticleShape::GlyphZ,
                priority,
                phase: self.next_random() as u8,
            },
            ParticlePreset::FallingDrop => Particle {
                x: origin_x.saturating_add(self.random_between(-8, 8) * SUBPIXEL_SCALE),
                y: origin_y,
                velocity_x: self.random_between(-1, 1) * SUBPIXEL_SCALE,
                velocity_y: 3 * SUBPIXEL_SCALE,
                acceleration_x: 0,
                acceleration_y: 12 * SUBPIXEL_SCALE,
                color: [72, 190, 255, 230],
                remaining: Duration::from_millis(900),
                lifetime: Duration::from_millis(900),
                shape: ParticleShape::Drop,
                priority,
                phase: 0,
            },
            ParticlePreset::FloatingHearts => Particle {
                x: origin_x.saturating_add(self.random_between(-10, 10) * SUBPIXEL_SCALE),
                y: origin_y.saturating_add(self.random_between(-2, 2) * SUBPIXEL_SCALE),
                velocity_x: self.random_between(-2, 2) * SUBPIXEL_SCALE,
                velocity_y: self.random_between(-7, -4) * SUBPIXEL_SCALE,
                acceleration_x: 0,
                acceleration_y: 2 * SUBPIXEL_SCALE,
                color: [255, 94, 156, 245],
                remaining: Duration::from_millis(1_100),
                lifetime: Duration::from_millis(1_100),
                shape: ParticleShape::Heart,
                priority,
                phase: 0,
            },
        }
    }

    fn next_random(&mut self) -> u64 {
        let mut value = self.rng_state;
        value ^= value << 13;
        value ^= value >> 7;
        value ^= value << 17;
        self.rng_state = value;
        value
    }

    fn random_between(&mut self, min: i32, max: i32) -> i32 {
        let span = i64::from(max) - i64::from(min) + 1;
        let offset = (self.next_random() % (span as u64)) as i64;
        bounded_i32(i64::from(min) + offset)
    }
}

fn bounded_i32(value: i64) -> i32 {
    value.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
}

fn blend_at(buffer: &mut PixelBuffer, x: i32, y: i32, color: [u8; 4]) {
    let (Ok(x), Ok(y)) = (u32::try_from(x), u32::try_from(y)) else {
        return;
    };
    buffer.blend_pixel(x, y, color);
}

fn render_shape(buffer: &mut PixelBuffer, x: i32, y: i32, shape: ParticleShape, color: [u8; 4]) {
    match shape {
        ParticleShape::Pixel => blend_at(buffer, x, y, color),
        ParticleShape::Quad2x2 => {
            for offset_y in 0..2 {
                for offset_x in 0..2 {
                    blend_at(buffer, x + offset_x, y + offset_y, color);
                }
            }
        }
        ParticleShape::Cross3x3 => {
            for (offset_x, offset_y) in [(1, 0), (0, 1), (1, 1), (2, 1), (1, 2)] {
                blend_at(buffer, x + offset_x, y + offset_y, color);
            }
        }
        ParticleShape::Star5x5 => {
            for (offset_x, offset_y) in [
                (2, 0),
                (2, 1),
                (0, 2),
                (1, 2),
                (2, 2),
                (3, 2),
                (4, 2),
                (2, 3),
                (2, 4),
            ] {
                blend_at(buffer, x + offset_x, y + offset_y, color);
            }
        }
        ParticleShape::GlyphZ => {
            for (offset_x, offset_y) in [
                (0, 0),
                (1, 0),
                (2, 0),
                (2, 1),
                (1, 2),
                (0, 3),
                (0, 4),
                (1, 4),
                (2, 4),
            ] {
                blend_at(buffer, x + offset_x, y + offset_y, color);
            }
        }
        ParticleShape::Drop => {
            for (offset_x, offset_y) in [(1, 0), (0, 1), (1, 1), (0, 2), (1, 2)] {
                blend_at(buffer, x + offset_x, y + offset_y, color);
            }
        }
        ParticleShape::Heart => {
            for (offset_x, offset_y) in [
                (0, 1),
                (1, 0),
                (2, 1),
                (3, 0),
                (4, 1),
                (0, 2),
                (1, 2),
                (2, 2),
                (3, 2),
                (4, 2),
                (1, 3),
                (2, 3),
                (3, 3),
                (2, 4),
            ] {
                blend_at(buffer, x + offset_x, y + offset_y, color);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_emission_et_expiration() {
        let mut engine = ParticleEngine::with_seed(1);
        assert_eq!(engine.emit(ParticlePreset::SparkBurst, (32, 32)), 12);
        assert_eq!(engine.active_count(), 12);
        assert!(engine.update(Duration::from_secs(10)));
        assert_eq!(engine.active_count(), 0);
        assert!(!engine.is_active());
    }

    #[test]
    fn test_delta_nul_ne_salit_pas_la_scene() {
        let mut engine = ParticleEngine::with_seed(1);
        engine.emit(ParticlePreset::RisingZ, (32, 32));
        assert!(!engine.update(Duration::ZERO));
        assert_eq!(engine.active_count(), 1);
    }

    #[test]
    fn test_pool_reste_borne() {
        let mut engine = ParticleEngine::with_seed(1);
        for _ in 0..20 {
            engine.emit(ParticlePreset::ConfettiBurst, (32, 32));
        }
        assert_eq!(engine.active_count(), MAX_PARTICLES);
    }

    #[test]
    fn test_effet_prioritaire_remplace_un_effet_ambiant() {
        let mut engine = ParticleEngine::with_seed(1);
        for _ in 0..MAX_PARTICLES {
            engine.emit(ParticlePreset::RisingZ, (32, 32));
        }
        assert_eq!(engine.active_count(), MAX_PARTICLES);
        assert_eq!(
            engine.emit(ParticlePreset::ConfettiBurst, (32, 32)),
            ParticlePreset::ConfettiBurst.count()
        );
        assert_eq!(engine.active_count(), MAX_PARTICLES);
    }

    #[test]
    fn test_rendu_clippe_les_origines_hostiles() {
        let mut engine = ParticleEngine::with_seed(1);
        engine.emit(ParticlePreset::FloatingHearts, (i32::MAX, i32::MIN));
        let mut buffer = PixelBuffer::new(64, 64);
        engine.render(&mut buffer);
        assert!(buffer.as_bytes().iter().all(|channel| *channel == 0));
    }

    #[test]
    fn test_graine_identique_produit_le_meme_rendu() {
        let mut first = ParticleEngine::with_seed(42);
        let mut second = ParticleEngine::with_seed(42);
        first.emit(ParticlePreset::SparkBurst, (32, 32));
        second.emit(ParticlePreset::SparkBurst, (32, 32));
        first.update(Duration::from_millis(80));
        second.update(Duration::from_millis(80));

        let mut first_buffer = PixelBuffer::new(64, 64);
        let mut second_buffer = PixelBuffer::new(64, 64);
        first.render(&mut first_buffer);
        second.render(&mut second_buffer);
        assert_eq!(first_buffer.as_bytes(), second_buffer.as_bytes());
    }
}
