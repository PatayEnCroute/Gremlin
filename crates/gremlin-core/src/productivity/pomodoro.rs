//! Minuteur de concentration, machine à états pure.
//!
//! Le minuteur ne lit aucune horloge : il reçoit des `Duration` mesurées par
//! l'orchestrateur et n'avance que sur du temps réellement vécu par le
//! processus. Trois conséquences, toutes voulues :
//!
//! * le rattrapage hors-ligne de [`PetState::tick`](crate::state::PetState::tick)
//!   ne le fait jamais progresser — Gremlin ne prétend pas qu'un processus
//!   arrêté a mesuré une session de travail ;
//! * une suspension de la machine met le minuteur en pause au lieu de sauter
//!   des phases ;
//! * un état `Running` rechargé depuis une sauvegarde devient
//!   `Paused(Restarted)` : le temps restant est préservé, la mesure ne l'est
//!   pas.
//!
//! Le minuteur n'accorde ni XP, ni objet, ni jour de série : il favorise la
//! santé, pas l'accumulation.

use crate::config::PomodoroConfig;
use crate::error::CoreError;
use crate::events::CoreEvent;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::time::Duration;

/// Nombre maximal de blocs de travail comptabilisés.
///
/// Un compteur lu depuis le disque ne doit pas pouvoir déborder ni afficher une
/// valeur absurde ; au-delà, le compteur sature.
const MAX_COMPLETED_WORK_BLOCKS: u16 = 9_999;

/// Nombre de millisecondes par seconde, nommé pour éviter la constante nue.
const MILLIS_PER_SEC: u32 = 1_000;

/// Phase courante du cycle de concentration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PomodoroPhase {
    /// Bloc de travail concentré.
    Work,
    /// Pause courte entre deux blocs.
    ShortBreak,
    /// Pause longue après plusieurs blocs.
    LongBreak,
}

impl PomodoroPhase {
    /// Durée nominale de la phase selon la configuration.
    #[must_use]
    pub const fn duration_secs(self, config: &PomodoroConfig) -> u32 {
        match self {
            Self::Work => config.work_secs,
            Self::ShortBreak => config.short_break_secs,
            Self::LongBreak => config.long_break_secs,
        }
    }

    /// Indique que la phase est une pause et non un bloc de travail.
    #[must_use]
    pub const fn is_break(self) -> bool {
        matches!(self, Self::ShortBreak | Self::LongBreak)
    }

    /// Libellé lisible de la phase.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Work => "concentration",
            Self::ShortBreak => "pause courte",
            Self::LongBreak => "pause longue",
        }
    }
}

impl fmt::Display for PomodoroPhase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// Raison pour laquelle le minuteur a été suspendu.
///
/// L'interface doit pouvoir dire *pourquoi* le compte à rebours s'est arrêté :
/// une pause volontaire et une suspension de la machine n'appellent pas le même
/// message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PauseReason {
    /// Pause demandée explicitement par l'utilisateur.
    User,
    /// Fin de phase : la phase suivante attend un démarrage volontaire.
    PhaseBoundary,
    /// Écart de temps live trop grand : la machine a probablement été suspendue.
    SystemSuspended,
    /// Session rechargée depuis une sauvegarde ; la mesure n'a pas eu lieu.
    Restarted,
    /// Fonctionnalité désactivée dans les réglages.
    FeatureDisabled,
    /// Le familier a été endormi volontairement.
    PetAsleep,
}

impl PauseReason {
    /// Libellé lisible de la raison de pause.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::User => "en pause",
            Self::PhaseBoundary => "phase suivante en attente",
            Self::SystemSuspended => "machine suspendue",
            Self::Restarted => "reprise après redémarrage",
            Self::FeatureDisabled => "minuteur désactivé",
            Self::PetAsleep => "Gremlin dort",
        }
    }
}

impl fmt::Display for PauseReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// Nature du rappel de bien-être proposé en fin de bloc de travail.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum WellbeingReminderKind {
    /// Se lever et s'étirer.
    Stretch,
    /// Boire un verre d'eau.
    Hydration,
}

impl WellbeingReminderKind {
    /// Libellé lisible du rappel.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Stretch => "étirement",
            Self::Hydration => "hydratation",
        }
    }
}

