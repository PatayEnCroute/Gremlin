//! Types d'erreurs pour le cœur du jeu Gremlin.

use crate::action::ActionKind;
use crate::mood::PetMood;
use thiserror::Error;

/// Erreurs pouvant survenir dans le moteur `gremlin-core`.
///
/// Le type n'implémente volontairement pas `Eq` : certaines variantes portent
/// une valeur flottante afin de rester exploitables par l'appelant sans passer
/// par une conversion en chaîne.
#[derive(Debug, Error, Clone, PartialEq)]
pub enum CoreError {
    /// Une valeur statistique est en dehors des bornes permises `[0.0, 100.0]`.
    #[error(
        "valeur statistique invalide pour '{name}' : {value} (doit être comprise entre 0.0 et 100.0)"
    )]
    InvalidStatValue {
        /// Nom de la jauge concernée.
        name: &'static str,
        /// Valeur refusée.
        value: f32,
    },

    /// Le montant fourni pour une action est négatif, infini ou `NaN`.
    #[error("montant invalide pour l'action '{action}' : {value} (attendu : valeur finie >= 0.0)")]
    InvalidActionAmount {
        /// Action concernée.
        action: ActionKind,
        /// Montant refusé.
        value: f32,
    },

    /// L'action demandée est impossible dans l'état émotionnel actuel du familier.
    #[error("action '{action}' impossible dans l'état actuel : {current_mood:?}")]
    InvalidActionForMood {
        /// Action refusée.
        action: ActionKind,
        /// Humeur bloquant l'action.
        current_mood: PetMood,
    },

    /// Le familier est décédé et ne peut plus effectuer cette action sans être ressuscité.
    #[error("le familier est décédé : impossible d'exécuter '{0}' sans renaissance")]
    PetIsDead(ActionKind),

    /// Échec lors de la désérialisation ou sérialisation de l'état du familier.
    #[error("erreur de sérialisation de l'état : {0}")]
    StateSerialization(String),

    /// La sauvegarde provient d'une version plus récente du format et ne peut être lue.
    #[error(
        "format de sauvegarde non pris en charge : version {found} (version maximale gérée : {supported})"
    )]
    UnsupportedSaveVersion {
        /// Version lue dans la sauvegarde.
        found: u32,
        /// Version maximale gérée par ce binaire.
        supported: u32,
    },

    /// Erreur de configuration métier.
    #[error("erreur de configuration : {0}")]
    ConfigurationError(String),
}
