//! Chute douce, amortissement et magnétisme d'écran — moteur purement géométrique.
//!
//! Ce module ne connaît **aucune fenêtre** : il reçoit une zone de travail, une
//! taille, une position et une durée écoulée, et renvoie une position. C'est ce
//! qui le rend éprouvable hors écran, sur des topologies synthétiques, sans
//! serveur graphique.
//!
//! Trois garanties tiennent la simulation :
//!
//! * **elle converge** — le pas est plafonné, subdivisé un nombre fixe de fois,
//!   et un compteur global termine la chute au point sûr si la physique
//!   n'aboutit pas ;
//! * **elle reste bornée** — vitesses et accélérations sont vérifiées finies, la
//!   fenêtre ne peut pas sortir de la zone de travail qui la contient ;
//! * **elle n'alloue pas** — tout est scalaire, ce qui autorise son appel à
//!   chaque réveil de la boucle sans pression sur l'allocateur.

// Les coordonnées et dimensions manipulées ici sont des pixels d'écran : elles
// tiennent très en dessous de 2^24, seuil au-delà duquel `f32` cesse de
// représenter les entiers exactement. Les conversions sont donc exactes, et la
// simulation reste au sous-pixel sans coûter la mémoire d'un `f64` par champ.
#![allow(clippy::cast_precision_loss)]

use gremlin_system::desktop_layout::PhysicalRect;
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Pas de simulation élémentaire, en millisecondes.
///
/// Fixe : intégrer avec le delta réel rendrait la trajectoire dépendante de la
/// charge machine, donc irreproductible en test.
const SUBSTEP_MILLIS: u32 = 16;

/// Durée maximale réellement simulée en un appel, en millisecondes.
///
/// Au-delà, l'application a été gelée ou la machine suspendue : la chute
/// s'achève au point d'arrivée plutôt que de rattraper.
const MAX_STEP_MILLIS: u64 = 100;

/// Nombre maximal de sous-pas par appel.
const MAX_SUBSTEPS_PER_CALL: u32 = 8;

/// Nombre maximal de sous-pas sur toute une chute.
///
/// Filet de convergence : environ vingt secondes de simulation, très au-delà
/// d'une chute réelle, même du haut d'un écran 4K.
const MAX_TOTAL_SUBSTEPS: u32 = 1_250;

/// Nombre maximal de rebonds.
const MAX_BOUNCES: u8 = 1;

/// Accélération de la chute par défaut, en pixels par seconde au carré, à 100 %.
const DEFAULT_GRAVITY: f32 = 2_600.0;

/// Vitesse verticale maximale, en pixels par seconde, à 100 %.
const DEFAULT_MAX_FALL_SPEED: f32 = 4_200.0;

/// Vitesse horizontale de glissement vers l'ancre, en pixels par seconde, à 100 %.
const DEFAULT_GLIDE_SPEED: f32 = 1_600.0;

/// Fraction de vitesse conservée après un rebond.
const DEFAULT_BOUNCE_DAMPING: f32 = 0.32;

/// Vitesse d'impact minimale, en pixels par seconde, en deçà de laquelle on ne rebondit pas.
const DEFAULT_MIN_BOUNCE_SPEED: f32 = 260.0;

/// Distance à un bord, en points de conception, en deçà de laquelle l'ancre y colle.
const DEFAULT_CORNER_SNAP_POINTS: u32 = 48;

/// Réglages de la chute et du magnétisme.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MotionConfig {
    /// Accélération de la chute, en pixels par seconde au carré à 100 %.
    pub gravity: f32,
    /// Vitesse verticale maximale, en pixels par seconde à 100 %.
    pub max_fall_speed: f32,
    /// Vitesse de glissement horizontal vers l'ancre, à 100 %.
    pub glide_speed: f32,
    /// Fraction de vitesse conservée après rebond, dans `[0, 1)`.
    pub bounce_damping: f32,
    /// Vitesse d'impact minimale pour qu'un rebond ait lieu.
    pub min_bounce_speed: f32,
    /// Distance d'accroche aux coins, en points de conception.
    pub corner_snap_points: u32,
}

