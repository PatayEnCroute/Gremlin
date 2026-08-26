//! Classification des gestes de souris sur la fenêtre du familier.
//!
//! Un clic, un déplacement et une caresse sont **trois gestes différents**, et
//! la fenêtre du familier n'a qu'un bouton pour les exprimer. Sans machine
//! explicite, la caresse se déclencherait après chaque déplacement et un
//! tremblement de souris changerait de sens selon la densité de l'écran.
//!
//! La règle retenue tient en quatre lignes :
//!
//! * l'appui sur un pixel visible **arme** un geste ;
//! * un relâchement bref et immobile produit une caresse ;
//! * un déplacement au-delà du seuil produit un glisser, puis une chute au
//!   relâchement ;
//! * une perte de focus, une fermeture ou un échec natif **annule** le geste,
//!   sans action métier.
//!
//! Le seuil est exprimé en points de conception et projeté en pixels physiques :
//! six points restent six points, que l'écran soit à 100 % ou à 200 %. La
//! comparaison se fait en distance au carré, en arithmétique entière — pas de
//! racine carrée, donc pas d'arrondi flottant sur une décision binaire.

use std::time::{Duration, Instant};

/// Seuil de déplacement par défaut, en points de conception.
pub const DEFAULT_MOVEMENT_THRESHOLD_POINTS: u32 = 6;

/// Durée maximale par défaut d'un clic, au-delà de laquelle ce n'est plus une caresse.
pub const DEFAULT_MAX_CLICK_DURATION: Duration = Duration::from_millis(500);

/// Amplitude maximale d'une position de curseur acceptée, en pixels.
///
/// Au-delà, l'événement est corrompu : aucun écran n'a un million de pixels de
/// côté, et une origine de geste fabriquée sur une telle valeur fausserait
/// toutes les comparaisons suivantes.
const MAX_CURSOR_COORDINATE: f64 = 1_000_000.0;

/// Seuil de déplacement minimal, en pixels physiques.
///
/// Empêche un facteur d'échelle minuscule de ramener le seuil à zéro, ce qui
/// classerait le moindre frémissement de souris comme un déplacement.
const MIN_THRESHOLD_PX: u32 = 2;

/// Réglages de classification des gestes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GestureConfig {
    /// Distance au-delà de laquelle le geste devient un déplacement.
    pub movement_threshold_points: u32,
    /// Durée maximale d'un clic bref.
    pub max_click_duration: Duration,
}

impl Default for GestureConfig {
    fn default() -> Self {
        Self {
            movement_threshold_points: DEFAULT_MOVEMENT_THRESHOLD_POINTS,
            max_click_duration: DEFAULT_MAX_CLICK_DURATION,
        }
    }
}

impl GestureConfig {
    /// Seuil de déplacement en pixels physiques pour la densité donnée.
    #[must_use]
    pub const fn threshold_px(self, scale_factor_milli: u32) -> u32 {
        // Arrondi au plus proche : tronquer rendrait le seuil plus strict que
        // les six points annoncés dès que la densité n'est pas entière, et un
        // geste immobile à 125 % passerait pour un déplacement.
        let scaled =
            (self.movement_threshold_points as u64 * scale_factor_milli as u64 + 500) / 1_000;
        if scaled < MIN_THRESHOLD_PX as u64 {
            MIN_THRESHOLD_PX
        } else if scaled > u32::MAX as u64 {
            u32::MAX
        } else {
            scaled as u32
        }
    }
}

/// Conclusion d'un geste relâché.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GestureOutcome {
    /// Aucune action métier : geste annulé, trop long, ou jamais armé.
    Ignored,
    /// Clic bref et immobile : une caresse, et une seule.
    Petted,
    /// La fenêtre a bougé : la chute peut commencer.
    Dropped,
}

/// Geste armé en cours.
#[derive(Debug, Clone, Copy)]
struct ArmedGesture {
    /// Instant de l'appui.
    started_at: Instant,
    /// Position physique du curseur à l'appui.
    cursor_origin: (i64, i64),
    /// Position physique externe de la fenêtre à l'appui.
    window_origin: (i64, i64),
    /// Le seuil de déplacement a déjà été franchi.
    ///
    /// Mémorisé plutôt que recalculé au relâchement : un aller-retour rapide
    /// ramènerait le curseur à son point de départ et ferait passer un vrai
    /// déplacement pour une caresse.
    moved: bool,
}

/// Machine de geste de la fenêtre du familier.
#[derive(Debug, Clone, Copy, Default)]
pub struct PetGesture {
    armed: Option<ArmedGesture>,
}

