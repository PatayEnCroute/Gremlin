//! Machine à états émotionnels du Gremlin et règles de transition.

use crate::config::MoodConfig;
use crate::stats::PetStats;
use serde::{Deserialize, Serialize};
use std::fmt;

/// État émotionnel et comportemental du Gremlin.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PetMood {
    /// État joyeux et épanoui (statistiques équilibrées).
    Happy,
    /// En train de coder / célébrer un commit récent.
    Coding,
    /// A faim (satiété faible).
    Hungry,
    /// Épuisé / Sommeil (énergie très basse).
    Tired,
    /// Malade ou affecté par la dette technique.
    Sick,
    /// En colère / Très mécontent (bonheur et satiété très bas).
    Angry,
    /// Endormi (mode repos actif).
    Sleeping,
    /// Décédé / En sommeil profond suite à un abandon prolongé.
    Dead,
}

impl PetMood {
    /// Évalue l'humeur appropriée selon les statistiques actuelles et les indicateurs d'activité.
    ///
    /// Ordre de priorité des états, tous les seuils provenant de [`MoodConfig`] :
    /// 1. `Dead` (énergie et satiété épuisées)
    /// 2. `Sleeping` (mode sommeil activé)
    /// 3. `Sick` (familier épuisé ou affamé)
    /// 4. `Angry` (bonheur et satiété sous leurs seuils respectifs)
    /// 5. `Hungry` (satiété sous son seuil)
    /// 6. `Tired` (énergie sous son seuil)
    /// 7. `Coding` (activité Git récente)
    /// 8. `Happy` (état nominal)
    #[must_use]
    pub fn evaluate(
        stats: &PetStats,
        config: &MoodConfig,
        is_sleeping: bool,
        is_recently_active: bool,
    ) -> Self {
        // `Happy` comme humeur de référence : aucune hystérésis n'est appliquée.
        Self::evaluate_from(Self::Happy, stats, config, is_sleeping, is_recently_active)
    }

    /// Évalue l'humeur en tenant compte de l'humeur précédente (hystérésis).
    ///
    /// Sortir d'une humeur négative exige de dépasser le seuil d'entrée augmenté
    /// de [`MoodConfig::hysteresis_margin`]. Sans cela, une jauge oscillant
    /// autour d'un seuil émettrait un événement `MoodChanged` à chaque tick.
    #[must_use]
    pub fn evaluate_from(
        previous: Self,
        stats: &PetStats,
        config: &MoodConfig,
        is_sleeping: bool,
        is_recently_active: bool,
    ) -> Self {
        if stats.is_dead() {
            return Self::Dead;
        }

        if is_sleeping {
            return Self::Sleeping;
        }

        // Seuil élargi tant que l'humeur candidate est celle déjà en cours.
        let sticky = |mood: Self, threshold: f32| -> f32 {
            if previous == mood {
                threshold + config.hysteresis_margin
            } else {
                threshold
            }
        };

        let sick_config = MoodConfig {
            sick_energy: sticky(Self::Sick, config.sick_energy),
            sick_satiety: sticky(Self::Sick, config.sick_satiety),
            ..*config
        };
        if stats.is_exhausted(&sick_config) || stats.is_starving(&sick_config) {
            return Self::Sick;
        }

        if stats.happiness() < sticky(Self::Angry, config.angry_happiness)
            && stats.satiety() < sticky(Self::Angry, config.angry_satiety)
        {
            return Self::Angry;
        }

        if stats.satiety() < sticky(Self::Hungry, config.hungry_satiety) {
            return Self::Hungry;
        }

        if stats.energy() < sticky(Self::Tired, config.tired_energy) {
            return Self::Tired;
        }

        if is_recently_active {
            return Self::Coding;
        }

        Self::Happy
    }

    /// Indique si le Gremlin est en vie.
    #[must_use]
    pub const fn is_alive(self) -> bool {
        !matches!(self, Self::Dead)
    }

    /// Indique si le familier peut recevoir des interactions directes (hors sommeil et mort).
    ///
    /// Ce prédicat est le garde-fou unique des actions `feed`, `pet`, `heal` et
    /// `rest` de [`PetState`](crate::state::PetState).
    #[must_use]
    pub const fn can_interact(self) -> bool {
        !matches!(self, Self::Dead | Self::Sleeping)
    }

    /// Nom lisible en français de l'humeur.
    #[must_use]
    pub const fn display_name(self) -> &'static str {
        match self {
            Self::Happy => "Joyeux",
            Self::Coding => "En train de coder",
            Self::Hungry => "Affamé",
            Self::Tired => "Fatigué",
            Self::Sick => "Malade",
            Self::Angry => "En colère",
            Self::Sleeping => "Endormi",
            Self::Dead => "Éteint (Mort)",
        }
    }
}

