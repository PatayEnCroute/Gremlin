//! Suivi pur et borné des sessions de focus estimées.

use crate::config::FocusConfig;
use std::time::Duration;

/// État d'activité injecté par l'orchestrateur.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivityState {
    /// Une interaction système récente est connue.
    Active,
    /// L'utilisateur est inactif depuis la durée indiquée.
    Idle(Duration),
    /// La plateforme ne fournit aucune mesure fiable.
    Unavailable,
}

/// Résultat compact d'un pas de suivi de focus.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct FocusUpdate {
    pub(crate) credited: Duration,
    pub(crate) milestones: [bool; 3],
    pub(crate) break_recommended: bool,
    pub(crate) idle_changed: Option<bool>,
}

/// État transitoire d'une session de focus.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct FocusTracker {
    armed: bool,
    elapsed: Duration,
    milestones_emitted: [bool; 3],
    break_emitted: bool,
    idle: bool,
}

impl FocusTracker {
    pub(crate) fn reset(&mut self) {
        *self = Self::default();
    }

    pub(crate) fn track(
        &mut self,
        elapsed: Duration,
        activity: ActivityState,
        development_seen: bool,
        config: &FocusConfig,
    ) -> FocusUpdate {
        if development_seen {
            self.armed = true;
        }

        match activity {
            ActivityState::Unavailable => FocusUpdate::default(),
            ActivityState::Idle(idle_for) => self.track_idle(idle_for, config),
            ActivityState::Active => self.track_active(elapsed, config),
        }
    }

    fn track_idle(&mut self, idle_for: Duration, config: &FocusConfig) -> FocusUpdate {
        if idle_for < config.idle_reset_threshold() || self.idle {
            return FocusUpdate::default();
        }

        self.armed = false;
        self.elapsed = Duration::ZERO;
        self.milestones_emitted = [false; 3];
        self.break_emitted = false;
        self.idle = true;
        FocusUpdate {
            idle_changed: Some(true),
            ..FocusUpdate::default()
        }
    }

    fn track_active(&mut self, elapsed: Duration, config: &FocusConfig) -> FocusUpdate {
        let idle_changed = self.idle.then_some(false);
        self.idle = false;

        if !self.armed || elapsed.is_zero() || elapsed > config.max_sample_duration() {
            return FocusUpdate {
                idle_changed,
                ..FocusUpdate::default()
            };
        }

        self.elapsed = self.elapsed.saturating_add(elapsed);
        let mut milestones = [false; 3];
        for (index, threshold) in config.milestone_durations().into_iter().enumerate() {
            if !self.milestones_emitted[index] && self.elapsed >= threshold {
                self.milestones_emitted[index] = true;
                milestones[index] = true;
            }
        }

        let break_recommended =
            !self.break_emitted && self.elapsed >= config.break_reminder_duration();
        if break_recommended {
            self.break_emitted = true;
        }

        FocusUpdate {
            credited: elapsed,
            milestones,
            break_recommended,
            idle_changed,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_focus_requires_development_signal() {
        let mut tracker = FocusTracker::default();
        let config = FocusConfig::default();
        let update = tracker.track(
            Duration::from_secs(1),
            ActivityState::Active,
            false,
            &config,
        );
        assert_eq!(update.credited, Duration::ZERO);
    }

    #[test]
    fn test_idle_resets_and_active_reports_return() {
        let mut tracker = FocusTracker::default();
        let config = FocusConfig::default();
        tracker.track(Duration::from_secs(1), ActivityState::Active, true, &config);
        let idle = tracker.track(
            Duration::from_secs(1),
            ActivityState::Idle(config.idle_reset_threshold()),
            false,
            &config,
        );
        assert_eq!(idle.idle_changed, Some(true));

        let active = tracker.track(
            Duration::from_secs(1),
            ActivityState::Active,
            false,
            &config,
        );
        assert_eq!(active.idle_changed, Some(false));
        assert_eq!(active.credited, Duration::ZERO);
    }
}
