//! Statistiques vitales et calcul de décroissance temporelle du Gremlin.
//!
//! Les champs sont privés : les jauges sont bornées dans `[0, 100]` et finies
//! *par construction*, y compris après désérialisation d'une sauvegarde
//! corrompue (voir [`PetStats::normalize`]).

use crate::config::{DecayConfig, MoodConfig};
use crate::error::CoreError;
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Valeur maximale autorisée pour une jauge de statistique.
pub const MAX_STAT_VALUE: f32 = 100.0;
/// Valeur minimale autorisée pour une jauge de statistique.
pub const MIN_STAT_VALUE: f32 = 0.0;

/// Ramène une jauge dans `[0, 100]`.
///
/// `NaN` est remplacé par [`MAX_STAT_VALUE`] : une jauge corrompue ne doit pas
/// tuer silencieusement le familier de l'utilisateur. Noter que `f32::clamp`
/// seul ne suffit pas, il propage `NaN`.
fn sanitize_gauge(value: f32) -> f32 {
    if value.is_nan() {
        MAX_STAT_VALUE
    } else {
        value.clamp(MIN_STAT_VALUE, MAX_STAT_VALUE)
    }
}

/// Neutralise un montant d'action non exploitable (négatif, infini ou `NaN`).
///
/// Les appels publics sont validés en amont par
/// [`PetState`](crate::state::PetState) ; cette barrière évite qu'un appel
/// direct à l'API bas niveau n'empoisonne les jauges.
fn sanitize_amount(amount: f32) -> f32 {
    if amount.is_finite() && amount >= 0.0 {
        amount
    } else {
        0.0
    }
}

/// Statistiques fondamentales du compagnon virtuel.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PetStats {
    energy: f32,
    satiety: f32,
    happiness: f32,
}

impl Default for PetStats {
    fn default() -> Self {
        Self {
            energy: MAX_STAT_VALUE,
            satiety: MAX_STAT_VALUE,
            happiness: MAX_STAT_VALUE,
        }
    }
}

impl PetStats {
    /// Crée de nouvelles statistiques en bornant automatiquement chaque valeur dans `[0.0, 100.0]`.
    ///
    /// Une valeur `NaN` est remplacée par [`MAX_STAT_VALUE`].
    #[must_use]
    pub fn new(energy: f32, satiety: f32, happiness: f32) -> Self {
        Self {
            energy: sanitize_gauge(energy),
            satiety: sanitize_gauge(satiety),
            happiness: sanitize_gauge(happiness),
        }
    }

    /// Tente de créer des statistiques en vérifiant que chaque valeur est comprise dans `[0.0, 100.0]`.
    ///
    /// # Errors
    /// Renvoie `CoreError::InvalidStatValue` si l'une des valeurs est invalide ou `NaN`.
    pub fn try_new(energy: f32, satiety: f32, happiness: f32) -> Result<Self, CoreError> {
        let validate = |name: &'static str, val: f32| -> Result<(), CoreError> {
            if val.is_nan() || !(MIN_STAT_VALUE..=MAX_STAT_VALUE).contains(&val) {
                Err(CoreError::InvalidStatValue { name, value: val })
            } else {
                Ok(())
            }
        };

        validate("energy", energy)?;
        validate("satiety", satiety)?;
        validate("happiness", happiness)?;

