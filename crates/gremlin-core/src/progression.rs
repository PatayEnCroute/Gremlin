//! Système de progression d'XP, niveaux et stades d'évolution morphologique.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::time::Duration;

/// Niveau minimal du familier.
pub const MIN_LEVEL: u32 = 1;

/// Stade d'évolution morphologique du Gremlin.
///
/// L'ordre de déclaration est significatif : `Ord` est dérivé et
/// [`PetProgression::add_xp`] compare les stades pour détecter une évolution.
/// Réordonner les variantes casserait silencieusement cette détection — un test
/// épingle l'ordre attendu.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum EvolutionStage {
    /// Nouveau-né (niveaux 1 à 4).
    Baby,
    /// Adolescent (niveaux 5 à 14).
    Teen,
    /// Adulte aguerri (niveaux 15 à 29).
    Adult,
    /// Cyber-Gremlin légendaire (niveau 30+).
    CyberGremlin,
}

impl EvolutionStage {
    /// Détermine le stade d'évolution correspondant à un niveau donné.
    #[must_use]
    pub const fn from_level(level: u32) -> Self {
        if level >= 30 {
            Self::CyberGremlin
        } else if level >= 15 {
            Self::Adult
        } else if level >= 5 {
            Self::Teen
        } else {
            Self::Baby
        }
    }

    /// Nom lisible du stade d'évolution.
    #[must_use]
    pub const fn display_name(&self) -> &'static str {
        match self {
            Self::Baby => "Bébé",
            Self::Teen => "Adolescent",
            Self::Adult => "Adulte",
            Self::CyberGremlin => "Cyber-Gremlin",
        }
    }
}

impl fmt::Display for EvolutionStage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.display_name())
    }
}

/// État de progression et d'expérience du Gremlin.
///
/// Les champs sont privés : `level` et `stage` sont entièrement dérivés de
/// `total_xp`, invariant restauré par [`PetProgression::normalize`] après
/// désérialisation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PetProgression {
    total_xp: u64,
    level: u32,
    stage: EvolutionStage,
    total_commits: u64,
    total_tests_passed: u64,
    total_tests_failed: u64,
    total_test_runs: u64,
    total_builds_succeeded: u64,
    total_focus_secs: u64,
}

impl Default for PetProgression {
    fn default() -> Self {
        Self {
            total_xp: 0,
            level: MIN_LEVEL,
            stage: EvolutionStage::Baby,
            total_commits: 0,
            total_tests_passed: 0,
            total_tests_failed: 0,
            total_test_runs: 0,
            total_builds_succeeded: 0,
            total_focus_secs: 0,
        }
    }
}

impl PetProgression {
    /// Crée une nouvelle progression avec un montant d'XP initial.
    #[must_use]
    pub fn with_initial_xp(initial_xp: u64) -> Self {
        let level = Self::level_from_total_xp(initial_xp);
        Self {
            total_xp: initial_xp,
            level,
            stage: EvolutionStage::from_level(level),
            total_commits: 0,
            total_tests_passed: 0,
            total_tests_failed: 0,
            total_test_runs: 0,
            total_builds_succeeded: 0,
            total_focus_secs: 0,
        }
    }

    /// Expérience totale accumulée.
    #[must_use]
    pub const fn total_xp(&self) -> u64 {
        self.total_xp
    }

    /// Niveau actuel du compagnon (minimum 1).
    #[must_use]
    pub const fn level(&self) -> u32 {
        self.level
    }

    /// Stade d'évolution morphologique courant.
    #[must_use]
    pub const fn stage(&self) -> EvolutionStage {
        self.stage
    }

    /// Nombre total de commits assimilés.
    #[must_use]
    pub const fn total_commits(&self) -> u64 {
        self.total_commits
    }

    /// Nombre cumulé de tests réussis.
    #[must_use]
    pub const fn total_tests_passed(&self) -> u64 {
        self.total_tests_passed
    }

