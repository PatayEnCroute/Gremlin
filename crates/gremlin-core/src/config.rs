//! Paramètres d'équilibrage et configurations du moteur métier.
//!
//! Toutes les structures de ce module sont désérialisées depuis des fichiers de
//! sauvegarde potentiellement corrompus ou modifiés à la main. Chacune expose
//! donc [`normalize`](CoreConfig::normalize) (correction silencieuse, utilisée
//! au chargement) et [`validate`](CoreConfig::validate) (diagnostic explicite).

use crate::error::CoreError;
use serde::{Deserialize, Serialize};

/// Pas de simulation par défaut pour le rattrapage hors-ligne, en secondes.
pub const DEFAULT_CATCHUP_STEP_SECS: u64 = 60;
/// Pas de simulation minimal accepté, en secondes.
pub const MIN_CATCHUP_STEP_SECS: u64 = 1;
/// Pas de simulation maximal accepté, en secondes (une heure).
pub const MAX_CATCHUP_STEP_SECS: u64 = 3_600;
/// Durée maximale réellement simulée lors d'un rattrapage hors-ligne (30 jours).
///
/// Au-delà, la simulation n'apporte plus rien (le familier est mort depuis
/// longtemps) et un horodatage corrompu ne doit jamais se traduire par des
/// centaines de millions d'itérations.
pub const MAX_CATCHUP_DURATION_SECS: u64 = 30 * 24 * 3_600;

/// Borne supérieure défensive appliquée aux taux de décroissance, par minute.
const MAX_DECAY_PER_MINUTE: f32 = 100.0;
/// Borne supérieure défensive appliquée aux montants d'action.
const MAX_ACTION_AMOUNT: f32 = 1_000.0;
/// Borne supérieure défensive appliquée à la durée de l'état « Coding ».
const MAX_CODING_DURATION_SECS: f32 = 86_400.0;
/// Borne supérieure défensive d'une récompense unitaire.
const MAX_REWARD_XP: u64 = 1_000_000;
/// Borne supérieure des cooldowns et seuils de focus (24 heures).
const MAX_SESSION_DURATION_SECS: u64 = 86_400;
/// Palier de focus minimal configurable.
const MIN_FOCUS_MILESTONE_SECS: u64 = 60;

/// Ramène une valeur flottante dans `[min, max]` en neutralisant `NaN`.
fn sanitize_f32(value: f32, min: f32, max: f32, fallback: f32) -> f32 {
    if value.is_nan() {
        fallback
    } else {
        value.clamp(min, max)
    }
}

/// Vérifie qu'une valeur flottante est finie et comprise dans `[min, max]`.
fn check_f32(name: &str, value: f32, min: f32, max: f32) -> Result<(), CoreError> {
    if value.is_finite() && (min..=max).contains(&value) {
        Ok(())
    } else {
        Err(CoreError::ConfigurationError(format!(
            "{name} = {value} hors des bornes [{min}, {max}]"
        )))
    }
}

fn check_u64(name: &str, value: u64, min: u64, max: u64) -> Result<(), CoreError> {
    if (min..=max).contains(&value) {
        Ok(())
    } else {
        Err(CoreError::ConfigurationError(format!(
            "{name} = {value} hors des bornes [{min}, {max}]"
        )))
    }
}

/// Taux de décroissance temporelle des jauges vitales.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DecayConfig {
    /// Décroissance d'énergie par minute éveillée (défaut : 0.5).
    pub energy_decay_per_minute: f32,
    /// Décroissance de satiété par minute éveillée (défaut : 1.0).
    pub satiety_decay_per_minute: f32,
    /// Décroissance de bonheur par minute éveillée (défaut : 0.8).
    pub happiness_decay_per_minute: f32,
    /// Multiplicateur de décroissance lorsque le familier est endormi (défaut : 0.1).
    pub sleep_decay_multiplier: f32,
}

impl Default for DecayConfig {
    fn default() -> Self {
        Self {
            energy_decay_per_minute: 0.5,
            satiety_decay_per_minute: 1.0,
            happiness_decay_per_minute: 0.8,
            sleep_decay_multiplier: 0.1,
        }
    }
}

