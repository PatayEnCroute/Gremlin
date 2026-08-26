//! Adaptation centralisée des événements métier vers les effets visuels.

use crate::dialogue::{DialogueEngine, DialogueId};
use gremlin_core::{ConsumableKind, CoreEvent, PetMood, PomodoroPhase, WellbeingReminderKind};
use gremlin_render::{
    ParticleEngine, ParticlePreset, PixelBuffer, SpeechBubbleView, TransitionController,
};
use std::time::Duration;

const DEFAULT_HEAD_ANCHOR: (i32, i32) = (32, 20);
const DEFAULT_EFFECT_ANCHOR: (i32, i32) = (32, 30);
const SLEEP_EMISSION_INTERVAL: Duration = Duration::from_millis(900);
const CRITICAL_EMISSION_INTERVAL: Duration = Duration::from_millis(1_200);

/// Points sémantiques fournis par le skin actif.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VisualAnchors {
    pub(super) head: (i32, i32),
    pub(super) effect: (i32, i32),
}

impl Default for VisualAnchors {
    fn default() -> Self {
        Self {
            head: DEFAULT_HEAD_ANCHOR,
            effect: DEFAULT_EFFECT_ANCHOR,
        }
    }
}

/// Commande visuelle qui ne relève pas du domaine métier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VisualCue {
    DragStarted,
}

/// Résultat compact de l'absorption d'un lot d'événements.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FeedbackOutcome {
    pub(super) dirty: bool,
    pub(super) mood_changed: bool,
}

#[derive(Debug, Clone, Copy)]
struct Reaction {
    priority: u8,
    dialogue: DialogueId,
    particles: Option<ParticlePreset>,
}

/// État borné des effets visuels pilotés par l'application.
#[derive(Debug, Clone)]
pub struct VisualFeedback {
    particles: ParticleEngine,
    dialogue: DialogueEngine,
    transition: TransitionController,
    ambient_preset: Option<ParticlePreset>,
    ambient_remaining: Duration,
    anchors: VisualAnchors,
}

impl Default for VisualFeedback {
    fn default() -> Self {
        Self::new()
    }
}

impl VisualFeedback {
    pub(super) fn new() -> Self {
        Self {
            particles: ParticleEngine::new(),
            dialogue: DialogueEngine::new(),
            transition: TransitionController::default(),
            ambient_preset: None,
            ambient_remaining: Duration::ZERO,
            anchors: VisualAnchors::default(),
        }
    }

    pub(super) fn set_anchors(&mut self, anchors: VisualAnchors) {
        self.anchors = anchors;
    }

    /// Absorbe tout le lot avant de déclencher une seule réaction de premier plan.
    pub(super) fn handle_core_events(&mut self, events: &[CoreEvent]) -> FeedbackOutcome {
        let mut selected: Option<Reaction> = None;
        let mut mood_changed = false;

        for event in events {
            match event {
                CoreEvent::MoodChanged { to, .. } => {
                    mood_changed = true;
                    self.set_ambient_for_mood(*to);
                }
                CoreEvent::FellAsleep => self.set_ambient_for_mood(PetMood::Sleeping),
                CoreEvent::WokeUp => self.set_ambient_for_mood(PetMood::Happy),
                CoreEvent::Died => self.set_ambient_for_mood(PetMood::Dead),
                CoreEvent::CommitReceived { .. }
                | CoreEvent::TestRunReceived { .. }
                | CoreEvent::BuildCompleted { .. }
                | CoreEvent::FocusMilestoneReached { .. }
                | CoreEvent::BreakRecommended { .. }
                | CoreEvent::IdleStateChanged { .. }
                | CoreEvent::LevelUp { .. }
                | CoreEvent::EvolutionUnlocked { .. }
                | CoreEvent::StatsDecayed { .. }
                | CoreEvent::Fed { .. }
                | CoreEvent::Petted { .. }
                | CoreEvent::Healed { .. }
                | CoreEvent::Rested { .. }
                | CoreEvent::Revived
                | CoreEvent::StreakChanged { .. }
                | CoreEvent::StreakRewardUnlocked { .. }
                | CoreEvent::ConsumableGranted { .. }
                | CoreEvent::ConsumableUsed { .. }
                | CoreEvent::PomodoroStarted { .. }
                | CoreEvent::PomodoroPaused { .. }
                | CoreEvent::PomodoroResumed { .. }
                | CoreEvent::PomodoroPhaseCompleted { .. }
                | CoreEvent::PomodoroStopped { .. }
                | CoreEvent::WellbeingReminder { .. } => {}
            }
            if let Some(reaction) = reaction_for_event(event) {
                select_reaction(&mut selected, reaction);
            }
        }

        let mut dirty = mood_changed;
        if let Some(reaction) = selected {
            dirty |= self.dialogue.push(reaction.dialogue);
            if let Some(preset) = reaction.particles {
                dirty |= self.particles.emit(preset, self.anchors.effect) > 0;
            }
        }

        FeedbackOutcome {
            dirty,
            mood_changed,
        }
    }

