//! # `gremlin-watcher`
//!
//! Surveillance passive du système de fichiers et détection zéro-config des dépôts Git.
//! Surveillance et rechargement à chaud des packs de skins et d'accessoires.
//! Émet des signaux métier (`DevSignal`, `AssetSignal`) vers l'application principale via des canaux asynchrones.

pub mod config;
pub mod debouncer;
pub mod error;
pub mod git_parser;
pub mod git_path;
pub mod scanner;
pub mod signals;
pub mod skin_watcher;
pub mod watcher;

mod worker;

#[cfg(test)]
mod test_support;

pub use config::{
    WatcherConfig, DEFAULT_ASSET_DEBOUNCE_MS, DEFAULT_DEBOUNCE_MS, DEFAULT_MAX_SCAN_DEPTH,
};
pub use debouncer::EventDebouncer;
pub use error::WatcherError;
pub use git_parser::{GitRefParser, ReflogEntry, RepoSnapshot};
pub use git_path::{find_repo_root, is_relevant_git_path, normalize_path, GitPathKind};
pub use scanner::GitScanner;
pub use signals::{DevSignal, WatcherStatus};
pub use skin_watcher::{AssetSignal, AssetWatcher};
pub use watcher::RepoWatcher;
