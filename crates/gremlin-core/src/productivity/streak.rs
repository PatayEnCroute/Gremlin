//! Séries de jours de commits et récompenses cosmétiques associées.
//!
//! Le suivi repose sur des **jours civils locaux** injectés par
//! l'orchestrateur, jamais sur des paquets de 24 heures ni sur une horloge lue
//! ici. Les règles retenues sont explicites :
//!
//! 1. plusieurs commits le même jour ne comptent qu'une fois ;
//! 2. un commit le lendemain du dernier jour actif prolonge la série ;
//! 3. après un jour civil manqué, le prochain commit repart à 1 ;
//! 4. la série reste affichée pendant tout le lendemain du dernier commit —
//!    c'est la **règle de grâce**, qui laisse la journée entière pour la
//!    prolonger ;
//! 5. elle tombe à 0 au deuxième jour manqué, sans jamais effacer la meilleure
//!    série ni le total de jours productifs.
//!
//! Les jours arrivent dans n'importe quel ordre, depuis plusieurs dépôts et en
//! plusieurs lots : la fusion est idempotente, un même jour ne peut pas être
//! compté deux fois, et rejouer un historique ne réattribue jamais d'XP.

use crate::calendar::{CivilDate, MAX_DAY_NUMBER, MIN_DAY_NUMBER};
use crate::config::StreakConfig;
use crate::events::CoreEvent;
use serde::{Deserialize, Serialize};
use std::fmt;

/// Nombre de récompenses de série existantes.
pub const STREAK_REWARD_COUNT: usize = 3;

/// Plafond appliqué à la meilleure série persistée, en jours (un siècle).
///
/// Une sauvegarde éditée à la main pourrait annoncer 65 535 jours ; la valeur
/// est ramenée à une durée qu'une vie de développeur peut atteindre.
const MAX_TRACKED_STREAK_DAYS: u16 = 36_500;

/// Nombre maximal de jours actifs conservés en mémoire quelle que soit la
/// configuration lue depuis le disque.
///
/// La désérialisation alloue avant que [`StreakTracker::normalize`] ne
/// s'exécute : ce plafond borne ce qui survit à la normalisation, la taille du
/// fichier de sauvegarde bornant pour sa part l'allocation initiale.
const HARD_MAX_ACTIVE_DAYS: usize = 4_096;

/// Cosmétique débloqué par un palier de série.
///
/// L'identifiant devient **stable** dès qu'une sauvegarde le référence :
/// renommer une variante invaliderait des récompenses déjà acquises.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum StreakReward {
    /// Petite feuille épinglée, premier palier.
    LeafPin,
    /// Casque de concentration, deuxième palier.
    FocusHeadphones,
    /// Aura aurorale, palier le plus rare.
    AuroraAura,
}

impl StreakReward {
    /// Les trois récompenses, de la plus accessible à la plus rare.
    ///
    /// L'ordre correspond terme à terme à
    /// [`StreakConfig::milestone_days`](crate::config::StreakConfig::milestone_days),
    /// que la normalisation garantit strictement croissant.
    pub const ALL: [Self; STREAK_REWARD_COUNT] =
        [Self::LeafPin, Self::FocusHeadphones, Self::AuroraAura];

    /// Index de la récompense dans [`Self::ALL`].
    #[must_use]
    pub const fn index(self) -> usize {
        match self {
            Self::LeafPin => 0,
            Self::FocusHeadphones => 1,
            Self::AuroraAura => 2,
        }
    }

    /// Bit occupé par la récompense dans le masque persisté.
    ///
    /// Le masque de bits remplace une liste d'identifiants textuels : une
    /// variante inconnue lue sur le disque est simplement masquée, là où une
    /// chaîne inattendue ferait échouer toute la désérialisation.
    #[must_use]
    pub const fn bit(self) -> u8 {
        1 << self.index()
    }