impl DecayConfig {
    /// Corrige silencieusement les valeurs aberrantes (négatives, infinies, `NaN`).
    ///
    /// Un taux de décroissance négatif ferait *croître* les jauges avec le temps :
    /// c'est le vecteur d'abus principal d'une sauvegarde modifiée à la main.
    pub fn normalize(&mut self) {
        let defaults = Self::default();
        self.energy_decay_per_minute = sanitize_f32(
            self.energy_decay_per_minute,
            0.0,
            MAX_DECAY_PER_MINUTE,
            defaults.energy_decay_per_minute,
        );
        self.satiety_decay_per_minute = sanitize_f32(
            self.satiety_decay_per_minute,
            0.0,
            MAX_DECAY_PER_MINUTE,
            defaults.satiety_decay_per_minute,
        );
        self.happiness_decay_per_minute = sanitize_f32(
            self.happiness_decay_per_minute,
            0.0,
            MAX_DECAY_PER_MINUTE,
            defaults.happiness_decay_per_minute,
        );
        self.sleep_decay_multiplier = sanitize_f32(
            self.sleep_decay_multiplier,
            0.0,
            1.0,
            defaults.sleep_decay_multiplier,
        );
    }

    /// Vérifie la cohérence de la configuration sans la modifier.
    ///
    /// # Errors
    /// Renvoie `CoreError::ConfigurationError` si un taux est négatif, non fini
    /// ou hors des bornes admissibles.
    pub fn validate(&self) -> Result<(), CoreError> {
        check_f32(
            "energy_decay_per_minute",
            self.energy_decay_per_minute,
            0.0,
            MAX_DECAY_PER_MINUTE,
        )?;
        check_f32(
            "satiety_decay_per_minute",
            self.satiety_decay_per_minute,
            0.0,
            MAX_DECAY_PER_MINUTE,
        )?;
        check_f32(
            "happiness_decay_per_minute",
            self.happiness_decay_per_minute,
            0.0,
            MAX_DECAY_PER_MINUTE,
        )?;
        check_f32(
            "sleep_decay_multiplier",
            self.sleep_decay_multiplier,
            0.0,
            1.0,
        )
    }
}

/// Paramètres de récompenses et d'effets des actions utilisateur.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ActionConfig {
    /// Gain de satiété par repas standard.
    pub default_feed_amount: f32,
    /// Gain de santé/vitalité lors d'un soin.
    pub default_heal_amount: f32,
    /// Gain de bonheur lors d'une caresse/interaction.
    pub default_pet_happiness: f32,
    /// Gain d'énergie lors d'un repos instantané.
    pub default_rest_energy: f32,
    /// Fraction du montant de soin répartie sur chacune des trois jauges.
    pub heal_split_ratio: f32,
    /// Boost d'énergie conféré par un commit Git.
    pub commit_energy_boost: f32,
    /// Boost de bonheur conféré par un commit Git.
    pub commit_happiness_boost: f32,
    /// Montant d'XP standard attribué par commit.
    pub commit_xp_reward: u64,
    /// Durée pendant laquelle le Gremlin reste dans l'état « Coding » après un commit, en secondes.
    pub coding_duration_secs: f32,
}

impl Default for ActionConfig {
    fn default() -> Self {
        Self {
            default_feed_amount: 30.0,
            default_heal_amount: 40.0,
            default_pet_happiness: 15.0,
            default_rest_energy: 25.0,
            heal_split_ratio: 0.5,
            commit_energy_boost: 15.0,
            commit_happiness_boost: 20.0,
            commit_xp_reward: 50,
            coding_duration_secs: 60.0,
        }
    }
}