        Ok(Self {
            energy,
            satiety,
            happiness,
        })
    }

    /// Niveau d'énergie actuel, dans `[0.0, 100.0]`.
    #[must_use]
    pub const fn energy(&self) -> f32 {
        self.energy
    }

    /// Satiété actuelle, dans `[0.0, 100.0]` (100 = rassasié, 0 = affamé).
    #[must_use]
    pub const fn satiety(&self) -> f32 {
        self.satiety
    }

    /// Bonheur actuel, dans `[0.0, 100.0]`.
    #[must_use]
    pub const fn happiness(&self) -> f32 {
        self.happiness
    }

    /// Restaure l'invariant de bornage après désérialisation.
    pub fn normalize(&mut self) {
        self.energy = sanitize_gauge(self.energy);
        self.satiety = sanitize_gauge(self.satiety);
        self.happiness = sanitize_gauge(self.happiness);
    }

    /// Applique la décroissance naturelle selon le temps écoulé et la configuration de décroissance.
    pub fn apply_decay_with_config(
        &mut self,
        elapsed: Duration,
        config: &DecayConfig,
        is_sleeping: bool,
    ) {
        let minutes = elapsed.as_secs_f32() / 60.0;
        let decay_mult = if is_sleeping {
            config.sleep_decay_multiplier
        } else {
            1.0
        };

        let energy_loss = config.energy_decay_per_minute * minutes * decay_mult;
        let satiety_loss = config.satiety_decay_per_minute * minutes * decay_mult;
        let happiness_loss = config.happiness_decay_per_minute * minutes * decay_mult;

        self.energy = sanitize_gauge(self.energy - energy_loss);
        self.satiety = sanitize_gauge(self.satiety - satiety_loss);
        self.happiness = sanitize_gauge(self.happiness - happiness_loss);
    }

    /// Applique un gain d'énergie suite à un repos.
    pub fn rest(&mut self, amount: f32) {
        self.energy = sanitize_gauge(self.energy + sanitize_amount(amount));
    }

    /// Nourrit le Gremlin pour augmenter sa satiété.
    pub fn feed(&mut self, amount: f32) {
        self.satiety = sanitize_gauge(self.satiety + sanitize_amount(amount));
    }

    /// Augmente le bonheur du Gremlin (caresse, interaction bienveillante).
    pub fn pet(&mut self, amount: f32) {
        self.happiness = sanitize_gauge(self.happiness + sanitize_amount(amount));
    }

    /// Soigne le Gremlin, répartissant `amount * split_ratio` sur chacune des trois jauges.
    pub fn heal(&mut self, amount: f32, split_ratio: f32) {
        let gain = sanitize_amount(amount) * sanitize_amount(split_ratio);
        self.energy = sanitize_gauge(self.energy + gain);
        self.satiety = sanitize_gauge(self.satiety + gain);
        self.happiness = sanitize_gauge(self.happiness + gain);
    }

    /// Stimule le Gremlin (ex : nouveau commit) pour remonter joie et énergie.
    pub fn boost_from_commit(&mut self, energy_boost: f32, happiness_boost: f32) {
        self.energy = sanitize_gauge(self.energy + sanitize_amount(energy_boost));
        self.happiness = sanitize_gauge(self.happiness + sanitize_amount(happiness_boost));
    }

    /// Indique si les statistiques traduisent une mort clinique (énergie et satiété épuisées).
    ///
    /// Les jauges étant bornées, la comparaison à [`MIN_STAT_VALUE`] est exacte.
    #[must_use]
    pub fn is_dead(&self) -> bool {
        self.energy <= MIN_STAT_VALUE && self.satiety <= MIN_STAT_VALUE
    }

    /// Indique si l'une des jauges vitales est en état critique.
    #[must_use]
    pub fn is_critical(&self, config: &MoodConfig) -> bool {
        self.energy < config.critical_gauge
            || self.satiety < config.critical_gauge
            || self.happiness < config.critical_gauge
    }

    /// Indique si le familier est affamé.
    #[must_use]
    pub fn is_starving(&self, config: &MoodConfig) -> bool {
        self.satiety < config.sick_satiety
    }

    /// Indique si le familier est épuisé.
    #[must_use]
    pub fn is_exhausted(&self, config: &MoodConfig) -> bool {
        self.energy < config.sick_energy
    }
}

#[cfg(test)]
#[allow(clippy::float_cmp, clippy::expect_used)]
mod tests {
    use super::*;

    fn decay_of(stats: &mut PetStats, secs: u64, is_sleeping: bool) {
        stats.apply_decay_with_config(
            Duration::from_secs(secs),
            &DecayConfig::default(),
            is_sleeping,
        );
    }

    #[test]
    fn test_stats_default_and_clamp() {
        let stats = PetStats::default();
        assert_eq!(stats.energy(), 100.0);
        assert_eq!(stats.satiety(), 100.0);
        assert_eq!(stats.happiness(), 100.0);

        let clamped = PetStats::new(150.0, -20.0, 50.0);
        assert_eq!(clamped.energy(), 100.0);
        assert_eq!(clamped.satiety(), 0.0);
        assert_eq!(clamped.happiness(), 50.0);
    }

    #[test]
    fn test_new_neutralises_nan() {
        let stats = PetStats::new(f32::NAN, f32::NAN, f32::NAN);
        assert_eq!(stats.energy(), MAX_STAT_VALUE);
        assert_eq!(stats.satiety(), MAX_STAT_VALUE);
        assert_eq!(stats.happiness(), MAX_STAT_VALUE);
        assert!(!stats.is_dead());
    }