    /// Nombre cumulé de tests échoués.
    #[must_use]
    pub const fn total_tests_failed(&self) -> u64 {
        self.total_tests_failed
    }

    /// Nombre cumulé de rapports de tests assimilés.
    #[must_use]
    pub const fn total_test_runs(&self) -> u64 {
        self.total_test_runs
    }

    /// Nombre cumulé de builds réussis.
    #[must_use]
    pub const fn total_builds_succeeded(&self) -> u64 {
        self.total_builds_succeeded
    }

    /// Secondes de focus estimées et cumulées.
    #[must_use]
    pub const fn total_focus_secs(&self) -> u64 {
        self.total_focus_secs
    }

    /// Restaure l'invariant `level`/`stage` dérivés de `total_xp`.
    ///
    /// Une sauvegarde modifiée à la main peut annoncer `level: 0` ou un stade
    /// incohérent avec son XP : les deux sont recalculés.
    pub fn normalize(&mut self) {
        self.level = Self::level_from_total_xp(self.total_xp);
        self.stage = EvolutionStage::from_level(self.level);
    }

    /// Calcule le total cumulé d'XP requis pour atteindre un niveau donné depuis le niveau 1.
    ///
    /// Formule : `total_xp = 50 * level * (level - 1)`, saturée pour ne jamais
    /// déborder — le produit dépasse `u64` bien avant `u32::MAX`.
    #[must_use]
    pub const fn total_xp_for_level(level: u32) -> u64 {
        match Self::total_xp_for_level_checked(level) {
            Some(value) => value,
            None => u64::MAX,
        }
    }

    /// Variante non saturante de [`PetProgression::total_xp_for_level`].
    ///
    /// Renvoie `None` lorsque le seuil n'est pas représentable sur `u64`, ce qui
    /// distingue un palier réellement atteignable d'une valeur saturée.
    #[must_use]
    pub const fn total_xp_for_level_checked(level: u32) -> Option<u64> {
        if level <= MIN_LEVEL {
            return Some(0);
        }
        let l = level as u64;
        match l.checked_mul(l - 1) {
            Some(product) => product.checked_mul(50),
            None => None,
        }
    }

    /// Calcule l'expérience nécessaire pour passer du niveau `level` au niveau `level + 1`.
    #[must_use]
    pub const fn xp_required_for_next_level(level: u32) -> u64 {
        (level as u64).saturating_mul(100)
    }

    /// Calcule le niveau atteint à partir de l'XP totale cumulée.
    ///
    /// Recherche dichotomique en arithmétique entière : elle garantit
    /// exactement l'invariant
    /// `total_xp_for_level(n) <= xp < total_xp_for_level(n + 1)` sur tout le
    /// domaine, là où la résolution analytique en `f64` dérivait au-delà de
    /// 2^53. Bornée à 32 itérations.
    #[must_use]
    pub const fn level_from_total_xp(total_xp: u64) -> u32 {
        let mut low = MIN_LEVEL;
        let mut high = u32::MAX;

        while low < high {
            // Milieu arrondi vers le haut : indispensable pour que la borne
            // basse progresse et que la boucle termine.
            let mid = low + (high - low).div_ceil(2);
            let reachable = match Self::total_xp_for_level_checked(mid) {
                Some(threshold) => threshold <= total_xp,
                None => false,
            };
            if reachable {
                low = mid;
            } else {
                high = mid - 1;
            }
        }

        low
    }

    /// Ajoute de l'expérience et calcule les montées de niveau et évolutions débloquées.
    ///
    /// Retourne `(levels_gained, evolution_unlocked)`.
    pub fn add_xp(&mut self, amount: u64) -> (u32, Option<EvolutionStage>) {
        self.total_xp = self.total_xp.saturating_add(amount);
        let previous_level = self.level;
        let previous_stage = self.stage;

        self.level = Self::level_from_total_xp(self.total_xp);
        self.stage = EvolutionStage::from_level(self.level);

        let levels_gained = self.level.saturating_sub(previous_level);
        let evolution_unlocked = if self.stage > previous_stage {
            Some(self.stage)
        } else {
            None
        };

        (levels_gained, evolution_unlocked)
    }