impl fmt::Display for WellbeingReminderKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// Session en cours : phase, temps restant et blocs déjà accomplis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PomodoroSession {
    /// Phase courante.
    phase: PomodoroPhase,
    /// Temps restant en millisecondes.
    ///
    /// Le stockage en millisecondes évite la dérive : avancer de 200 ms sur un
    /// compteur en secondes entières ne retrancherait jamais rien.
    remaining_millis: u32,
    /// Blocs de travail entièrement accomplis depuis le démarrage.
    completed_work_blocks: u16,
}

impl PomodoroSession {
    /// Phase courante de la session.
    #[must_use]
    pub const fn phase(self) -> PomodoroPhase {
        self.phase
    }

    /// Temps restant avant la fin de la phase.
    #[must_use]
    pub const fn remaining(self) -> Duration {
        Duration::from_millis(self.remaining_millis as u64)
    }

    /// Blocs de travail entièrement accomplis.
    #[must_use]
    pub const fn completed_work_blocks(self) -> u16 {
        self.completed_work_blocks
    }

    /// Crée une session neuve démarrant au début de `phase`.
    fn fresh(phase: PomodoroPhase, completed_work_blocks: u16, config: &PomodoroConfig) -> Self {
        Self {
            phase,
            remaining_millis: phase.duration_secs(config).saturating_mul(MILLIS_PER_SEC),
            completed_work_blocks,
        }
    }
}

/// État du minuteur.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum PomodoroState {
    /// Aucun cycle en cours.
    #[default]
    Idle,
    /// Compte à rebours actif.
    Running(PomodoroSession),
    /// Compte à rebours suspendu ; le temps restant est conservé.
    Paused(PomodoroSession, PauseReason),
}

impl PomodoroState {
    /// Nom court de l'état, utilisé dans les messages d'erreur.
    const fn name(self) -> &'static str {
        match self {
            Self::Idle => "arrêté",
            Self::Running(_) => "en cours",
            Self::Paused(_, _) => "en pause",
        }
    }

    /// Session portée par l'état, s'il y en a une.
    #[must_use]
    pub const fn session(self) -> Option<PomodoroSession> {
        match self {
            Self::Idle => None,
            Self::Running(session) | Self::Paused(session, _) => Some(session),
        }
    }
}

/// Minuteur de concentration complet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct PomodoroTimer {
    /// État courant de la machine.
    state: PomodoroState,
    /// Compteur d'alternance des rappels de bien-être.
    ///
    /// Il rend la séquence étirement / hydratation déterministe et reproductible
    /// en test, sans tirage aléatoire.
    reminder_index: u16,
}

impl PomodoroTimer {
    /// État courant.
    #[must_use]
    pub const fn state(&self) -> PomodoroState {
        self.state
    }

    /// Phase courante, si un cycle est engagé.
    #[must_use]
    pub const fn phase(&self) -> Option<PomodoroPhase> {
        match self.state.session() {
            Some(session) => Some(session.phase),
            None => None,
        }
    }

    /// Temps restant dans la phase courante, si un cycle est engagé.
    #[must_use]
    pub const fn remaining(&self) -> Option<Duration> {
        match self.state.session() {
            Some(session) => Some(session.remaining()),
            None => None,
        }
    }

    /// Blocs de travail entièrement accomplis.
    #[must_use]
    pub const fn completed_work_blocks(&self) -> u16 {
        match self.state.session() {
            Some(session) => session.completed_work_blocks,
            None => 0,
        }
    }

    /// Indique que le compte à rebours avance réellement.
    #[must_use]
    pub const fn is_running(&self) -> bool {
        matches!(self.state, PomodoroState::Running(_))
    }

    /// Indique qu'un bloc de travail est en cours de mesure.
    ///
    /// C'est cette condition — et non la simple activation de la fonctionnalité
    /// — qui autorise la posture studieuse du familier.
    #[must_use]
    pub const fn is_work_in_progress(&self) -> bool {
        matches!(
            self.state,
            PomodoroState::Running(PomodoroSession {
                phase: PomodoroPhase::Work,
                ..
            })
        )
    }

    /// Raison de la pause courante, s'il y en a une.
    #[must_use]
    pub const fn pause_reason(&self) -> Option<PauseReason> {
        match self.state {
            PomodoroState::Paused(_, reason) => Some(reason),
            _ => None,
        }
    }

