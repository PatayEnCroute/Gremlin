//! Types métier liés aux rapports de tests et de builds.

use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Nombre maximal défensif de tests accepté dans un résumé.
pub const MAX_TEST_COUNT: u32 = 10_000_000;
/// Durée maximale défensive d'un run d'outillage (sept jours).
pub const MAX_TOOLING_DURATION: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// Identifiant opaque d'un dépôt pendant la session courante.
///
/// Il est attribué par l'orchestrateur : le cœur ne manipule ainsi aucun chemin OS.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RepositoryId(u64);

impl RepositoryId {
    /// Construit un identifiant de dépôt opaque.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Valeur opaque, utile aux adaptateurs et aux tests.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Écosystème ayant produit un rapport de tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TestFramework {
    CargoTest,
    JavaScript,
    Pytest,
    GoTest,
    DotnetTest,
    GenericJunit,
}

impl TestFramework {
    /// Libellé utilisateur centralisé.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::CargoTest => "Cargo Test",
            Self::JavaScript => "JavaScript",
            Self::Pytest => "Pytest",
            Self::GoTest => "Go Test",
            Self::DotnetTest => ".NET Test",
            Self::GenericJunit => "JUnit",
        }
    }
}

/// Résumé normalisé d'une exécution de tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TestSummary {
    framework: TestFramework,
    passed: u32,
    failed: u32,
    skipped: u32,
    duration: Duration,
}

impl TestSummary {
    /// Construit un résumé en bornant toutes les valeurs externes.
    #[must_use]
    pub fn new(
        framework: TestFramework,
        passed: u32,
        failed: u32,
        skipped: u32,
        duration: Duration,
    ) -> Self {
        Self {
            framework,
            passed: passed.min(MAX_TEST_COUNT),
            failed: failed.min(MAX_TEST_COUNT),
            skipped: skipped.min(MAX_TEST_COUNT),
            duration: duration.min(MAX_TOOLING_DURATION),
        }
    }

    #[must_use]
    pub const fn framework(self) -> TestFramework {
        self.framework
    }

    #[must_use]
    pub const fn passed(self) -> u32 {
        self.passed
    }

    #[must_use]
    pub const fn failed(self) -> u32 {
        self.failed
    }

    #[must_use]
    pub const fn skipped(self) -> u32 {
        self.skipped
    }

    #[must_use]
    pub const fn duration(self) -> Duration {
        self.duration
    }

    #[must_use]
    pub const fn total(self) -> u32 {
        self.passed
            .saturating_add(self.failed)
            .saturating_add(self.skipped)
    }

    #[must_use]
    pub const fn has_executed_tests(self) -> bool {
        self.passed > 0 || self.failed > 0
    }

    #[must_use]
    pub const fn is_all_passed(self) -> bool {
        self.failed == 0 && self.passed > 0
    }
}

/// Outil ayant produit un résultat de build explicite.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BuildTool {
    Cargo,
    Npm,
    WebpackOrVite,
    Python,
    Go,
    Dotnet,
    Generic,
}

/// Résumé normalisé d'un build.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuildSummary {
    tool: BuildTool,
    success: bool,
    duration: Duration,
}

impl BuildSummary {
    #[must_use]
    pub fn new(tool: BuildTool, success: bool, duration: Duration) -> Self {
        Self {
            tool,
            success,
            duration: duration.min(MAX_TOOLING_DURATION),
        }
    }

    #[must_use]
    pub const fn tool(self) -> BuildTool {
        self.tool
    }

    #[must_use]
    pub const fn success(self) -> bool {
        self.success
    }

    #[must_use]
    pub const fn duration(self) -> Duration {
        self.duration
    }
}

/// Motif d'une recommandation de pause.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BreakReason {
    FocusProlonged,
}

const MAX_TRACKED_REPOSITORIES: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq)]
struct RepositoryToolingState {
    id: RepositoryId,
    last_test_failed: Option<bool>,
    test_reward_cooldown: Duration,
    test_feedback_cooldown: Duration,
    build_reward_cooldown: Duration,
    build_feedback_cooldown: Duration,
}

impl RepositoryToolingState {
    const fn new(id: RepositoryId) -> Self {
        Self {
            id,
            last_test_failed: None,
            test_reward_cooldown: Duration::ZERO,
            test_feedback_cooldown: Duration::ZERO,
            build_reward_cooldown: Duration::ZERO,
            build_feedback_cooldown: Duration::ZERO,
        }
    }