impl ActionConfig {
    /// Corrige silencieusement les valeurs aberrantes.
    pub fn normalize(&mut self) {
        let defaults = Self::default();
        self.default_feed_amount = sanitize_f32(
            self.default_feed_amount,
            0.0,
            MAX_ACTION_AMOUNT,
            defaults.default_feed_amount,
        );
        self.default_heal_amount = sanitize_f32(
            self.default_heal_amount,
            0.0,
            MAX_ACTION_AMOUNT,
            defaults.default_heal_amount,
        );
        self.default_pet_happiness = sanitize_f32(
            self.default_pet_happiness,
            0.0,
            MAX_ACTION_AMOUNT,
            defaults.default_pet_happiness,
        );
        self.default_rest_energy = sanitize_f32(
            self.default_rest_energy,
            0.0,
            MAX_ACTION_AMOUNT,
            defaults.default_rest_energy,
        );
        self.heal_split_ratio =
            sanitize_f32(self.heal_split_ratio, 0.0, 1.0, defaults.heal_split_ratio);
        self.commit_energy_boost = sanitize_f32(
            self.commit_energy_boost,
            0.0,
            MAX_ACTION_AMOUNT,
            defaults.commit_energy_boost,
        );
        self.commit_happiness_boost = sanitize_f32(
            self.commit_happiness_boost,
            0.0,
            MAX_ACTION_AMOUNT,
            defaults.commit_happiness_boost,
        );
        self.commit_xp_reward = self.commit_xp_reward.min(MAX_REWARD_XP);
        self.coding_duration_secs = sanitize_f32(
            self.coding_duration_secs,
            0.0,
            MAX_CODING_DURATION_SECS,
            defaults.coding_duration_secs,
        );
    }

    /// Vérifie la cohérence de la configuration sans la modifier.
    ///
    /// # Errors
    /// Renvoie `CoreError::ConfigurationError` si un montant est négatif, non
    /// fini ou hors des bornes admissibles.
    pub fn validate(&self) -> Result<(), CoreError> {
        check_f32(
            "default_feed_amount",
            self.default_feed_amount,
            0.0,
            MAX_ACTION_AMOUNT,
        )?;
        check_u64("commit_xp_reward", self.commit_xp_reward, 0, MAX_REWARD_XP)?;
        check_f32(
            "default_heal_amount",
            self.default_heal_amount,
            0.0,
            MAX_ACTION_AMOUNT,
        )?;
        check_f32(
            "default_pet_happiness",
            self.default_pet_happiness,
            0.0,
            MAX_ACTION_AMOUNT,
        )?;
        check_f32(
            "default_rest_energy",
            self.default_rest_energy,
            0.0,
            MAX_ACTION_AMOUNT,
        )?;
        check_f32("heal_split_ratio", self.heal_split_ratio, 0.0, 1.0)?;
        check_f32(
            "commit_energy_boost",
            self.commit_energy_boost,
            0.0,
            MAX_ACTION_AMOUNT,
        )?;
        check_f32(
            "commit_happiness_boost",
            self.commit_happiness_boost,
            0.0,
            MAX_ACTION_AMOUNT,
        )?;
        check_f32(
            "coding_duration_secs",
            self.coding_duration_secs,
            0.0,
            MAX_CODING_DURATION_SECS,
        )
    }
}

/// Paramètres de récompense et d'anti-spam des rapports d'outillage.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ToolingRewardsConfig {
    pub test_pass_xp: u64,
    pub test_fix_bonus_xp: u64,
    pub build_success_xp: u64,
    pub test_pass_happiness_boost: f32,
    pub test_fail_happiness_penalty: f32,
    pub reward_cooldown_secs: u64,
    pub feedback_cooldown_secs: u64,
}

impl Default for ToolingRewardsConfig {
    fn default() -> Self {
        Self {
            test_pass_xp: 25,
            test_fix_bonus_xp: 50,
            build_success_xp: 15,
            test_pass_happiness_boost: 6.0,
            test_fail_happiness_penalty: 2.0,
            reward_cooldown_secs: 30,
            feedback_cooldown_secs: 10,
        }
    }
}

impl ToolingRewardsConfig {
    pub fn normalize(&mut self) {
        let defaults = Self::default();
        self.test_pass_xp = self.test_pass_xp.min(MAX_REWARD_XP);
        self.test_fix_bonus_xp = self.test_fix_bonus_xp.min(MAX_REWARD_XP);
        self.build_success_xp = self.build_success_xp.min(MAX_REWARD_XP);
        self.test_pass_happiness_boost = sanitize_f32(
            self.test_pass_happiness_boost,
            0.0,
            MAX_ACTION_AMOUNT,
            defaults.test_pass_happiness_boost,
        );
        self.test_fail_happiness_penalty = sanitize_f32(
            self.test_fail_happiness_penalty,
            0.0,
            MAX_ACTION_AMOUNT,
            defaults.test_fail_happiness_penalty,
        );
        self.reward_cooldown_secs = self
            .reward_cooldown_secs
            .clamp(1, MAX_SESSION_DURATION_SECS);
        self.feedback_cooldown_secs = self
            .feedback_cooldown_secs
            .clamp(1, MAX_SESSION_DURATION_SECS);
    }