    /// Démarre un premier bloc de travail.
    ///
    /// # Errors
    /// Renvoie [`CoreError::InvalidPomodoroTransition`] si un cycle est déjà
    /// engagé : reprendre une session suspendue passe par [`Self::resume`], ce
    /// qui évite d'effacer silencieusement le temps restant.
    pub fn start(&mut self, config: &PomodoroConfig) -> Result<Vec<CoreEvent>, CoreError> {
        if !matches!(self.state, PomodoroState::Idle) {
            return Err(CoreError::InvalidPomodoroTransition {
                attempted: "démarrer",
                state: self.state.name(),
            });
        }

        let session = PomodoroSession::fresh(PomodoroPhase::Work, 0, config);
        self.state = PomodoroState::Running(session);
        Ok(vec![CoreEvent::PomodoroStarted {
            phase: session.phase,
            remaining: session.remaining(),
        }])
    }

    /// Suspend le compte à rebours.
    ///
    /// Suspendre un minuteur déjà en pause est idempotent et n'émet rien, sauf
    /// si la raison change — auquel cas l'interface doit être informée.
    ///
    /// # Errors
    /// Renvoie [`CoreError::InvalidPomodoroTransition`] si aucun cycle n'est
    /// engagé.
    pub fn pause(&mut self, reason: PauseReason) -> Result<Vec<CoreEvent>, CoreError> {
        match self.state {
            PomodoroState::Idle => Err(CoreError::InvalidPomodoroTransition {
                attempted: "mettre en pause",
                state: self.state.name(),
            }),
            PomodoroState::Paused(session, current) => {
                if current == reason {
                    return Ok(Vec::new());
                }
                self.state = PomodoroState::Paused(session, reason);
                Ok(vec![CoreEvent::PomodoroPaused {
                    phase: session.phase,
                    remaining: session.remaining(),
                    reason,
                }])
            }
            PomodoroState::Running(session) => {
                self.state = PomodoroState::Paused(session, reason);
                Ok(vec![CoreEvent::PomodoroPaused {
                    phase: session.phase,
                    remaining: session.remaining(),
                    reason,
                }])
            }
        }
    }

    /// Reprend un compte à rebours suspendu.
    ///
    /// # Errors
    /// Renvoie [`CoreError::InvalidPomodoroTransition`] si le minuteur n'est pas
    /// en pause : une reprise n'est jamais implicite.
    pub fn resume(&mut self) -> Result<Vec<CoreEvent>, CoreError> {
        match self.state {
            PomodoroState::Paused(session, _) => {
                self.state = PomodoroState::Running(session);
                Ok(vec![CoreEvent::PomodoroResumed {
                    phase: session.phase,
                    remaining: session.remaining(),
                }])
            }
            _ => Err(CoreError::InvalidPomodoroTransition {
                attempted: "reprendre",
                state: self.state.name(),
            }),
        }
    }

    /// Arrête le cycle et remet le minuteur au repos.
    ///
    /// Idempotent : arrêter un minuteur déjà arrêté n'émet rien.
    pub fn stop(&mut self) -> Vec<CoreEvent> {
        let Some(session) = self.state.session() else {
            return Vec::new();
        };
        self.state = PomodoroState::Idle;
        vec![CoreEvent::PomodoroStopped {
            phase: session.phase,
            completed_work_blocks: session.completed_work_blocks,
        }]
    }

    /// Passe une pause en cours et prépare le bloc de travail suivant.
    ///
    /// Le saut est **réservé aux pauses**. Sauter un bloc de travail
    /// enregistrerait un bloc jamais accompli ; c'est refusé.
    ///
    /// # Errors
    /// Renvoie [`CoreError::InvalidPomodoroTransition`] si aucun cycle n'est
    /// engagé ou si la phase courante est un bloc de travail.
    pub fn skip_break(&mut self, config: &PomodoroConfig) -> Result<Vec<CoreEvent>, CoreError> {
        let Some(session) = self.state.session() else {
            return Err(CoreError::InvalidPomodoroTransition {
                attempted: "passer la pause",
                state: self.state.name(),
            });
        };
        if !session.phase.is_break() {
            return Err(CoreError::InvalidPomodoroTransition {
                attempted: "passer la pause",
                state: "bloc de travail en cours",
            });
        }

        let next =
            PomodoroSession::fresh(PomodoroPhase::Work, session.completed_work_blocks, config);
        self.state = PomodoroState::Paused(next, PauseReason::PhaseBoundary);
        Ok(vec![CoreEvent::PomodoroPaused {
            phase: next.phase,
            remaining: next.remaining(),
            reason: PauseReason::PhaseBoundary,
        }])
    }