    /// Enregistre la complétion d'un commit Git avec gain d'XP associé.
    pub fn record_commit(&mut self, xp_reward: u64) -> (u32, Option<EvolutionStage>) {
        self.total_commits = self.total_commits.saturating_add(1);
        self.add_xp(xp_reward)
    }

    /// Enregistre les compteurs d'un rapport de tests et attribue l'XP fournie.
    pub fn record_test_run(
        &mut self,
        passed: u32,
        failed: u32,
        xp_reward: u64,
    ) -> (u32, Option<EvolutionStage>) {
        self.total_test_runs = self.total_test_runs.saturating_add(1);
        self.total_tests_passed = self.total_tests_passed.saturating_add(u64::from(passed));
        self.total_tests_failed = self.total_tests_failed.saturating_add(u64::from(failed));
        self.add_xp(xp_reward)
    }

    /// Enregistre un build et attribue l'XP fournie.
    pub fn record_build(&mut self, success: bool, xp_reward: u64) -> (u32, Option<EvolutionStage>) {
        if success {
            self.total_builds_succeeded = self.total_builds_succeeded.saturating_add(1);
        }
        self.add_xp(xp_reward)
    }

    /// Cumule une durée de focus et attribue le bonus du palier éventuel.
    pub fn record_focus(
        &mut self,
        credited: Duration,
        xp_reward: u64,
    ) -> (u32, Option<EvolutionStage>) {
        self.total_focus_secs = self.total_focus_secs.saturating_add(credited.as_secs());
        self.add_xp(xp_reward)
    }

    /// Calcule l'XP accumulée au sein du niveau actuel.
    #[must_use]
    pub fn xp_in_current_level(&self) -> u64 {
        self.total_xp
            .saturating_sub(Self::total_xp_for_level(self.level))
    }

    /// Calcule l'XP restante nécessaire pour franchir le niveau suivant.
    #[must_use]
    pub fn xp_remaining_for_next_level(&self) -> u64 {
        Self::total_xp_for_level(self.level.saturating_add(1)).saturating_sub(self.total_xp)
    }

    /// Calcule la progression relative vers le niveau suivant, dans `[0.0, 1.0]`.
    #[must_use]
    pub fn progress_percentage_to_next_level(&self) -> f32 {
        let current_xp = self.xp_in_current_level();
        let needed_xp = Self::xp_required_for_next_level(self.level);
        if needed_xp == 0 {
            1.0
        } else {
            #[allow(clippy::cast_precision_loss)]
            let ratio = current_xp as f32 / needed_xp as f32;
            ratio.clamp(0.0, 1.0)
        }
    }
}

