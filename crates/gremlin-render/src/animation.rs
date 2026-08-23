//! Système de gestion des animations de sprites 2D.
//!
//! Permet de cadencer et d'interpoler les frames de sprites selon des timelines configurables
//! avec différents modes de lecture (`Loop`, `Once`, `PingPong`).

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;
use tracing::warn;

/// Mode de lecture d'une animation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum PlayMode {
    /// Répétition infinie de l'animation en boucle (0 -> 1 -> 2 -> 0 ...).
    #[default]
    Loop,
    /// Lecture unique de l'animation qui reste figée sur la dernière frame.
    Once,
    /// Lecture en va-et-vient (0 -> 1 -> 2 -> 1 -> 0 ...).
    PingPong,
}

/// Description d'une frame individuelle au sein d'une animation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnimationFrame {
    /// Clé du sprite dans le `SpriteAtlas`.
    pub sprite_key: String,
    /// Durée d'affichage de cette frame.
    pub duration: Duration,
}

impl AnimationFrame {
    /// Crée une nouvelle frame avec une clé et une durée.
    #[must_use]
    pub fn new(sprite_key: impl Into<String>, duration: Duration) -> Self {
        Self {
            sprite_key: sprite_key.into(),
            duration,
        }
    }
}

/// Séquence ordonnée de frames composant une animation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpriteAnimation {
    /// Identifiant de l'animation (ex: "idle", "happy", "sleep").
    pub name: String,
    /// Séquence des frames.
    pub frames: Vec<AnimationFrame>,
    /// Mode de répétition.
    pub mode: PlayMode,
}

impl SpriteAnimation {
    /// Crée une animation avec des durées de frames personnalisées.
    #[must_use]
    pub fn new(name: impl Into<String>, frames: Vec<AnimationFrame>, mode: PlayMode) -> Self {
        Self {
            name: name.into(),
            frames,
            mode,
        }
    }

    /// Crée une animation où toutes les frames ont la même durée d'affichage.
    #[must_use]
    pub fn uniform<S: AsRef<str>>(
        name: impl Into<String>,
        sprite_keys: &[S],
        frame_duration: Duration,
        mode: PlayMode,
    ) -> Self {
        let frames = sprite_keys
            .iter()
            .map(|k| AnimationFrame::new(k.as_ref(), frame_duration))
            .collect();

        Self {
            name: name.into(),
            frames,
            mode,
        }
    }

    /// Durée totale d'un cycle complet de l'animation.
    #[must_use]
    pub fn total_duration(&self) -> Duration {
        self.frames.iter().map(|f| f.duration).sum()
    }
}

/// Contrôleur d'animation gérant l'état temporel, le changement de frame et la détection de dirty state.
#[derive(Debug, Default, Clone)]
pub struct AnimationController {
    animations: HashMap<String, SpriteAnimation>,
    current_animation: Option<String>,
    current_frame_index: usize,
    elapsed_in_frame: Duration,
    is_finished: bool,
    is_reverse: bool,
}

impl AnimationController {
    /// Crée un nouveau contrôleur d'animation vide.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Enregistre une animation dans le contrôleur.
    ///
    /// Si l'animation remplace celle en cours de lecture (cas typique du rechargement
    /// à chaud d'un skin), l'état de lecture est réinitialisé : sans cela l'index de
    /// frame courant pourrait pointer au-delà de la nouvelle séquence.
    pub fn register(&mut self, animation: SpriteAnimation) {
        let replaces_current = self.current_animation.as_deref() == Some(animation.name.as_str());
        self.animations.insert(animation.name.clone(), animation);

        if replaces_current {
            self.reset_playback_state();
        }
    }

    /// Réinitialise l'état temporel de lecture sans changer l'animation active.
    fn reset_playback_state(&mut self) {
        self.current_frame_index = 0;
        self.elapsed_in_frame = Duration::ZERO;
        self.is_finished = false;
        self.is_reverse = false;
    }

