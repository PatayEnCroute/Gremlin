//! # `gremlin-watcher`
//!
//! Surveillance passive et **ciblée** du système de fichiers : seuls les dépôts
//! Git explicitement confiés par l'utilisateur (`WatcherConfig::tracked_repos`)
//! sont observés. Aucun parcours d'arborescence, aucune racine de projets, aucune
//! découverte automatique — c'est au développeur de déclarer les espaces de
//! travail qu'il confie à son Gremlin.
//!
//! Surveillance et rechargement à chaud des packs de skins et d'accessoires.
//! Émet des signaux métier (`DevSignal`, `AssetSignal`) vers l'application principale via des canaux asynchrones.

pub mod config;
pub mod debouncer;
pub mod error;
pub mod git_parser;
pub mod git_path;
pub mod parsers;
pub mod signals;
pub mod skin_watcher;
pub mod watcher;

mod tooling;
mod worker;

#[cfg(test)]
mod test_support;

pub use config::{
    ToolingFrameworkHint, ToolingReportFormat, ToolingSourceConfig, WatcherConfig,
    DEFAULT_ASSET_DEBOUNCE_MS, DEFAULT_DEBOUNCE_MS, DEFAULT_TOOLING_DEBOUNCE_MS, MAX_TRACKED_REPOS,
};
pub use debouncer::EventDebouncer;
pub use error::WatcherError;
pub use git_parser::{CommitDayHistory, GitRefParser, ReflogEntry, RepoSnapshot};
pub use git_path::{
    find_repo_root, is_git_repo, is_relevant_git_path, normalize_path, GitPathKind,
};
pub use signals::{
    DevSignal, GitCommitStamp, ParsedBuildReport, ParsedTestReport, ReportBuildTool,
    ReportFramework, ToolingStateAck, WatcherStatus,
};
pub use skin_watcher::{AssetSignal, AssetWatcher};
pub use watcher::RepoWatcher;