    /// Identifiant stable de l'accessoire correspondant.
    #[must_use]
    pub const fn accessory_id(self) -> &'static str {
        match self {
            Self::LeafPin => "streak_leaf_pin",
            Self::FocusHeadphones => "focus_headphones",
            Self::AuroraAura => "aurora_aura",
        }
    }

    /// Libellé lisible de la récompense.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::LeafPin => "Feuille porte-bonheur",
            Self::FocusHeadphones => "Casque de concentration",
            Self::AuroraAura => "Aura aurorale",
        }
    }
}

impl fmt::Display for StreakReward {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// Masque de tous les bits de récompense connus.
const KNOWN_REWARDS_MASK: u8 = StreakReward::LeafPin.bit()
    | StreakReward::FocusHeadphones.bit()
    | StreakReward::AuroraAura.bit();

/// Vue immuable de la série, destinée à l'interface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StreakSnapshot {
    /// Série courante affichée, règle de grâce comprise.
    pub current_days: u16,
    /// Meilleure série jamais prouvée.
    pub longest_days: u16,
    /// Nombre total de jours de commits distincts observés.
    pub total_productive_days: u32,
    /// Dernier jour actif connu, s'il en existe un.
    pub last_active_day: Option<CivilDate>,
}

/// Résultat d'une fusion de jours dans le tracker.
///
/// La récompense quotidienne n'est pas attribuée ici : le tracker ignore
/// l'inventaire. Il signale seulement qu'elle est **due**, et l'agrégat
/// [`ProductivityState`](super::ProductivityState) réalise la transaction.
#[derive(Debug, Clone, Default)]
pub(crate) struct StreakUpdate {
    /// Événements décrivant les changements visibles.
    pub events: Vec<CoreEvent>,
    /// Une récompense quotidienne reste à attribuer pour le jour courant.
    pub daily_reward_due: bool,
}

/// Suivi persistant des jours de commits et des paliers atteints.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct StreakTracker {
    /// Numéros de jours actifs, triés, uniques et bornés par la rétention.
    active_days: Vec<i32>,
    /// Meilleure série jamais prouvée, conservée au-delà de l'élagage.
    longest_days: u16,
    /// Nombre de jours de commits distincts observés depuis toujours.
    total_productive_days: u32,
    /// Plancher des jours déjà comptabilisés puis élagués.
    ///
    /// Tout jour strictement antérieur a déjà été compté dans
    /// `total_productive_days` : le réobserver ne doit pas le compter deux fois.
    counted_floor_day: Option<i32>,
    /// Dernier jour ayant donné lieu à une récompense quotidienne.
    last_daily_reward_day: Option<i32>,
    /// Nombre de récompenses quotidiennes déjà attribuées, pilote la rotation.
    rewarded_day_count: u32,
    /// Masque des récompenses cosmétiques débloquées.
    unlocked_rewards_mask: u8,
    /// Dernier couple (série, record) annoncé, volontairement non persisté.
    ///
    /// Il évite de réémettre `StreakChanged` à chaque réveil sans changement,
    /// tout en garantissant une première annonce après chaque démarrage.
    #[serde(skip)]
    last_reported: Option<(u16, u16)>,
}

impl StreakTracker {
    /// Série courante affichée pour le jour donné, règle de grâce comprise.
    #[must_use]
    pub fn current_streak(&self, today: CivilDate) -> u16 {
        let Some(&last) = self.active_days.last() else {
            return 0;
        };
        let gap = today.day_number() - last;
        // `gap == 0` : commit aujourd'hui. `gap == 1` : grâce du lendemain.
        // Au-delà, la série est rompue. Un `gap` négatif vient d'une donnée
        // future qui n'aurait pas dû être insérée : elle n'affiche rien.
        if !(0..=1).contains(&gap) {
            return 0;
        }
        self.run_ending_at_last()
    }

    /// Meilleure série jamais prouvée.
    #[must_use]
    pub const fn longest_days(&self) -> u16 {
        self.longest_days
    }

    /// Nombre total de jours de commits distincts observés.
    #[must_use]
    pub const fn total_productive_days(&self) -> u32 {
        self.total_productive_days
    }