    pub(super) fn handle_cue(&mut self, cue: VisualCue) -> bool {
        match cue {
            VisualCue::DragStarted => self.dialogue.push(DialogueId::Dragged),
        }
    }

    /// Avance les timelines et les émissions ambiantes.
    pub(super) fn update(&mut self, delta: Duration) -> bool {
        let mut dirty = self.particles.update(delta);
        dirty |= self.dialogue.update(delta);
        dirty |= self.transition.update(delta);

        if let Some(preset) = self.ambient_preset {
            if delta >= self.ambient_remaining {
                let origin = if preset == ParticlePreset::RisingZ {
                    (self.anchors.head.0.saturating_add(8), self.anchors.head.1)
                } else {
                    self.anchors.effect
                };
                dirty |= self.particles.emit(preset, origin) > 0;
                self.ambient_remaining = ambient_interval(preset);
            } else {
                self.ambient_remaining = self.ambient_remaining.saturating_sub(delta);
            }
        }
        dirty
    }

    pub(super) fn next_wake_delay(&self) -> Option<Duration> {
        [
            self.particles.next_wake_delay(),
            self.dialogue.next_wake_delay(),
            self.transition.next_wake_delay(),
            self.ambient_preset.map(|_| self.ambient_remaining),
        ]
        .into_iter()
        .flatten()
        .min()
    }

    pub(super) fn render_particles(&self, buffer: &mut PixelBuffer) {
        self.particles.render(buffer);
    }

    pub(super) fn dialogue_view(&self) -> Option<SpeechBubbleView<'static>> {
        self.dialogue.view(self.anchors.head)
    }

    pub(super) const fn transition(&self) -> &TransitionController {
        &self.transition
    }

    pub(super) fn start_transition(&mut self) {
        self.transition.start();
    }

    pub(super) fn cancel_transition(&mut self) {
        self.transition.cancel();
    }

    #[cfg(test)]
    pub(super) const fn active_particle_count(&self) -> usize {
        self.particles.active_count()
    }

    #[cfg(test)]
    pub(super) fn active_dialogue(&self) -> Option<DialogueId> {
        self.dialogue.active_id()
    }

    #[cfg(test)]
    pub(super) const fn anchors(&self) -> VisualAnchors {
        self.anchors
    }

    fn set_ambient_for_mood(&mut self, mood: PetMood) {
        let preset = match mood {
            PetMood::Sleeping => Some(ParticlePreset::RisingZ),
            PetMood::Hungry | PetMood::Tired | PetMood::Sick => Some(ParticlePreset::FallingDrop),
            PetMood::Dead | PetMood::Happy | PetMood::Coding | PetMood::Angry => None,
        };
        if self.ambient_preset != preset {
            self.ambient_preset = preset;
            self.ambient_remaining = Duration::ZERO;
        }
    }
}