    fn advance(&mut self, elapsed: Duration) {
        self.test_reward_cooldown = self.test_reward_cooldown.saturating_sub(elapsed);
        self.test_feedback_cooldown = self.test_feedback_cooldown.saturating_sub(elapsed);
        self.build_reward_cooldown = self.build_reward_cooldown.saturating_sub(elapsed);
        self.build_feedback_cooldown = self.build_feedback_cooldown.saturating_sub(elapsed);
    }
}

/// Décision issue de la politique anti-spam d'un rapport de tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TestRunDecision {
    pub(crate) reward_allowed: bool,
    pub(crate) feedback_allowed: bool,
    pub(crate) is_fixed: bool,
    pub(crate) entered_failure: bool,
}

/// État borné et transitoire des rapports reçus pendant la session.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ToolingSession {
    repositories: Vec<RepositoryToolingState>,
}

impl ToolingSession {
    pub(crate) fn advance(&mut self, elapsed: Duration) {
        for repository in &mut self.repositories {
            repository.advance(elapsed);
        }
    }

    pub(crate) fn register_test_run(
        &mut self,
        id: RepositoryId,
        failed: bool,
        meaningful: bool,
        reward_cooldown: Duration,
        feedback_cooldown: Duration,
    ) -> TestRunDecision {
        let repository = self.repository_mut(id);
        if !meaningful {
            return TestRunDecision {
                reward_allowed: false,
                feedback_allowed: false,
                is_fixed: false,
                entered_failure: false,
            };
        }

        let is_fixed = !failed && repository.last_test_failed == Some(true);
        let entered_failure = failed && repository.last_test_failed != Some(true);
        let outcome_changed = repository.last_test_failed != Some(failed);
        let reward_allowed = !failed && repository.test_reward_cooldown.is_zero();
        let feedback_allowed = outcome_changed || repository.test_feedback_cooldown.is_zero();

        repository.last_test_failed = Some(failed);
        if reward_allowed {
            repository.test_reward_cooldown = reward_cooldown;
        }
        if feedback_allowed {
            repository.test_feedback_cooldown = feedback_cooldown;
        }

        TestRunDecision {
            reward_allowed,
            feedback_allowed,
            is_fixed,
            entered_failure,
        }
    }

    pub(crate) fn register_build(
        &mut self,
        id: RepositoryId,
        success: bool,
        reward_cooldown: Duration,
        feedback_cooldown: Duration,
    ) -> (bool, bool) {
        let repository = self.repository_mut(id);
        let reward_allowed = success && repository.build_reward_cooldown.is_zero();
        let feedback_allowed = repository.build_feedback_cooldown.is_zero();
        if reward_allowed {
            repository.build_reward_cooldown = reward_cooldown;
        }
        if feedback_allowed {
            repository.build_feedback_cooldown = feedback_cooldown;
        }
        (reward_allowed, feedback_allowed)
    }

    fn repository_mut(&mut self, id: RepositoryId) -> &mut RepositoryToolingState {
        if let Some(index) = self.repositories.iter().position(|entry| entry.id == id) {
            return &mut self.repositories[index];
        }

        if self.repositories.len() >= MAX_TRACKED_REPOSITORIES {
            self.repositories.remove(0);
        }
        self.repositories.push(RepositoryToolingState::new(id));
        let index = self.repositories.len().saturating_sub(1);
        &mut self.repositories[index]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_summary_is_bounded() {
        let summary = TestSummary::new(
            TestFramework::CargoTest,
            u32::MAX,
            u32::MAX,
            u32::MAX,
            Duration::from_secs(u64::MAX),
        );
        assert_eq!(summary.passed(), MAX_TEST_COUNT);
        assert_eq!(summary.duration(), MAX_TOOLING_DURATION);
        assert_eq!(summary.total(), MAX_TEST_COUNT.saturating_mul(3));
    }

    #[test]
    fn test_red_to_green_and_cooldown_are_tracked() {
        let mut session = ToolingSession::default();
        let id = RepositoryId::new(1);
        let cooldown = Duration::from_secs(30);

        let failed = session.register_test_run(id, true, true, cooldown, cooldown);
        assert!(failed.entered_failure);
        assert!(!failed.reward_allowed);

        let fixed = session.register_test_run(id, false, true, cooldown, cooldown);
        assert!(fixed.is_fixed);
        assert!(fixed.reward_allowed);

        let repeated = session.register_test_run(id, false, true, cooldown, cooldown);
        assert!(!repeated.reward_allowed);
        assert!(!repeated.feedback_allowed);

        session.advance(cooldown);
        let after = session.register_test_run(id, false, true, cooldown, cooldown);
        assert!(after.reward_allowed);
        assert!(after.feedback_allowed);
    }
}
