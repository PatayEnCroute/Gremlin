//! Agrégat de productivité : séries, inventaire et minuteur de concentration.
//!
//! Ces trois mécaniques partagent une même contrainte — elles dépendent du
//! **jour courant**, que `gremlin-core` ne peut pas lire lui-même. Les
//! regrouper derrière un seul champ persistant de
//! [`PetState`](crate::state::PetState) évite d'éparpiller une douzaine de
//! champs corrélés dans l'agrégat racine, et donne un point unique où la
//! récompense quotidienne se décide : le tracker sait *qu'elle est due*,
//! l'inventaire sait *s'il peut la recevoir*, et seul cet agrégat voit les
//! deux.

pub mod inventory;
pub mod pomodoro;
pub mod streak;

use crate::calendar::CivilDate;
use crate::config::CoreConfig;
use crate::events::CoreEvent;
use serde::{Deserialize, Serialize};

pub use inventory::{
    ConsumableEffect, ConsumableKind, GrantOutcome, GrantReason, Inventory, CONSUMABLE_COUNT,
};
pub use pomodoro::{
    PauseReason, PomodoroPhase, PomodoroSession, PomodoroState, PomodoroTimer,
    WellbeingReminderKind,
};
pub use streak::{StreakReward, StreakSnapshot, StreakTracker, STREAK_REWARD_COUNT};

/// Ordre de rotation de la récompense quotidienne.
///
/// La rotation est déterministe et fondée sur le nombre de jours déjà
/// récompensés : aucun tirage aléatoire, donc un scénario de test reproductible
/// et aucun moyen de « relancer » pour obtenir un autre objet.
const DAILY_REWARD_ROTATION: [ConsumableKind; CONSUMABLE_COUNT] = [
    ConsumableKind::Snack,
    ConsumableKind::Coffee,
    ConsumableKind::DebugPotion,
];

/// État de productivité persistant du familier.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ProductivityState {
    /// Jours de commits, série courante, records et récompenses.
    streak: StreakTracker,
    /// Stocks de consommables.
    inventory: Inventory,
    /// Minuteur de concentration.
    pomodoro: PomodoroTimer,
}

impl ProductivityState {
    /// Suivi des séries, en lecture seule.
    #[must_use]
    pub const fn streak(&self) -> &StreakTracker {
        &self.streak
    }

    /// Stocks de consommables, en lecture seule.
    #[must_use]
    pub const fn inventory(&self) -> &Inventory {
        &self.inventory
    }

    /// Minuteur de concentration, en lecture seule.
    #[must_use]
    pub const fn pomodoro(&self) -> &PomodoroTimer {
        &self.pomodoro
    }

    /// Minuteur de concentration, en écriture.
    ///
    /// Les transitions sont déjà gardées par la machine à états : l'exposer en
    /// écriture ne permet pas de fabriquer un état incohérent.
    pub fn pomodoro_mut(&mut self) -> &mut PomodoroTimer {
        &mut self.pomodoro
    }

    /// Assimile un jour de commit observé en direct.
    ///
    /// Ce chemin peut attribuer la récompense quotidienne ; il n'attribue
    /// jamais d'XP, qui reste la responsabilité de
    /// [`PetState::handle_commit`](crate::state::PetState::handle_commit).
    pub fn record_commit_day(
        &mut self,
        commit_day: CivilDate,
        today: CivilDate,
        config: &CoreConfig,
    ) -> Vec<CoreEvent> {
        self.merge_days([commit_day], today, config)
    }

    /// Réconcilie un historique de jours lu dans les reflogs.
    ///
    /// La réconciliation peut débloquer un cosmétique réellement mérité et
    /// attribuer la récompense du jour courant si un commit d'aujourd'hui y
    /// figure. Elle ne rejoue **jamais** l'XP des commits passés.
    pub fn reconcile_commit_history(
        &mut self,
        days: impl IntoIterator<Item = CivilDate>,
        today: CivilDate,
        config: &CoreConfig,
    ) -> Vec<CoreEvent> {
        self.merge_days(days, today, config)
    }

    /// Recalcule la série visible pour un nouveau jour courant.
    pub fn refresh_for_day(&mut self, today: CivilDate) -> Vec<CoreEvent> {
        self.streak.refresh_for_day(today)
    }