impl Default for MotionConfig {
    fn default() -> Self {
        Self {
            gravity: DEFAULT_GRAVITY,
            max_fall_speed: DEFAULT_MAX_FALL_SPEED,
            glide_speed: DEFAULT_GLIDE_SPEED,
            bounce_damping: DEFAULT_BOUNCE_DAMPING,
            min_bounce_speed: DEFAULT_MIN_BOUNCE_SPEED,
            corner_snap_points: DEFAULT_CORNER_SNAP_POINTS,
        }
    }
}

impl MotionConfig {
    /// Ramène chaque paramètre dans ses bornes, `NaN` neutralisé.
    ///
    /// Idempotente. Un `NaN` traverserait `clamp` sans être corrigé : il est
    /// intercepté d'abord.
    pub fn normalize(&mut self) {
        let defaults = Self::default();
        self.gravity = sanitize(self.gravity, 100.0, 20_000.0, defaults.gravity);
        self.max_fall_speed = sanitize(
            self.max_fall_speed,
            100.0,
            40_000.0,
            defaults.max_fall_speed,
        );
        self.glide_speed = sanitize(self.glide_speed, 50.0, 40_000.0, defaults.glide_speed);
        self.bounce_damping = sanitize(self.bounce_damping, 0.0, 0.9, defaults.bounce_damping);
        self.min_bounce_speed = sanitize(
            self.min_bounce_speed,
            1.0,
            10_000.0,
            defaults.min_bounce_speed,
        );
        self.corner_snap_points = self.corner_snap_points.min(512);
    }
}

/// Ramène une valeur flottante dans `[min, max]` en neutralisant `NaN`.
fn sanitize(value: f32, min: f32, max: f32, fallback: f32) -> f32 {
    if value.is_finite() {
        value.clamp(min, max)
    } else {
        fallback
    }
}

/// Bord ou coin où le familier vient se poser.
///
/// L'intention est persistée, jamais une coordonnée globale : un écran
/// débranché ou une définition changée rendrait une coordonnée absurde, alors
/// qu'une ancre se reprojette toujours.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ScreenAnchor {
    /// Coin bas-gauche de la zone de travail.
    BottomLeft,
    /// Plancher de la zone de travail, position libre le long du bord.
    #[default]
    BottomEdge,
    /// Coin bas-droit de la zone de travail.
    BottomRight,
}

impl ScreenAnchor {
    /// Libellé lisible de l'ancre.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::BottomLeft => "coin bas-gauche",
            Self::BottomEdge => "bord bas",
            Self::BottomRight => "coin bas-droit",
        }
    }
}

/// Intention de placement, indépendante de toute coordonnée absolue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct PlacementIntent {
    /// Bord ou coin visé.
    pub anchor: ScreenAnchor,
    /// Position le long du bord, en millièmes de la largeur utile.
    pub along_edge_per_mille: u16,
}

impl PlacementIntent {
    /// Borne la position le long du bord. Idempotente.
    pub fn normalize(&mut self) {
        self.along_edge_per_mille = self.along_edge_per_mille.min(1_000);
    }

    /// Déduit l'intention d'une fenêtre déjà posée dans une zone de travail.
    #[must_use]
    pub fn from_window(window: PhysicalRect, work_area: PhysicalRect, snap_px: u32) -> Self {
        let travel = travel_range(window.width, work_area.width);
        let offset = i64::from(window.x) - i64::from(work_area.x);

        let anchor = if offset <= i64::from(snap_px) {
            ScreenAnchor::BottomLeft
        } else if offset >= travel - i64::from(snap_px) {
            ScreenAnchor::BottomRight
        } else {
            ScreenAnchor::BottomEdge
        };

        let along_edge_per_mille = if travel <= 0 {
            0
        } else {
            // Arrondi au plus proche, et non troncature : sur un écran large, un
            // millième vaut presque deux pixels, et deux troncatures successives
            // — ici puis à la reprojection — décalaient l'ancre de deux pixels.
            let scaled = offset.clamp(0, travel) * 1_000 + travel / 2;
            u16::try_from(scaled / travel).unwrap_or(1_000)
        };

        Self {
            anchor,
            along_edge_per_mille: along_edge_per_mille.min(1_000),
        }
    }

