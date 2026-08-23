//! État global encapsulé et gestionnaire du cycle de vie du Gremlin.
//!
//! `PetState` est l'agrégat racine du domaine : ses champs sont privés et
//! toutes les mutations passent par ses méthodes, ce qui garantit deux
//! invariants impossibles à tenir avec des champs publics — les jauges restent
//! bornées et finies, et `mood` reste toujours cohérente avec `stats`.

use crate::action::ActionKind;
use crate::config::{CoreConfig, MAX_CATCHUP_DURATION_SECS};
use crate::error::CoreError;
use crate::events::CoreEvent;
use crate::mood::PetMood;
use crate::progression::PetProgression;
use crate::stats::PetStats;
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Version courante du format de sauvegarde.
///
/// Incrémenter cette constante à chaque changement incompatible de la
/// structure ; une sauvegarde annonçant une version supérieure est refusée
/// plutôt que réinterprétée de travers.
pub const SAVE_FORMAT_VERSION: u32 = 1;

/// Nom attribué à un familier dont le nom est absent ou vide.
const DEFAULT_NAME: &str = "Gremlin";
/// Longueur maximale du nom, en caractères (et non en octets).
const MAX_NAME_CHARS: usize = 48;

/// État complet et persistant du compagnon virtuel.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PetState {
    version: u32,
    name: String,
    stats: PetStats,
    mood: PetMood,
    progression: PetProgression,
    config: CoreConfig,
    is_sleeping: bool,
    coding_timer_secs: f32,
}

impl Default for PetState {
    fn default() -> Self {
        Self {
            version: SAVE_FORMAT_VERSION,
            name: String::from(DEFAULT_NAME),
            stats: PetStats::default(),
            mood: PetMood::Happy,
            progression: PetProgression::default(),
            config: CoreConfig::default(),
            is_sleeping: false,
            coding_timer_secs: 0.0,
        }
    }
}

