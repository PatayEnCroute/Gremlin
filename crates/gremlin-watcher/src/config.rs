//! Configuration du module de surveillance de dépôts Git.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

/// Durée de stabilisation par défaut des événements de dépôts Git (millisecondes).
pub const DEFAULT_DEBOUNCE_MS: u64 = 200;

/// Durée de stabilisation par défaut des événements d'assets (millisecondes).
pub const DEFAULT_ASSET_DEBOUNCE_MS: u64 = 200;

/// Durée de stabilisation par défaut des rapports d'outillage.
pub const DEFAULT_TOOLING_DEBOUNCE_MS: u64 = 250;

/// Profondeur maximale par défaut de parcours récursif lors des scans de dépôts.
pub const DEFAULT_MAX_SCAN_DEPTH: usize = 5;

const MIN_DEBOUNCE_MS: u64 = 10;
const MAX_DEBOUNCE_MS: u64 = 10_000;
const MAX_SCAN_DEPTH: usize = 16;
const MAX_CUSTOM_ROOTS: usize = 64;
const MAX_TOOLING_SOURCES: usize = 32;

/// Format attendu pour une source de rapports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ToolingReportFormat {
    Auto,
    Junit,
    Trx,
    JestJson,
    GremlinJson,
}

/// Indice d'écosystème associé à une source de rapports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ToolingFrameworkHint {
    Auto,
    Rust,
    JavaScript,
    Python,
    Go,
    Dotnet,
    Generic,
}

/// Source de rapports relative à chaque dépôt surveillé.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolingSourceConfig {
    pub relative_path: PathBuf,
    pub format: ToolingReportFormat,
    pub framework: ToolingFrameworkHint,
}

impl ToolingSourceConfig {
    #[must_use]
    pub fn new(relative_path: impl Into<PathBuf>, format: ToolingReportFormat) -> Self {
        Self {
            relative_path: relative_path.into(),
            format,
            framework: ToolingFrameworkHint::Auto,
        }
    }

    #[must_use]
    pub fn is_safe(&self) -> bool {
        is_safe_relative_path(&self.relative_path)
    }
}

fn default_tooling_sources() -> Vec<ToolingSourceConfig> {
    vec![
        ToolingSourceConfig::new("target/nextest", ToolingReportFormat::Junit),
        ToolingSourceConfig::new("test-results", ToolingReportFormat::Auto),
        ToolingSourceConfig::new("TestResults", ToolingReportFormat::Trx),
        ToolingSourceConfig::new(".gremlin/results", ToolingReportFormat::GremlinJson),
        ToolingSourceConfig::new("junit.xml", ToolingReportFormat::Junit),
    ]
}

fn is_safe_relative_path(path: &Path) -> bool {
    !path.as_os_str().is_empty()
        && !path.is_absolute()
        && path
            .components()
            .all(|component| matches!(component, Component::Normal(_) | Component::CurDir))
}

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
    /// Durée de stabilisation des rapports de tests et builds.
    pub tooling_debounce_duration_ms: u64,
    /// Active la surveillance passive des rapports configurés.
    pub tooling_enabled: bool,
    /// Sources de rapports relatives aux dépôts.
    pub tooling_sources: Vec<ToolingSourceConfig>,
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
            tooling_debounce_duration_ms: DEFAULT_TOOLING_DEBOUNCE_MS,
            tooling_enabled: true,
            tooling_sources: default_tooling_sources(),
            custom_roots: Vec::new(),
            auto_discovery: true,
            max_scan_depth: DEFAULT_MAX_SCAN_DEPTH,
        }
    }
}

impl WatcherConfig {
    /// Restaure les bornes et élimine les chemins hostiles après désérialisation.
    ///
    /// Renvoie `true` si une valeur a été corrigée.
    pub fn normalize(&mut self) -> bool {
        let before = self.clone();
        self.debounce_duration_ms = self
            .debounce_duration_ms
            .clamp(MIN_DEBOUNCE_MS, MAX_DEBOUNCE_MS);
        self.asset_debounce_duration_ms = self
            .asset_debounce_duration_ms
            .clamp(MIN_DEBOUNCE_MS, MAX_DEBOUNCE_MS);
        self.tooling_debounce_duration_ms = self
            .tooling_debounce_duration_ms
            .clamp(MIN_DEBOUNCE_MS, MAX_DEBOUNCE_MS);
        self.max_scan_depth = self.max_scan_depth.clamp(1, MAX_SCAN_DEPTH);
        self.custom_roots.truncate(MAX_CUSTOM_ROOTS);
        self.custom_roots.sort();
        self.custom_roots.dedup();
        self.tooling_sources.retain(ToolingSourceConfig::is_safe);
        self.tooling_sources.truncate(MAX_TOOLING_SOURCES);
        let mut seen_sources = HashSet::new();
        self.tooling_sources
            .retain(|source| seen_sources.insert(source.relative_path.clone()));
        *self != before
    }

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

    /// Durée de debounce des rapports.
    #[must_use]
    pub const fn tooling_debounce_duration(&self) -> Duration {
        Duration::from_millis(self.tooling_debounce_duration_ms)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ToolingReportFormat, ToolingSourceConfig, WatcherConfig, DEFAULT_ASSET_DEBOUNCE_MS,
        DEFAULT_DEBOUNCE_MS, MAX_TOOLING_SOURCES,
    };
    use std::path::PathBuf;
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

    #[test]
    fn test_normalize_rejects_hostile_and_duplicate_sources() {
        let valid = ToolingSourceConfig::new("reports", ToolingReportFormat::Junit);
        let mut config = WatcherConfig {
            tooling_debounce_duration_ms: 0,
            tooling_sources: vec![
                valid.clone(),
                ToolingSourceConfig::new("../outside", ToolingReportFormat::Junit),
                valid,
                ToolingSourceConfig::new(PathBuf::from("/absolute"), ToolingReportFormat::Junit),
            ],
            ..WatcherConfig::default()
        };
        assert!(config.normalize());
        assert_eq!(config.tooling_sources.len(), 1);
        assert_eq!(
            config.tooling_sources[0].relative_path,
            PathBuf::from("reports")
        );
        assert!(config.tooling_sources.len() <= MAX_TOOLING_SOURCES);
        let once = config.clone();
        assert!(!config.normalize());
        assert_eq!(config, once);
    }
}