    /// Dernier jour actif connu.
    #[must_use]
    pub fn last_active_day(&self) -> Option<CivilDate> {
        self.active_days
            .last()
            .and_then(|day| CivilDate::from_day_number(*day).ok())
    }

    /// Nombre de jours actifs actuellement retenus.
    #[must_use]
    pub fn active_day_count(&self) -> usize {
        self.active_days.len()
    }

    /// Indique si ce jour est enregistré comme actif.
    #[must_use]
    pub fn has_active_day(&self, day: CivilDate) -> bool {
        self.active_days.binary_search(&day.day_number()).is_ok()
    }

    /// Vue complète destinée à l'interface.
    #[must_use]
    pub fn snapshot(&self, today: CivilDate) -> StreakSnapshot {
        StreakSnapshot {
            current_days: self.current_streak(today),
            longest_days: self.longest_days,
            total_productive_days: self.total_productive_days,
            last_active_day: self.last_active_day(),
        }
    }

    /// Indique si cette récompense est acquise.
    #[must_use]
    pub const fn is_unlocked(&self, reward: StreakReward) -> bool {
        self.unlocked_rewards_mask & reward.bit() != 0
    }

    /// Récompenses acquises, dans l'ordre canonique.
    #[must_use]
    pub fn unlocked_rewards(&self) -> Vec<StreakReward> {
        StreakReward::ALL
            .into_iter()
            .filter(|reward| self.is_unlocked(*reward))
            .collect()
    }

    /// Nombre de jours restant avant le prochain palier, avec sa récompense.
    #[must_use]
    pub fn next_milestone(
        &self,
        today: CivilDate,
        config: StreakConfig,
    ) -> Option<(StreakReward, u16)> {
        let current = self.current_streak(today);
        StreakReward::ALL
            .into_iter()
            .zip(config.milestone_days)
            .find(|(reward, _)| !self.is_unlocked(*reward))
            .map(|(reward, required)| (reward, required.saturating_sub(current)))
    }

    /// Fusionne un lot de jours observés et met à jour la série.
    ///
    /// Les jours peuvent arriver en désordre, en doublon et depuis plusieurs
    /// dépôts : le résultat ne dépend que de l'ensemble des jours fournis.
    pub(crate) fn merge_days(
        &mut self,
        days: impl IntoIterator<Item = CivilDate>,
        today: CivilDate,
        config: StreakConfig,
    ) -> StreakUpdate {
        for day in days {
            self.insert_day(day, today);
        }
        self.prune(config);
        self.refresh_records();

        let mut update = StreakUpdate::default();
        self.unlock_milestones(config, &mut update.events);
        self.report(today, &mut update.events);
        update.daily_reward_due = self.is_daily_reward_due(today);
        update
    }

    /// Recalcule la série visible pour un nouveau jour courant.
    ///
    /// Appelée au passage de minuit et après une reprise : c'est ce qui fait
    /// tomber une série à 0 au deuxième jour manqué sans qu'aucun commit ne
    /// survienne.
    pub(crate) fn refresh_for_day(&mut self, today: CivilDate) -> Vec<CoreEvent> {
        let mut events = Vec::new();
        self.report(today, &mut events);
        events
    }

    /// Marque la récompense quotidienne du jour comme traitée.
    ///
    /// Appelée même lorsque l'inventaire est plein : sans cela, vider un slot
    /// puis redémarrer permettrait de réclamer indéfiniment.
    pub(crate) fn mark_daily_reward_claimed(&mut self, today: CivilDate) {
        self.last_daily_reward_day = Some(today.day_number());
        self.rewarded_day_count = self.rewarded_day_count.saturating_add(1);
    }

    /// Rang de la prochaine récompense quotidienne, pour la rotation d'objets.
    pub(crate) const fn rewarded_day_count(&self) -> u32 {
        self.rewarded_day_count
    }