    /// Fait avancer le compte à rebours du temps réellement écoulé.
    ///
    /// L'appel est sans effet si le minuteur n'est pas en cours. Un écart
    /// supérieur au pas live configuré est interprété comme une suspension de
    /// la machine : le minuteur passe en pause au lieu de rattraper.
    ///
    /// Une seule frontière de phase est franchie par appel, et le reliquat au-delà
    /// de la frontière est abandonné. Un `Duration` hostile ne peut donc ni
    /// enchaîner des phases, ni faire boucler l'appel.
    pub fn advance(&mut self, elapsed: Duration, config: &PomodoroConfig) -> Vec<CoreEvent> {
        let PomodoroState::Running(mut session) = self.state else {
            return Vec::new();
        };

        if elapsed.as_secs() > u64::from(config.max_live_step_secs) {
            self.state = PomodoroState::Paused(session, PauseReason::SystemSuspended);
            return vec![CoreEvent::PomodoroPaused {
                phase: session.phase,
                remaining: session.remaining(),
                reason: PauseReason::SystemSuspended,
            }];
        }

        // `elapsed` est déjà borné par le test ci-dessus : la conversion en
        // millisecondes tient dans un `u64` puis dans un `u32`.
        let elapsed_millis = u32::try_from(elapsed.as_millis()).unwrap_or(u32::MAX);
        if elapsed_millis < session.remaining_millis {
            session.remaining_millis -= elapsed_millis;
            self.state = PomodoroState::Running(session);
            return Vec::new();
        }

        self.complete_phase(session, config)
    }

    /// Termine la phase courante et prépare la suivante, en attente de démarrage.
    fn complete_phase(
        &mut self,
        session: PomodoroSession,
        config: &PomodoroConfig,
    ) -> Vec<CoreEvent> {
        let mut events = Vec::new();
        let completed_phase = session.phase;

        let (blocks, next_phase) = if completed_phase == PomodoroPhase::Work {
            let blocks = session
                .completed_work_blocks
                .saturating_add(1)
                .min(MAX_COMPLETED_WORK_BLOCKS);
            let long_break_due = u16::from(config.blocks_before_long_break) > 0
                && blocks.is_multiple_of(u16::from(config.blocks_before_long_break));
            (
                blocks,
                if long_break_due {
                    PomodoroPhase::LongBreak
                } else {
                    PomodoroPhase::ShortBreak
                },
            )
        } else {
            (session.completed_work_blocks, PomodoroPhase::Work)
        };

        events.push(CoreEvent::PomodoroPhaseCompleted {
            phase: completed_phase,
            completed_work_blocks: blocks,
        });

        if completed_phase == PomodoroPhase::Work {
            let kind = if self.reminder_index.is_multiple_of(2) {
                WellbeingReminderKind::Stretch
            } else {
                WellbeingReminderKind::Hydration
            };
            self.reminder_index = self.reminder_index.wrapping_add(1);
            events.push(CoreEvent::WellbeingReminder { kind });
        }

        let next = PomodoroSession::fresh(next_phase, blocks, config);
        self.state = PomodoroState::Paused(next, PauseReason::PhaseBoundary);
        events.push(CoreEvent::PomodoroPaused {
            phase: next.phase,
            remaining: next.remaining(),
            reason: PauseReason::PhaseBoundary,
        });

        events
    }

    /// Transforme une session `Running` rechargée en session suspendue.
    ///
    /// Appelée après désérialisation uniquement : un processus arrêté n'a
    /// mesuré aucun temps, et prétendre le contraire fabriquerait des blocs de
    /// travail jamais réalisés.
    pub fn mark_restarted(&mut self) {
        if let PomodoroState::Running(session) = self.state {
            self.state = PomodoroState::Paused(session, PauseReason::Restarted);
        }
    }

    /// Répare un minuteur désérialisé.
    ///
    /// Idempotente : un temps restant supérieur à la phase est ramené à sa
    /// durée, et un temps restant nul — qui bloquerait le compte à rebours à
    /// zéro — est réarmé sur la phase complète.
    pub fn normalize(&mut self, config: &PomodoroConfig) {
        let repaired = match self.state {
            PomodoroState::Idle => PomodoroState::Idle,
            PomodoroState::Running(session) => {
                PomodoroState::Running(Self::normalized_session(session, config))
            }
            PomodoroState::Paused(session, reason) => {
                PomodoroState::Paused(Self::normalized_session(session, config), reason)
            }
        };
        self.state = repaired;
    }