fn select_reaction(selected: &mut Option<Reaction>, candidate: Reaction) {
    if selected.is_none_or(|current| candidate.priority > current.priority) {
        *selected = Some(candidate);
    }
}

// Correspondance exhaustive événement -> réaction : la longueur vient du
// nombre d'événements du domaine, pas d'une logique enchevêtrée. La découper
// éparpillerait la table de priorités, qui se lit d'un seul tenant.
#[allow(clippy::too_many_lines)]
fn reaction_for_event(event: &CoreEvent) -> Option<Reaction> {
    let reaction = match event {
        CoreEvent::CommitReceived { .. } => Reaction {
            priority: 50,
            dialogue: DialogueId::Commit,
            particles: Some(ParticlePreset::SparkBurst),
        },
        CoreEvent::TestRunReceived { .. } => return test_reaction(event),
        CoreEvent::BuildCompleted {
            summary,
            feedback_allowed,
            ..
        } => return build_reaction(*summary, *feedback_allowed),
        CoreEvent::FocusMilestoneReached { .. } => Reaction {
            priority: 20,
            dialogue: DialogueId::FocusMilestone,
            particles: Some(ParticlePreset::SparkBurst),
        },
        CoreEvent::BreakRecommended { .. } => Reaction {
            priority: 20,
            dialogue: DialogueId::BreakReminder,
            particles: None,
        },
        CoreEvent::IdleStateChanged { is_idle: false } => Reaction {
            priority: 30,
            dialogue: DialogueId::Returned,
            particles: Some(ParticlePreset::FloatingHearts),
        },
        CoreEvent::IdleStateChanged { is_idle: true } | CoreEvent::StatsDecayed { .. } => {
            return None;
        }
        CoreEvent::LevelUp { .. } => Reaction {
            priority: 90,
            dialogue: DialogueId::LevelUp,
            particles: Some(ParticlePreset::ConfettiBurst),
        },
        CoreEvent::EvolutionUnlocked { .. } => Reaction {
            priority: 100,
            dialogue: DialogueId::Evolution,
            particles: Some(ParticlePreset::ConfettiBurst),
        },
        CoreEvent::Fed { .. } => Reaction {
            priority: 60,
            dialogue: DialogueId::Fed,
            particles: Some(ParticlePreset::FloatingHearts),
        },
        CoreEvent::Petted { .. } => Reaction {
            priority: 60,
            dialogue: DialogueId::Petted,
            particles: Some(ParticlePreset::FloatingHearts),
        },
        CoreEvent::Healed { .. } | CoreEvent::Rested { .. } => Reaction {
            priority: 70,
            dialogue: DialogueId::Healed,
            particles: Some(ParticlePreset::FloatingHearts),
        },
        CoreEvent::FellAsleep => Reaction {
            priority: 30,
            dialogue: DialogueId::Sleeping,
            particles: None,
        },
        CoreEvent::WokeUp => Reaction {
            priority: 30,
            dialogue: DialogueId::WokeUp,
            particles: None,
        },
        CoreEvent::Died => Reaction {
            priority: 80,
            dialogue: DialogueId::Died,
            particles: None,
        },
        // --- Phase 8 ---
        // Un déblocage de cosmétique se célèbre, mais reste sous l'évolution et
        // le décès : le système de priorités existant s'en charge.
        CoreEvent::StreakRewardUnlocked { .. } => Reaction {
            priority: 75,
            dialogue: DialogueId::StreakReward,
            particles: Some(ParticlePreset::ConfettiBurst),
        },
        CoreEvent::ConsumableUsed { kind, .. } => Reaction {
            priority: 60,
            dialogue: match kind {
                ConsumableKind::Coffee => DialogueId::Coffee,
                ConsumableKind::DebugPotion => DialogueId::DebugPotion,
                ConsumableKind::Snack => DialogueId::Snack,
            },
            particles: Some(match kind {
                // Le café réveille : des étincelles, pas des cœurs.
                ConsumableKind::Coffee => ParticlePreset::SparkBurst,
                ConsumableKind::DebugPotion | ConsumableKind::Snack => {
                    ParticlePreset::FloatingHearts
                }
            }),
        },
        CoreEvent::PomodoroStarted { phase, .. } | CoreEvent::PomodoroResumed { phase, .. } => {
            Reaction {
                priority: 20,
                dialogue: if *phase == PomodoroPhase::Work {
                    DialogueId::FocusStarted
                } else {
                    DialogueId::BreakDone
                },
                particles: None,
            }
        }
        CoreEvent::PomodoroPhaseCompleted { phase, .. } => Reaction {
            priority: 20,
            dialogue: if *phase == PomodoroPhase::Work {
                DialogueId::FocusDone
            } else {
                DialogueId::BreakDone
            },
            particles: None,
        },
        CoreEvent::WellbeingReminder { kind } => Reaction {
            priority: 20,
            dialogue: match kind {
                WellbeingReminderKind::Stretch => DialogueId::Stretch,
                WellbeingReminderKind::Hydration => DialogueId::Hydrate,
            },
            particles: None,
        },
        // Une série qui progresse se remarque une seule fois, à l'arrivée d'un
        // nouveau jour : les valeurs répétées sont filtrées par le core.
        CoreEvent::StreakChanged { current_days, .. } if *current_days > 0 => Reaction {
            priority: 20,
            dialogue: DialogueId::StreakKept,
            particles: None,
        },
        // Aucun retour visuel : un octroi silencieux, une pause de minuteur, un
        // arrêt volontaire ou une série retombée à zéro n'appellent pas de bulle.
        CoreEvent::StreakChanged { .. }
        | CoreEvent::ConsumableGranted { .. }
        | CoreEvent::PomodoroPaused { .. }
        | CoreEvent::PomodoroStopped { .. } => return None,
        CoreEvent::Revived => Reaction {
            priority: 80,
            dialogue: DialogueId::Revived,
            particles: Some(ParticlePreset::FloatingHearts),
        },
        CoreEvent::MoodChanged { to, .. } => Reaction {
            priority: 40,
            dialogue: dialogue_for_mood(*to)?,
            particles: None,
        },
    };
    Some(reaction)
}