    /// Répare un tracker désérialisé.
    ///
    /// Idempotente : jours hors calendrier écartés, doublons fusionnés, ordre
    /// rétabli, rétention appliquée et compteurs bornés.
    pub fn normalize(&mut self, config: StreakConfig) {
        self.active_days
            .retain(|day| (MIN_DAY_NUMBER..=MAX_DAY_NUMBER).contains(day));
        self.active_days.sort_unstable();
        self.active_days.dedup();
        if self.active_days.len() > HARD_MAX_ACTIVE_DAYS {
            let excess = self.active_days.len() - HARD_MAX_ACTIVE_DAYS;
            self.raise_floor_to(self.active_days[excess]);
            self.active_days.drain(..excess);
        }
        self.prune(config);

        self.counted_floor_day = self
            .counted_floor_day
            .filter(|day| (MIN_DAY_NUMBER..=MAX_DAY_NUMBER).contains(day));
        self.last_daily_reward_day = self
            .last_daily_reward_day
            .filter(|day| (MIN_DAY_NUMBER..=MAX_DAY_NUMBER).contains(day));
        self.unlocked_rewards_mask &= KNOWN_REWARDS_MASK;

        self.refresh_records();
        self.longest_days = self.longest_days.min(MAX_TRACKED_STREAK_DAYS);
    }

    /// Insère un jour observé, en refusant le futur et les jours déjà comptés.
    fn insert_day(&mut self, day: CivilDate, today: CivilDate) {
        let number = day.day_number();
        if number > today.day_number() {
            return;
        }
        if self.counted_floor_day.is_some_and(|floor| number < floor) {
            return;
        }
        if let Err(position) = self.active_days.binary_search(&number) {
            self.active_days.insert(position, number);
            self.total_productive_days = self.total_productive_days.saturating_add(1);
        }
    }

    /// Applique la rétention en conservant les jours les plus récents.
    fn prune(&mut self, config: StreakConfig) {
        let retention = usize::from(config.retention_days).min(HARD_MAX_ACTIVE_DAYS);
        if self.active_days.len() <= retention {
            return;
        }
        // La meilleure série est mise à jour **avant** l'élagage : un record
        // porté par des jours anciens ne doit pas disparaître avec eux.
        self.refresh_records();
        let excess = self.active_days.len() - retention;
        self.raise_floor_to(self.active_days[excess]);
        self.active_days.drain(..excess);
    }

    /// Relève le plancher des jours déjà comptabilisés.
    fn raise_floor_to(&mut self, day: i32) {
        self.counted_floor_day = Some(match self.counted_floor_day {
            Some(existing) if existing >= day => existing,
            _ => day,
        });
    }

    /// Réaligne meilleure série et total sur les jours réellement retenus.
    fn refresh_records(&mut self) {
        let longest_retained = self.longest_run();
        self.longest_days = self
            .longest_days
            .max(longest_retained)
            .min(MAX_TRACKED_STREAK_DAYS);
        let retained = u32::try_from(self.active_days.len()).unwrap_or(u32::MAX);
        self.total_productive_days = self.total_productive_days.max(retained);
    }

    /// Longueur de la plus longue suite consécutive parmi les jours retenus.
    fn longest_run(&self) -> u16 {
        let mut best: u16 = 0;
        let mut run: u16 = 0;
        let mut previous: Option<i32> = None;
        for &day in &self.active_days {
            run = match previous {
                Some(prev) if day == prev + 1 => run.saturating_add(1),
                _ => 1,
            };
            best = best.max(run);
            previous = Some(day);
        }
        best
    }

    /// Longueur de la suite consécutive qui se termine au dernier jour actif.
    fn run_ending_at_last(&self) -> u16 {
        let Some(&last) = self.active_days.last() else {
            return 0;
        };
        let mut run: u16 = 1;
        let mut expected = last;
        for &day in self.active_days.iter().rev().skip(1) {
            let Some(previous) = expected.checked_sub(1) else {
                break;
            };
            if day != previous {
                break;
            }
            run = run.saturating_add(1);
            expected = previous;
        }
        run
    }

