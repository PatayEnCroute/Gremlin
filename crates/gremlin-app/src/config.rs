//! Configuration utilisateur persistante de Gremlin.
//!
//! Cette structure est désérialisée depuis un fichier que l'utilisateur peut
//! éditer à la main. Elle est donc systématiquement passée par
//! [`AppConfig::normalize`] au chargement : une échelle de fenêtre à 0
//! produirait une fenêtre de dimension nulle, et un intervalle de sauvegarde
//! nul déclencherait une écriture disque à chaque réveil de la boucle.

use crate::ui::UiPreferences;
use gremlin_render::WardrobeEquipment;
use gremlin_watcher::WatcherConfig;
use serde::{Deserialize, Serialize};

/// Configuration générale et persistante de l'application.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    /// Identifiant du skin actif.
    pub active_skin: String,
    /// Équipement cosmétique actif (garde-robe).
    pub wardrobe: WardrobeEquipment,
    /// Mode click-through activé par défaut.
    pub click_through_enabled: bool,
    /// Démarrage automatique au boot de l'OS.
    pub autostart_enabled: bool,
    /// Intervalle de sauvegarde automatique régulière, en secondes.
    pub auto_save_interval_secs: u64,
    /// Activer la simulation de rattrapage temporel hors-ligne au démarrage.
    pub offline_catchup_enabled: bool,
    /// Plafond maximal d'heures simulées hors-ligne.
    pub max_offline_catchup_hours: u32,
    /// Configuration du système de surveillance et de scan Git.
    pub watcher: WatcherConfig,
    /// Active l'estimation locale des sessions de focus.
    pub focus_tracking_enabled: bool,
    /// Active les rappels discrets après une session prolongée.
    pub break_reminders_enabled: bool,
    /// Échelle d'affichage de la fenêtre (multiplicateur de pixels).
    pub scale_factor: u32,
    /// Préférences d'affichage et d'accessibilité du panneau de paramètres.
    pub ui: UiPreferences,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            active_skin: String::from("default"),
            wardrobe: WardrobeEquipment::default(),
            click_through_enabled: false,
            autostart_enabled: false,
            auto_save_interval_secs: 300,
            offline_catchup_enabled: true,
            max_offline_catchup_hours: 48,
            watcher: WatcherConfig::default(),
            focus_tracking_enabled: true,
            break_reminders_enabled: true,
            scale_factor: 2,
            ui: UiPreferences::default(),
        }
    }
}

impl AppConfig {
    /// Échelle d'affichage minimale acceptée.
    pub const MIN_SCALE_FACTOR: u32 = 1;
    /// Échelle d'affichage maximale acceptée.
    pub const MAX_SCALE_FACTOR: u32 = 5;
    /// Intervalle de sauvegarde automatique minimal, en secondes.
    pub const MIN_AUTO_SAVE_INTERVAL_SECS: u64 = 30;
    /// Intervalle de sauvegarde automatique maximal, en secondes (24 h).
    pub const MAX_AUTO_SAVE_INTERVAL_SECS: u64 = 86_400;
    /// Plafond maximal de rattrapage hors-ligne, en heures (30 jours).
    pub const MAX_OFFLINE_CATCHUP_HOURS: u32 = 720;
    /// Longueur maximale acceptée pour l'identifiant de skin.
    pub const MAX_SKIN_NAME_CHARS: usize = 64;

    /// Corrige silencieusement les valeurs hors bornes après désérialisation.
    ///
    /// Renvoie `true` si au moins une valeur a dû être ajustée, afin que
    /// l'appelant puisse le signaler dans les journaux.
    pub fn normalize(&mut self) -> bool {
        let before = self.clone();

        self.scale_factor = self
            .scale_factor
            .clamp(Self::MIN_SCALE_FACTOR, Self::MAX_SCALE_FACTOR);

        self.auto_save_interval_secs = self.auto_save_interval_secs.clamp(
            Self::MIN_AUTO_SAVE_INTERVAL_SECS,
            Self::MAX_AUTO_SAVE_INTERVAL_SECS,
        );

        self.max_offline_catchup_hours = self
            .max_offline_catchup_hours
            .clamp(1, Self::MAX_OFFLINE_CATCHUP_HOURS);

        // La normalisation des préférences d'interface est déléguée : le
        // conteneur ne connaît pas leurs bornes.
        let ui_adjusted = self.ui.normalize();
        let watcher_adjusted = self.watcher.normalize();

        // Un identifiant de skin ne doit ni être vide, ni contenir de
        // séparateur de chemin : il est concaténé à un répertoire de base.
        let sanitized_skin: String = self
            .active_skin
            .trim()
            .chars()
            .filter(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | '.'))
            .take(Self::MAX_SKIN_NAME_CHARS)
            .collect();