    /// Projette l'intention en position physique sur une zone de travail donnée.
    ///
    /// Garantit que la fenêtre reste entièrement dans la zone lorsque celle-ci
    /// peut la contenir. Sinon, son coin supérieur gauche est au moins rendu
    /// visible plutôt que rejeté hors écran.
    #[must_use]
    pub fn resolve(self, window_size: (u32, u32), work_area: PhysicalRect) -> (i32, i32) {
        let travel = travel_range(window_size.0, work_area.width);
        let offset = match self.anchor {
            ScreenAnchor::BottomLeft => 0,
            ScreenAnchor::BottomRight => travel,
            ScreenAnchor::BottomEdge => {
                // Arrondi symétrique de celui de `from_window` : l'aller-retour
                // reste alors exact au pixel près.
                (travel * i64::from(self.along_edge_per_mille.min(1_000)) + 500) / 1_000
            }
        };

        let x = i64::from(work_area.x) + offset.max(0);
        let y = work_area.bottom() - i64::from(window_size.1);
        // Une fenêtre plus haute que la zone déborderait par le haut : son coin
        // supérieur gauche est ramené dans la zone, ce qui la garde attrapable.
        let y = y.max(i64::from(work_area.y));

        (clamp_to_i32(x), clamp_to_i32(y))
    }
}

/// Amplitude de déplacement d'une fenêtre le long d'un bord, jamais négative.
fn travel_range(window_width: u32, work_area_width: u32) -> i64 {
    (i64::from(work_area_width) - i64::from(window_width)).max(0)
}

/// Ramène un entier large dans les bornes `i32`.
fn clamp_to_i32(value: i64) -> i32 {
    value.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
}

/// Phase courante du mouvement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MotionPhase {
    /// Aucun mouvement en cours ; la boucle ne se réveille pas pour la physique.
    #[default]
    Idle,
    /// Chute vers le plancher de la zone de travail.
    Falling,
}

/// Résultat d'un pas de simulation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MotionUpdate {
    /// Position physique de la fenêtre après ce pas.
    pub position: (i32, i32),
    /// Phase après ce pas.
    pub phase: MotionPhase,
    /// La position finale est atteinte : plus aucun réveil physique n'est requis.
    pub settled: bool,
}

/// Moteur de chute et de magnétisme.
#[derive(Debug, Clone, Copy, Default)]
pub struct DesktopMotion {
    phase: MotionPhase,
    /// Position courante en pixels physiques, avec sa partie sous-pixel.
    x: f32,
    y: f32,
    /// Vitesse verticale courante, en pixels par seconde.
    velocity_y: f32,
    /// Point d'arrivée visé.
    target: (f32, f32),
    /// Paramètres mis à l'échelle de l'écran courant.
    gravity: f32,
    max_fall_speed: f32,
    glide_speed: f32,
    bounce_damping: f32,
    min_bounce_speed: f32,
    /// Rebonds déjà effectués.
    bounces: u8,
    /// Sous-pas déjà simulés sur cette chute.
    substeps: u32,
}

impl DesktopMotion {
    /// Phase courante.
    #[must_use]
    pub const fn phase(&self) -> MotionPhase {
        self.phase
    }

    /// Indique qu'une chute est en cours.
    #[must_use]
    pub const fn is_active(&self) -> bool {
        matches!(self.phase, MotionPhase::Falling)
    }

