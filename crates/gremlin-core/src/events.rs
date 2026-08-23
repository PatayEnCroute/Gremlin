//! Événements du domaine métier émis par le moteur du Gremlin.

use crate::mood::PetMood;
use crate::progression::EvolutionStage;
use crate::stats::PetStats;
use serde::{Deserialize, Serialize};

/// Événements de cycle de vie et d'état émis par `gremlin-core`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum CoreEvent {
    /// Un commit a été assimilé par le Gremlin.
    CommitReceived {
        repo: String,
        branch: String,
        xp_gained: u64,
    },
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
}