impl fmt::Display for PetMood {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.display_name())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn evaluate(stats: &PetStats, is_sleeping: bool, is_recently_active: bool) -> PetMood {
        PetMood::evaluate(
            stats,
            &MoodConfig::default(),
            is_sleeping,
            is_recently_active,
        )
    }

    #[test]
    fn test_mood_priority_and_transitions() {
        let healthy = PetStats::new(100.0, 100.0, 100.0);
        assert_eq!(evaluate(&healthy, false, false), PetMood::Happy);
        assert_eq!(evaluate(&healthy, false, true), PetMood::Coding);
        assert_eq!(evaluate(&healthy, true, false), PetMood::Sleeping);
        assert_eq!(evaluate(&healthy, true, true), PetMood::Sleeping);

        let hungry = PetStats::new(80.0, 35.0, 80.0);
        assert_eq!(evaluate(&hungry, false, false), PetMood::Hungry);

        let tired = PetStats::new(20.0, 80.0, 80.0);
        assert_eq!(evaluate(&tired, false, false), PetMood::Tired);

        let sick = PetStats::new(10.0, 80.0, 80.0);
        assert_eq!(evaluate(&sick, false, false), PetMood::Sick);

        let angry = PetStats::new(80.0, 25.0, 15.0);
        assert_eq!(evaluate(&angry, false, false), PetMood::Angry);

        let dead = PetStats::new(0.0, 0.0, 0.0);
        assert_eq!(evaluate(&dead, false, false), PetMood::Dead);
        assert_eq!(evaluate(&dead, true, true), PetMood::Dead);
    }

    #[test]
    fn test_exact_threshold_boundaries_are_exclusive() {
        let config = MoodConfig::default();

        // Exactement au seuil : la comparaison est stricte, l'humeur négative
        // ne se déclenche donc pas.
        let at_hungry = PetStats::new(80.0, config.hungry_satiety, 80.0);
        assert_eq!(evaluate(&at_hungry, false, false), PetMood::Happy);
        let just_below = PetStats::new(80.0, config.hungry_satiety - 0.1, 80.0);
        assert_eq!(evaluate(&just_below, false, false), PetMood::Hungry);

        let at_tired = PetStats::new(config.tired_energy, 80.0, 80.0);
        assert_eq!(evaluate(&at_tired, false, false), PetMood::Happy);

        let at_sick_energy = PetStats::new(config.sick_energy, 80.0, 80.0);
        assert_eq!(evaluate(&at_sick_energy, false, false), PetMood::Tired);

        let at_sick_satiety = PetStats::new(80.0, config.sick_satiety, 80.0);
        assert_eq!(evaluate(&at_sick_satiety, false, false), PetMood::Hungry);
    }

    #[test]
    fn test_hysteresis_prevents_mood_flapping() {
        let config = MoodConfig::default();
        // Satiété juste au-dessus du seuil de faim : sans historique le
        // familier est joyeux, mais s'il avait faim il le reste.
        let borderline = PetStats::new(80.0, config.hungry_satiety + 1.0, 80.0);

        assert_eq!(
            PetMood::evaluate_from(PetMood::Happy, &borderline, &config, false, false),
            PetMood::Happy
        );
        assert_eq!(
            PetMood::evaluate_from(PetMood::Hungry, &borderline, &config, false, false),
            PetMood::Hungry
        );

        // Au-delà de la marge, la sortie de l'humeur négative est effective.
        let recovered = PetStats::new(
            80.0,
            config.hungry_satiety + config.hysteresis_margin + 1.0,
            80.0,
        );
        assert_eq!(
            PetMood::evaluate_from(PetMood::Hungry, &recovered, &config, false, false),
            PetMood::Happy
        );
    }

    #[test]
    fn test_death_ignores_hysteresis() {
        let config = MoodConfig::default();
        let dead = PetStats::new(0.0, 0.0, 100.0);
        assert_eq!(
            PetMood::evaluate_from(PetMood::Happy, &dead, &config, false, true),
            PetMood::Dead
        );
    }

    #[test]
    fn test_mood_predicates() {
        assert!(PetMood::Happy.is_alive());
        assert!(PetMood::Happy.can_interact());
        assert!(PetMood::Coding.can_interact());

        assert!(PetMood::Sleeping.is_alive());
        assert!(!PetMood::Sleeping.can_interact());

        assert!(!PetMood::Dead.is_alive());
        assert!(!PetMood::Dead.can_interact());
    }
}