impl PetState {
    /// Crée un nouveau Gremlin avec un nom donné et la configuration par défaut.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        let mut state = Self {
            name: name.into(),
            ..Default::default()
        };
        state.normalize();
        state
    }

    /// Crée un nouveau Gremlin avec un nom et une configuration personnalisée.
    #[must_use]
    pub fn with_config(name: impl Into<String>, config: CoreConfig) -> Self {
        let mut state = Self {
            name: name.into(),
            config,
            ..Default::default()
        };
        state.normalize();
        state
    }

    /// Version du format de sauvegarde de cet état.
    #[must_use]
    pub const fn version(&self) -> u32 {
        self.version
    }

    /// Nom du Gremlin.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Statistiques vitales actuelles.
    #[must_use]
    pub const fn stats(&self) -> PetStats {
        self.stats
    }

    /// Humeur courante, toujours cohérente avec les statistiques.
    #[must_use]
    pub const fn mood(&self) -> PetMood {
        self.mood
    }

    /// Progression et expérience.
    #[must_use]
    pub const fn progression(&self) -> PetProgression {
        self.progression
    }

    /// Configuration d'équilibrage du moteur.
    #[must_use]
    pub const fn config(&self) -> CoreConfig {
        self.config
    }

    /// Indique si le familier est en sommeil volontaire.
    #[must_use]
    pub const fn is_sleeping(&self) -> bool {
        self.is_sleeping
    }

    /// Temps restant, en secondes, pendant lequel le familier est considéré en activité de code.
    #[must_use]
    pub const fn coding_timer_secs(&self) -> f32 {
        self.coding_timer_secs
    }

    /// Renomme le familier ; le nom est normalisé (rogné et tronqué).
    pub fn set_name(&mut self, name: impl Into<String>) {
        self.name = name.into();
        self.normalize_name();
    }

    /// Remplace la configuration d'équilibrage, puis restaure les invariants.
    pub fn set_config(&mut self, config: CoreConfig) {
        self.config = config;
        self.normalize();
    }

    /// Force les jauges vitales, puis réévalue l'humeur.
    ///
    /// Réservé aux outils de diagnostic, aux scénarios de test et au mode debug :
    /// le déroulement normal du jeu passe par les actions et par `tick`.
    pub fn set_stats(&mut self, stats: PetStats) {
        self.stats = stats;
        self.stats.normalize();
        self.refresh_mood();
    }

    /// Indique si le Gremlin est en vie.
    #[must_use]
    pub const fn is_alive(&self) -> bool {
        self.mood.is_alive()
    }

    /// Exécute un pas de temps (`tick`) et met à jour humeurs, statistiques et timers.
    ///
    /// Pour les sauts de temps importants (rattrapage hors-ligne), la simulation
    /// est découpée en tranches de `config.catchup_step_secs`. La durée est
    /// plafonnée à [`MAX_CATCHUP_DURATION_SECS`] et le découpage se fait en
    /// arithmétique entière : un horodatage corrompu ne peut donc ni provoquer
    /// une boucle interminable, ni faire paniquer la conversion de durée.
    pub fn tick(&mut self, elapsed: Duration) -> Vec<CoreEvent> {
        let mut events = Vec::new();

        if self.mood == PetMood::Dead {
            return events;
        }

        let capped = Self::cap_elapsed(elapsed);
        if capped.is_zero() {
            return events;
        }

        let step_secs = self.config.effective_catchup_step_secs();
        let total_secs = capped.as_secs();
        let full_steps = total_secs / step_secs;
        let step_duration = Duration::from_secs(step_secs);
        let remainder = Duration::new(total_secs % step_secs, capped.subsec_nanos());

        for _ in 0..full_steps {
            if self.simulate_step(step_duration, &mut events) {
                return events;
            }
        }

        if !remainder.is_zero() && self.simulate_step(remainder, &mut events) {
            return events;
        }

        events.push(CoreEvent::StatsDecayed { stats: self.stats });
        events
    }

    /// Réagit à l'arrivée d'un commit Git détecté par le watcher.
    ///
    /// # Errors
    /// Renvoie `CoreError::PetIsDead` si le familier est décédé — un commit
    /// ignoré silencieusement empêcherait l'appelant de distinguer « rien à
    /// faire » de « il faut d'abord réanimer le familier ».
    pub fn handle_commit(&mut self, repo: &str, branch: &str) -> Result<Vec<CoreEvent>, CoreError> {
        if self.mood == PetMood::Dead {
            return Err(CoreError::PetIsDead(ActionKind::Commit));
        }

        let mut events = Vec::new();

        // Un commit réveille le familier endormi.
        if self.is_sleeping {
            self.is_sleeping = false;
            events.push(CoreEvent::WokeUp);
        }

        self.stats.boost_from_commit(
            self.config.actions.commit_energy_boost,
            self.config.actions.commit_happiness_boost,
        );
        self.coding_timer_secs = self.config.actions.coding_duration_secs;

        let (levels_gained, evolution) = self
            .progression
            .record_commit(self.config.actions.commit_xp_reward);

        events.push(CoreEvent::CommitReceived {
            repo: repo.to_string(),
            branch: branch.to_string(),
            xp_gained: self.config.actions.commit_xp_reward,
        });

        if levels_gained > 0 {
            events.push(CoreEvent::LevelUp {
                new_level: self.progression.level(),
                total_xp: self.progression.total_xp(),
            });
        }

        if let Some(new_stage) = evolution {
            events.push(CoreEvent::EvolutionUnlocked { new_stage });
        }

        self.reevaluate_mood(&mut events);
        Ok(events)
    }

    /// Nourrit le Gremlin.
    ///
    /// # Errors
    /// Renvoie `CoreError::PetIsDead` si le familier est décédé,
    /// `CoreError::InvalidActionForMood` s'il dort, et
    /// `CoreError::InvalidActionAmount` si le montant fourni est négatif,
    /// infini ou `NaN`.
    pub fn feed(&mut self, amount: Option<f32>) -> Result<Vec<CoreEvent>, CoreError> {
        self.ensure_actionable(ActionKind::Feed)?;
        let feed_val = Self::validated_amount(
            ActionKind::Feed,
            amount,
            self.config.actions.default_feed_amount,
        )?;

        self.stats.feed(feed_val);
        let mut events = vec![CoreEvent::Fed { amount: feed_val }];
        self.reevaluate_mood(&mut events);
        Ok(events)
    }

    /// Caresse ou interagit avec le Gremlin pour augmenter son bonheur.
    ///
    /// # Errors
    /// Voir [`PetState::feed`] : mêmes conditions de refus.
    pub fn pet(&mut self, amount: Option<f32>) -> Result<Vec<CoreEvent>, CoreError> {
        self.ensure_actionable(ActionKind::Pet)?;
        let pet_val = Self::validated_amount(
            ActionKind::Pet,
            amount,
            self.config.actions.default_pet_happiness,
        )?;

        self.stats.pet(pet_val);
        let mut events = vec![CoreEvent::Petted { amount: pet_val }];
        self.reevaluate_mood(&mut events);
        Ok(events)
    }

    /// Soigne le Gremlin en cas de maladie.
    ///
    /// # Errors
    /// Voir [`PetState::feed`] : mêmes conditions de refus. Le soin est bloqué
    /// pendant le sommeil, conformément au contrat de
    /// [`PetMood::can_interact`].
    pub fn heal(&mut self, amount: Option<f32>) -> Result<Vec<CoreEvent>, CoreError> {
        self.ensure_actionable(ActionKind::Heal)?;
        let heal_val = Self::validated_amount(
            ActionKind::Heal,
            amount,
            self.config.actions.default_heal_amount,
        )?;

        self.stats
            .heal(heal_val, self.config.actions.heal_split_ratio);
        let mut events = vec![CoreEvent::Healed { amount: heal_val }];
        self.reevaluate_mood(&mut events);
        Ok(events)
    }

    /// Repose instantanément le Gremlin pour restaurer de l'énergie.
    ///
    /// # Errors
    /// Voir [`PetState::feed`] : mêmes conditions de refus.
    pub fn rest(&mut self, amount: Option<f32>) -> Result<Vec<CoreEvent>, CoreError> {
        self.ensure_actionable(ActionKind::Rest)?;
        let rest_val = Self::validated_amount(
            ActionKind::Rest,
            amount,
            self.config.actions.default_rest_energy,
        )?;

        self.stats.rest(rest_val);
        let mut events = vec![CoreEvent::Rested { amount: rest_val }];
        self.reevaluate_mood(&mut events);
        Ok(events)
    }

    /// Bascule le Gremlin en mode sommeil.
    ///
    /// L'opération est idempotente : endormir un familier déjà endormi réussit
    /// sans émettre d'événement.
    ///
    /// # Errors
    /// Renvoie `CoreError::PetIsDead` si le familier est décédé.
    pub fn sleep(&mut self) -> Result<Vec<CoreEvent>, CoreError> {
        self.ensure_not_dead(ActionKind::Sleep)?;
        if self.is_sleeping {
            return Ok(Vec::new());
        }

        self.is_sleeping = true;
        let mut events = vec![CoreEvent::FellAsleep];
        self.reevaluate_mood(&mut events);
        Ok(events)
    }

    /// Réveille le Gremlin.
    ///
    /// L'opération est idempotente : réveiller un familier déjà éveillé réussit
    /// sans émettre d'événement.
    ///
    /// # Errors
    /// Renvoie `CoreError::PetIsDead` si le familier est décédé.
    pub fn wake_up(&mut self) -> Result<Vec<CoreEvent>, CoreError> {
        self.ensure_not_dead(ActionKind::WakeUp)?;
        if !self.is_sleeping {
            return Ok(Vec::new());
        }

        self.is_sleeping = false;
        let mut events = vec![CoreEvent::WokeUp];
        self.reevaluate_mood(&mut events);
        Ok(events)
    }

    /// Bascule l'état de sommeil du Gremlin.
    ///
    /// # Errors
    /// Renvoie `CoreError::PetIsDead` si le familier est décédé.
    pub fn toggle_sleep(&mut self) -> Result<Vec<CoreEvent>, CoreError> {
        if self.is_sleeping {
            self.wake_up()
        } else {
            self.sleep()
        }
    }

    /// Réanime un Gremlin décédé et réinitialise ses jauges.
    ///
    /// # Errors
    /// Renvoie `CoreError::InvalidActionForMood` si le familier est vivant :
    /// la réanimation d'un familier en bonne santé remettrait ses jauges à
    /// neuf, ce qui n'est jamais l'intention de l'appelant.
    pub fn revive(&mut self) -> Result<Vec<CoreEvent>, CoreError> {
        if self.mood != PetMood::Dead {
            return Err(CoreError::InvalidActionForMood {
                action: ActionKind::Revive,
                current_mood: self.mood,
            });
        }

        self.stats = PetStats::default();
        self.is_sleeping = false;
        self.coding_timer_secs = 0.0;
        self.mood = PetMood::Happy;

        Ok(vec![
            CoreEvent::Revived,
            CoreEvent::MoodChanged {
                from: PetMood::Dead,
                to: PetMood::Happy,
            },
        ])
    }

    /// Restaure tous les invariants de l'agrégat.
    ///
    /// Appelée après toute désérialisation : une sauvegarde éditée à la main
    /// peut annoncer des jauges hors bornes, un niveau incohérent avec l'XP,
    /// des taux de décroissance négatifs ou une humeur en désaccord avec les
    /// statistiques. Cette méthode est idempotente.
    pub fn normalize(&mut self) {
        self.version = SAVE_FORMAT_VERSION;
        self.normalize_name();
        self.config.normalize();
        self.stats.normalize();
        self.progression.normalize();

        self.coding_timer_secs = if self.coding_timer_secs.is_finite() {
            self.coding_timer_secs
                .clamp(0.0, self.config.actions.coding_duration_secs)
        } else {
            0.0
        };

        self.refresh_mood();
    }

    /// Sérialise l'état au format JSON indenté.
    ///
    /// # Errors
    /// Renvoie `CoreError::StateSerialization` en cas d'échec de sérialisation.
    pub fn to_json(&self) -> Result<String, CoreError> {
        serde_json::to_string_pretty(self).map_err(|e| CoreError::StateSerialization(e.to_string()))
    }

    /// Désérialise l'état à partir d'une chaîne JSON, puis restaure ses invariants.
    ///
    /// # Errors
    /// Renvoie `CoreError::StateSerialization` si le format JSON est corrompu,
    /// ou `CoreError::UnsupportedSaveVersion` si la sauvegarde provient d'une
    /// version plus récente du logiciel.
    pub fn from_json(json_str: &str) -> Result<Self, CoreError> {
        let mut state: Self = serde_json::from_str(json_str)
            .map_err(|e| CoreError::StateSerialization(e.to_string()))?;

        if state.version > SAVE_FORMAT_VERSION {
            return Err(CoreError::UnsupportedSaveVersion {
                found: state.version,
                supported: SAVE_FORMAT_VERSION,
            });
        }

        state.normalize();
        Ok(state)
    }

    /// Plafonne la durée écoulée réellement simulée.
    fn cap_elapsed(elapsed: Duration) -> Duration {
        if elapsed.as_secs() >= MAX_CATCHUP_DURATION_SECS {
            Duration::from_secs(MAX_CATCHUP_DURATION_SECS)
        } else {
            elapsed
        }
    }

    /// Valide un montant d'action fourni par l'appelant.
    fn validated_amount(
        action: ActionKind,
        amount: Option<f32>,
        default: f32,
    ) -> Result<f32, CoreError> {
        let value = amount.unwrap_or(default);
        if value.is_finite() && value >= 0.0 {
            Ok(value)
        } else {
            Err(CoreError::InvalidActionAmount { action, value })
        }
    }

    /// Refuse une action si le familier est décédé.
    fn ensure_not_dead(&self, action: ActionKind) -> Result<(), CoreError> {
        if self.mood == PetMood::Dead {
            Err(CoreError::PetIsDead(action))
        } else {
            Ok(())
        }
    }

    /// Refuse une action si le familier ne peut pas interagir (mort ou endormi).
    fn ensure_actionable(&self, action: ActionKind) -> Result<(), CoreError> {
        self.ensure_not_dead(action)?;
        if self.mood.can_interact() {
            Ok(())
        } else {
            Err(CoreError::InvalidActionForMood {
                action,
                current_mood: self.mood,
            })
        }
    }

    /// Rogne et tronque le nom sur une frontière de caractère.
    fn normalize_name(&mut self) {
        let trimmed = self.name.trim();
        if trimmed.is_empty() {
            self.name = String::from(DEFAULT_NAME);
        } else if trimmed.chars().count() > MAX_NAME_CHARS {
            self.name = trimmed.chars().take(MAX_NAME_CHARS).collect();
        } else if trimmed.len() != self.name.len() {
            self.name = trimmed.to_owned();
        }
    }

    /// Recalcule l'humeur à partir des statistiques courantes.
    fn refresh_mood(&mut self) {
        self.mood = PetMood::evaluate_from(
            self.mood,
            &self.stats,
            &self.config.mood,
            self.is_sleeping,
            self.coding_timer_secs > 0.0,
        );
    }

    /// Simule un pas de temps.
    ///
    /// Renvoie `true` si le familier est mort durant ce pas ; les événements
    /// terminaux ont alors déjà été empilés dans le bon ordre.
    fn simulate_step(&mut self, step: Duration, events: &mut Vec<CoreEvent>) -> bool {
        if self.coding_timer_secs > 0.0 {
            self.coding_timer_secs = (self.coding_timer_secs - step.as_secs_f32()).max(0.0);
        }

        self.stats
            .apply_decay_with_config(step, &self.config.decay, self.is_sleeping);

        let previous_mood = self.mood;
        self.refresh_mood();

        if self.mood == previous_mood {
            return false;
        }

        if self.mood == PetMood::Dead {
            // `StatsDecayed` précède volontairement `Died` : l'ordre inverse
            // exposait aux consommateurs des statistiques post-mortem après
            // l'événement de décès.
            events.push(CoreEvent::StatsDecayed { stats: self.stats });
            events.push(CoreEvent::MoodChanged {
                from: previous_mood,
                to: self.mood,
            });
            events.push(CoreEvent::Died);
            return true;
        }

        events.push(CoreEvent::MoodChanged {
            from: previous_mood,
            to: self.mood,
        });
        false
    }

    /// Réévalue l'humeur après une action et empile les événements induits.
    fn reevaluate_mood(&mut self, events: &mut Vec<CoreEvent>) {
        let previous_mood = self.mood;
        self.refresh_mood();

        if self.mood != previous_mood {
            events.push(CoreEvent::MoodChanged {
                from: previous_mood,
                to: self.mood,
            });

            if self.mood == PetMood::Dead {
                events.push(CoreEvent::Died);
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::float_cmp)]
mod tests {
    use super::*;

    #[test]
    fn test_handle_commit_and_level_up() {
        let mut pet = PetState::new("Gizmo");
        assert_eq!(pet.progression().level(), 1);
        assert_eq!(pet.mood(), PetMood::Happy);

        let events = pet.handle_commit("gremlin", "main").unwrap();
        assert_eq!(pet.mood(), PetMood::Coding);
        assert_eq!(pet.progression().total_commits(), 1);
        assert!(events
            .iter()
            .any(|e| matches!(e, CoreEvent::CommitReceived { .. })));
    }

    #[test]
    fn test_handle_commit_on_dead_pet_is_reported() {
        let mut pet = PetState::new("Gizmo");
        pet.set_stats(PetStats::new(0.0, 0.0, 0.0));
        assert_eq!(pet.mood(), PetMood::Dead);

        assert!(matches!(
            pet.handle_commit("repo", "main"),
            Err(CoreError::PetIsDead(ActionKind::Commit))
        ));
    }

    #[test]
    fn test_tick_decay_and_death() {
        let mut pet = PetState::new("Gizmo");
        pet.set_stats(PetStats::new(0.01, 0.01, 0.01));

        let events = pet.tick(Duration::from_secs(3600));
        assert_eq!(pet.mood(), PetMood::Dead);
        assert!(events.contains(&CoreEvent::Died));
        assert!(!pet.is_alive());
    }

    #[test]
    fn test_stats_decayed_precedes_death_event() {
        let mut pet = PetState::new("Gizmo");
        pet.set_stats(PetStats::new(0.01, 0.01, 0.01));
        let events = pet.tick(Duration::from_secs(3600));

        let decayed = events
            .iter()
            .position(|e| matches!(e, CoreEvent::StatsDecayed { .. }))
            .expect("StatsDecayed doit être émis");
        let died = events
            .iter()
            .position(|e| matches!(e, CoreEvent::Died))
            .expect("Died doit être émis");

        assert!(
            decayed < died,
            "les jauges post-mortem fuitaient après Died"
        );
    }

    #[test]
    fn test_zero_duration_tick_is_a_noop() {
        let mut pet = PetState::new("Gizmo");
        let before = pet.clone();
        let events = pet.tick(Duration::ZERO);

        assert!(events.is_empty());
        assert_eq!(before, pet);
    }

    #[test]
    fn test_absurd_elapsed_terminates_and_is_capped() {
        // Reproduit un horodatage de sauvegarde corrompu : l'ancienne boucle
        // en f32 ne terminait plus au-delà de ~2^29 secondes.
        let mut pet = PetState::new("Gizmo");
        let events = pet.tick(Duration::from_secs(u64::MAX));

        assert_eq!(pet.mood(), PetMood::Dead);
        assert!(events.contains(&CoreEvent::Died));
    }

    #[test]
    fn test_absurd_catchup_step_does_not_panic() {
        // `Duration::from_secs_f32` paniquait sur un pas de simulation démesuré.
        let mut config = CoreConfig::new();
        config.catchup_step_secs = u64::MAX;
        let mut pet = PetState::with_config("Gizmo", config);

        let events = pet.tick(Duration::from_secs(120));
        assert!(!events.is_empty());
        assert!(pet.stats().energy().is_finite());
    }

    #[test]
    fn test_sleep_wake_and_interactions() {
        let mut pet = PetState::new("Gizmo");
        let sleep_events = pet.sleep().unwrap();
        assert_eq!(pet.mood(), PetMood::Sleeping);
        assert!(sleep_events.contains(&CoreEvent::FellAsleep));

        // Toutes les interactions directes sont refusées pendant le sommeil.
        assert!(matches!(
            pet.feed(None),
            Err(CoreError::InvalidActionForMood { .. })
        ));
        assert!(matches!(
            pet.heal(None),
            Err(CoreError::InvalidActionForMood { .. })
        ));
        assert!(matches!(
            pet.rest(None),
            Err(CoreError::InvalidActionForMood { .. })
        ));

        let wake_events = pet.wake_up().unwrap();
        assert_eq!(pet.mood(), PetMood::Happy);
        assert!(wake_events.contains(&CoreEvent::WokeUp));

        let feed_ok = pet.feed(Some(20.0)).unwrap();
        assert!(feed_ok
            .iter()
            .any(|e| matches!(e, CoreEvent::Fed { amount } if *amount == 20.0)));
    }

    #[test]
    fn test_sleep_and_wake_are_idempotent() {
        let mut pet = PetState::new("Gizmo");
        assert!(pet.wake_up().unwrap().is_empty());
        assert!(!pet.sleep().unwrap().is_empty());
        assert!(pet.sleep().unwrap().is_empty());
    }

    #[test]
    fn test_poisoned_action_amounts_are_rejected() {
        let mut pet = PetState::new("Gizmo");

        for amount in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -1000.0] {
            assert!(
                matches!(
                    pet.feed(Some(amount)),
                    Err(CoreError::InvalidActionAmount { .. })
                ),
                "montant {amount} accepté par feed"
            );
            assert!(matches!(
                pet.pet(Some(amount)),
                Err(CoreError::InvalidActionAmount { .. })
            ));
            assert!(matches!(
                pet.heal(Some(amount)),
                Err(CoreError::InvalidActionAmount { .. })
            ));
            assert!(matches!(
                pet.rest(Some(amount)),
                Err(CoreError::InvalidActionAmount { .. })
            ));
        }

        // Aucune jauge n'a été empoisonnée et l'état reste sérialisable.
        assert!(pet.stats().energy().is_finite());
        assert!(pet.stats().satiety().is_finite());
        assert!(pet.stats().happiness().is_finite());
        let json = pet.to_json().unwrap();
        assert!(PetState::from_json(&json).is_ok());
    }

    #[test]
    fn test_revive_flow() {
        let mut pet = PetState::new("Gizmo");
        pet.set_stats(PetStats::new(0.0, 0.0, 0.0));
        assert_eq!(pet.mood(), PetMood::Dead);

        assert!(pet.pet(None).is_err());

        let revive_events = pet.revive().unwrap();
        assert_eq!(pet.mood(), PetMood::Happy);
        assert!(revive_events.contains(&CoreEvent::Revived));
        assert!(pet.is_alive());
    }

    #[test]
    fn test_revive_on_living_pet_is_refused() {
        let mut pet = PetState::new("Gizmo");
        assert!(matches!(
            pet.revive(),
            Err(CoreError::InvalidActionForMood {
                action: ActionKind::Revive,
                ..
            })
        ));
    }

    #[test]
    fn test_serde_roundtrip() {
        let mut pet = PetState::new("Gizmo");
        pet.handle_commit("gremlin", "main").unwrap();

        let json = pet.to_json().unwrap();
        let deserialized = PetState::from_json(&json).unwrap();

        assert_eq!(pet, deserialized);
        assert_eq!(deserialized.version(), SAVE_FORMAT_VERSION);
    }

    #[test]
    fn test_hostile_save_is_normalized() {
        let hostile = r#"{
            "version": 1,
            "name": "   ",
            "stats": { "energy": 9999.0, "satiety": -80.0, "happiness": 50.0 },
            "mood": "Happy",
            "progression": { "total_xp": 10500, "level": 0, "stage": "Baby", "total_commits": 4 },
            "config": {
                "decay": { "energy_decay_per_minute": -50.0, "satiety_decay_per_minute": 1.0,
                           "happiness_decay_per_minute": 0.8, "sleep_decay_multiplier": 0.1 },
                "catchup_step_secs": 0
            },
            "is_sleeping": false,
            "coding_timer_secs": -12.0
        }"#;

        let pet =
            PetState::from_json(hostile).expect("la sauvegarde doit être réparée, pas rejetée");

        assert_eq!(pet.name(), "Gremlin");
        assert_eq!(pet.stats().energy(), 100.0);
        assert_eq!(pet.stats().satiety(), 0.0);
        assert_eq!(pet.progression().level(), 15);
        assert!(pet.config().validate().is_ok());
        assert_eq!(pet.coding_timer_secs(), 0.0);
        // L'humeur annoncée était incohérente avec des jauges à plat.
        assert_eq!(pet.mood(), PetMood::Sick);
    }

    #[test]
    fn test_future_save_version_is_refused() {
        let json = format!(r#"{{"version": {}}}"#, SAVE_FORMAT_VERSION + 1);
        assert!(matches!(
            PetState::from_json(&json),
            Err(CoreError::UnsupportedSaveVersion { .. })
        ));
    }

    #[test]
    fn test_legacy_save_without_version_still_loads() {
        let legacy = r#"{"name": "Ancien", "is_sleeping": true}"#;
        let pet = PetState::from_json(legacy).expect("compatibilité ascendante");

        assert_eq!(pet.name(), "Ancien");
        assert_eq!(pet.version(), SAVE_FORMAT_VERSION);
        assert_eq!(pet.mood(), PetMood::Sleeping);
    }

    #[test]
    fn test_normalize_is_idempotent() {
        let mut pet = PetState::new("  Gizmo  ");
        pet.handle_commit("repo", "dev").unwrap();
        pet.normalize();
        let once = pet.clone();
        pet.normalize();
        assert_eq!(once, pet);
    }
}