    /// Vérifie que les récompenses et cooldowns respectent leurs bornes.
    ///
    /// # Errors
    ///
    /// Renvoie [`CoreError::ConfigurationError`] à la première valeur invalide.
    pub fn validate(&self) -> Result<(), CoreError> {
        check_u64("test_pass_xp", self.test_pass_xp, 0, MAX_REWARD_XP)?;
        check_u64(
            "test_fix_bonus_xp",
            self.test_fix_bonus_xp,
            0,
            MAX_REWARD_XP,
        )?;
        check_u64("build_success_xp", self.build_success_xp, 0, MAX_REWARD_XP)?;
        check_f32(
            "test_pass_happiness_boost",
            self.test_pass_happiness_boost,
            0.0,
            MAX_ACTION_AMOUNT,
        )?;
        check_f32(
            "test_fail_happiness_penalty",
            self.test_fail_happiness_penalty,
            0.0,
            MAX_ACTION_AMOUNT,
        )?;
        check_u64(
            "reward_cooldown_secs",
            self.reward_cooldown_secs,
            1,
            MAX_SESSION_DURATION_SECS,
        )?;
        check_u64(
            "feedback_cooldown_secs",
            self.feedback_cooldown_secs,
            1,
            MAX_SESSION_DURATION_SECS,
        )
    }
}

/// Paramètres de l'estimation de focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct FocusConfig {
    pub milestone_secs: [u64; 3],
    pub milestone_xp: [u64; 3],
    pub break_reminder_secs: u64,
    pub idle_reset_threshold_secs: u64,
    pub max_sample_secs: u64,
}

impl Default for FocusConfig {
    fn default() -> Self {
        Self {
            milestone_secs: [25 * 60, 50 * 60, 90 * 60],
            milestone_xp: [20, 35, 60],
            break_reminder_secs: 60 * 60,
            idle_reset_threshold_secs: 10 * 60,
            max_sample_secs: 5,
        }
    }
}

impl FocusConfig {
    pub fn normalize(&mut self) {
        let mut milestones = [
            (self.milestone_secs[0], self.milestone_xp[0]),
            (self.milestone_secs[1], self.milestone_xp[1]),
            (self.milestone_secs[2], self.milestone_xp[2]),
        ];
        milestones.sort_unstable_by_key(|(duration, _)| *duration);
        for (index, (duration, xp)) in milestones.into_iter().enumerate() {
            let reserved_upper_bound =
                MAX_SESSION_DURATION_SECS.saturating_sub(2_usize.saturating_sub(index) as u64);
            self.milestone_secs[index] =
                duration.clamp(MIN_FOCUS_MILESTONE_SECS, reserved_upper_bound);
            self.milestone_xp[index] = xp.min(MAX_REWARD_XP);
        }
        for index in 1..self.milestone_secs.len() {
            self.milestone_secs[index] = self.milestone_secs[index]
                .max(self.milestone_secs[index - 1].saturating_add(1))
                .min(MAX_SESSION_DURATION_SECS);
        }
        self.break_reminder_secs = self
            .break_reminder_secs
            .clamp(self.milestone_secs[0], MAX_SESSION_DURATION_SECS);
        self.idle_reset_threshold_secs = self
            .idle_reset_threshold_secs
            .clamp(1, MAX_SESSION_DURATION_SECS);
        self.max_sample_secs = self.max_sample_secs.clamp(1, 60);
    }