        self.active_skin = if sanitized_skin.is_empty()
            || sanitized_skin.chars().all(|c| c == '.')
            || sanitized_skin.contains("..")
        {
            String::from("default")
        } else {
            sanitized_skin
        };

        ui_adjusted || watcher_adjusted || before != *self
    }

    /// Échelle d'affichage suivante dans le cycle, avec bouclage au minimum.
    #[must_use]
    pub const fn next_scale_factor(&self) -> u32 {
        if self.scale_factor >= Self::MAX_SCALE_FACTOR {
            Self::MIN_SCALE_FACTOR
        } else {
            self.scale_factor + 1
        }
    }

    /// Indique si la configuration respecte déjà toutes ses bornes.
    #[must_use]
    pub fn is_normalized(&self) -> bool {
        let mut probe = self.clone();
        !probe.normalize()
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config_is_already_normalized() {
        assert!(AppConfig::default().is_normalized());
    }

    #[test]
    fn test_normalize_repairs_hostile_values() {
        let mut config = AppConfig {
            scale_factor: 0,
            auto_save_interval_secs: 0,
            max_offline_catchup_hours: 0,
            active_skin: String::new(),
            ..AppConfig::default()
        };

        assert!(config.normalize());
        assert_eq!(config.scale_factor, AppConfig::MIN_SCALE_FACTOR);
        assert_eq!(
            config.auto_save_interval_secs,
            AppConfig::MIN_AUTO_SAVE_INTERVAL_SECS
        );
        assert_eq!(config.max_offline_catchup_hours, 1);
        assert_eq!(config.active_skin, "default");
        assert!(config.is_normalized());
    }

    #[test]
    fn test_normalize_caps_absurd_values() {
        let mut config = AppConfig {
            scale_factor: 1_000_000,
            auto_save_interval_secs: u64::MAX,
            max_offline_catchup_hours: u32::MAX,
            ..AppConfig::default()
        };

        config.normalize();
        assert_eq!(config.scale_factor, AppConfig::MAX_SCALE_FACTOR);
        assert_eq!(
            config.auto_save_interval_secs,
            AppConfig::MAX_AUTO_SAVE_INTERVAL_SECS
        );
        assert_eq!(
            config.max_offline_catchup_hours,
            AppConfig::MAX_OFFLINE_CATCHUP_HOURS
        );
    }

    #[test]
    fn test_skin_name_cannot_escape_its_directory() {
        for hostile in [
            "../../../etc/passwd",
            "..",
            "skins/../../secret",
            "  ",
            "/absolute",
            "C:\\Windows\\System32",
        ] {
            let mut config = AppConfig {
                active_skin: hostile.to_owned(),
                ..AppConfig::default()
            };
            config.normalize();
            assert!(
                !config.active_skin.contains("..")
                    && !config.active_skin.contains('/')
                    && !config.active_skin.contains('\\'),
                "identifiant de skin non assaini : {hostile} -> {}",
                config.active_skin
            );
        }
    }

    #[test]
    fn test_valid_skin_name_is_preserved() {
        let mut config = AppConfig {
            active_skin: String::from("neon-gremlin_v2"),
            ..AppConfig::default()
        };
        config.normalize();
        assert_eq!(config.active_skin, "neon-gremlin_v2");
    }

    #[test]
    fn test_scale_cycle_wraps() {
        let at_max = AppConfig {
            scale_factor: AppConfig::MAX_SCALE_FACTOR,
            ..AppConfig::default()
        };
        assert_eq!(at_max.next_scale_factor(), AppConfig::MIN_SCALE_FACTOR);

        let mid = AppConfig {
            scale_factor: 2,
            ..AppConfig::default()
        };
        assert_eq!(mid.next_scale_factor(), 3);
    }

    #[test]
    fn test_partial_json_keeps_defaults() {
        // Compatibilité ascendante : un fichier écrit par une version
        // antérieure n'a pas tous les champs.
        let config: AppConfig =
            serde_json::from_str(r#"{"scale_factor": 3}"#).expect("désérialisation partielle");

        assert_eq!(config.scale_factor, 3);
        assert_eq!(config.active_skin, "default");
        assert_eq!(config.auto_save_interval_secs, 300);
    }
}