    /// Délai avant le prochain pas de simulation, ou `None` au repos.
    #[must_use]
    pub const fn next_step_delay(&self) -> Option<Duration> {
        match self.phase {
            MotionPhase::Idle => None,
            MotionPhase::Falling => Some(Duration::from_millis(SUBSTEP_MILLIS as u64)),
        }
    }

    /// Interrompt le mouvement sans déplacer la fenêtre.
    pub fn cancel(&mut self) {
        self.phase = MotionPhase::Idle;
        self.velocity_y = 0.0;
    }

    /// Lance une chute depuis la position courante de la fenêtre.
    ///
    /// En mouvement réduit, ou lorsque la fenêtre est déjà posée, le point
    /// d'arrivée est atteint immédiatement : la préférence d'accessibilité
    /// supprime le glissement animé, pas le placement.
    pub fn begin(
        &mut self,
        window: PhysicalRect,
        work_area: PhysicalRect,
        intent: PlacementIntent,
        config: MotionConfig,
        scale_factor_milli: u32,
        reduced_motion: bool,
    ) -> MotionUpdate {
        let target = intent.resolve((window.width, window.height), work_area);
        let scale = (scale_factor_milli.clamp(250, 8_000) as f32) / 1_000.0;

        self.x = window.x as f32;
        self.y = window.y as f32;
        self.target = (target.0 as f32, target.1 as f32);
        self.velocity_y = 0.0;
        self.bounces = 0;
        self.substeps = 0;
        self.gravity = config.gravity * scale;
        self.max_fall_speed = config.max_fall_speed * scale;
        self.glide_speed = config.glide_speed * scale;
        self.bounce_damping = config.bounce_damping;
        self.min_bounce_speed = config.min_bounce_speed * scale;

        // `>=` et non `>` : une fenêtre déjà sous le plancher — après un
        // changement de définition d'écran — doit remonter d'un coup, pas
        // « tomber » vers le haut.
        let already_placed =
            (self.y - self.target.1).abs() < 1.0 && (self.x - self.target.0).abs() < 1.0;

        if reduced_motion || already_placed || self.y >= self.target.1 {
            return self.settle();
        }

        self.phase = MotionPhase::Falling;
        MotionUpdate {
            position: (self.x.round() as i32, self.y.round() as i32),
            phase: MotionPhase::Falling,
            settled: false,
        }
    }

    /// Fait avancer la chute du temps écoulé.
    ///
    /// Sans effet au repos. Un écart démesuré termine la chute au point
    /// d'arrivée plutôt que de déclencher une boucle de rattrapage.
    pub fn advance(&mut self, elapsed: Duration) -> MotionUpdate {
        if !self.is_active() {
            return self.current_update();
        }

        let elapsed_millis = elapsed.as_millis().min(u128::from(u64::MAX)) as u64;
        if elapsed_millis > MAX_STEP_MILLIS {
            return self.settle();
        }

        let mut remaining = elapsed_millis;
        let mut performed = 0_u32;
        while remaining > 0 && performed < MAX_SUBSTEPS_PER_CALL {
            let step_millis = remaining.min(u64::from(SUBSTEP_MILLIS));
            remaining -= step_millis;
            performed += 1;
            self.substeps = self.substeps.saturating_add(1);

            if self.substeps > MAX_TOTAL_SUBSTEPS {
                // Filet de convergence : jamais atteint par une chute réelle.
                return self.settle();
            }
            if self.integrate(step_millis as f32 / 1_000.0) {
                return self.settle();
            }
        }

        self.current_update()
    }