    /// Déclenche la lecture d'une animation par son nom.
    ///
    /// Si `restart_if_same` est `false` et que l'animation est déjà en cours de lecture,
    /// la lecture continue sans interruption.
    pub fn play(&mut self, name: &str, restart_if_same: bool) {
        if !restart_if_same && self.current_animation.as_deref() == Some(name) && !self.is_finished
        {
            return;
        }

        if self.animations.contains_key(name) {
            self.current_animation = Some(name.to_string());
            self.reset_playback_state();
        }
    }

    /// Met à jour l'animation selon le temps écoulé.
    ///
    /// Renvoie `true` si la frame active a changé (indiquant qu'un redessin est requis).
    ///
    /// # Robustesse
    /// Le rattrapage temporel est **structurellement borné** : au plus deux cycles
    /// complets de frames sont consommés par appel, et une frame de durée nulle
    /// (manifest hostile ou état construit à la main) interrompt la boucle au lieu
    /// de la rendre infinie. L'index de frame est lu défensivement : un index périmé
    /// (animation remplacée à chaud par une séquence plus courte) est corrigé au lieu
    /// de provoquer une panique d'indexation.
    pub fn update(&mut self, delta: Duration) -> bool {
        let Some(anim_name) = self.current_animation.as_deref() else {
            return false;
        };

        let Some(animation) = self.animations.get(anim_name) else {
            return false;
        };

        let frame_count = animation.frames.len();
        if frame_count == 0 || self.is_finished {
            return false;
        }

        // Index périmé après un `register` de séquence plus courte : on se recale.
        if self.current_frame_index >= frame_count {
            warn!(
                animation = %anim_name,
                index = self.current_frame_index,
                frame_count,
                "Index de frame périmé : recalage sur la première frame"
            );
            self.current_frame_index = 0;
            self.elapsed_in_frame = Duration::ZERO;
            return true;
        }

        self.elapsed_in_frame += delta;

        // Borne dure du rattrapage : deux cycles suffisent à absorber n'importe quel
        // delta (au-delà, le reliquat est abandonné) et garantissent la terminaison.
        let max_steps = frame_count.saturating_mul(2);
        let mode = animation.mode;
        let mut frame_changed = false;
        let mut steps = 0usize;

        loop {
            let Some(frame) = animation.frames.get(self.current_frame_index) else {
                self.current_frame_index = 0;
                self.elapsed_in_frame = Duration::ZERO;
                return true;
            };
            let current_frame_duration = frame.duration;

            if current_frame_duration.is_zero() {
                warn!(
                    animation = %anim_name,
                    index = self.current_frame_index,
                    "Frame de durée nulle : cadencement impossible, rattrapage interrompu"
                );
                self.elapsed_in_frame = Duration::ZERO;
                break;
            }

            if self.elapsed_in_frame < current_frame_duration {
                break;
            }

            if steps >= max_steps {
                // Delta démesuré : on abandonne le reliquat plutôt que d'itérer
                // proportionnellement au temps écoulé.
                self.elapsed_in_frame = Duration::ZERO;
                break;
            }
            steps += 1;

            self.elapsed_in_frame -= current_frame_duration;
            frame_changed = true;

            match mode {
                PlayMode::Loop => {
                    self.current_frame_index = (self.current_frame_index + 1) % frame_count;
                }
                PlayMode::Once => {
                    if self.current_frame_index + 1 < frame_count {
                        self.current_frame_index += 1;
                    } else {
                        self.is_finished = true;
                        self.elapsed_in_frame = Duration::ZERO;
                        break;
                    }
                }
                PlayMode::PingPong => {
                    if frame_count <= 1 {
                        self.current_frame_index = 0;
                    } else if self.is_reverse {
                        if self.current_frame_index == 0 {
                            self.is_reverse = false;
                            self.current_frame_index = 1;
                        } else {
                            self.current_frame_index -= 1;
                        }
                    } else if self.current_frame_index + 1 >= frame_count {
                        self.is_reverse = true;
                        self.current_frame_index = frame_count - 2;
                    } else {
                        self.current_frame_index += 1;
                    }
                }
            }
        }

        frame_changed
    }

    /// Récupère la clé de texture correspondant à la frame courante.
    #[must_use]
    pub fn current_frame_key(&self) -> Option<&str> {
        let anim_name = self.current_animation.as_ref()?;
        let animation = self.animations.get(anim_name)?;
        animation
            .frames
            .get(self.current_frame_index)
            .map(|f| f.sprite_key.as_str())
    }