    /// Vérifie l'ordre et les bornes de la configuration de focus.
    ///
    /// # Errors
    ///
    /// Renvoie [`CoreError::ConfigurationError`] si un palier ou une durée est invalide.
    pub fn validate(&self) -> Result<(), CoreError> {
        for (index, value) in self.milestone_secs.into_iter().enumerate() {
            check_u64(
                &format!("milestone_secs[{index}]"),
                value,
                MIN_FOCUS_MILESTONE_SECS,
                MAX_SESSION_DURATION_SECS,
            )?;
            check_u64(
                &format!("milestone_xp[{index}]"),
                self.milestone_xp[index],
                0,
                MAX_REWARD_XP,
            )?;
            if index > 0 && value <= self.milestone_secs[index - 1] {
                return Err(CoreError::ConfigurationError(String::from(
                    "les paliers de focus doivent être strictement croissants",
                )));
            }
        }
        check_u64(
            "break_reminder_secs",
            self.break_reminder_secs,
            self.milestone_secs[0],
            MAX_SESSION_DURATION_SECS,
        )?;
        check_u64(
            "idle_reset_threshold_secs",
            self.idle_reset_threshold_secs,
            1,
            MAX_SESSION_DURATION_SECS,
        )?;
        check_u64("max_sample_secs", self.max_sample_secs, 1, 60)
    }

    #[must_use]
    pub fn milestone_durations(self) -> [std::time::Duration; 3] {
        self.milestone_secs.map(std::time::Duration::from_secs)
    }

    #[must_use]
    pub const fn milestone_rewards(self) -> [u64; 3] {
        self.milestone_xp
    }

    #[must_use]
    pub const fn break_reminder_duration(self) -> std::time::Duration {
        std::time::Duration::from_secs(self.break_reminder_secs)
    }

    #[must_use]
    pub const fn idle_reset_threshold(self) -> std::time::Duration {
        std::time::Duration::from_secs(self.idle_reset_threshold_secs)
    }

    #[must_use]
    pub const fn max_sample_duration(self) -> std::time::Duration {
        std::time::Duration::from_secs(self.max_sample_secs)
    }
}

/// Seuils de bascule de la machine à états émotionnels.
///
/// Ces valeurs sont l'unique source de vérité : les prédicats de
/// [`PetStats`](crate::stats::PetStats) et l'évaluation d'humeur
/// [`PetMood::evaluate`](crate::mood::PetMood::evaluate) les consomment tous les deux.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MoodConfig {
    /// En dessous de cette énergie, le familier est épuisé donc malade.
    pub sick_energy: f32,
    /// En dessous de cette satiété, le familier est affamé donc malade.
    pub sick_satiety: f32,
    /// Bonheur en dessous duquel la colère peut se déclencher.
    pub angry_happiness: f32,
    /// Satiété en dessous de laquelle la colère peut se déclencher.
    pub angry_satiety: f32,
    /// Satiété en dessous de laquelle le familier a faim.
    pub hungry_satiety: f32,
    /// Énergie en dessous de laquelle le familier est fatigué.
    pub tired_energy: f32,
    /// Seuil en dessous duquel une jauge est considérée critique.
    pub critical_gauge: f32,
    /// Marge d'hystérésis appliquée pour sortir d'une humeur négative.
    ///
    /// Sans elle, une jauge oscillant autour d'un seuil émettrait un
    /// `MoodChanged` à chaque tick.
    pub hysteresis_margin: f32,
}

impl Default for MoodConfig {
    fn default() -> Self {
        Self {
            sick_energy: 15.0,
            sick_satiety: 20.0,
            angry_happiness: 20.0,
            angry_satiety: 30.0,
            hungry_satiety: 40.0,
            tired_energy: 25.0,
            critical_gauge: 15.0,
            hysteresis_margin: 3.0,
        }
    }
}

impl MoodConfig {
    /// Corrige silencieusement les seuils aberrants.
    pub fn normalize(&mut self) {
        let defaults = Self::default();
        self.sick_energy = sanitize_f32(self.sick_energy, 0.0, 100.0, defaults.sick_energy);
        self.sick_satiety = sanitize_f32(self.sick_satiety, 0.0, 100.0, defaults.sick_satiety);
        self.angry_happiness =
            sanitize_f32(self.angry_happiness, 0.0, 100.0, defaults.angry_happiness);
        self.angry_satiety = sanitize_f32(self.angry_satiety, 0.0, 100.0, defaults.angry_satiety);
        self.hungry_satiety =
            sanitize_f32(self.hungry_satiety, 0.0, 100.0, defaults.hungry_satiety);
        self.tired_energy = sanitize_f32(self.tired_energy, 0.0, 100.0, defaults.tired_energy);
        self.critical_gauge =
            sanitize_f32(self.critical_gauge, 0.0, 100.0, defaults.critical_gauge);
        self.hysteresis_margin = sanitize_f32(
            self.hysteresis_margin,
            0.0,
            50.0,
            defaults.hysteresis_margin,
        );
    }