    /// Intègre un sous-pas ; renvoie `true` si la chute est terminée.
    fn integrate(&mut self, dt: f32) -> bool {
        if !dt.is_finite() || dt <= 0.0 {
            return false;
        }

        self.velocity_y = self
            .gravity
            .mul_add(dt, self.velocity_y)
            .clamp(-self.max_fall_speed, self.max_fall_speed);
        self.y += self.velocity_y * dt;

        // Glissement horizontal vers l'ancre, à vitesse constante : la
        // trajectoire reste lisible et n'ajoute pas de seconde oscillation.
        let dx = self.target.0 - self.x;
        let max_glide = self.glide_speed * dt;
        self.x += dx.clamp(-max_glide, max_glide);

        if !self.x.is_finite() || !self.y.is_finite() || !self.velocity_y.is_finite() {
            return true;
        }

        if self.y < self.target.1 {
            return false;
        }

        // Contact avec le plancher.
        self.y = self.target.1;
        let impact = self.velocity_y;
        if self.bounces < MAX_BOUNCES && impact >= self.min_bounce_speed {
            self.bounces += 1;
            self.velocity_y = -impact * self.bounce_damping;
            return false;
        }

        // Le rebond terminé, la chute ne s'achève que si l'ancre horizontale est
        // atteinte : sinon le familier resterait figé à mi-parcours.
        (self.target.0 - self.x).abs() < 1.0
    }

    /// Termine le mouvement au point d'arrivée.
    fn settle(&mut self) -> MotionUpdate {
        self.x = self.target.0;
        self.y = self.target.1;
        self.velocity_y = 0.0;
        self.phase = MotionPhase::Idle;
        MotionUpdate {
            position: (self.x.round() as i32, self.y.round() as i32),
            phase: MotionPhase::Idle,
            settled: true,
        }
    }

    /// Position et phase courantes, sans avancer la simulation.
    fn current_update(&self) -> MotionUpdate {
        MotionUpdate {
            position: (self.x.round() as i32, self.y.round() as i32),
            phase: self.phase,
            settled: matches!(self.phase, MotionPhase::Idle),
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::float_cmp)]
mod tests {
    use super::*;

    const WORK_AREA: PhysicalRect = PhysicalRect::new(0, 0, 1920, 1040);
    const WINDOW: (u32, u32) = (128, 128);

    fn window_at(x: i32, y: i32) -> PhysicalRect {
        PhysicalRect::new(x, y, WINDOW.0, WINDOW.1)
    }

    fn free_intent() -> PlacementIntent {
        PlacementIntent {
            anchor: ScreenAnchor::BottomEdge,
            along_edge_per_mille: 500,
        }
    }

    /// Fait tourner la chute jusqu'à stabilisation, avec un pas régulier.
    fn run_to_rest(motion: &mut DesktopMotion, step: Duration) -> (MotionUpdate, u32) {
        let mut steps = 0;
        loop {
            let update = motion.advance(step);
            steps += 1;
            if update.settled {
                return (update, steps);
            }
            assert!(steps < 5_000, "la chute ne converge pas");
        }
    }

    #[test]
    fn test_placement_intent_resolves_to_the_work_area_floor() {
        let intent = PlacementIntent {
            anchor: ScreenAnchor::BottomLeft,
            along_edge_per_mille: 0,
        };
        assert_eq!(intent.resolve(WINDOW, WORK_AREA), (0, 1040 - 128));

        let right = PlacementIntent {
            anchor: ScreenAnchor::BottomRight,
            along_edge_per_mille: 0,
        };
        assert_eq!(right.resolve(WINDOW, WORK_AREA), (1920 - 128, 1040 - 128));

        assert_eq!(free_intent().resolve(WINDOW, WORK_AREA), (896, 912));
    }

    #[test]
    fn test_intent_reprojects_onto_a_different_screen() {
        // La même intention, sur un écran plus petit à origine négative.
        let other = PhysicalRect::new(-1280, -720, 1280, 700);
        let (x, y) = free_intent().resolve(WINDOW, other);
        assert_eq!(y, -720 + 700 - 128);
        assert!(x >= -1280 && x + 128 <= -1280 + 1280);
    }

