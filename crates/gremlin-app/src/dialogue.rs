//! Catalogue et timeline bornée des répliques du compagnon.

use gremlin_render::{BubbleRect, SpeechBubbleView};
use std::time::Duration;

const FADE_IN: Duration = Duration::from_millis(100);
const HOLD: Duration = Duration::from_millis(2_500);
const FADE_OUT: Duration = Duration::from_millis(250);
const FRAME_INTERVAL: Duration = Duration::from_millis(16);
const QUEUE_CAPACITY: usize = 4;

/// Réplique courte compatible avec la grille 5×7 du canevas 64×64.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogueId {
    Commit,
    LevelUp,
    Evolution,
    Fed,
    Petted,
    Healed,
    Sleeping,
    WokeUp,
    Hungry,
    Tired,
    Sick,
    Angry,
    Died,
    Revived,
    Dragged,
}

impl DialogueId {
    pub(super) const fn text(self) -> &'static str {
        match self {
            Self::Commit => "Bon commit",
            Self::LevelUp => "Niveau +1",
            Self::Evolution => "Évolution",
            Self::Fed => "Merci !",
            Self::Petted => "<3 Merci",
            Self::Healed => "Ça va mieux",
            Self::Sleeping => "Au repos",
            Self::WokeUp => "Me revoilà",
            Self::Hungry => "J'ai faim",
            Self::Tired => "Une pause ?",
            Self::Sick => "Besoin aide",
            Self::Angry => "Grrr...",
            Self::Died => "Oh non...",
            Self::Revived => "De retour",
            Self::Dragged => "Waaaah !",
        }
    }

    const fn priority(self) -> u8 {
        match self {
            Self::Evolution => 100,
            Self::LevelUp => 90,
            Self::Died | Self::Revived => 80,
            Self::Healed => 70,
            Self::Fed | Self::Petted => 60,
            Self::Commit => 50,
            Self::Sick | Self::Hungry | Self::Tired | Self::Angry => 40,
            Self::Sleeping | Self::WokeUp => 30,
            Self::Dragged => 20,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct ActiveDialogue {
    id: DialogueId,
    elapsed: Duration,
}

/// Une réplique active et une file fixe, sans allocation à l'exécution.
#[derive(Debug, Clone)]
pub struct DialogueEngine {
    active: Option<ActiveDialogue>,
    queue: [Option<DialogueId>; QUEUE_CAPACITY],
}

impl Default for DialogueEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl DialogueEngine {
    pub(super) const fn new() -> Self {
        Self {
            active: None,
            queue: [None; QUEUE_CAPACITY],
        }
    }

    /// Affiche ou met en file une réplique en respectant sa priorité.
    pub(super) fn push(&mut self, id: DialogueId) -> bool {
        let Some(active) = self.active else {
            self.active = Some(ActiveDialogue {
                id,
                elapsed: Duration::ZERO,
            });
            return true;
        };

        if active.id == id || self.queue.contains(&Some(id)) {
            return false;
        }

        if id.priority() > active.id.priority() {
            self.active = Some(ActiveDialogue {
                id,
                elapsed: Duration::ZERO,
            });
            return true;
        }

        if let Some(slot) = self.queue.iter_mut().find(|slot| slot.is_none()) {
            *slot = Some(id);
            return false;
        }

        let replacement = self
            .queue
            .iter()
            .enumerate()
            .filter_map(|(index, queued)| queued.map(|queued| (index, queued.priority())))
            .min_by_key(|(_, priority)| *priority);
        if let Some((index, priority)) = replacement {
            if id.priority() > priority {
                self.queue[index] = Some(id);
            }
        }
        false
    }

    /// Avance la timeline et renvoie `true` uniquement si l'image a changé.
    pub(super) fn update(&mut self, delta: Duration) -> bool {
        if delta.is_zero() {
            return false;
        }
        let before_id = self.active.map(|active| active.id);
        let before_opacity = self.opacity();

        if let Some(mut active) = self.active {
            active.elapsed = active.elapsed.saturating_add(delta);
            if active.elapsed >= total_duration() {
                self.active = self.take_next().map(|id| ActiveDialogue {
                    id,
                    elapsed: Duration::ZERO,
                });
            } else {
                self.active = Some(active);
            }
        }

        before_id != self.active.map(|active| active.id) || before_opacity != self.opacity()
    }

    /// Vue prête à être donnée au renderer sans clone de texte.
    pub(super) fn view(&self, head_anchor: (i32, i32)) -> Option<SpeechBubbleView<'static>> {
        let active = self.active?;
        Some(SpeechBubbleView {
            text: active.id.text(),
            opacity: self.opacity(),
            bounds: BubbleRect::companion_default(),
            target_anchor: head_anchor,
        })
    }

    pub(super) fn next_wake_delay(&self) -> Option<Duration> {
        let active = self.active?;
        let elapsed = active.elapsed;
        if elapsed < FADE_IN {
            return Some(FADE_IN.saturating_sub(elapsed).min(FRAME_INTERVAL));
        }
        let fade_out_start = FADE_IN.saturating_add(HOLD);
        if elapsed < fade_out_start {
            return Some(fade_out_start.saturating_sub(elapsed));
        }
        Some(total_duration().saturating_sub(elapsed).min(FRAME_INTERVAL))
    }

    #[cfg(test)]
    pub(super) fn active_id(&self) -> Option<DialogueId> {
        self.active.map(|active| active.id)
    }

    fn opacity(&self) -> u8 {
        let Some(active) = self.active else {
            return 0;
        };
        let elapsed = active.elapsed;
        if elapsed < FADE_IN {
            return ratio_to_u8(elapsed, FADE_IN);
        }
        let fade_out_start = FADE_IN.saturating_add(HOLD);
        if elapsed < fade_out_start {
            return 255;
        }
        let fade_elapsed = elapsed.saturating_sub(fade_out_start);
        255u8.saturating_sub(ratio_to_u8(fade_elapsed, FADE_OUT))
    }

    fn take_next(&mut self) -> Option<DialogueId> {
        let next_index = self
            .queue
            .iter()
            .enumerate()
            .filter_map(|(index, queued)| queued.map(|queued| (index, queued.priority())))
            .max_by_key(|(_, priority)| *priority)
            .map(|(index, _)| index)?;
        self.queue[next_index].take()
    }
}

const fn total_duration() -> Duration {
    FADE_IN.saturating_add(HOLD).saturating_add(FADE_OUT)
}

fn ratio_to_u8(value: Duration, total: Duration) -> u8 {
    if total.is_zero() {
        return 255;
    }
    let numerator = value.as_nanos().saturating_mul(255);
    (numerator / total.as_nanos()).min(255) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_prioritaire_interrompt_le_message_courant() {
        let mut engine = DialogueEngine::new();
        assert!(engine.push(DialogueId::Commit));
        assert!(engine.push(DialogueId::LevelUp));
        assert_eq!(engine.active_id(), Some(DialogueId::LevelUp));
    }

    #[test]
    fn test_doublon_est_ignore() {
        let mut engine = DialogueEngine::new();
        engine.push(DialogueId::Sleeping);
        assert!(!engine.push(DialogueId::Sleeping));
        assert_eq!(engine.queue.iter().flatten().count(), 0);
    }

    #[test]
    fn test_plateau_nentraine_pas_de_redessin_continu() {
        let mut engine = DialogueEngine::new();
        engine.push(DialogueId::Commit);
        assert!(engine.update(FADE_IN));
        assert_eq!(engine.opacity(), 255);
        assert!(!engine.update(Duration::from_secs(1)));
        assert_eq!(engine.opacity(), 255);
    }

    #[test]
    fn test_message_en_file_prend_le_relais() {
        let mut engine = DialogueEngine::new();
        engine.push(DialogueId::Evolution);
        engine.push(DialogueId::Commit);
        assert!(engine.update(total_duration()));
        assert_eq!(engine.active_id(), Some(DialogueId::Commit));
    }

    #[test]
    fn test_vue_emprunte_un_texte_statique() {
        let mut engine = DialogueEngine::new();
        engine.push(DialogueId::Healed);
        let Some(view) = engine.view((30, 18)) else {
            panic!("bulle attendue");
        };
        assert_eq!(view.text, "Ça va mieux");
        assert_eq!(view.target_anchor, (30, 18));
    }
}
