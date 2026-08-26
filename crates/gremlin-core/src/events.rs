//! Événements du domaine métier émis par le moteur du Gremlin.

use crate::mood::PetMood;
use crate::productivity::{
    ConsumableEffect, ConsumableKind, GrantReason, PauseReason, PomodoroPhase, StreakReward,
    WellbeingReminderKind,
};
use crate::progression::EvolutionStage;
use crate::stats::PetStats;
use crate::tooling::{BreakReason, BuildSummary, TestSummary};
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Événements de cycle de vie et d'état émis par `gremlin-core`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum CoreEvent {
    /// Un commit a été assimilé par le Gremlin.
    CommitReceived {
        repo: String,
        branch: String,
        xp_gained: u64,
    },
    /// Un rapport de tests terminé a été assimilé.
    TestRunReceived {
        repo: String,
        summary: TestSummary,
        xp_gained: u64,
        is_fixed: bool,
        feedback_allowed: bool,
    },
    /// Un résultat de build explicite a été assimilé.
    BuildCompleted {
        repo: String,
        summary: BuildSummary,
        xp_gained: u64,
        feedback_allowed: bool,
    },
    /// Un palier d'une session de focus estimée a été franchi.
    FocusMilestoneReached { duration: Duration, bonus_xp: u64 },
    /// Une pause discrète est recommandée.
    BreakRecommended { reason: BreakReason },
    /// L'état d'inactivité prolongée a changé.
    IdleStateChanged { is_idle: bool },
    /// L'humeur du Gremlin a changé suite à un tick ou une action.
    MoodChanged { from: PetMood, to: PetMood },
    /// Le Gremlin a gagné un ou plusieurs niveaux.
    LevelUp { new_level: u32, total_xp: u64 },
    /// Le Gremlin a atteint un nouveau stade d'évolution morphologique.
    EvolutionUnlocked { new_stage: EvolutionStage },
    /// Décroissance temporelle des statistiques appliquée.
    StatsDecayed { stats: PetStats },
    /// Le Gremlin a été nourri.
    Fed { amount: f32 },
    /// Le Gremlin a reçu une caresse ou interaction bienveillante.
    Petted { amount: f32 },
    /// Le Gremlin a été soigné.
    Healed { amount: f32 },
    /// Le Gremlin s'est reposé ou a regagné de l'énergie.
    Rested { amount: f32 },
    /// Le Gremlin a été mis en sommeil.
    FellAsleep,
    /// Le Gremlin a été réveillé.
    WokeUp,
    /// Le Gremlin est décédé par négligence prolongée.
    Died,
    /// Le Gremlin a été réanimé / ressuscité.
    Revived,

    // --- Phase 8 : productivité et bien-être ---
    /// La série de jours de commits visible a changé.
    StreakChanged {
        /// Série courante affichée, règle de grâce comprise.
        current_days: u16,
        /// Meilleure série jamais prouvée.
        longest_days: u16,
    },
    /// Un palier de série a débloqué un cosmétique, définitivement.
    StreakRewardUnlocked {
        /// Récompense acquise.
        reward: StreakReward,
        /// Nombre de jours qui l'a débloquée.
        required_days: u16,
    },
    /// Des consommables ont été ajoutés à l'inventaire.
    ConsumableGranted {
        /// Type d'objet octroyé.
        kind: ConsumableKind,
        /// Quantité réellement ajoutée, capacité déduite.
        quantity: u8,
        /// Origine de l'octroi.
        reason: GrantReason,
    },
    /// Un consommable a été utilisé et son effet appliqué.
    ConsumableUsed {
        /// Type d'objet consommé.
        kind: ConsumableKind,
        /// Stock restant après consommation.
        remaining: u8,
        /// Effet réellement appliqué, jauges plafonnées comprises.
        applied: ConsumableEffect,
        /// Statistiques après application.
        stats: PetStats,
    },
    /// Le minuteur de concentration a démarré.
    PomodoroStarted {
        /// Phase démarrée.
        phase: PomodoroPhase,
        /// Durée de la phase.
        remaining: Duration,
    },
    /// Le minuteur de concentration a été suspendu.
    PomodoroPaused {
        /// Phase suspendue.
        phase: PomodoroPhase,
        /// Temps restant conservé.
        remaining: Duration,
        /// Raison de la suspension.
        reason: PauseReason,
    },
    /// Le minuteur de concentration a repris.
    PomodoroResumed {
        /// Phase reprise.
        phase: PomodoroPhase,
        /// Temps restant au moment de la reprise.
        remaining: Duration,
    },
    /// Une phase du cycle de concentration s'est achevée.
    PomodoroPhaseCompleted {
        /// Phase achevée.
        phase: PomodoroPhase,
        /// Blocs de travail accomplis, après mise à jour.
        completed_work_blocks: u16,
    },
    /// Le cycle de concentration a été arrêté.
    PomodoroStopped {
        /// Phase en cours au moment de l'arrêt.
        phase: PomodoroPhase,
        /// Blocs de travail accomplis pendant le cycle.
        completed_work_blocks: u16,
    },
    /// Un rappel de bien-être discret est proposé.
    WellbeingReminder {
        /// Nature du rappel.
        kind: WellbeingReminderKind,
    },
}