    #[test]
    fn test_a_window_larger_than_the_work_area_keeps_its_top_left_visible() {
        let cramped = PhysicalRect::new(100, 200, 64, 64);
        let (x, y) = free_intent().resolve((256, 256), cramped);
        assert_eq!((x, y), (100, 200));
    }

    #[test]
    fn test_intent_is_deduced_from_a_placed_window() {
        let left = PlacementIntent::from_window(window_at(4, 912), WORK_AREA, 48);
        assert_eq!(left.anchor, ScreenAnchor::BottomLeft);

        let right = PlacementIntent::from_window(window_at(1920 - 128 - 4, 912), WORK_AREA, 48);
        assert_eq!(right.anchor, ScreenAnchor::BottomRight);

        let middle = PlacementIntent::from_window(window_at(896, 912), WORK_AREA, 48);
        assert_eq!(middle.anchor, ScreenAnchor::BottomEdge);
        assert_eq!(middle.along_edge_per_mille, 500);
    }

    #[test]
    fn test_intent_round_trip_lands_within_a_pixel() {
        for x in [0, 137, 640, 1200, 1920 - 128] {
            let intent = PlacementIntent::from_window(window_at(x, 912), WORK_AREA, 0);
            let (resolved_x, _) = intent.resolve(WINDOW, WORK_AREA);
            assert!(
                (resolved_x - x).abs() <= 1,
                "aller-retour d'ancre imprécis : {x} -> {resolved_x}"
            );
        }
    }

    #[test]
    fn test_intent_normalization_is_idempotent() {
        let mut intent = PlacementIntent {
            anchor: ScreenAnchor::BottomEdge,
            along_edge_per_mille: 60_000,
        };
        intent.normalize();
        assert_eq!(intent.along_edge_per_mille, 1_000);
        let once = intent;
        intent.normalize();
        assert_eq!(intent, once);
    }

    #[test]
    fn test_a_fall_lands_on_the_floor_and_settles() {
        let mut motion = DesktopMotion::default();
        let start = motion.begin(
            window_at(896, 100),
            WORK_AREA,
            free_intent(),
            MotionConfig::default(),
            1_000,
            false,
        );
        assert!(!start.settled);
        assert!(motion.is_active());

        let (final_update, _) = run_to_rest(&mut motion, Duration::from_millis(16));
        assert_eq!(final_update.position, (896, 912));
        assert!(final_update.settled);
        assert!(!motion.is_active());
        assert!(motion.next_step_delay().is_none());
    }

    #[test]
    fn test_irregular_steps_reach_the_same_landing_point() {
        let make = || {
            let mut motion = DesktopMotion::default();
            motion.begin(
                window_at(300, 50),
                WORK_AREA,
                free_intent(),
                MotionConfig::default(),
                1_000,
                false,
            );
            motion
        };

        let mut regular = make();
        let (regular_end, _) = run_to_rest(&mut regular, Duration::from_millis(16));

        let mut irregular = make();
        let mut steps = 0;
        let pattern = [3_u64, 25, 8, 40, 11, 60, 5];
        let end = loop {
            let step = Duration::from_millis(pattern[steps % pattern.len()]);
            let update = irregular.advance(step);
            steps += 1;
            if update.settled {
                break update;
            }
            assert!(steps < 5_000, "la chute irrégulière ne converge pas");
        };

        assert!(
            (end.position.0 - regular_end.position.0).abs() <= 1
                && (end.position.1 - regular_end.position.1).abs() <= 1,
            "points d'arrivée divergents : {:?} vs {:?}",
            end.position,
            regular_end.position
        );
    }