impl PetGesture {
    /// Arme un geste sur appui du bouton gauche.
    ///
    /// L'appelant a déjà vérifié que le mode interactif est actif et que le
    /// pixel visé est opaque : cette machine ne connaît pas le sprite.
    pub fn arm(&mut self, now: Instant, cursor: (f64, f64), window_origin: (i32, i32)) {
        let Some(cursor_origin) = physical_point(cursor) else {
            // Une position non finie vient d'un événement incomplet : le geste
            // n'est pas armé plutôt qu'armé sur une origine inventée.
            self.armed = None;
            return;
        };
        self.armed = Some(ArmedGesture {
            started_at: now,
            cursor_origin,
            window_origin: (i64::from(window_origin.0), i64::from(window_origin.1)),
            moved: false,
        });
    }

    /// Indique si un geste est en cours.
    #[must_use]
    pub const fn is_armed(&self) -> bool {
        self.armed.is_some()
    }

    /// Indique si le geste courant a déjà franchi le seuil de déplacement.
    #[must_use]
    pub const fn has_moved(&self) -> bool {
        match &self.armed {
            Some(gesture) => gesture.moved,
            None => false,
        }
    }

    /// Prend acte d'un déplacement du curseur.
    pub fn note_cursor(&mut self, cursor: (f64, f64), threshold_px: u32) {
        let Some(point) = physical_point(cursor) else {
            return;
        };
        let Some(gesture) = &mut self.armed else {
            return;
        };
        if exceeds(point, gesture.cursor_origin, threshold_px) {
            gesture.moved = true;
        }
    }

    /// Prend acte d'un déplacement de la fenêtre.
    ///
    /// Le glisser natif déplace la fenêtre sans que l'application reçoive
    /// nécessairement un `CursorMoved` : c'est la seule preuve fiable, sur
    /// certains gestionnaires de fenêtres, qu'un déplacement a bien eu lieu.
    pub fn note_window_moved(&mut self, position: (i32, i32), threshold_px: u32) {
        let point = (i64::from(position.0), i64::from(position.1));
        let Some(gesture) = &mut self.armed else {
            return;
        };
        if exceeds(point, gesture.window_origin, threshold_px) {
            gesture.moved = true;
        }
    }

    /// Annule le geste courant sans produire d'action métier.
    ///
    /// Appelée sur perte de focus, fermeture de fenêtre, échec du glisser natif
    /// ou topologie devenue invalide.
    pub fn cancel(&mut self) {
        self.armed = None;
    }

    /// Classe le geste au relâchement du bouton.
    ///
    /// Le geste est consommé : un second relâchement ne peut pas produire une
    /// deuxième caresse.
    pub fn release(
        &mut self,
        now: Instant,
        cursor: (f64, f64),
        window_origin: (i32, i32),
        config: GestureConfig,
        threshold_px: u32,
    ) -> GestureOutcome {
        let Some(gesture) = self.armed.take() else {
            return GestureOutcome::Ignored;
        };

        let moved = gesture.moved
            || physical_point(cursor)
                .is_some_and(|point| exceeds(point, gesture.cursor_origin, threshold_px))
            || exceeds(
                (i64::from(window_origin.0), i64::from(window_origin.1)),
                gesture.window_origin,
                threshold_px,
            );

        if moved {
            return GestureOutcome::Dropped;
        }

        // `checked_duration_since` plutôt qu'une soustraction : un instant
        // antérieur à l'appui — horloge reculée, événement rejoué — annule le
        // geste au lieu de paniquer.
        let held = now.checked_duration_since(gesture.started_at);
        match held {
            Some(held) if held <= config.max_click_duration => GestureOutcome::Petted,
            _ => GestureOutcome::Ignored,
        }
    }
}

/// Convertit une position de curseur en point physique entier.
///
/// Renvoie `None` sur une valeur non finie ou hors des bornes représentables :
/// un événement incomplet ne doit pas fabriquer une origine de geste.
fn physical_point(cursor: (f64, f64)) -> Option<(i64, i64)> {
    if !cursor.0.is_finite() || !cursor.1.is_finite() {
        return None;
    }
    if cursor.0.abs() > MAX_CURSOR_COORDINATE || cursor.1.abs() > MAX_CURSOR_COORDINATE {
        return None;
    }
    Some((cursor.0.round() as i64, cursor.1.round() as i64))
}