    /// Débloque les paliers atteints, une seule fois chacun.
    fn unlock_milestones(&mut self, config: StreakConfig, events: &mut Vec<CoreEvent>) {
        for (reward, required) in StreakReward::ALL.into_iter().zip(config.milestone_days) {
            if self.is_unlocked(reward) || self.longest_days < required {
                continue;
            }
            self.unlocked_rewards_mask |= reward.bit();
            events.push(CoreEvent::StreakRewardUnlocked {
                reward,
                required_days: required,
            });
        }
    }

    /// Émet `StreakChanged` uniquement si la valeur visible a bougé.
    fn report(&mut self, today: CivilDate, events: &mut Vec<CoreEvent>) {
        let visible = (self.current_streak(today), self.longest_days);
        if self.last_reported == Some(visible) {
            return;
        }
        self.last_reported = Some(visible);
        events.push(CoreEvent::StreakChanged {
            current_days: visible.0,
            longest_days: visible.1,
        });
    }

    /// Indique qu'un commit du jour courant n'a pas encore été récompensé.
    fn is_daily_reward_due(&self, today: CivilDate) -> bool {
        self.has_active_day(today) && self.last_daily_reward_day != Some(today.day_number())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    fn date(year: i32, month: u8, day: u8) -> CivilDate {
        CivilDate::new(year, month, day).unwrap_or_else(|_| {
            unreachable!("date de test invalide : {year}-{month}-{day}");
        })
    }

    fn config() -> StreakConfig {
        let mut config = StreakConfig::default();
        config.normalize();
        config
    }

    /// Fusionne des jours et renvoie la série courante résultante.
    fn merge(tracker: &mut StreakTracker, days: &[CivilDate], today: CivilDate) -> u16 {
        tracker.merge_days(days.iter().copied(), today, config());
        tracker.current_streak(today)
    }

    #[test]
    fn test_a_fresh_tracker_shows_nothing() {
        let tracker = StreakTracker::default();
        let today = date(2024, 5, 10);
        assert_eq!(tracker.current_streak(today), 0);
        assert_eq!(tracker.longest_days(), 0);
        assert_eq!(tracker.total_productive_days(), 0);
        assert!(tracker.last_active_day().is_none());
        assert!(tracker.unlocked_rewards().is_empty());
    }

    #[test]
    fn test_streak_grows_day_after_day() {
        let mut tracker = StreakTracker::default();
        assert_eq!(
            merge(&mut tracker, &[date(2024, 5, 8)], date(2024, 5, 8)),
            1
        );
        assert_eq!(
            merge(&mut tracker, &[date(2024, 5, 9)], date(2024, 5, 9)),
            2
        );
        assert_eq!(
            merge(&mut tracker, &[date(2024, 5, 10)], date(2024, 5, 10)),
            3
        );
        assert_eq!(tracker.longest_days(), 3);
        assert_eq!(tracker.total_productive_days(), 3);
    }

    #[test]
    fn test_several_commits_the_same_day_count_once() {
        let mut tracker = StreakTracker::default();
        let today = date(2024, 5, 10);
        let day = date(2024, 5, 10);
        assert_eq!(merge(&mut tracker, &[day, day, day], today), 1);
        assert_eq!(tracker.total_productive_days(), 1);
        assert_eq!(tracker.active_day_count(), 1);
    }

    #[test]
    fn test_days_from_several_repositories_merge_without_double_counting() {
        let mut tracker = StreakTracker::default();
        let today = date(2024, 5, 10);

        // Trois dépôts ayant commité les mêmes deux jours.
        tracker.merge_days([date(2024, 5, 9), date(2024, 5, 10)], today, config());
        tracker.merge_days([date(2024, 5, 10), date(2024, 5, 9)], today, config());
        tracker.merge_days([date(2024, 5, 9)], today, config());

        assert_eq!(tracker.current_streak(today), 2);
        assert_eq!(tracker.total_productive_days(), 2);
    }

    #[test]
    fn test_merge_order_does_not_change_the_result() {
        let today = date(2024, 5, 10);
        let ordered = [date(2024, 5, 8), date(2024, 5, 9), date(2024, 5, 10)];
        let shuffled = [date(2024, 5, 10), date(2024, 5, 8), date(2024, 5, 9)];

        let mut first = StreakTracker::default();
        first.merge_days(ordered, today, config());

        let mut second = StreakTracker::default();
        for day in shuffled {
            second.merge_days([day], today, config());
        }

        assert_eq!(first.current_streak(today), second.current_streak(today));
        assert_eq!(first.active_day_count(), second.active_day_count());
        assert_eq!(
            first.total_productive_days(),
            second.total_productive_days()
        );
    }

    #[test]
    fn test_grace_day_keeps_the_streak_visible() {
        let mut tracker = StreakTracker::default();
        merge(
            &mut tracker,
            &[date(2024, 5, 8), date(2024, 5, 9), date(2024, 5, 10)],
            date(2024, 5, 10),
        );

        // Lendemain sans commit : la série reste affichée toute la journée.
        assert_eq!(tracker.current_streak(date(2024, 5, 11)), 3);
        // Deuxième jour manqué : elle tombe.
        assert_eq!(tracker.current_streak(date(2024, 5, 12)), 0);
        // Le record survit.
        assert_eq!(tracker.longest_days(), 3);
    }

    #[test]
    fn test_streak_restarts_at_one_after_a_missed_day() {
        let mut tracker = StreakTracker::default();
        merge(
            &mut tracker,
            &[date(2024, 5, 8), date(2024, 5, 9)],
            date(2024, 5, 9),
        );
        // Le 10 est manqué ; le commit du 11 repart à 1.
        assert_eq!(
            merge(&mut tracker, &[date(2024, 5, 11)], date(2024, 5, 11)),
            1
        );
        assert_eq!(tracker.longest_days(), 2);
        assert_eq!(tracker.total_productive_days(), 3);
    }

    #[test]
    fn test_streak_crosses_month_and_year_boundaries() {
        let mut tracker = StreakTracker::default();
        let days = [
            date(2023, 12, 30),
            date(2023, 12, 31),
            date(2024, 1, 1),
            date(2024, 1, 2),
        ];
        assert_eq!(merge(&mut tracker, &days, date(2024, 1, 2)), 4);

        let mut leap = StreakTracker::default();
        let leap_days = [date(2024, 2, 28), date(2024, 2, 29), date(2024, 3, 1)];
        assert_eq!(merge(&mut leap, &leap_days, date(2024, 3, 1)), 3);
    }

    #[test]
    fn test_future_days_are_ignored() {
        let mut tracker = StreakTracker::default();
        let today = date(2024, 5, 10);
        merge(
            &mut tracker,
            &[today, date(2024, 5, 11), date(2030, 1, 1)],
            today,
        );

        assert_eq!(tracker.active_day_count(), 1);
        assert_eq!(tracker.current_streak(today), 1);
        assert_eq!(tracker.total_productive_days(), 1);
    }

    #[test]
    fn test_milestones_unlock_once_each() {
        let mut tracker = StreakTracker::default();
        let config = config();
        let mut unlocked = Vec::new();

        // Sept jours consécutifs franchissent les paliers 3 et 7.
        for offset in 0..7 {
            let day = date(2024, 5, 1).checked_add_days(offset).unwrap();
            let update = tracker.merge_days([day], day, config);
            for event in update.events {
                if let CoreEvent::StreakRewardUnlocked { reward, .. } = event {
                    unlocked.push(reward);
                }
            }
        }

        assert_eq!(
            unlocked,
            vec![StreakReward::LeafPin, StreakReward::FocusHeadphones]
        );
        assert!(tracker.is_unlocked(StreakReward::LeafPin));
        assert!(!tracker.is_unlocked(StreakReward::AuroraAura));

        // Rejouer les mêmes jours ne redébloque rien.
        let replay = tracker.merge_days(
            (0..7).filter_map(|o| date(2024, 5, 1).checked_add_days(o)),
            date(2024, 5, 7),
            config,
        );
        assert!(replay
            .events
            .iter()
            .all(|e| !matches!(e, CoreEvent::StreakRewardUnlocked { .. })));
    }

    #[test]
    fn test_rewards_survive_a_broken_streak() {
        let mut tracker = StreakTracker::default();
        let config = config();
        for offset in 0..3 {
            let day = date(2024, 5, 1).checked_add_days(offset).unwrap();
            tracker.merge_days([day], day, config);
        }
        assert!(tracker.is_unlocked(StreakReward::LeafPin));

        // Un mois plus tard, la série est retombée à 1 : la récompense reste.
        let later = date(2024, 6, 15);
        tracker.merge_days([later], later, config);
        assert_eq!(tracker.current_streak(later), 1);
        assert!(tracker.is_unlocked(StreakReward::LeafPin));
    }

    #[test]
    fn test_streak_changed_is_emitted_only_on_change() {
        let mut tracker = StreakTracker::default();
        let config = config();
        let today = date(2024, 5, 10);

        let first = tracker.merge_days([today], today, config);
        assert!(first
            .events
            .iter()
            .any(|e| matches!(e, CoreEvent::StreakChanged { .. })));

        let second = tracker.merge_days([today], today, config);
        assert!(second
            .events
            .iter()
            .all(|e| !matches!(e, CoreEvent::StreakChanged { .. })));
    }

    #[test]
    fn test_refresh_for_day_drops_the_streak_at_the_second_missed_day() {
        let mut tracker = StreakTracker::default();
        let config = config();
        let today = date(2024, 5, 10);
        tracker.merge_days([today], today, config);

        assert!(tracker.refresh_for_day(date(2024, 5, 11)).is_empty());
        let events = tracker.refresh_for_day(date(2024, 5, 12));
        assert_eq!(
            events,
            vec![CoreEvent::StreakChanged {
                current_days: 0,
                longest_days: 1,
            }]
        );
    }

    #[test]
    fn test_daily_reward_is_due_once_per_day() {
        let mut tracker = StreakTracker::default();
        let config = config();
        let today = date(2024, 5, 10);

        let update = tracker.merge_days([today], today, config);
        assert!(update.daily_reward_due);

        tracker.mark_daily_reward_claimed(today);
        let again = tracker.merge_days([today], today, config);
        assert!(!again.daily_reward_due, "récompense réclamée deux fois");

        // Le lendemain, un nouveau commit redonne droit à une récompense.
        let tomorrow = date(2024, 5, 11);
        let next = tracker.merge_days([tomorrow], tomorrow, config);
        assert!(next.daily_reward_due);
        assert_eq!(tracker.rewarded_day_count(), 1);
    }

    #[test]
    fn test_history_seed_without_today_grants_no_daily_reward() {
        let mut tracker = StreakTracker::default();
        let config = config();
        let today = date(2024, 5, 10);

        let update = tracker.merge_days([date(2024, 5, 1), date(2024, 5, 2)], today, config);
        assert!(!update.daily_reward_due);
    }

    #[test]
    fn test_retention_prunes_old_days_but_keeps_the_record() {
        let mut config = StreakConfig {
            milestone_days: [3, 7, 30],
            retention_days: 10,
        };
        config.normalize();

        let mut tracker = StreakTracker::default();
        let start = date(2024, 1, 1);
        let days: Vec<CivilDate> = (0..40).filter_map(|o| start.checked_add_days(o)).collect();
        let today = days.last().copied().unwrap();
        tracker.merge_days(days.iter().copied(), today, config);

        assert_eq!(tracker.active_day_count(), 10, "rétention non appliquée");
        assert_eq!(tracker.longest_days(), 40, "record perdu à l'élagage");
        assert_eq!(tracker.total_productive_days(), 40);
        assert_eq!(tracker.current_streak(today), 10);
    }

    #[test]
    fn test_pruned_days_are_not_counted_twice_when_reobserved() {
        let mut config = StreakConfig {
            milestone_days: [3, 7, 30],
            retention_days: 10,
        };
        config.normalize();

        let mut tracker = StreakTracker::default();
        let start = date(2024, 1, 1);
        let days: Vec<CivilDate> = (0..40).filter_map(|o| start.checked_add_days(o)).collect();
        let today = days.last().copied().unwrap();
        tracker.merge_days(days.iter().copied(), today, config);
        let total_before = tracker.total_productive_days();

        // Un second seed rejoue tout l'historique, y compris les jours élagués.
        tracker.merge_days(days.iter().copied(), today, config);
        assert_eq!(tracker.total_productive_days(), total_before);
    }

    #[test]
    fn test_next_milestone_reports_the_remaining_days() {
        let tracker = StreakTracker::default();
        let today = date(2024, 5, 10);
        let (reward, remaining) = tracker.next_milestone(today, config()).unwrap();
        assert_eq!(reward, StreakReward::LeafPin);
        assert_eq!(remaining, 3);
    }

    #[test]
    fn test_normalize_repairs_a_hand_edited_tracker_and_is_idempotent() {
        let hostile = r#"{
            "active_days": [20000, 19999, 20000, -5, 2147483647, 19998],
            "longest_days": 65535,
            "total_productive_days": 0,
            "counted_floor_day": -42,
            "last_daily_reward_day": 2147483647,
            "rewarded_day_count": 7,
            "unlocked_rewards_mask": 255
        }"#;
        let mut tracker: StreakTracker = serde_json::from_str(hostile).unwrap();
        let config = config();

        tracker.normalize(config);
        let once = tracker.clone();

        assert_eq!(
            tracker.active_day_count(),
            3,
            "doublons ou hors-bornes gardés"
        );
        assert_eq!(tracker.longest_days(), MAX_TRACKED_STREAK_DAYS);
        assert_eq!(tracker.total_productive_days(), 3);
        assert_eq!(tracker.unlocked_rewards().len(), STREAK_REWARD_COUNT);

        tracker.normalize(config);
        assert_eq!(tracker, once, "normalisation non idempotente");
    }