    #[test]
    fn test_at_most_one_bounce_occurs() {
        let mut motion = DesktopMotion::default();
        motion.begin(
            window_at(896, 0),
            WORK_AREA,
            free_intent(),
            MotionConfig::default(),
            1_000,
            false,
        );

        let mut floor_touches = 0;
        let mut previously_below = false;
        for _ in 0..2_000 {
            let update = motion.advance(Duration::from_millis(16));
            let at_floor = update.position.1 >= 912;
            if at_floor && !previously_below {
                floor_touches += 1;
            }
            previously_below = at_floor;
            if update.settled {
                break;
            }
        }
        assert!(
            floor_touches <= MAX_BOUNCES as usize + 1,
            "trop de contacts avec le plancher : {floor_touches}"
        );
    }

    #[test]
    fn test_reduced_motion_places_immediately() {
        let mut motion = DesktopMotion::default();
        let update = motion.begin(
            window_at(50, 20),
            WORK_AREA,
            free_intent(),
            MotionConfig::default(),
            1_000,
            true,
        );
        assert!(update.settled);
        assert_eq!(update.position, (896, 912));
        assert!(!motion.is_active());
        assert!(motion.next_step_delay().is_none());
    }

    #[test]
    fn test_a_huge_time_jump_lands_safely_without_catching_up() {
        let mut motion = DesktopMotion::default();
        motion.begin(
            window_at(896, 0),
            WORK_AREA,
            free_intent(),
            MotionConfig::default(),
            1_000,
            false,
        );

        let update = motion.advance(Duration::from_secs(3_600));
        assert!(update.settled);
        assert_eq!(update.position, (896, 912));
    }

    #[test]
    fn test_absurd_durations_do_not_panic() {
        let mut motion = DesktopMotion::default();
        motion.begin(
            window_at(896, 0),
            WORK_AREA,
            free_intent(),
            MotionConfig::default(),
            1_000,
            false,
        );
        let update = motion.advance(Duration::MAX);
        assert!(update.settled);
    }

    #[test]
    fn test_zero_duration_does_not_move_or_settle() {
        let mut motion = DesktopMotion::default();
        motion.begin(
            window_at(896, 0),
            WORK_AREA,
            free_intent(),
            MotionConfig::default(),
            1_000,
            false,
        );
        let update = motion.advance(Duration::ZERO);
        assert!(!update.settled);
        assert_eq!(update.position, (896, 0));
    }

    #[test]
    fn test_advancing_an_idle_engine_is_a_noop() {
        let mut motion = DesktopMotion::default();
        let update = motion.advance(Duration::from_millis(16));
        assert!(update.settled);
        assert_eq!(update.phase, MotionPhase::Idle);
    }

    #[test]
    fn test_cancelling_stops_the_fall_in_place() {
        let mut motion = DesktopMotion::default();
        motion.begin(
            window_at(896, 0),
            WORK_AREA,
            free_intent(),
            MotionConfig::default(),
            1_000,
            false,
        );
        motion.advance(Duration::from_millis(64));
        let before = motion.current_update().position;

        motion.cancel();
        assert!(!motion.is_active());
        assert_eq!(motion.current_update().position, before);
    }

    #[test]
    fn test_a_window_already_on_the_floor_settles_without_moving() {
        let mut motion = DesktopMotion::default();
        let update = motion.begin(
            window_at(896, 912),
            WORK_AREA,
            free_intent(),
            MotionConfig::default(),
            1_000,
            false,
        );
        assert!(update.settled);
        assert_eq!(update.position, (896, 912));
    }

    #[test]
    fn test_a_window_below_the_floor_is_brought_back_up() {
        let mut motion = DesktopMotion::default();
        let update = motion.begin(
            window_at(896, 5_000),
            WORK_AREA,
            free_intent(),
            MotionConfig::default(),
            1_000,
            false,
        );
        assert!(update.settled);
        assert_eq!(update.position, (896, 912));
    }