    /// Fusionne des jours puis règle la récompense quotidienne éventuelle.
    fn merge_days(
        &mut self,
        days: impl IntoIterator<Item = CivilDate>,
        today: CivilDate,
        config: &CoreConfig,
    ) -> Vec<CoreEvent> {
        let update = self.streak.merge_days(days, today, config.streak);
        let mut events = update.events;
        if update.daily_reward_due {
            self.grant_daily_reward(today, config, &mut events);
        }
        events
    }

    /// Attribue la récompense quotidienne, puis marque le jour comme traité.
    ///
    /// Le marquage a lieu même si tous les slots sont pleins : sans cela, vider
    /// un slot puis redémarrer permettrait de réclamer indéfiniment.
    fn grant_daily_reward(
        &mut self,
        today: CivilDate,
        config: &CoreConfig,
        events: &mut Vec<CoreEvent>,
    ) {
        let rotation_index = (self.streak.rewarded_day_count() as usize) % CONSUMABLE_COUNT;
        let preferred = DAILY_REWARD_ROTATION[rotation_index];

        // Repli sur les autres objets dans l'ordre canonique : une préférence
        // pleine ne doit pas faire perdre la récompense du jour.
        let candidates = std::iter::once(preferred).chain(
            ConsumableKind::ALL
                .into_iter()
                .filter(move |kind| *kind != preferred),
        );

        for kind in candidates {
            let outcome = self.inventory.grant(kind, 1, &config.inventory);
            if outcome.added > 0 {
                events.push(CoreEvent::ConsumableGranted {
                    kind,
                    quantity: outcome.added,
                    reason: GrantReason::DailyReward,
                });
                break;
            }
        }

        self.streak.mark_daily_reward_claimed(today);
    }

    /// Retire un exemplaire du stock, une fois l'effet validé par l'appelant.
    pub(crate) fn take_consumable(&mut self, kind: ConsumableKind) -> bool {
        self.inventory.take_one(kind)
    }

    /// Répare l'agrégat après désérialisation. Idempotente.
    pub fn normalize(&mut self, config: &CoreConfig) {
        self.streak.normalize(config.streak);
        self.inventory.normalize(&config.inventory);
        self.pomodoro.normalize(&config.pomodoro);
    }