    #[test]
    fn test_normalize_applies_retention_to_a_bloated_save() {
        let mut config = StreakConfig {
            retention_days: 12,
            ..StreakConfig::default()
        };
        config.normalize();

        let days: Vec<i32> = (0..500).map(|d| 19_000 + d).collect();
        let json = serde_json::to_string(&serde_json::json!({ "active_days": days })).unwrap();
        let mut tracker: StreakTracker = serde_json::from_str(&json).unwrap();

        tracker.normalize(config);
        assert_eq!(tracker.active_day_count(), 12);
        assert_eq!(tracker.longest_days(), 500);
    }

    #[test]
    fn test_roundtrip_preserves_records_and_rewards() {
        let mut tracker = StreakTracker::default();
        let config = config();
        for offset in 0..4 {
            let day = date(2024, 5, 1).checked_add_days(offset).unwrap();
            tracker.merge_days([day], day, config);
        }
        tracker.mark_daily_reward_claimed(date(2024, 5, 4));

        let json = serde_json::to_string(&tracker).unwrap();
        let mut restored: StreakTracker = serde_json::from_str(&json).unwrap();
        restored.normalize(config);

        assert_eq!(restored.longest_days(), tracker.longest_days());
        assert_eq!(restored.unlocked_rewards(), tracker.unlocked_rewards());
        assert_eq!(restored.rewarded_day_count(), tracker.rewarded_day_count());
        assert_eq!(
            restored.current_streak(date(2024, 5, 4)),
            tracker.current_streak(date(2024, 5, 4))
        );
        // Le champ d'annonce n'est pas persisté : la première annonce revient.
        assert!(!restored.refresh_for_day(date(2024, 5, 4)).is_empty());
    }

    #[test]
    fn test_huge_gap_does_not_loop_or_panic() {
        let mut tracker = StreakTracker::default();
        let config = config();
        tracker.merge_days([date(1970, 1, 1)], date(9999, 12, 31), config);
        assert_eq!(tracker.current_streak(date(9999, 12, 31)), 0);
        assert_eq!(tracker.longest_days(), 1);
    }
}