    #[test]
    fn test_try_new_validation() {
        assert!(PetStats::try_new(50.0, 50.0, 50.0).is_ok());
        assert!(PetStats::try_new(101.0, 50.0, 50.0).is_err());
        assert!(PetStats::try_new(50.0, -1.0, 50.0).is_err());
        assert!(PetStats::try_new(50.0, 50.0, f32::NAN).is_err());
    }

    #[test]
    fn test_normalize_repairs_hostile_deserialization() {
        let hostile = r#"{"energy": 9999.0, "satiety": -50.0, "happiness": null}"#;
        // `null` n'est pas un f32 valide : la valeur par défaut ne s'applique
        // qu'aux champs absents, on vérifie donc les deux cas séparément.
        assert!(serde_json::from_str::<PetStats>(hostile).is_err());

        let mut stats: PetStats = serde_json::from_str(r#"{"energy": 9999.0, "satiety": -50.0}"#)
            .expect("champs absents");
        stats.normalize();

        assert_eq!(stats.energy(), 100.0);
        assert_eq!(stats.satiety(), 0.0);
        assert_eq!(stats.happiness(), 100.0);
    }

    #[test]
    fn test_stats_decay_awake_and_asleep() {
        let mut stats = PetStats::new(100.0, 100.0, 100.0);
        decay_of(&mut stats, 3600, false);

        assert_eq!(stats.energy(), 70.0);
        assert_eq!(stats.satiety(), 40.0);
        assert_eq!(stats.happiness(), 52.0);

        // 60 minutes endormi : taux réduit à 10 %.
        let mut asleep_stats = PetStats::new(100.0, 100.0, 100.0);
        decay_of(&mut asleep_stats, 3600, true);

        assert_eq!(asleep_stats.energy(), 97.0);
        assert_eq!(asleep_stats.satiety(), 94.0);
        assert!((asleep_stats.happiness() - 95.2).abs() < 1e-4);
    }

    #[test]
    fn test_zero_duration_decay_is_a_noop() {
        let mut stats = PetStats::new(50.0, 50.0, 50.0);
        let before = stats;
        stats.apply_decay_with_config(Duration::ZERO, &DecayConfig::default(), false);
        assert_eq!(before, stats);
    }

    #[test]
    fn test_stats_actions() {
        let mood = MoodConfig::default();
        let mut stats = PetStats::new(10.0, 10.0, 10.0);
        assert!(stats.is_critical(&mood));
        assert!(stats.is_starving(&mood));
        assert!(stats.is_exhausted(&mood));

        stats.feed(30.0);
        assert_eq!(stats.satiety(), 40.0);
        assert!(!stats.is_starving(&mood));

        stats.rest(20.0);
        assert_eq!(stats.energy(), 30.0);
        assert!(!stats.is_exhausted(&mood));

        stats.pet(40.0);
        assert_eq!(stats.happiness(), 50.0);

        stats.heal(20.0, 0.5);
        assert_eq!(stats.energy(), 40.0);
        assert_eq!(stats.satiety(), 50.0);
        assert_eq!(stats.happiness(), 60.0);

        stats.boost_from_commit(10.0, 15.0);
        assert_eq!(stats.energy(), 50.0);
        assert_eq!(stats.happiness(), 75.0);
    }

    #[test]
    fn test_actions_ignore_poisoned_amounts() {
        let mut stats = PetStats::new(50.0, 50.0, 50.0);
        let before = stats;

        stats.feed(f32::NAN);
        stats.pet(f32::INFINITY);
        stats.rest(-1_000.0);
        stats.heal(f32::NAN, 0.5);
        stats.boost_from_commit(f32::NEG_INFINITY, f32::NAN);

        assert_eq!(before, stats, "aucune jauge ne doit être empoisonnée");
        assert!(stats.energy().is_finite());
        assert!(stats.satiety().is_finite());
        assert!(stats.happiness().is_finite());
    }

    #[test]
    fn test_is_dead_boundary() {
        assert!(PetStats::new(0.0, 0.0, 50.0).is_dead());
        assert!(!PetStats::new(0.1, 0.0, 0.0).is_dead());
        assert!(!PetStats::new(0.0, 0.1, 0.0).is_dead());
    }
}
