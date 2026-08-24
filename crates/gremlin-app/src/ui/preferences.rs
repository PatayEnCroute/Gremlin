//! Préférences d'interface persistées.
//!
//! # Pourquoi ces réglages existent
//!
//! Trois d'entre eux sont des réponses directes à des limites assumées ailleurs
//! dans le panneau :
//!
//! * **Taille du texte** — la police est bitmap, elle ne suit donc l'échelle du
//!   système que par paliers. Là où la mise à l'échelle automatique ne peut
//!   atteindre qu'un cran voisin, ce réglage rend la main à l'utilisateur.
//! * **Thème** — le suivi du système est le défaut, mais un environnement de
//!   fenêtres qui ne rapporte pas son thème obligerait sinon à subir le repli.
//!   Le mode contraste renforcé s'adresse aux vision basses.
//! * **Mouvement réduit** — le curseur de saisie clignote, ce qui est une
//!   animation permanente dans le champ de vision. Certaines personnes y sont
//!   sensibles, et un lecteur d'écran n'a de toute façon aucun usage du
//!   clignotement.
//!
//! # Frontière de confiance
//!
//! Ces valeurs viennent d'un fichier que l'utilisateur peut éditer à la main.
//! Elles sont donc désérialisées avec `#[serde(default)]` au niveau du
//! conteneur, puis passées par [`UiPreferences::normalize`]. Les énumérations
//! sont intrinsèquement bornées ; un variant inconnu dans le fichier ferait
//! échouer la désérialisation du conteneur entier, ce que la stratégie de
//! chargement existante traite déjà en mettant la sauvegarde de côté.

use crate::ui::layout::TextSize;
use crate::ui::theme::ThemePreference;
use serde::{Deserialize, Serialize};

/// Préférences d'affichage et d'accessibilité du panneau.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct UiPreferences {
    /// Taille du texte du panneau.
    pub text_size: TextSize,
    /// Thème de couleurs du panneau.
    pub theme: ThemePreference,
    /// Supprime les animations non essentielles, dont le curseur clignotant.
    pub reduced_motion: bool,
    /// Referme le panneau lorsqu'il perd le focus, à la façon de Raycast.
    ///
    /// Activé par défaut parce que c'est le geste attendu d'une palette de
    /// commandes ; désactivable parce qu'il surprend quand on consulte le
    /// panneau en travaillant à côté.
    pub close_on_focus_loss: bool,
}

impl Default for UiPreferences {
    fn default() -> Self {
        Self {
            text_size: TextSize::Normal,
            theme: ThemePreference::System,
            reduced_motion: false,
            close_on_focus_loss: true,
        }
    }
}

impl UiPreferences {
    /// Corrige les valeurs incohérentes après désérialisation.
    ///
    /// Renvoie `true` si un ajustement a eu lieu, afin que l'appelant puisse le
    /// signaler dans les journaux — même contrat que
    /// [`crate::config::AppConfig::normalize`].
    ///
    /// Les champs sont tous intrinsèquement bornés : deux énumérations fermées et
    /// deux booléens. La méthode existe pour que l'ajout d'un futur réglage
    /// numérique trouve sa place, et pour que la garantie de normalisation soit
    /// vérifiable dès aujourd'hui.
    #[allow(clippy::unused_self)]
    pub const fn normalize(&mut self) -> bool {
        false
    }

    /// Libellé du réglage de mouvement, pour l'affichage.
    #[must_use]
    pub const fn motion_label(self) -> &'static str {
        if self.reduced_motion {
            "Réduit"
        } else {
            "Complet"
        }
    }
}

impl TextSize {
    /// Libellé affiché dans le panneau.
    #[must_use]
    pub const fn display_name(self) -> &'static str {
        match self {
            Self::Compact => "Compact",
            Self::Normal => "Normal",
            Self::Large => "Grand",
        }
    }

    /// Valeur suivante dans le cycle.
    #[must_use]
    pub const fn next(self) -> Self {
        match self {
            Self::Compact => Self::Normal,
            Self::Normal => Self::Large,
            Self::Large => Self::Compact,
        }
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn test_defaults_are_already_normalized() {
        let mut preferences = UiPreferences::default();
        assert!(
            !preferences.normalize(),
            "les valeurs par défaut ne doivent rien avoir à corriger"
        );
    }

    #[test]
    fn test_partial_json_keeps_the_other_defaults() {
        // Compatibilité ascendante : une sauvegarde écrite avant l'ajout de ces
        // réglages n'en contient aucun, et une sauvegarde écrite après un ajout
        // futur n'en contiendra qu'une partie.
        let preferences: UiPreferences =
            serde_json::from_str(r#"{"reduced_motion": true}"#).expect("désérialisation partielle");

        assert!(preferences.reduced_motion);
        assert_eq!(preferences.text_size, TextSize::Normal);
        assert_eq!(preferences.theme, ThemePreference::System);
        assert!(preferences.close_on_focus_loss);
    }

    #[test]
    fn test_empty_json_yields_the_defaults() {
        let preferences: UiPreferences =
            serde_json::from_str("{}").expect("objet vide accepté par serde(default)");
        assert_eq!(preferences, UiPreferences::default());
    }

    #[test]
    fn test_roundtrip_through_json_is_lossless() {
        let original = UiPreferences {
            text_size: TextSize::Large,
            theme: ThemePreference::HighContrast,
            reduced_motion: true,
            close_on_focus_loss: false,
        };

        let encoded = serde_json::to_string(&original).expect("sérialisation");
        let decoded: UiPreferences = serde_json::from_str(&encoded).expect("désérialisation");
        assert_eq!(decoded, original);
    }

    #[test]
    fn test_text_size_cycle_visits_every_value() {
        let mut seen = Vec::new();
        let mut size = TextSize::Normal;
        for _ in 0..3 {
            seen.push(size);
            size = size.next();
        }

        assert_eq!(size, TextSize::Normal, "le cycle doit boucler");
        for value in [TextSize::Compact, TextSize::Normal, TextSize::Large] {
            assert!(seen.contains(&value), "{value:?} absent du cycle");
        }
    }

    #[test]
    fn test_unknown_variant_is_rejected_rather_than_guessed() {
        // Mieux vaut un échec de lecture — que la stratégie de chargement traite
        // en mettant le fichier de côté — qu'une valeur devinée silencieusement.
        let outcome: Result<UiPreferences, _> = serde_json::from_str(r#"{"theme": "Fluorescent"}"#);
        assert!(outcome.is_err(), "un variant inconnu doit être refusé");
    }
}