fn test_reaction(event: &CoreEvent) -> Option<Reaction> {
    let CoreEvent::TestRunReceived {
        summary,
        is_fixed,
        feedback_allowed,
        ..
    } = event
    else {
        return None;
    };
    if !feedback_allowed {
        return None;
    }
    if *is_fixed {
        return Some(Reaction {
            priority: 55,
            dialogue: DialogueId::TestsFixed,
            particles: Some(ParticlePreset::ConfettiBurst),
        });
    }
    Some(Reaction {
        priority: 50,
        dialogue: if summary.is_all_passed() {
            DialogueId::TestsPassed
        } else {
            DialogueId::TestsFailed
        },
        particles: Some(if summary.is_all_passed() {
            ParticlePreset::SparkBurst
        } else {
            ParticlePreset::FallingDrop
        }),
    })
}

const fn build_reaction(
    summary: gremlin_core::BuildSummary,
    feedback_allowed: bool,
) -> Option<Reaction> {
    if !feedback_allowed {
        return None;
    }
    Some(Reaction {
        priority: 50,
        dialogue: if summary.success() {
            DialogueId::BuildPassed
        } else {
            DialogueId::BuildFailed
        },
        particles: Some(if summary.success() {
            ParticlePreset::SparkBurst
        } else {
            ParticlePreset::FallingDrop
        }),
    })
}

const fn dialogue_for_mood(mood: PetMood) -> Option<DialogueId> {
    match mood {
        PetMood::Sleeping => Some(DialogueId::Sleeping),
        PetMood::Hungry => Some(DialogueId::Hungry),
        PetMood::Tired => Some(DialogueId::Tired),
        PetMood::Sick => Some(DialogueId::Sick),
        PetMood::Angry => Some(DialogueId::Angry),
        PetMood::Dead | PetMood::Happy | PetMood::Coding => None,
    }
}