    /// Renvoie le temps restant avant la prochaine frame.
    ///
    /// Utilisé par l'Event Loop pour calculer le `ControlFlow::WaitUntil` optimal.
    #[must_use]
    pub fn time_until_next_frame(&self) -> Option<Duration> {
        let anim_name = self.current_animation.as_ref()?;
        let animation = self.animations.get(anim_name)?;
        if self.is_finished || animation.frames.is_empty() {
            return None;
        }

        let frame = animation.frames.get(self.current_frame_index)?;
        Some(frame.duration.saturating_sub(self.elapsed_in_frame))
    }

    /// Nom de l'animation en cours de lecture.
    #[must_use]
    pub fn current_animation_name(&self) -> Option<&str> {
        self.current_animation.as_deref()
    }

    /// Indique si une animation est actuellement en cours et non terminée.
    #[must_use]
    pub fn is_playing(&self) -> bool {
        self.current_animation.is_some() && !self.is_finished
    }

    /// Indique si l'animation active est terminée (mode `Once`).
    #[must_use]
    pub const fn is_finished(&self) -> bool {
        self.is_finished
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_uniform_animation_loop() {
        let mut controller = AnimationController::new();
        let anim = SpriteAnimation::uniform(
            "idle",
            &["idle_0", "idle_1", "idle_2"],
            Duration::from_millis(100),
            PlayMode::Loop,
        );
        controller.register(anim);
        controller.play("idle", false);

        assert_eq!(controller.current_frame_key(), Some("idle_0"));
        assert_eq!(
            controller.time_until_next_frame(),
            Some(Duration::from_millis(100))
        );

        // Avance de 50ms -> pas de changement de frame
        let changed = controller.update(Duration::from_millis(50));
        assert!(!changed);
        assert_eq!(controller.current_frame_key(), Some("idle_0"));
        assert_eq!(
            controller.time_until_next_frame(),
            Some(Duration::from_millis(50))
        );

        // Avance de 60ms supplémentaires (total 110ms) -> passage à idle_1
        let changed = controller.update(Duration::from_millis(60));
        assert!(changed);
        assert_eq!(controller.current_frame_key(), Some("idle_1"));

        // Avance de 200ms -> saute à idle_0 (boucle)
        controller.update(Duration::from_millis(200));
        assert_eq!(controller.current_frame_key(), Some("idle_0"));
    }

    #[test]
    fn test_animation_once_mode() {
        let mut controller = AnimationController::new();
        let anim = SpriteAnimation::uniform(
            "eat",
            &["eat_0", "eat_1"],
            Duration::from_millis(100),
            PlayMode::Once,
        );
        controller.register(anim);
        controller.play("eat", false);

        assert_eq!(controller.current_frame_key(), Some("eat_0"));
        controller.update(Duration::from_millis(100));
        assert_eq!(controller.current_frame_key(), Some("eat_1"));
        assert!(!controller.is_finished());

        controller.update(Duration::from_millis(100));
        assert_eq!(controller.current_frame_key(), Some("eat_1"));
        assert!(controller.is_finished());
        assert_eq!(controller.time_until_next_frame(), None);
    }

    #[test]
    fn test_animation_ping_pong_mode() {
        let mut controller = AnimationController::new();
        let anim = SpriteAnimation::uniform(
            "dance",
            &["dance_0", "dance_1", "dance_2"],
            Duration::from_millis(100),
            PlayMode::PingPong,
        );
        controller.register(anim);
        controller.play("dance", false);

        assert_eq!(controller.current_frame_key(), Some("dance_0"));
        controller.update(Duration::from_millis(100));
        assert_eq!(controller.current_frame_key(), Some("dance_1"));
        controller.update(Duration::from_millis(100));
        assert_eq!(controller.current_frame_key(), Some("dance_2"));
        // Rebond vers l'arrière
        controller.update(Duration::from_millis(100));
        assert_eq!(controller.current_frame_key(), Some("dance_1"));
        controller.update(Duration::from_millis(100));
        assert_eq!(controller.current_frame_key(), Some("dance_0"));
        // Rebond vers l'avant
        controller.update(Duration::from_millis(100));
        assert_eq!(controller.current_frame_key(), Some("dance_1"));
    }

    // ---------------------------------------------------------------------
    // Robustesse face aux données non fiables et au rechargement à chaud
    // ---------------------------------------------------------------------

    #[test]
    fn test_duree_nulle_en_boucle_ne_bloque_pas() {
        // Régression : une frame de durée nulle rendait la boucle de rattrapage
        // non convergente (déni de service via manifest de skin).
        let mut controller = AnimationController::new();
        controller.register(SpriteAnimation::uniform(
            "idle",
            &["a", "b"],
            Duration::ZERO,
            PlayMode::Loop,
        ));
        controller.play("idle", true);

        controller.update(Duration::from_secs(1));
        assert_eq!(controller.current_frame_key(), Some("a"));
    }

    #[test]
    fn test_duree_nulle_en_pingpong_ne_bloque_pas() {
        let mut controller = AnimationController::new();
        controller.register(SpriteAnimation::uniform(
            "dance",
            &["a", "b", "c"],
            Duration::ZERO,
            PlayMode::PingPong,
        ));
        controller.play("dance", true);

        controller.update(Duration::from_secs(86_400));
        assert!(controller.current_frame_key().is_some());
    }

    #[test]
    fn test_rattrapage_borne_sur_delta_demesure() {
        let mut controller = AnimationController::new();
        controller.register(SpriteAnimation::uniform(
            "idle",
            &["a", "b", "c"],
            Duration::from_millis(1),
            PlayMode::Loop,
        ));
        controller.play("idle", true);

        // 10 ans de retard : doit rendre la main immédiatement et repartir propre.
        assert!(controller.update(Duration::from_secs(315_360_000)));
        assert!(controller.current_frame_key().is_some());
        assert_eq!(
            controller.time_until_next_frame(),
            Some(Duration::from_millis(1)),
            "le reliquat de rattrapage doit être abandonné"
        );
    }

    #[test]
    fn test_register_reinitialise_l_etat_de_l_animation_courante() {
        // Régression : remplacer à chaud l'animation en cours par une séquence plus
        // courte laissait un index périmé qui faisait paniquer `update`.
        let mut controller = AnimationController::new();
        controller.register(SpriteAnimation::uniform(
            "idle",
            &["a", "b", "c", "d"],
            Duration::from_millis(100),
            PlayMode::Loop,
        ));
        controller.play("idle", true);
        controller.update(Duration::from_millis(300));
        assert_eq!(controller.current_frame_key(), Some("d"));

        // Rechargement à chaud : la nouvelle séquence n'a qu'une seule frame.
        controller.register(SpriteAnimation::uniform(
            "idle",
            &["x"],
            Duration::from_millis(100),
            PlayMode::Loop,
        ));
        assert_eq!(controller.current_frame_key(), Some("x"));
        controller.update(Duration::from_millis(100));
        assert_eq!(controller.current_frame_key(), Some("x"));
        assert!(controller.time_until_next_frame().is_some());
    }

    #[test]
    fn test_register_ne_perturbe_pas_une_autre_animation() {
        let mut controller = AnimationController::new();
        controller.register(SpriteAnimation::uniform(
            "idle",
            &["a", "b"],
            Duration::from_millis(100),
            PlayMode::Loop,
        ));
        controller.play("idle", true);
        controller.update(Duration::from_millis(100));
        assert_eq!(controller.current_frame_key(), Some("b"));

        controller.register(SpriteAnimation::uniform(
            "happy",
            &["h"],
            Duration::from_millis(100),
            PlayMode::Loop,
        ));
        assert_eq!(controller.current_frame_key(), Some("b"));
    }

    #[test]
    fn test_animation_sans_frame_est_inerte() {
        let mut controller = AnimationController::new();
        controller.register(SpriteAnimation::new("vide", Vec::new(), PlayMode::Loop));
        controller.play("vide", true);

        assert!(!controller.update(Duration::from_secs(10)));
        assert_eq!(controller.current_frame_key(), None);
        assert_eq!(controller.time_until_next_frame(), None);
    }
}
