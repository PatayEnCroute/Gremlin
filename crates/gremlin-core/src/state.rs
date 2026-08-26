//! État global encapsulé et gestionnaire du cycle de vie du Gremlin.
//!
//! `PetState` est l'agrégat racine du domaine : ses champs sont privés et
//! toutes les mutations passent par ses méthodes, ce qui garantit deux
//! invariants impossibles à tenir avec des champs publics — les jauges restent
//! bornées et finies, et `mood` reste toujours cohérente avec `stats`.

use crate::action::ActionKind;
use crate::calendar::CivilDate;
use crate::config::{CoreConfig, MAX_CATCHUP_DURATION_SECS};
use crate::error::CoreError;
use crate::events::CoreEvent;
use crate::focus::{ActivityState, FocusTracker};
use crate::mood::PetMood;
use crate::productivity::{ConsumableKind, PauseReason, ProductivityState};
use crate::progression::PetProgression;
use crate::stats::PetStats;
use crate::tooling::{BreakReason, BuildSummary, RepositoryId, TestSummary, ToolingSession};
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
/// Longueur maximale d'un libellé de dépôt recopié dans un événement.
const MAX_REPO_LABEL_CHARS: usize = 96;

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
    /// Séries de commits, inventaire et minuteur de concentration.
    ///
    /// Regroupés dans un seul champ : ces trois mécaniques dépendent toutes du
    /// jour courant injecté, et les disperser dans l'agrégat racine aurait
    /// ajouté une dizaine de champs corrélés.
    productivity: ProductivityState,
    /// Cooldowns et transitions propres au processus courant.
    #[serde(skip)]
    tooling_session: ToolingSession,
    /// Session de focus courante, volontairement non persistée.
    #[serde(skip)]
    focus_tracker: FocusTracker,
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
            productivity: ProductivityState::default(),
            tooling_session: ToolingSession::default(),
            focus_tracker: FocusTracker::default(),
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

        let capped = Self::cap_elapsed(elapsed);
        self.tooling_session.advance(capped);

        if self.mood == PetMood::Dead {
            return events;
        }
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

        self.wake_for_development(&mut events);

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

        self.push_progression_events(&mut events, levels_gained, evolution);

        self.reevaluate_mood(&mut events);
        Ok(events)
    }

    /// Assimile un rapport de tests déjà validé par l'orchestrateur.
    ///
    /// # Errors
    /// Renvoie `CoreError::PetIsDead` si le familier doit être réanimé avant
    /// de pouvoir recevoir une activité de développement.
    pub fn handle_test_run(
        &mut self,
        repository_id: RepositoryId,
        repo_label: &str,
        summary: TestSummary,
    ) -> Result<Vec<CoreEvent>, CoreError> {
        self.ensure_not_dead(ActionKind::TestRun)?;
        let tooling = self.config.tooling;
        let decision = self.tooling_session.register_test_run(
            repository_id,
            summary.failed() > 0,
            summary.has_executed_tests(),
            Duration::from_secs(tooling.reward_cooldown_secs),
            Duration::from_secs(tooling.feedback_cooldown_secs),
        );

        let mut events = Vec::new();
        self.wake_for_development(&mut events);
        self.coding_timer_secs = self.config.actions.coding_duration_secs;

        let xp_gained = if decision.reward_allowed {
            let fix_bonus = if decision.is_fixed {
                tooling.test_fix_bonus_xp
            } else {
                0
            };
            self.stats
                .adjust_happiness(tooling.test_pass_happiness_boost);
            tooling.test_pass_xp.saturating_add(fix_bonus)
        } else {
            if decision.entered_failure {
                self.stats
                    .adjust_happiness(-tooling.test_fail_happiness_penalty);
            }
            0
        };

        let (levels_gained, evolution) =
            self.progression
                .record_test_run(summary.passed(), summary.failed(), xp_gained);
        events.push(CoreEvent::TestRunReceived {
            repo: Self::normalize_repo_label(repo_label),
            summary,
            xp_gained,
            is_fixed: decision.is_fixed,
            feedback_allowed: decision.feedback_allowed,
        });
        self.push_progression_events(&mut events, levels_gained, evolution);
        self.reevaluate_mood(&mut events);
        Ok(events)
    }

    /// Assimile un résultat de build explicite.
    ///
    /// # Errors
    /// Renvoie `CoreError::PetIsDead` si le familier est décédé.
    pub fn handle_build_result(
        &mut self,
        repository_id: RepositoryId,
        repo_label: &str,
        summary: BuildSummary,
    ) -> Result<Vec<CoreEvent>, CoreError> {
        self.ensure_not_dead(ActionKind::Build)?;
        let tooling = self.config.tooling;
        let (reward_allowed, feedback_allowed) = self.tooling_session.register_build(
            repository_id,
            summary.success(),
            Duration::from_secs(tooling.reward_cooldown_secs),
            Duration::from_secs(tooling.feedback_cooldown_secs),
        );

        let mut events = Vec::new();
        self.wake_for_development(&mut events);
        self.coding_timer_secs = self.config.actions.coding_duration_secs;
        let xp_gained = if reward_allowed {
            tooling.build_success_xp
        } else {
            0
        };
        let (levels_gained, evolution) =
            self.progression.record_build(summary.success(), xp_gained);
        events.push(CoreEvent::BuildCompleted {
            repo: Self::normalize_repo_label(repo_label),
            summary,
            xp_gained,
            feedback_allowed,
        });
        self.push_progression_events(&mut events, levels_gained, evolution);
        self.reevaluate_mood(&mut events);
        Ok(events)
    }

    /// Avance l'estimation de focus avec un état d'activité injecté.
    ///
    /// Cette méthode est réservée au temps live ; le rattrapage hors-ligne ne
    /// doit appeler que [`Self::tick`].
    pub fn track_focus(
        &mut self,
        elapsed: Duration,
        activity: ActivityState,
        development_seen: bool,
    ) -> Vec<CoreEvent> {
        if self.mood == PetMood::Dead {
            return Vec::new();
        }

        let update =
            self.focus_tracker
                .track(elapsed, activity, development_seen, &self.config.focus);
        let rewards = self.config.focus.milestone_rewards();
        let bonus_xp = update
            .milestones
            .iter()
            .zip(rewards)
            .filter_map(|(reached, reward)| reached.then_some(reward))
            .fold(0_u64, u64::saturating_add);
        let (levels_gained, evolution) = self.progression.record_focus(update.credited, bonus_xp);

        let mut events = Vec::new();
        if let Some(is_idle) = update.idle_changed {
            events.push(CoreEvent::IdleStateChanged { is_idle });
        }
        for (index, reached) in update.milestones.into_iter().enumerate() {
            if reached {
                events.push(CoreEvent::FocusMilestoneReached {
                    duration: self.config.focus.milestone_durations()[index],
                    bonus_xp: rewards[index],
                });
            }
        }
        if update.break_recommended {
            events.push(CoreEvent::BreakRecommended {
                reason: BreakReason::FocusProlonged,
            });
        }
        self.push_progression_events(&mut events, levels_gained, evolution);
        events
    }

    /// Réinitialise la session de focus sans toucher aux statistiques cumulées.
    pub fn reset_focus_session(&mut self) {
        self.focus_tracker.reset();
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
        // Un familier endormi ne mesure pas un bloc de concentration : le
        // minuteur est suspendu plutôt que laissé à courir dans le vide.
        // L'échec est attendu quand aucun cycle n'est engagé.
        if let Ok(paused) = self
            .productivity
            .pomodoro_mut()
            .pause(PauseReason::PetAsleep)
        {
            events.extend(paused);
        }
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
        self.focus_tracker.reset();
        self.mood = PetMood::Happy;

        Ok(vec![
            CoreEvent::Revived,
            CoreEvent::MoodChanged {
                from: PetMood::Dead,
                to: PetMood::Happy,
            },
        ])
    }

    /// Séries, inventaire et minuteur de concentration, en lecture seule.
    #[must_use]
    pub const fn productivity(&self) -> &ProductivityState {
        &self.productivity
    }

    /// Enregistre le jour civil d'un commit observé en direct.
    ///
    /// Le gain d'XP et la réaction émotionnelle restent l'affaire de
    /// [`Self::handle_commit`] : cette méthode ne traite que la série et la
    /// récompense quotidienne. Les deux chemins sont séparés parce qu'un commit
    /// peut être détecté sans horodatage fiable — le familier réagit alors sans
    /// que la série ne bouge.
    pub fn record_commit_activity(
        &mut self,
        commit_day: CivilDate,
        today: CivilDate,
    ) -> Vec<CoreEvent> {
        self.productivity
            .record_commit_day(commit_day, today, &self.config)
    }

    /// Réconcilie un historique de jours de commits lu dans les reflogs.
    ///
    /// Aucun XP n'est rejoué : un historique prouve des journées de travail, pas
    /// des commits à récompenser une seconde fois.
    pub fn reconcile_commit_history(
        &mut self,
        days: impl IntoIterator<Item = CivilDate>,
        today: CivilDate,
    ) -> Vec<CoreEvent> {
        self.productivity
            .reconcile_commit_history(days, today, &self.config)
    }

    /// Recalcule la série visible pour un nouveau jour courant.
    ///
    /// Appelée au passage de minuit et après une reprise de la machine.
    pub fn refresh_current_day(&mut self, today: CivilDate) -> Vec<CoreEvent> {
        self.productivity.refresh_for_day(today)
    }

    /// Consomme un objet de l'inventaire et applique son effet.
    ///
    /// La transaction est atomique et ordonnée : état validé, effet réel
    /// calculé, stock décrémenté, jauges modifiées, humeur réévaluée. Un refus
    /// intervient donc toujours **avant** toute mutation — le stock reste
    /// intact.
    ///
    /// # Errors
    /// Renvoie `CoreError::PetIsDead` si le familier est décédé,
    /// `CoreError::InvalidActionForMood` s'il dort,
    /// `CoreError::ConsumableOutOfStock` si le stock est vide et
    /// `CoreError::ConsumableWithoutEffect` si les jauges concernées sont déjà
    /// pleines.
    pub fn use_consumable(&mut self, kind: ConsumableKind) -> Result<Vec<CoreEvent>, CoreError> {
        self.ensure_actionable(ActionKind::UseConsumable)?;

        if self.productivity.inventory().quantity(kind) == 0 {
            return Err(CoreError::ConsumableOutOfStock(kind));
        }

        let applied = kind.potential_effect(self.stats, &self.config.inventory);
        if !applied.is_meaningful() {
            return Err(CoreError::ConsumableWithoutEffect(kind));
        }

        if !self.productivity.take_consumable(kind) {
            return Err(CoreError::ConsumableOutOfStock(kind));
        }

        self.stats.rest(applied.energy);
        self.stats.feed(applied.satiety);
        self.stats.pet(applied.happiness);

        let mut events = vec![CoreEvent::ConsumableUsed {
            kind,
            remaining: self.productivity.inventory().quantity(kind),
            applied,
            stats: self.stats,
        }];
        self.reevaluate_mood(&mut events);
        Ok(events)
    }

    /// Démarre un bloc de concentration.
    ///
    /// # Errors
    /// Renvoie `CoreError::PetIsDead` si le familier est décédé, ou
    /// `CoreError::InvalidPomodoroTransition` si un cycle est déjà engagé.
    pub fn start_pomodoro(&mut self) -> Result<Vec<CoreEvent>, CoreError> {
        self.ensure_not_dead(ActionKind::UseConsumable)?;
        self.productivity
            .pomodoro_mut()
            .start(&self.config.pomodoro)
    }

    /// Suspend le minuteur de concentration.
    ///
    /// # Errors
    /// Renvoie `CoreError::InvalidPomodoroTransition` si aucun cycle n'est engagé.
    pub fn pause_pomodoro(&mut self, reason: PauseReason) -> Result<Vec<CoreEvent>, CoreError> {
        self.productivity.pomodoro_mut().pause(reason)
    }

    /// Reprend le minuteur de concentration.
    ///
    /// # Errors
    /// Renvoie `CoreError::PetIsDead` si le familier est décédé, ou
    /// `CoreError::InvalidPomodoroTransition` si le minuteur n'est pas en pause.
    pub fn resume_pomodoro(&mut self) -> Result<Vec<CoreEvent>, CoreError> {
        self.ensure_not_dead(ActionKind::UseConsumable)?;
        self.productivity.pomodoro_mut().resume()
    }

    /// Arrête le cycle de concentration. Idempotent.
    pub fn stop_pomodoro(&mut self) -> Vec<CoreEvent> {
        self.productivity.pomodoro_mut().stop()
    }

    /// Passe la pause en cours et prépare le bloc de travail suivant.
    ///
    /// # Errors
    /// Renvoie `CoreError::InvalidPomodoroTransition` si la phase courante est
    /// un bloc de travail : un bloc non accompli ne se comptabilise pas.
    pub fn skip_pomodoro_break(&mut self) -> Result<Vec<CoreEvent>, CoreError> {
        self.productivity
            .pomodoro_mut()
            .skip_break(&self.config.pomodoro)
    }

    /// Fait avancer le minuteur du temps réellement vécu par le processus.
    ///
    /// Volontairement distincte de [`Self::tick`] : le rattrapage hors-ligne
    /// simule la décroissance des jauges pendant une absence, alors que le
    /// minuteur ne doit progresser que sur du temps réellement mesuré.
    pub fn advance_live_productivity(&mut self, elapsed: Duration) -> Vec<CoreEvent> {
        let config = self.config.pomodoro;
        self.productivity.pomodoro_mut().advance(elapsed, &config)
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
        self.productivity.normalize(&self.config);

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
        // Un minuteur laissé « en cours » dans le fichier n'a mesuré aucun
        // temps pendant l'arrêt du processus. La conversion vit ici et non dans
        // `normalize`, qui est aussi appelée par `set_config` : elle y
        // suspendrait une session légitimement en cours.
        state.productivity.mark_restarted();
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

    fn normalize_repo_label(label: &str) -> String {
        let trimmed = label.trim();
        let source = if trimmed.is_empty() {
            "Dépôt"
        } else {
            trimmed
        };
        source.chars().take(MAX_REPO_LABEL_CHARS).collect()
    }

    fn wake_for_development(&mut self, events: &mut Vec<CoreEvent>) {
        if self.is_sleeping {
            self.is_sleeping = false;
            events.push(CoreEvent::WokeUp);
        }
    }

    fn push_progression_events(
        &self,
        events: &mut Vec<CoreEvent>,
        levels_gained: u32,
        evolution: Option<crate::progression::EvolutionStage>,
    ) {
        if levels_gained > 0 {
            events.push(CoreEvent::LevelUp {
                new_level: self.progression.level(),
                total_xp: self.progression.total_xp(),
            });
        }
        if let Some(new_stage) = evolution {
            events.push(CoreEvent::EvolutionUnlocked { new_stage });
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
    use crate::{BuildTool, TestFramework};

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
    fn test_test_runs_apply_cooldown_and_red_to_green_once() {
        let mut pet = PetState::new("Gizmo");
        let repo = RepositoryId::new(7);
        let failing = TestSummary::new(TestFramework::CargoTest, 3, 1, 0, Duration::from_secs(1));
        let happiness_before = pet.stats().happiness();
        let first_failure = pet.handle_test_run(repo, "gremlin", failing).unwrap();
        assert!(pet.stats().happiness() < happiness_before);
        assert!(matches!(
            first_failure.first(),
            Some(CoreEvent::TestRunReceived {
                xp_gained: 0,
                feedback_allowed: true,
                ..
            })
        ));

        let happiness_after_failure = pet.stats().happiness();
        let repeated_failure = pet.handle_test_run(repo, "gremlin", failing).unwrap();
        assert_eq!(pet.stats().happiness(), happiness_after_failure);
        assert!(matches!(
            repeated_failure.first(),
            Some(CoreEvent::TestRunReceived {
                feedback_allowed: false,
                ..
            })
        ));

        let green = TestSummary::new(TestFramework::CargoTest, 4, 0, 0, Duration::from_secs(1));
        let fixed = pet.handle_test_run(repo, "gremlin", green).unwrap();
        assert!(matches!(
            fixed.first(),
            Some(CoreEvent::TestRunReceived {
                xp_gained: 75,
                is_fixed: true,
                ..
            })
        ));
        let repeated_green = pet.handle_test_run(repo, "gremlin", green).unwrap();
        assert!(matches!(
            repeated_green.first(),
            Some(CoreEvent::TestRunReceived {
                xp_gained: 0,
                is_fixed: false,
                feedback_allowed: false,
                ..
            })
        ));
        assert_eq!(pet.progression().total_test_runs(), 4);
        assert_eq!(pet.progression().total_tests_failed(), 2);
    }

    #[test]
    fn test_empty_test_run_is_neutral_and_build_reward_is_cooled_down() {
        let mut pet = PetState::new("Gizmo");
        let repo = RepositoryId::new(2);
        let empty = TestSummary::new(TestFramework::GenericJunit, 0, 0, 5, Duration::ZERO);
        let events = pet.handle_test_run(repo, "gremlin", empty).unwrap();
        assert!(matches!(
            events.first(),
            Some(CoreEvent::TestRunReceived {
                xp_gained: 0,
                feedback_allowed: false,
                ..
            })
        ));

        let build = BuildSummary::new(BuildTool::Cargo, true, Duration::from_secs(1));
        let first = pet.handle_build_result(repo, "gremlin", build).unwrap();
        let second = pet.handle_build_result(repo, "gremlin", build).unwrap();
        assert!(matches!(
            first.first(),
            Some(CoreEvent::BuildCompleted { xp_gained: 15, .. })
        ));
        assert!(matches!(
            second.first(),
            Some(CoreEvent::BuildCompleted {
                xp_gained: 0,
                feedback_allowed: false,
                ..
            })
        ));
        assert_eq!(pet.progression().total_builds_succeeded(), 2);
    }

    #[test]
    fn test_focus_milestone_is_unique_and_unavailable_time_is_not_credited() {
        let mut config = CoreConfig::default();
        config.focus.milestone_secs = [60, 120, 180];
        config.focus.break_reminder_secs = 180;
        let mut pet = PetState::with_config("Gizmo", config);

        assert!(pet
            .track_focus(Duration::from_secs(5), ActivityState::Unavailable, true)
            .is_empty());
        assert_eq!(pet.progression().total_focus_secs(), 0);

        let mut milestones = 0;
        for _ in 0..13 {
            milestones += pet
                .track_focus(Duration::from_secs(5), ActivityState::Active, false)
                .iter()
                .filter(|event| matches!(event, CoreEvent::FocusMilestoneReached { .. }))
                .count();
        }
        assert_eq!(milestones, 1);
        assert_eq!(pet.progression().total_focus_secs(), 65);

        let events = pet.track_focus(
            Duration::from_secs(1),
            ActivityState::Idle(config.focus.idle_reset_threshold()),
            false,
        );
        assert!(events
            .iter()
            .any(|event| matches!(event, CoreEvent::IdleStateChanged { is_idle: true })));
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