    /// Suspend une session en cours après rechargement d'une sauvegarde.
    pub fn mark_restarted(&mut self) {
        self.pomodoro.mark_restarted();
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    fn date(year: i32, month: u8, day: u8) -> CivilDate {
        CivilDate::new(year, month, day).unwrap_or_else(|_| {
            unreachable!("date de test invalide");
        })
    }

    fn config() -> CoreConfig {
        let mut config = CoreConfig::new();
        config.normalize();
        config
    }

    /// Extrait les octrois de consommables d'une liste d'événements.
    fn grants(events: &[CoreEvent]) -> Vec<(ConsumableKind, u8)> {
        events
            .iter()
            .filter_map(|event| match event {
                CoreEvent::ConsumableGranted { kind, quantity, .. } => Some((*kind, *quantity)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn test_a_fresh_state_holds_the_initial_stock_and_an_idle_timer() {
        let state = ProductivityState::default();
        assert_eq!(state.inventory().quantity(ConsumableKind::Snack), 2);
        assert_eq!(state.pomodoro().state(), PomodoroState::Idle);
        assert_eq!(state.streak().current_streak(date(2024, 5, 10)), 0);
    }

    #[test]
    fn test_first_commit_of_the_day_grants_the_rotation_head() {
        let config = config();
        let mut state = ProductivityState::default();
        let today = date(2024, 5, 10);

        let events = state.record_commit_day(today, today, &config);
        assert_eq!(
            grants(&events),
            vec![(ConsumableKind::Snack, 1)],
            "la rotation commence par la collation"
        );
        assert_eq!(state.inventory().quantity(ConsumableKind::Snack), 3);
    }

    #[test]
    fn test_second_commit_of_the_day_grants_nothing_more() {
        let config = config();
        let mut state = ProductivityState::default();
        let today = date(2024, 5, 10);

        state.record_commit_day(today, today, &config);
        let events = state.record_commit_day(today, today, &config);
        assert!(grants(&events).is_empty());
    }

    #[test]
    fn test_daily_reward_rotates_over_three_days() {
        let config = config();
        let mut state = ProductivityState::default();
        let mut granted = Vec::new();

        for offset in 0..3 {
            let day = date(2024, 5, 1).checked_add_days(offset).unwrap();
            granted.extend(grants(&state.record_commit_day(day, day, &config)));
        }

        assert_eq!(
            granted,
            vec![
                (ConsumableKind::Snack, 1),
                (ConsumableKind::Coffee, 1),
                (ConsumableKind::DebugPotion, 1),
            ]
        );
    }

    #[test]
    fn test_daily_reward_falls_back_when_the_preferred_slot_is_full() {
        let config = config();
        let mut state = ProductivityState::default();
        let today = date(2024, 5, 10);

        // La collation est le premier choix : on la remplit d'abord.
        while !state
            .inventory()
            .is_full(ConsumableKind::Snack, &config.inventory)
        {
            state
                .inventory
                .grant(ConsumableKind::Snack, 1, &config.inventory);
        }

        let events = state.record_commit_day(today, today, &config);
        assert_eq!(grants(&events), vec![(ConsumableKind::Coffee, 1)]);
    }

    #[test]
    fn test_a_full_inventory_still_consumes_the_daily_claim() {
        let config = config();
        let mut state = ProductivityState::default();
        let today = date(2024, 5, 10);

        for kind in ConsumableKind::ALL {
            state
                .inventory
                .grant(kind, config.inventory.capacity, &config.inventory);
        }

        let events = state.record_commit_day(today, today, &config);
        assert!(grants(&events).is_empty(), "octroi impossible mais annoncé");

        // Libérer un slot puis recommiter le même jour ne redonne rien : le
        // jour a bien été marqué comme traité.
        state.take_consumable(ConsumableKind::Coffee);
        let again = state.record_commit_day(today, today, &config);
        assert!(grants(&again).is_empty(), "récompense réclamée deux fois");
    }

    #[test]
    fn test_history_reconciliation_grants_nothing_for_past_days() {
        let config = config();
        let mut state = ProductivityState::default();
        let today = date(2024, 5, 10);
        let before = state.inventory().total();

        let events = state.reconcile_commit_history(
            (0..5).filter_map(|o| date(2024, 5, 1).checked_add_days(o)),
            today,
            &config,
        );

        assert!(grants(&events).is_empty());
        assert_eq!(state.inventory().total(), before);
        assert!(events
            .iter()
            .any(|e| matches!(e, CoreEvent::StreakRewardUnlocked { .. })));
    }

    #[test]
    fn test_history_reconciliation_including_today_grants_once() {
        let config = config();
        let mut state = ProductivityState::default();
        let today = date(2024, 5, 10);

        let events = state.reconcile_commit_history(
            (0..10).filter_map(|o| date(2024, 5, 1).checked_add_days(o)),
            today,
            &config,
        );
        assert_eq!(grants(&events).len(), 1);

        // Un second seed identique — rattachement d'un autre dépôt, rescan —
        // ne réattribue rien.
        let replay = state.reconcile_commit_history(
            (0..10).filter_map(|o| date(2024, 5, 1).checked_add_days(o)),
            today,
            &config,
        );
        assert!(grants(&replay).is_empty());
    }

    #[test]
    fn test_normalize_is_idempotent() {
        let config = config();
        let mut state = ProductivityState::default();
        let today = date(2024, 5, 10);
        state.record_commit_day(today, today, &config);

        state.normalize(&config);
        let once = state.clone();
        state.normalize(&config);
        assert_eq!(state, once);
    }

    #[test]
    fn test_roundtrip_preserves_the_aggregate() {
        let config = config();
        let mut state = ProductivityState::default();
        let today = date(2024, 5, 10);
        state.record_commit_day(today, today, &config);
        state.pomodoro_mut().start(&config.pomodoro).unwrap();

        let json = serde_json::to_string(&state).unwrap();
        let mut restored: ProductivityState = serde_json::from_str(&json).unwrap();
        restored.normalize(&config);
        restored.mark_restarted();

        assert_eq!(restored.inventory(), state.inventory());
        assert_eq!(
            restored.pomodoro().pause_reason(),
            Some(PauseReason::Restarted)
        );
        assert_eq!(
            restored.streak().current_streak(today),
            state.streak().current_streak(today)
        );
    }

    #[test]
    fn test_missing_field_yields_the_default_aggregate() {
        let state: ProductivityState = serde_json::from_str("{}").unwrap();
        assert_eq!(state, ProductivityState::default());
    }
}
