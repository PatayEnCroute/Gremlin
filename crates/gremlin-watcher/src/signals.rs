//! Signaux émis par le module de surveillance Git vers l'orchestrateur.

use std::path::PathBuf;
use std::time::Duration;

/// Écosystème identifié par le watcher, indépendant du modèle métier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportFramework {
    Rust,
    JavaScript,
    Python,
    Go,
    Dotnet,
    Generic,
}

/// Résumé de tests validé à la frontière disque.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParsedTestReport {
    pub framework: ReportFramework,
    pub passed: u32,
    pub failed: u32,
    pub skipped: u32,
    pub duration: Duration,
}

/// Outil de build identifié par le watcher.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportBuildTool {
    Cargo,
    Npm,
    WebpackOrVite,
    Python,
    Go,
    Dotnet,
    Generic,
}

/// Résumé de build validé à la frontière disque.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParsedBuildReport {
    pub tool: ReportBuildTool,
    pub success: bool,
    pub duration: Duration,
}

/// Confirmation dédiée d'une bascule de surveillance d'outillage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolingStateAck {
    pub enabled: bool,
    pub error: Option<String>,
}

/// Signal de développement émis par le système de surveillance de fichiers et dépôts Git.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DevSignal {
    /// Un commit a été effectué sur un dépôt surveillé.
    CommitCreated {
        /// Nom ou identifiant du dépôt.
        repo_name: String,
        /// Branche concernée (ex: "main", "feature/pet").
        branch: String,
        /// SHA du commit s'il a pu être extrait (40 caractères hexadécimaux).
        commit_sha: Option<String>,
        /// Message du commit s'il est disponible.
        message: Option<String>,
        /// Chemin absolu du dépôt.
        repo_path: PathBuf,
    },
    /// Un changement de branche active a été détecté (ex: `git checkout`, `git switch`).
    BranchChanged {
        /// Nom ou identifiant du dépôt.
        repo_name: String,
        /// Ancienne branche active.
        old_branch: String,
        /// Nouvelle branche active.
        new_branch: String,
        /// Chemin absolu du dépôt.
        repo_path: PathBuf,
    },
    /// Un nouveau dépôt Git a été découvert et ajouté à la surveillance.
    RepoDiscovered {
        /// Nom du dépôt.
        repo_name: String,
        /// Chemin absolu vers la racine du dépôt.
        path: PathBuf,
    },
    /// Un dépôt Git a été supprimé ou n'est plus accessible.
    ///
    /// Émis aussi bien sur désinscription explicite (`RepoWatcher::unwatch_repo`) que
    /// lorsque le répertoire `.git` disparaît du disque : la surveillance OS est alors
    /// libérée et l'état interne du dépôt oublié.
    RepoRemoved {
        /// Nom du dépôt.
        repo_name: String,
        /// Chemin absolu du dépôt.
        path: PathBuf,
    },
    /// Un rapport de tests complet a été détecté.
    TestCompleted {
        repo_name: String,
        repo_path: PathBuf,
        report_path: PathBuf,
        run_id: Option<String>,
        summary: ParsedTestReport,
    },
    /// Un résultat de build explicite a été détecté.
    BuildCompleted {
        repo_name: String,
        repo_path: PathBuf,
        report_path: PathBuf,
        run_id: String,
        summary: ParsedBuildReport,
    },
}

/// Incident de fonctionnement de la surveillance, transmis sur un canal facultatif.
///
/// Ces événements ne sont pas des signaux métier : ils renseignent l'appelant sur la
/// **fiabilité** de la surveillance (échec d'enregistrement, événements perdus), qui
/// n'était auparavant visible que dans les journaux.
/// Voir [`crate::RepoWatcher::set_status_sender`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WatcherStatus {
    /// L'enregistrement d'une surveillance a échoué (chemin inaccessible, quota OS atteint...).
    WatchFailed {
        /// Chemin concerné.
        path: PathBuf,
        /// Description de la cause.
        reason: String,
    },
    /// Des événements ont été perdus ; les dépôts connus sont relus pour resynchronisation.
    EventsLost {
        /// Nombre d'événements écartés (0 si le backend ne le quantifie pas).
        dropped: u64,
        /// Description de la cause.
        reason: String,
    },
    /// Un rapport a été refusé après la fin des tentatives bornées.
    ReportRejected { path: PathBuf, reason: String },
    /// La surveillance des rapports a effectivement changé d'état dans le worker.
    ToolingStateChanged {
        /// État désormais appliqué.
        enabled: bool,
        /// Cause d'un éventuel échec partiel d'enregistrement.
        error: Option<String>,
    },
}