    /// Vérifie la cohérence des seuils sans les modifier.
    ///
    /// # Errors
    /// Renvoie `CoreError::ConfigurationError` si un seuil est hors de `[0, 100]`
    /// ou si la marge d'hystérésis est hors de `[0, 50]`.
    pub fn validate(&self) -> Result<(), CoreError> {
        check_f32("sick_energy", self.sick_energy, 0.0, 100.0)?;
        check_f32("sick_satiety", self.sick_satiety, 0.0, 100.0)?;
        check_f32("angry_happiness", self.angry_happiness, 0.0, 100.0)?;
        check_f32("angry_satiety", self.angry_satiety, 0.0, 100.0)?;
        check_f32("hungry_satiety", self.hungry_satiety, 0.0, 100.0)?;
        check_f32("tired_energy", self.tired_energy, 0.0, 100.0)?;
        check_f32("critical_gauge", self.critical_gauge, 0.0, 100.0)?;
        check_f32("hysteresis_margin", self.hysteresis_margin, 0.0, 50.0)
    }
}

/// Configuration globale du moteur de jeu Gremlin.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CoreConfig {
    /// Paramètres de décroissance des jauges.
    pub decay: DecayConfig,
    /// Paramètres des actions et récompenses.
    pub actions: ActionConfig,
    /// Seuils de la machine à états émotionnels.
    pub mood: MoodConfig,
    /// Récompenses et protections anti-spam des rapports d'outillage.
    pub tooling: ToolingRewardsConfig,
    /// Seuils de l'estimation de focus.
    pub focus: FocusConfig,
    /// Intervalle maximal en secondes d'un pas de simulation pour le rattrapage hors-ligne.
    pub catchup_step_secs: u64,
}

impl Default for CoreConfig {
    /// Identique à [`CoreConfig::new`].
    ///
    /// L'implémentation est manuelle : la version dérivée produisait
    /// `catchup_step_secs = 0`, soit un rattrapage hors-ligne simulé seconde par
    /// seconde (60 fois plus d'itérations que documenté).
    fn default() -> Self {
        Self::new()
    }
}

impl CoreConfig {
    /// Crée une configuration par défaut avec un pas de simulation hors-ligne de 60 secondes.
    #[must_use]
    pub fn new() -> Self {
        Self {
            decay: DecayConfig::default(),
            actions: ActionConfig::default(),
            mood: MoodConfig::default(),
            tooling: ToolingRewardsConfig::default(),
            focus: FocusConfig::default(),
            catchup_step_secs: DEFAULT_CATCHUP_STEP_SECS,
        }
    }

    /// Pas de simulation effectif, toujours compris dans les bornes admissibles.
    #[must_use]
    pub const fn effective_catchup_step_secs(&self) -> u64 {
        if self.catchup_step_secs < MIN_CATCHUP_STEP_SECS {
            MIN_CATCHUP_STEP_SECS
        } else if self.catchup_step_secs > MAX_CATCHUP_STEP_SECS {
            MAX_CATCHUP_STEP_SECS
        } else {
            self.catchup_step_secs
        }
    }

    /// Corrige silencieusement l'ensemble de la configuration.
    ///
    /// Appelé systématiquement après désérialisation d'une sauvegarde.
    pub fn normalize(&mut self) {
        self.decay.normalize();
        self.actions.normalize();
        self.mood.normalize();
        self.tooling.normalize();
        self.focus.normalize();
        self.catchup_step_secs = self
            .catchup_step_secs
            .clamp(MIN_CATCHUP_STEP_SECS, MAX_CATCHUP_STEP_SECS);
    }