/// Indique que deux points sont séparés de plus de `threshold_px`.
///
/// Comparaison en distance au carré : ni racine carrée, ni flottant, donc pas
/// d'écart d'arrondi entre deux exécutions.
fn exceeds(point: (i64, i64), origin: (i64, i64), threshold_px: u32) -> bool {
    let dx = point.0.saturating_sub(origin.0);
    let dy = point.1.saturating_sub(origin.1);
    let distance_squared = dx.saturating_mul(dx).saturating_add(dy.saturating_mul(dy));
    let threshold = i64::from(threshold_px);
    distance_squared > threshold.saturating_mul(threshold)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    fn config() -> GestureConfig {
        GestureConfig::default()
    }

    #[test]
    fn test_threshold_follows_the_display_density() {
        let config = config();
        assert_eq!(config.threshold_px(1_000), 6);
        assert_eq!(config.threshold_px(1_250), 8);
        assert_eq!(config.threshold_px(1_500), 9);
        assert_eq!(config.threshold_px(2_000), 12);
        // Une densité minuscule ne doit pas ramener le seuil à zéro.
        assert_eq!(config.threshold_px(1), MIN_THRESHOLD_PX);
    }

    #[test]
    fn test_a_short_still_click_is_a_single_pet() {
        let start = Instant::now();
        let mut gesture = PetGesture::default();
        gesture.arm(start, (100.0, 100.0), (10, 10));

        let outcome = gesture.release(
            start + Duration::from_millis(80),
            (101.0, 101.0),
            (10, 10),
            config(),
            6,
        );
        assert_eq!(outcome, GestureOutcome::Petted);

        // Le geste est consommé : pas de seconde caresse.
        let replay = gesture.release(
            start + Duration::from_millis(90),
            (101.0, 101.0),
            (10, 10),
            config(),
            6,
        );
        assert_eq!(replay, GestureOutcome::Ignored);
    }

    #[test]
    fn test_a_long_still_press_pets_nothing() {
        let start = Instant::now();
        let mut gesture = PetGesture::default();
        gesture.arm(start, (100.0, 100.0), (10, 10));

        let outcome = gesture.release(
            start + Duration::from_millis(900),
            (100.0, 100.0),
            (10, 10),
            config(),
            6,
        );
        assert_eq!(outcome, GestureOutcome::Ignored);
    }

    #[test]
    fn test_a_release_without_press_is_ignored() {
        let mut gesture = PetGesture::default();
        assert_eq!(
            gesture.release(Instant::now(), (0.0, 0.0), (0, 0), config(), 6),
            GestureOutcome::Ignored
        );
    }

    #[test]
    fn test_cursor_movement_beyond_the_threshold_becomes_a_drop() {
        let start = Instant::now();
        let mut gesture = PetGesture::default();
        gesture.arm(start, (100.0, 100.0), (10, 10));
        gesture.note_cursor((110.0, 100.0), 6);
        assert!(gesture.has_moved());

        let outcome = gesture.release(
            start + Duration::from_millis(50),
            (110.0, 100.0),
            (10, 10),
            config(),
            6,
        );
        assert_eq!(outcome, GestureOutcome::Dropped);
    }

    #[test]
    fn test_movement_exactly_at_the_threshold_is_still_a_pet() {
        let start = Instant::now();
        let mut gesture = PetGesture::default();
        gesture.arm(start, (100.0, 100.0), (10, 10));
        // Distance exactement 6 : le seuil est franchi *au-delà*, pas *à*.
        gesture.note_cursor((106.0, 100.0), 6);
        assert!(!gesture.has_moved());

        assert_eq!(
            gesture.release(
                start + Duration::from_millis(50),
                (106.0, 100.0),
                (10, 10),
                config(),
                6
            ),
            GestureOutcome::Petted
        );
    }

    #[test]
    fn test_a_round_trip_still_counts_as_a_drop() {
        let start = Instant::now();
        let mut gesture = PetGesture::default();
        gesture.arm(start, (100.0, 100.0), (10, 10));
        gesture.note_cursor((200.0, 200.0), 6);
        // Retour au point de départ : sans mémoire du franchissement, ce
        // déplacement passerait pour une caresse.
        gesture.note_cursor((100.0, 100.0), 6);

        assert_eq!(
            gesture.release(
                start + Duration::from_millis(60),
                (100.0, 100.0),
                (10, 10),
                config(),
                6
            ),
            GestureOutcome::Dropped
        );
    }

    #[test]
    fn test_window_movement_alone_is_enough_to_detect_a_drag() {
        // Certains gestionnaires de fenêtres capturent le pointeur pendant un
        // glisser natif : aucun `CursorMoved` n'arrive, seule la fenêtre bouge.
        let start = Instant::now();
        let mut gesture = PetGesture::default();
        gesture.arm(start, (100.0, 100.0), (10, 10));
        gesture.note_window_moved((300, 400), 6);
        assert!(gesture.has_moved());

        assert_eq!(
            gesture.release(
                start + Duration::from_millis(60),
                (100.0, 100.0),
                (300, 400),
                config(),
                6
            ),
            GestureOutcome::Dropped
        );
    }

    #[test]
    fn test_movement_detected_only_at_release_still_counts() {
        let start = Instant::now();
        let mut gesture = PetGesture::default();
        gesture.arm(start, (100.0, 100.0), (10, 10));

        // Aucun événement intermédiaire : le relâchement doit malgré tout voir
        // que la fenêtre a bougé.
        assert_eq!(
            gesture.release(
                start + Duration::from_millis(60),
                (100.0, 100.0),
                (900, 10),
                config(),
                6
            ),
            GestureOutcome::Dropped
        );
    }

    #[test]
    fn test_cancel_produces_no_business_action() {
        let start = Instant::now();
        let mut gesture = PetGesture::default();
        gesture.arm(start, (100.0, 100.0), (10, 10));
        assert!(gesture.is_armed());

        gesture.cancel();
        assert!(!gesture.is_armed());
        assert_eq!(
            gesture.release(
                start + Duration::from_millis(10),
                (100.0, 100.0),
                (10, 10),
                config(),
                6
            ),
            GestureOutcome::Ignored
        );
    }

    #[test]
    fn test_a_non_finite_cursor_never_arms_a_gesture() {
        let mut gesture = PetGesture::default();
        gesture.arm(Instant::now(), (f64::NAN, 0.0), (0, 0));
        assert!(!gesture.is_armed());

        gesture.arm(Instant::now(), (f64::INFINITY, 0.0), (0, 0));
        assert!(!gesture.is_armed());

        gesture.arm(Instant::now(), (1e12, 0.0), (0, 0));
        assert!(!gesture.is_armed());
    }

    #[test]
    fn test_a_non_finite_cursor_move_is_ignored_without_panicking() {
        let start = Instant::now();
        let mut gesture = PetGesture::default();
        gesture.arm(start, (100.0, 100.0), (10, 10));
        gesture.note_cursor((f64::NAN, f64::NAN), 6);
        assert!(!gesture.has_moved());
    }

    #[test]
    fn test_extreme_window_coordinates_do_not_overflow() {
        let start = Instant::now();
        let mut gesture = PetGesture::default();
        gesture.arm(start, (0.0, 0.0), (i32::MIN, i32::MIN));
        gesture.note_window_moved((i32::MAX, i32::MAX), 6);
        assert!(gesture.has_moved());
    }

    #[test]
    fn test_a_clock_that_went_backwards_cancels_instead_of_panicking() {
        let start = Instant::now();
        let mut gesture = PetGesture::default();
        gesture.arm(start + Duration::from_secs(10), (0.0, 0.0), (0, 0));

        assert_eq!(
            gesture.release(start, (0.0, 0.0), (0, 0), config(), 6),
            GestureOutcome::Ignored
        );
    }

    #[test]
    fn test_the_same_gesture_is_classified_identically_at_every_density() {
        let config = config();
        // Un déplacement de 6 points doit rester sous le seuil quelle que soit
        // la densité, et un déplacement de 20 points le franchir partout.
        for scale_milli in [1_000_u32, 1_250, 1_500, 2_000, 3_000] {
            let threshold = config.threshold_px(scale_milli);
            let start = Instant::now();

            let small = f64::from(6 * scale_milli) / 1_000.0;
            let mut still = PetGesture::default();
            still.arm(start, (500.0, 500.0), (0, 0));
            still.note_cursor((500.0 + small, 500.0), threshold);
            assert_eq!(
                still.release(
                    start + Duration::from_millis(50),
                    (500.0 + small, 500.0),
                    (0, 0),
                    config,
                    threshold
                ),
                GestureOutcome::Petted,
                "seuil trop bas à {scale_milli} millièmes"
            );

            let large = f64::from(20 * scale_milli) / 1_000.0;
            let mut moved = PetGesture::default();
            moved.arm(start, (500.0, 500.0), (0, 0));
            moved.note_cursor((500.0 + large, 500.0), threshold);
            assert_eq!(
                moved.release(
                    start + Duration::from_millis(50),
                    (500.0 + large, 500.0),
                    (0, 0),
                    config,
                    threshold
                ),
                GestureOutcome::Dropped,
                "seuil trop haut à {scale_milli} millièmes"
            );
        }
    }
}