    #[test]
    fn test_negative_origin_work_areas_are_handled() {
        let left_screen = PhysicalRect::new(-2560, -200, 2560, 1400);
        let mut motion = DesktopMotion::default();
        motion.begin(
            PhysicalRect::new(-2000, -150, WINDOW.0, WINDOW.1),
            left_screen,
            PlacementIntent {
                anchor: ScreenAnchor::BottomLeft,
                along_edge_per_mille: 0,
            },
            MotionConfig::default(),
            1_000,
            false,
        );
        let (update, _) = run_to_rest(&mut motion, Duration::from_millis(16));
        assert_eq!(update.position, (-2560, -200 + 1400 - 128));
    }

    #[test]
    fn test_the_landing_window_stays_inside_the_work_area() {
        for x in [-5_000, -1, 0, 900, 1_900, 50_000] {
            let mut motion = DesktopMotion::default();
            motion.begin(
                window_at(x, 0),
                WORK_AREA,
                PlacementIntent::from_window(window_at(x, 0), WORK_AREA, 48),
                MotionConfig::default(),
                1_000,
                true,
            );
            let update = motion.advance(Duration::from_millis(16));
            let (px, py) = update.position;
            assert!(
                px >= WORK_AREA.x && i64::from(px) + 128 <= WORK_AREA.right(),
                "fenêtre hors zone horizontalement depuis {x} : {px}"
            );
            assert!(
                py >= WORK_AREA.y && i64::from(py) + 128 <= WORK_AREA.bottom(),
                "fenêtre hors zone verticalement depuis {x} : {py}"
            );
        }
    }

    #[test]
    fn test_higher_density_falls_at_a_comparable_visual_speed() {
        // À densité double, la distance en pixels double : la durée de chute doit
        // rester du même ordre, sinon le familier paraît deux fois plus lent.
        let mut standard = DesktopMotion::default();
        standard.begin(
            window_at(896, 0),
            WORK_AREA,
            free_intent(),
            MotionConfig::default(),
            1_000,
            false,
        );
        let (_, standard_steps) = run_to_rest(&mut standard, Duration::from_millis(16));

        let dense_area = PhysicalRect::new(0, 0, 3840, 2080);
        let mut dense = DesktopMotion::default();
        dense.begin(
            PhysicalRect::new(1792, 0, 256, 256),
            dense_area,
            free_intent(),
            MotionConfig::default(),
            2_000,
            false,
        );
        let (_, dense_steps) = run_to_rest(&mut dense, Duration::from_millis(16));

        let ratio = f64::from(dense_steps) / f64::from(standard_steps);
        assert!(
            (0.7..=1.4).contains(&ratio),
            "durées de chute trop différentes : {standard_steps} vs {dense_steps}"
        );
    }

    #[test]
    fn test_config_normalization_neutralises_hostile_values() {
        let mut config = MotionConfig {
            gravity: f32::NAN,
            max_fall_speed: f32::INFINITY,
            glide_speed: -1.0,
            bounce_damping: 5.0,
            min_bounce_speed: f32::NEG_INFINITY,
            corner_snap_points: u32::MAX,
        };
        config.normalize();

        assert_eq!(config.gravity, MotionConfig::default().gravity);
        assert!(config.max_fall_speed.is_finite());
        assert!(config.glide_speed >= 50.0);
        assert!(config.bounce_damping <= 0.9);
        assert!(config.min_bounce_speed.is_finite());
        assert_eq!(config.corner_snap_points, 512);

        let once = config;
        config.normalize();
        assert_eq!(config, once, "normalisation non idempotente");
    }

    #[test]
    fn test_a_fall_with_hostile_config_still_converges() {
        let mut config = MotionConfig {
            gravity: f32::NAN,
            bounce_damping: f32::NAN,
            ..MotionConfig::default()
        };
        config.normalize();

        let mut motion = DesktopMotion::default();
        motion.begin(
            window_at(896, 0),
            WORK_AREA,
            free_intent(),
            config,
            1_000,
            false,
        );
        let (update, _) = run_to_rest(&mut motion, Duration::from_millis(16));
        assert_eq!(update.position, (896, 912));
    }
}
