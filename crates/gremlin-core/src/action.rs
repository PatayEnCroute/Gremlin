//! Actions métier applicables au familier.
//!
//! Ce type remplace les libellés d'actions passés sous forme de `String` :
//! il évite les allocations, garantit l'exhaustivité des correspondances et
//! isole la langue d'affichage dans une seule fonction ([`ActionKind::label`]).

use serde::{Deserialize, Serialize};
use std::fmt;

/// Nature d'une action tentée sur le familier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ActionKind {
    /// Nourrir le familier.
    Feed,
    /// Caresser le familier.
    Pet,
    /// Soigner le familier.
    Heal,
    /// Faire reposer le familier.
    Rest,
    /// Endormir le familier.
    Sleep,
    /// Réveiller le familier.
    WakeUp,
    /// Réanimer un familier décédé.
    Revive,
    /// Assimiler un commit Git.
    Commit,
    /// Assimiler un rapport de tests.
    TestRun,
    /// Assimiler un résultat de build.
    Build,
    /// Utiliser un consommable de l'inventaire.
    UseConsumable,
}

impl ActionKind {
    /// Libellé lisible de l'action.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Feed => "nourrir",
            Self::Pet => "caresser",
            Self::Heal => "soigner",
            Self::Rest => "reposer",
            Self::Sleep => "endormir",
            Self::WakeUp => "réveiller",
            Self::Revive => "réanimer",
            Self::Commit => "assimiler un commit",
            Self::TestRun => "assimiler un rapport de tests",
            Self::Build => "assimiler un build",
            Self::UseConsumable => "utiliser un objet",
        }
    }
}

impl fmt::Display for ActionKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

#[cfg(test)]
mod tests {
    use super::ActionKind;

    #[test]
    fn test_every_action_has_a_distinct_label() {
        let actions = [
            ActionKind::Feed,
            ActionKind::Pet,
            ActionKind::Heal,
            ActionKind::Rest,
            ActionKind::Sleep,
            ActionKind::WakeUp,
            ActionKind::Revive,
            ActionKind::Commit,
            ActionKind::TestRun,
            ActionKind::Build,
            ActionKind::UseConsumable,
        ];

        let mut labels: Vec<&str> = actions.iter().map(|a| a.label()).collect();
        labels.sort_unstable();
        let count_before = labels.len();
        labels.dedup();

        assert_eq!(labels.len(), count_before, "libellés d'actions dupliqués");
        assert!(labels.iter().all(|l| !l.is_empty()));
    }
}