    fn normalized_session(
        mut session: PomodoroSession,
        config: &PomodoroConfig,
    ) -> PomodoroSession {
        let full_millis = session
            .phase
            .duration_secs(config)
            .saturating_mul(MILLIS_PER_SEC);
        if session.remaining_millis == 0 || session.remaining_millis > full_millis {
            session.remaining_millis = full_millis;
        }
        session.completed_work_blocks =
            session.completed_work_blocks.min(MAX_COMPLETED_WORK_BLOCKS);
        session
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    fn short_config() -> PomodoroConfig {
        let mut config = PomodoroConfig {
            work_secs: 60,
            short_break_secs: 30,
            long_break_secs: 45,
            blocks_before_long_break: 4,
            max_live_step_secs: 120,
        };
        config.normalize();
        config
    }

    /// Fait tourner le minuteur jusqu'à la fin de la phase courante.
    fn run_to_phase_end(timer: &mut PomodoroTimer, config: &PomodoroConfig) -> Vec<CoreEvent> {
        let remaining = timer.remaining().unwrap_or_default();
        timer.advance(remaining, config)
    }

    #[test]
    fn test_a_fresh_timer_is_idle_and_measures_nothing() {
        let timer = PomodoroTimer::default();
        assert_eq!(timer.state(), PomodoroState::Idle);
        assert!(timer.phase().is_none());
        assert!(timer.remaining().is_none());
        assert!(!timer.is_running());
        assert!(!timer.is_work_in_progress());
        assert_eq!(timer.completed_work_blocks(), 0);
    }

    #[test]
    fn test_start_begins_a_work_block() {
        let config = short_config();
        let mut timer = PomodoroTimer::default();

        let events = timer.start(&config).unwrap();
        assert_eq!(
            events,
            vec![CoreEvent::PomodoroStarted {
                phase: PomodoroPhase::Work,
                remaining: Duration::from_secs(60),
            }]
        );
        assert!(timer.is_work_in_progress());
    }

    #[test]
    fn test_starting_twice_is_refused_instead_of_resetting() {
        let config = short_config();
        let mut timer = PomodoroTimer::default();
        timer.start(&config).unwrap();
        timer.advance(Duration::from_secs(30), &config);

        assert!(timer.start(&config).is_err());
        assert_eq!(timer.remaining(), Some(Duration::from_secs(30)));
    }

    #[test]
    fn test_pause_resume_preserves_the_remaining_time() {
        let config = short_config();
        let mut timer = PomodoroTimer::default();
        timer.start(&config).unwrap();
        timer.advance(Duration::from_secs(20), &config);

        let paused = timer.pause(PauseReason::User).unwrap();
        assert_eq!(
            paused,
            vec![CoreEvent::PomodoroPaused {
                phase: PomodoroPhase::Work,
                remaining: Duration::from_secs(40),
                reason: PauseReason::User,
            }]
        );

        // Une pause n'avance pas.
        assert!(timer.advance(Duration::from_secs(10), &config).is_empty());
        assert_eq!(timer.remaining(), Some(Duration::from_secs(40)));

        let resumed = timer.resume().unwrap();
        assert_eq!(
            resumed,
            vec![CoreEvent::PomodoroResumed {
                phase: PomodoroPhase::Work,
                remaining: Duration::from_secs(40),
            }]
        );
    }

    #[test]
    fn test_pausing_twice_with_the_same_reason_is_silent() {
        let config = short_config();
        let mut timer = PomodoroTimer::default();
        timer.start(&config).unwrap();

        assert_eq!(timer.pause(PauseReason::User).unwrap().len(), 1);
        assert!(timer.pause(PauseReason::User).unwrap().is_empty());
        // Un changement de raison reste annoncé.
        assert_eq!(timer.pause(PauseReason::PetAsleep).unwrap().len(), 1);
    }

    #[test]
    fn test_invalid_transitions_are_refused() {
        let config = short_config();
        let mut timer = PomodoroTimer::default();

        assert!(timer.pause(PauseReason::User).is_err());
        assert!(timer.resume().is_err());
        assert!(timer.skip_break(&config).is_err());
        assert!(timer.stop().is_empty());

        timer.start(&config).unwrap();
        assert!(timer.resume().is_err());
        assert!(
            timer.skip_break(&config).is_err(),
            "un bloc de travail ne se saute pas"
        );
    }

    #[test]
    fn test_work_completion_offers_a_break_without_starting_it() {
        let config = short_config();
        let mut timer = PomodoroTimer::default();
        timer.start(&config).unwrap();

        let events = run_to_phase_end(&mut timer, &config);
        assert_eq!(
            events,
            vec![
                CoreEvent::PomodoroPhaseCompleted {
                    phase: PomodoroPhase::Work,
                    completed_work_blocks: 1,
                },
                CoreEvent::WellbeingReminder {
                    kind: WellbeingReminderKind::Stretch,
                },
                CoreEvent::PomodoroPaused {
                    phase: PomodoroPhase::ShortBreak,
                    remaining: Duration::from_secs(30),
                    reason: PauseReason::PhaseBoundary,
                },
            ]
        );
        assert!(!timer.is_running(), "la pause ne démarre pas toute seule");
        assert_eq!(timer.completed_work_blocks(), 1);
    }

    #[test]
    fn test_long_break_arrives_on_the_fourth_block() {
        let config = short_config();
        let mut timer = PomodoroTimer::default();
        timer.start(&config).unwrap();

        let mut phases = Vec::new();
        for _ in 0..4 {
            run_to_phase_end(&mut timer, &config);
            phases.push(timer.phase().unwrap_or(PomodoroPhase::Work));
            timer.resume().unwrap();
            run_to_phase_end(&mut timer, &config);
            timer.resume().unwrap();
        }

        assert_eq!(
            phases,
            vec![
                PomodoroPhase::ShortBreak,
                PomodoroPhase::ShortBreak,
                PomodoroPhase::ShortBreak,
                PomodoroPhase::LongBreak,
            ]
        );
    }

    #[test]
    fn test_reminders_alternate_deterministically() {
        let config = short_config();
        let mut timer = PomodoroTimer::default();
        timer.start(&config).unwrap();

        let mut reminders = Vec::new();
        for _ in 0..4 {
            for event in run_to_phase_end(&mut timer, &config) {
                if let CoreEvent::WellbeingReminder { kind } = event {
                    reminders.push(kind);
                }
            }
            timer.resume().unwrap();
            run_to_phase_end(&mut timer, &config);
            timer.resume().unwrap();
        }

        assert_eq!(
            reminders,
            vec![
                WellbeingReminderKind::Stretch,
                WellbeingReminderKind::Hydration,
                WellbeingReminderKind::Stretch,
                WellbeingReminderKind::Hydration,
            ]
        );
    }

    #[test]
    fn test_exact_boundary_completes_the_phase_once() {
        let config = short_config();
        let mut timer = PomodoroTimer::default();
        timer.start(&config).unwrap();

        let events = timer.advance(Duration::from_secs(60), &config);
        assert_eq!(events.len(), 3);
        // Un second appel sur un minuteur en pause n'enchaîne pas.
        assert!(timer.advance(Duration::from_secs(60), &config).is_empty());
    }

    #[test]
    fn test_small_remainder_is_discarded_at_the_boundary() {
        let config = short_config();
        let mut timer = PomodoroTimer::default();
        timer.start(&config).unwrap();
        timer.advance(Duration::from_millis(59_500), &config);

        // 500 ms restent ; un pas d'une seconde franchit la frontière sans
        // entamer la phase suivante.
        timer.advance(Duration::from_secs(1), &config);
        assert_eq!(timer.phase(), Some(PomodoroPhase::ShortBreak));
        assert_eq!(timer.remaining(), Some(Duration::from_secs(30)));
    }

    #[test]
    fn test_sub_second_steps_do_not_drift() {
        let config = short_config();
        let mut timer = PomodoroTimer::default();
        timer.start(&config).unwrap();

        for _ in 0..100 {
            timer.advance(Duration::from_millis(100), &config);
        }
        assert_eq!(timer.remaining(), Some(Duration::from_secs(50)));
    }

    #[test]
    fn test_zero_delta_changes_nothing() {
        let config = short_config();
        let mut timer = PomodoroTimer::default();
        timer.start(&config).unwrap();

        assert!(timer.advance(Duration::ZERO, &config).is_empty());
        assert_eq!(timer.remaining(), Some(Duration::from_secs(60)));
    }

    #[test]
    fn test_huge_delta_suspends_instead_of_catching_up() {
        let config = short_config();
        let mut timer = PomodoroTimer::default();
        timer.start(&config).unwrap();

        let events = timer.advance(Duration::from_secs(86_400), &config);
        assert_eq!(
            events,
            vec![CoreEvent::PomodoroPaused {
                phase: PomodoroPhase::Work,
                remaining: Duration::from_secs(60),
                reason: PauseReason::SystemSuspended,
            }]
        );
        assert_eq!(timer.completed_work_blocks(), 0, "aucun bloc fabriqué");
    }

    #[test]
    fn test_absurd_delta_does_not_panic() {
        let config = short_config();
        let mut timer = PomodoroTimer::default();
        timer.start(&config).unwrap();
        let events = timer.advance(Duration::MAX, &config);
        assert_eq!(events.len(), 1);
        assert_eq!(timer.pause_reason(), Some(PauseReason::SystemSuspended));
    }

    #[test]
    fn test_skip_break_prepares_work_without_counting_a_block() {
        let config = short_config();
        let mut timer = PomodoroTimer::default();
        timer.start(&config).unwrap();
        run_to_phase_end(&mut timer, &config);

        let events = timer.skip_break(&config).unwrap();
        assert_eq!(
            events,
            vec![CoreEvent::PomodoroPaused {
                phase: PomodoroPhase::Work,
                remaining: Duration::from_secs(60),
                reason: PauseReason::PhaseBoundary,
            }]
        );
        assert_eq!(timer.completed_work_blocks(), 1);
    }

    #[test]
    fn test_stop_returns_to_idle_from_any_state() {
        let config = short_config();
        let mut timer = PomodoroTimer::default();
        timer.start(&config).unwrap();
        timer.advance(Duration::from_secs(10), &config);

        let events = timer.stop();
        assert_eq!(
            events,
            vec![CoreEvent::PomodoroStopped {
                phase: PomodoroPhase::Work,
                completed_work_blocks: 0,
            }]
        );
        assert_eq!(timer.state(), PomodoroState::Idle);
        assert!(timer.stop().is_empty(), "arrêt idempotent");
    }

    #[test]
    fn test_restart_suspends_a_running_session_without_losing_time() {
        let config = short_config();
        let mut timer = PomodoroTimer::default();
        timer.start(&config).unwrap();
        timer.advance(Duration::from_secs(25), &config);

        timer.mark_restarted();
        assert_eq!(timer.pause_reason(), Some(PauseReason::Restarted));
        assert_eq!(timer.remaining(), Some(Duration::from_secs(35)));
        assert!(!timer.is_running());

        // Idempotent : un second appel ne repasse pas la raison à Restarted.
        timer.pause(PauseReason::User).unwrap();
        timer.mark_restarted();
        assert_eq!(timer.pause_reason(), Some(PauseReason::User));
    }

    #[test]
    fn test_normalize_repairs_a_hand_edited_session_and_is_idempotent() {
        let config = short_config();
        let hostile = r#"{"state":{"Running":{"phase":"Work","remaining_millis":4294967295,"completed_work_blocks":65535}},"reminder_index":0}"#;
        let mut timer: PomodoroTimer = serde_json::from_str(hostile).unwrap();

        timer.normalize(&config);
        let once = timer;
        assert_eq!(timer.remaining(), Some(Duration::from_secs(60)));
        assert_eq!(timer.completed_work_blocks(), MAX_COMPLETED_WORK_BLOCKS);

        timer.normalize(&config);
        assert_eq!(timer, once, "normalisation non idempotente");
    }

    #[test]
    fn test_normalize_rearms_a_session_stuck_at_zero() {
        let config = short_config();
        let stuck = r#"{"state":{"Running":{"phase":"ShortBreak","remaining_millis":0,"completed_work_blocks":2}},"reminder_index":0}"#;
        let mut timer: PomodoroTimer = serde_json::from_str(stuck).unwrap();

        timer.normalize(&config);
        assert_eq!(timer.remaining(), Some(Duration::from_secs(30)));
    }

    #[test]
    fn test_roundtrip_preserves_the_paused_session() {
        let config = short_config();
        let mut timer = PomodoroTimer::default();
        timer.start(&config).unwrap();
        timer.advance(Duration::from_secs(15), &config);
        timer.pause(PauseReason::User).unwrap();

        let json = serde_json::to_string(&timer).unwrap();
        let restored: PomodoroTimer = serde_json::from_str(&json).unwrap();
        assert_eq!(restored, timer);
    }
}