const fn ambient_interval(preset: ParticlePreset) -> Duration {
    match preset {
        ParticlePreset::RisingZ => SLEEP_EMISSION_INTERVAL,
        ParticlePreset::FallingDrop
        | ParticlePreset::SparkBurst
        | ParticlePreset::ConfettiBurst
        | ParticlePreset::FloatingHearts => CRITICAL_EMISSION_INTERVAL,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gremlin_core::{EvolutionStage, TestFramework, TestSummary};

    #[test]
    fn test_lot_compose_selectionne_evolution() {
        let mut feedback = VisualFeedback::new();
        let outcome = feedback.handle_core_events(&[
            CoreEvent::CommitReceived {
                repo: "gremlin".into(),
                branch: "main".into(),
                xp_gained: 10,
            },
            CoreEvent::LevelUp {
                new_level: 2,
                total_xp: 100,
            },
            CoreEvent::EvolutionUnlocked {
                new_stage: EvolutionStage::Teen,
            },
        ]);
        assert!(outcome.dirty);
        assert_eq!(feedback.active_dialogue(), Some(DialogueId::Evolution));
        assert!(feedback.active_particle_count() > 0);
    }

    #[test]
    fn test_evenements_sommeil_sont_dedoublonnes() {
        let mut feedback = VisualFeedback::new();
        feedback.handle_core_events(&[
            CoreEvent::MoodChanged {
                from: PetMood::Happy,
                to: PetMood::Sleeping,
            },
            CoreEvent::FellAsleep,
        ]);
        assert_eq!(feedback.active_dialogue(), Some(DialogueId::Sleeping));
        feedback.update(Duration::from_millis(1));
        assert!(feedback.active_particle_count() > 0);
    }

    #[test]
    fn test_sortie_humeur_critique_arrete_emission_ambiante() {
        let mut feedback = VisualFeedback::new();
        feedback.handle_core_events(&[CoreEvent::MoodChanged {
            from: PetMood::Happy,
            to: PetMood::Hungry,
        }]);
        feedback.update(Duration::from_millis(1));
        let emitted = feedback.active_particle_count();
        assert!(emitted > 0);

        feedback.handle_core_events(&[CoreEvent::MoodChanged {
            from: PetMood::Hungry,
            to: PetMood::Happy,
        }]);
        feedback.update(CRITICAL_EMISSION_INTERVAL);
        assert!(feedback.active_particle_count() <= emitted);
    }

    #[test]
    fn test_feedback_interdit_ne_declenche_ni_bulle_ni_particule() {
        let mut feedback = VisualFeedback::new();
        feedback.handle_core_events(&[CoreEvent::TestRunReceived {
            repo: "gremlin".into(),
            summary: TestSummary::new(TestFramework::CargoTest, 10, 0, 0, Duration::from_secs(1)),
            xp_gained: 0,
            is_fixed: false,
            feedback_allowed: false,
        }]);
        assert_eq!(feedback.active_dialogue(), None);
        assert_eq!(feedback.active_particle_count(), 0);
    }

    #[test]
    fn test_evolution_prime_sur_un_result_de_tests() {
        let mut feedback = VisualFeedback::new();
        feedback.handle_core_events(&[
            CoreEvent::TestRunReceived {
                repo: "gremlin".into(),
                summary: TestSummary::new(
                    TestFramework::CargoTest,
                    10,
                    0,
                    0,
                    Duration::from_secs(1),
                ),
                xp_gained: 25,
                is_fixed: false,
                feedback_allowed: true,
            },
            CoreEvent::EvolutionUnlocked {
                new_stage: EvolutionStage::Teen,
            },
        ]);
        assert_eq!(feedback.active_dialogue(), Some(DialogueId::Evolution));
    }
}