#[cfg(test)]
#[allow(clippy::float_cmp, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn test_evolution_stage_declaration_order_is_pinned() {
        // `add_xp` détecte une évolution via `self.stage > previous_stage` :
        // cet ordre ne doit jamais changer.
        assert!(EvolutionStage::Baby < EvolutionStage::Teen);
        assert!(EvolutionStage::Teen < EvolutionStage::Adult);
        assert!(EvolutionStage::Adult < EvolutionStage::CyberGremlin);
    }

    #[test]
    fn test_progression_math_exactness() {
        assert_eq!(PetProgression::total_xp_for_level(1), 0);
        assert_eq!(PetProgression::level_from_total_xp(0), 1);
        assert_eq!(PetProgression::level_from_total_xp(99), 1);

        assert_eq!(PetProgression::total_xp_for_level(2), 100);
        assert_eq!(PetProgression::level_from_total_xp(100), 2);
        assert_eq!(PetProgression::level_from_total_xp(299), 2);

        assert_eq!(PetProgression::total_xp_for_level(5), 1000);
        assert_eq!(EvolutionStage::from_level(5), EvolutionStage::Teen);
    }

    #[test]
    fn test_level_from_xp_stays_exact_beyond_f64_precision() {
        // Au-delà de 2^53, la résolution analytique en f64 dérivait :
        // la dichotomie entière doit garantir l'encadrement exact partout.
        for total_xp in [
            0,
            1,
            99,
            100,
            u64::MAX,
            u64::MAX / 2,
            1_u64 << 60,
            (1_u64 << 53) + 1,
        ] {
            let level = PetProgression::level_from_total_xp(total_xp);
            assert!(
                PetProgression::total_xp_for_level_checked(level)
                    .is_some_and(|threshold| threshold <= total_xp),
                "niveau {level} trop haut pour {total_xp} XP"
            );
            assert!(
                PetProgression::total_xp_for_level_checked(level.saturating_add(1))
                    .is_none_or(|threshold| threshold > total_xp),
                "niveau {level} trop bas pour {total_xp} XP"
            );
        }
    }

    #[test]
    fn test_total_xp_for_level_saturates_instead_of_wrapping() {
        // 50 * l * (l - 1) déborde u64 bien avant u32::MAX.
        assert_eq!(PetProgression::total_xp_for_level(u32::MAX), u64::MAX);
    }

    #[test]
    fn test_xp_saturates_at_ceiling() {
        let mut prog = PetProgression::default();
        prog.add_xp(u64::MAX);
        assert_eq!(prog.total_xp(), u64::MAX);

        // Un second ajout ne doit ni déborder ni régresser.
        let (levels, _) = prog.add_xp(u64::MAX);
        assert_eq!(prog.total_xp(), u64::MAX);
        assert_eq!(levels, 0);
        assert_eq!(prog.stage(), EvolutionStage::CyberGremlin);
    }

    #[test]
    fn test_incremental_add_xp_no_drift() {
        let mut prog = PetProgression::default();
        assert_eq!(prog.level(), 1);
        assert_eq!(prog.stage(), EvolutionStage::Baby);

        let (levels, evo) = prog.add_xp(50);
        assert_eq!(levels, 0);
        assert_eq!(evo, None);
        assert_eq!(prog.xp_in_current_level(), 50);
        assert_eq!(prog.xp_remaining_for_next_level(), 50);
        assert_eq!(prog.progress_percentage_to_next_level(), 0.5);

        let (levels, evo) = prog.add_xp(50);
        assert_eq!(levels, 1);
        assert_eq!(evo, None);
        assert_eq!(prog.level(), 2);
        assert_eq!(prog.xp_in_current_level(), 0);
        assert_eq!(prog.xp_remaining_for_next_level(), 200);

        let (levels, evo) = prog.add_xp(900);
        assert_eq!(levels, 3);
        assert_eq!(evo, Some(EvolutionStage::Teen));
        assert_eq!(prog.level(), 5);
        assert_eq!(prog.stage(), EvolutionStage::Teen);
    }

    #[test]
    fn test_record_commit() {
        let mut prog = PetProgression::default();
        let (levels, evo) = prog.record_commit(50);
        assert_eq!(prog.total_commits(), 1);
        assert_eq!(prog.total_xp(), 50);
        assert_eq!(levels, 0);
        assert_eq!(evo, None);
    }

    #[test]
    fn test_normalize_recomputes_level_and_stage() {
        let mut prog: PetProgression = serde_json::from_str(
            r#"{"total_xp": 10500, "level": 0, "stage": "Baby", "total_commits": 3}"#,
        )
        .expect("désérialisation");

        prog.normalize();

        assert_eq!(prog.level(), 15);
        assert_eq!(prog.stage(), EvolutionStage::Adult);
        assert_eq!(prog.total_commits(), 3);
    }
}
