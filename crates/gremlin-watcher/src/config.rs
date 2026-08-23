//! Configuration du module de surveillance de dépôts Git.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;

/// Durée de stabilisation par défaut des événements de dépôts Git (millisecondes).
pub const DEFAULT_DEBOUNCE_MS: u64 = 200;

/// Durée de stabilisation par défaut des événements d'assets (millisecondes).
pub const DEFAULT_ASSET_DEBOUNCE_MS: u64 = 200;

/// Profondeur maximale par défaut de parcours récursif lors des scans de dépôts.
pub const DEFAULT_MAX_SCAN_DEPTH: usize = 5;

/// Configuration du système de surveillance et de scan Git.
///
/// `#[serde(default)]` garantit la compatibilité ascendante : un fichier de
/// configuration écrit par une version antérieure reste déserialisable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct WatcherConfig {
    /// Durée de stabilisation des événements de fichiers Git (debouncing).
    pub debounce_duration_ms: u64,
    /// Durée de stabilisation des événements de packs d'assets (skins, accessoires).
    pub asset_debounce_duration_ms: u64,
    /// Chemins racines personnalisés à surveiller en plus des chemins par défaut.
    pub custom_roots: Vec<PathBuf>,
    /// Activation de la découverte automatique des dossiers de projets conventionnels.
    pub auto_discovery: bool,
    /// Profondeur maximale de parcours récursif de dossiers.
    pub max_scan_depth: usize,
}

impl Default for WatcherConfig {
    fn default() -> Self {
        Self {
            debounce_duration_ms: DEFAULT_DEBOUNCE_MS,
            asset_debounce_duration_ms: DEFAULT_ASSET_DEBOUNCE_MS,
            custom_roots: Vec::new(),
            auto_discovery: true,
            max_scan_depth: DEFAULT_MAX_SCAN_DEPTH,
        }
    }
}

impl WatcherConfig {
    /// Récupère la durée de debounce des dépôts Git sous forme de `std::time::Duration`.
    #[must_use]
    pub const fn debounce_duration(&self) -> Duration {
        Duration::from_millis(self.debounce_duration_ms)
    }

    /// Récupère la durée de debounce des assets sous forme de `std::time::Duration`.
    #[must_use]
    pub const fn asset_debounce_duration(&self) -> Duration {
        Duration::from_millis(self.asset_debounce_duration_ms)
    }
}

#[cfg(test)]
mod tests {
    use super::{WatcherConfig, DEFAULT_ASSET_DEBOUNCE_MS, DEFAULT_DEBOUNCE_MS};
    use std::time::Duration;

    #[test]
    fn test_default_durations_are_derived_from_constants() {
        let config = WatcherConfig::default();
        assert_eq!(
            config.debounce_duration(),
            Duration::from_millis(DEFAULT_DEBOUNCE_MS)
        );
        assert_eq!(
            config.asset_debounce_duration(),
            Duration::from_millis(DEFAULT_ASSET_DEBOUNCE_MS)
        );
    }

    #[test]
    fn test_partial_config_deserialization_keeps_defaults() {
        // Un fichier écrit par une version antérieure ne connaît pas les nouveaux champs.
        let json = r#"{"debounce_duration_ms": 42}"#;
        let parsed: WatcherConfig = match serde_json::from_str(json) {
            Ok(c) => c,
            Err(e) => panic!("désérialisation partielle impossible : {e}"),
        };
        assert_eq!(parsed.debounce_duration_ms, 42);
        assert_eq!(parsed.asset_debounce_duration_ms, DEFAULT_ASSET_DEBOUNCE_MS);
        assert!(parsed.auto_discovery);
    }
}