    /// Vérifie la cohérence complète de la configuration sans la modifier.
    ///
    /// # Errors
    /// Renvoie `CoreError::ConfigurationError` décrivant le premier paramètre invalide.
    pub fn validate(&self) -> Result<(), CoreError> {
        self.decay.validate()?;
        self.actions.validate()?;
        self.mood.validate()?;
        self.tooling.validate()?;
        self.focus.validate()?;
        if !(MIN_CATCHUP_STEP_SECS..=MAX_CATCHUP_STEP_SECS).contains(&self.catchup_step_secs) {
            return Err(CoreError::ConfigurationError(format!(
                "catchup_step_secs = {} hors des bornes [{MIN_CATCHUP_STEP_SECS}, {MAX_CATCHUP_STEP_SECS}]",
                self.catchup_step_secs
            )));
        }
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::float_cmp, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn test_default_matches_new() {
        assert_eq!(CoreConfig::default(), CoreConfig::new());
        assert_eq!(
            CoreConfig::default().catchup_step_secs,
            DEFAULT_CATCHUP_STEP_SECS
        );
    }

    #[test]
    fn test_default_config_is_valid() {
        assert!(CoreConfig::default().validate().is_ok());
    }

    #[test]
    fn test_effective_step_is_always_bounded() {
        let mut config = CoreConfig::new();

        config.catchup_step_secs = 0;
        assert_eq!(config.effective_catchup_step_secs(), MIN_CATCHUP_STEP_SECS);

        config.catchup_step_secs = u64::MAX;
        assert_eq!(config.effective_catchup_step_secs(), MAX_CATCHUP_STEP_SECS);

        config.catchup_step_secs = 42;
        assert_eq!(config.effective_catchup_step_secs(), 42);
    }

    #[test]
    fn test_normalize_repairs_hostile_values() {
        let mut config = CoreConfig::new();
        config.decay.energy_decay_per_minute = -5.0;
        config.decay.satiety_decay_per_minute = f32::NAN;
        config.decay.sleep_decay_multiplier = 900.0;
        config.actions.heal_split_ratio = f32::INFINITY;
        config.tooling.test_pass_happiness_boost = f32::NAN;
        config.tooling.reward_cooldown_secs = 0;
        config.focus.milestone_secs = [u64::MAX, 0, 60];
        config.focus.milestone_xp = [u64::MAX; 3];
        config.focus.max_sample_secs = 0;
        config.mood.hungry_satiety = -1.0;
        config.catchup_step_secs = u64::MAX;

        assert!(config.validate().is_err());
        config.normalize();
        assert!(config.validate().is_ok());

        assert_eq!(config.decay.energy_decay_per_minute, 0.0);
        assert_eq!(config.decay.satiety_decay_per_minute, 1.0);
        assert_eq!(config.decay.sleep_decay_multiplier, 1.0);
        assert_eq!(config.actions.heal_split_ratio, 1.0);
        assert_eq!(
            config.tooling.test_pass_happiness_boost,
            ToolingRewardsConfig::default().test_pass_happiness_boost
        );
        assert_eq!(config.tooling.reward_cooldown_secs, 1);
        assert!(config
            .focus
            .milestone_secs
            .windows(2)
            .all(|pair| pair[0] < pair[1]));
        assert_eq!(config.mood.hungry_satiety, 0.0);
        assert_eq!(config.catchup_step_secs, MAX_CATCHUP_STEP_SECS);
    }

    #[test]
    fn test_normalize_is_idempotent() {
        let mut config = CoreConfig::new();
        config.decay.happiness_decay_per_minute = f32::NEG_INFINITY;
        config.normalize();
        let once = config;
        config.normalize();
        assert_eq!(once, config);
    }

    #[test]
    fn test_partial_json_falls_back_on_defaults() {
        // Une sauvegarde antérieure à l'ajout de `mood` et `heal_split_ratio`
        // doit rester lisible.
        let json = r#"{"catchup_step_secs": 30}"#;
        let config: CoreConfig =
            serde_json::from_str(json).expect("la désérialisation partielle doit réussir");

        assert_eq!(config.catchup_step_secs, 30);
        assert_eq!(config.mood, MoodConfig::default());
        assert_eq!(config.actions.heal_split_ratio, 0.5);
        assert_eq!(config.tooling, ToolingRewardsConfig::default());
        assert_eq!(config.focus, FocusConfig::default());
    }
}
