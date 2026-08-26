//! Inventaire borné de consommables et transactions associées.
//!
//! Les stocks sont privés et saturants : aucune addition ne peut dépasser la
//! capacité, aucune soustraction ne peut passer sous zéro. L'inventaire ne
//! connaît pas les jauges du familier — c'est [`PetState`](crate::state::PetState)
//! qui orchestre la transaction complète, afin qu'un refus n'entame jamais le
//! stock.

use crate::config::InventoryConfig;
use crate::stats::{PetStats, MAX_STAT_VALUE};
use serde::{Deserialize, Serialize};
use std::fmt;

/// Nombre de types de consommables existants.
///
/// L'ensemble est **fermé** : le stock est un tableau fixe indexé par une
/// conversion exhaustive, pas une table de hachage réallouée à chaque action.
pub const CONSUMABLE_COUNT: usize = 3;

/// Nature d'un consommable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ConsumableKind {
    /// Café : rend de l'énergie.
    Coffee,
    /// Potion de debug : rend un peu des trois jauges.
    DebugPotion,
    /// Collation : rend de la satiété.
    Snack,
}

impl ConsumableKind {
    /// Les trois consommables, dans leur ordre canonique.
    ///
    /// Cet ordre est celui de l'inventaire, des raccourcis `1`/`2`/`3` et du
    /// repli de la récompense quotidienne. Il est donc stable.
    pub const ALL: [Self; CONSUMABLE_COUNT] = [Self::Coffee, Self::DebugPotion, Self::Snack];

    /// Index du consommable dans le tableau de stocks.
    #[must_use]
    pub const fn index(self) -> usize {
        match self {
            Self::Coffee => 0,
            Self::DebugPotion => 1,
            Self::Snack => 2,
        }
    }

    /// Libellé lisible, employé au fil d'une phrase.
    ///
    /// Minuscule : il apparaît dans « aucun exemplaire de café en stock ». Pour
    /// un titre de ligne, voir [`Self::title`].
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Coffee => "café",
            Self::DebugPotion => "potion de debug",
            Self::Snack => "collation",
        }
    }

    /// Libellé de titre, capitalisé.
    ///
    /// Une ligne de panneau commence par une capitale comme toutes les autres :
    /// capitaliser [`Self::label`] au moment de l'affichage aurait dispersé la
    /// règle de langue dans le rendu.
    #[must_use]
    pub const fn title(self) -> &'static str {
        match self {
            Self::Coffee => "Café",
            Self::DebugPotion => "Potion de debug",
            Self::Snack => "Collation",
        }
    }

    /// Identifiant stable, utilisé par l'interface et les tests.
    #[must_use]
    pub const fn id(self) -> &'static str {
        match self {
            Self::Coffee => "coffee",
            Self::DebugPotion => "debug_potion",
            Self::Snack => "snack",
        }
    }

    /// Effet que cet objet aurait **réellement** sur les jauges fournies.
    ///
    /// Les gains de la configuration sont nominaux ; les jauges étant
    /// plafonnées, un café bu à 95 d'énergie n'en rend que 5. C'est cette
    /// valeur qui décide si l'objet est utilisable, qui est appliquée et qui
    /// est rapportée — jamais la valeur demandée.
    #[must_use]
    pub fn potential_effect(self, stats: PetStats, config: &InventoryConfig) -> ConsumableEffect {
        let headroom = |current: f32| (MAX_STAT_VALUE - current).max(0.0);
        match self {
            Self::Coffee => ConsumableEffect {
                energy: config.coffee_energy.min(headroom(stats.energy())),
                ..ConsumableEffect::default()
            },
            Self::Snack => ConsumableEffect {
                satiety: config.snack_satiety.min(headroom(stats.satiety())),
                ..ConsumableEffect::default()
            },
            Self::DebugPotion => {
                let amount = config.debug_potion_amount;
                ConsumableEffect {
                    energy: amount.min(headroom(stats.energy())),
                    satiety: amount.min(headroom(stats.satiety())),
                    happiness: amount.min(headroom(stats.happiness())),
                }
            }
        }
    }
}

impl fmt::Display for ConsumableKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// Raison pour laquelle un consommable a été octroyé.
///
/// Un enum fermé plutôt qu'une chaîne libre : l'interface doit pouvoir
/// distinguer les cas sans comparer du texte.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum GrantReason {
    /// Dotation de départ d'un familier neuf.
    InitialStock,
    /// Récompense du premier commit de la journée.
    DailyReward,
}

/// Résultat d'un octroi : ce qui a été ajouté et ce que la capacité a refusé.
///
/// Le reliquat n'est jamais jeté en silence : le chemin déclenché par
/// l'utilisateur l'affiche (« stock plein »), et l'appelant peut décider de
/// reporter l'octroi sur un autre slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GrantOutcome {
    /// Quantité réellement ajoutée au stock.
    pub added: u8,
    /// Quantité refusée faute de place.
    pub rejected: u8,
}

impl GrantOutcome {
    /// Indique qu'au moins un exemplaire a été ajouté.
    #[must_use]
    pub const fn is_partial_or_full_success(self) -> bool {
        self.added > 0
    }
}

/// Effet réellement appliqué aux jauges par un consommable.
///
/// Les gains sont nominaux dans la configuration ; les jauges étant plafonnées
/// à 100, l'effet observé peut être plus faible. C'est cette valeur-là qui est
/// rapportée, jamais la valeur demandée.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
pub struct ConsumableEffect {
    /// Énergie effectivement rendue.
    pub energy: f32,
    /// Satiété effectivement rendue.
    pub satiety: f32,
    /// Bonheur effectivement rendu.
    pub happiness: f32,
}

impl ConsumableEffect {
    /// Indique que l'objet aurait au moins un effet mesurable.
    #[must_use]
    pub fn is_meaningful(self) -> bool {
        self.energy > 0.0 || self.satiety > 0.0 || self.happiness > 0.0
    }
}

/// Stocks de consommables détenus par le familier.
///
/// [`Default`] fournit la dotation de départ : une sauvegarde antérieure à la
/// phase 8 ne contient pas ce champ et reçoit donc ce stock exactement une
/// fois, à la désérialisation. Une sauvegarde récente dont l'inventaire est
/// vide reste vide — le stock initial n'est pas redistribué à chaque
/// chargement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Inventory {
    /// Quantités indexées par [`ConsumableKind::index`].
    quantities: [u8; CONSUMABLE_COUNT],
}

impl Default for Inventory {
    fn default() -> Self {
        Self::with_initial_stock(&InventoryConfig::default())
    }
}

impl Inventory {
    /// Construit l'inventaire de départ décrit par la configuration.
    #[must_use]
    pub fn with_initial_stock(config: &InventoryConfig) -> Self {
        let mut inventory = Self {
            quantities: [0; CONSUMABLE_COUNT],
        };
        inventory.quantities[ConsumableKind::Coffee.index()] = config.initial_coffee;
        inventory.quantities[ConsumableKind::DebugPotion.index()] = config.initial_debug_potion;
        inventory.quantities[ConsumableKind::Snack.index()] = config.initial_snack;
        inventory.normalize(config);
        inventory
    }

    /// Inventaire entièrement vide.
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            quantities: [0; CONSUMABLE_COUNT],
        }
    }

    /// Quantité détenue pour ce type d'objet.
    #[must_use]
    pub const fn quantity(&self, kind: ConsumableKind) -> u8 {
        self.quantities[kind.index()]
    }

    /// Nombre total d'objets détenus, tous types confondus.
    #[must_use]
    pub fn total(&self) -> u32 {
        self.quantities.iter().map(|q| u32::from(*q)).sum()
    }

    /// Indique si le stock de ce type a atteint la capacité configurée.
    #[must_use]
    pub const fn is_full(&self, kind: ConsumableKind, config: &InventoryConfig) -> bool {
        self.quantities[kind.index()] >= config.capacity
    }

    /// Ajoute des exemplaires en respectant la capacité.
    ///
    /// L'addition est saturante et son reliquat explicite : l'appelant sait
    /// exactement ce qui a été refusé.
    pub fn grant(
        &mut self,
        kind: ConsumableKind,
        amount: u8,
        config: &InventoryConfig,
    ) -> GrantOutcome {
        let slot = &mut self.quantities[kind.index()];
        let room = config.capacity.saturating_sub(*slot);
        let added = amount.min(room);
        *slot = slot.saturating_add(added);
        GrantOutcome {
            added,
            rejected: amount.saturating_sub(added),
        }
    }

    /// Retire un exemplaire, ou renvoie `false` si le stock est vide.
    ///
    /// Cette opération n'est appelée qu'**après** validation de l'effet, pour
    /// qu'un refus ne consomme jamais rien.
    pub fn take_one(&mut self, kind: ConsumableKind) -> bool {
        let slot = &mut self.quantities[kind.index()];
        if *slot == 0 {
            return false;
        }
        *slot -= 1;
        true
    }

    /// Ramène chaque stock sous la capacité configurée.
    ///
    /// Idempotente : une sauvegarde annonçant 255 exemplaires est ramenée à la
    /// capacité, et un second appel ne change plus rien.
    pub fn normalize(&mut self, config: &InventoryConfig) {
        for slot in &mut self.quantities {
            *slot = (*slot).min(config.capacity);
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::float_cmp)]
mod tests {
    use super::*;

    #[test]
    fn test_indices_are_distinct_and_exhaustive() {
        let mut indices: Vec<usize> = ConsumableKind::ALL.iter().map(|k| k.index()).collect();
        indices.sort_unstable();
        assert_eq!(indices, vec![0, 1, 2]);
    }

    #[test]
    fn test_labels_and_ids_are_distinct() {
        let mut labels: Vec<&str> = ConsumableKind::ALL.iter().map(|k| k.label()).collect();
        labels.sort_unstable();
        labels.dedup();
        assert_eq!(labels.len(), CONSUMABLE_COUNT);

        let mut ids: Vec<&str> = ConsumableKind::ALL.iter().map(|k| k.id()).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), CONSUMABLE_COUNT);
    }

    #[test]
    fn test_default_matches_the_documented_initial_stock() {
        let inventory = Inventory::default();
        assert_eq!(inventory.quantity(ConsumableKind::Coffee), 1);
        assert_eq!(inventory.quantity(ConsumableKind::DebugPotion), 1);
        assert_eq!(inventory.quantity(ConsumableKind::Snack), 2);
        assert_eq!(inventory.total(), 4);
    }

    #[test]
    fn test_empty_inventory_is_not_the_initial_stock() {
        assert_eq!(Inventory::empty().total(), 0);
    }

    #[test]
    fn test_grant_saturates_at_capacity_and_reports_the_remainder() {
        let config = InventoryConfig::default();
        let mut inventory = Inventory::empty();

        let outcome = inventory.grant(ConsumableKind::Coffee, 4, &config);
        assert_eq!(outcome.added, 4);
        assert_eq!(outcome.rejected, 0);

        let outcome = inventory.grant(ConsumableKind::Coffee, 200, &config);
        assert_eq!(outcome.added, config.capacity - 4);
        assert_eq!(outcome.rejected, 200 - (config.capacity - 4));
        assert_eq!(inventory.quantity(ConsumableKind::Coffee), config.capacity);
        assert!(inventory.is_full(ConsumableKind::Coffee, &config));

        let outcome = inventory.grant(ConsumableKind::Coffee, 1, &config);
        assert_eq!(outcome.added, 0);
        assert_eq!(outcome.rejected, 1);
        assert!(!outcome.is_partial_or_full_success());
    }

    #[test]
    fn test_granting_zero_changes_nothing() {
        let config = InventoryConfig::default();
        let mut inventory = Inventory::empty();
        let outcome = inventory.grant(ConsumableKind::Snack, 0, &config);
        assert_eq!(outcome.added, 0);
        assert_eq!(outcome.rejected, 0);
        assert_eq!(inventory.total(), 0);
    }

    #[test]
    fn test_take_one_refuses_an_empty_slot() {
        let mut inventory = Inventory::empty();
        assert!(!inventory.take_one(ConsumableKind::Coffee));
        assert_eq!(inventory.quantity(ConsumableKind::Coffee), 0);

        let config = InventoryConfig::default();
        inventory.grant(ConsumableKind::Coffee, 1, &config);
        assert!(inventory.take_one(ConsumableKind::Coffee));
        assert_eq!(inventory.quantity(ConsumableKind::Coffee), 0);
        assert!(!inventory.take_one(ConsumableKind::Coffee));
    }

    #[test]
    fn test_normalize_clamps_a_hand_edited_save_and_is_idempotent() {
        let config = InventoryConfig::default();
        let hostile = r#"{"quantities":[255,255,255]}"#;
        let mut inventory: Inventory = serde_json::from_str(hostile).unwrap();

        inventory.normalize(&config);
        let once = inventory;
        assert_eq!(inventory.quantity(ConsumableKind::Coffee), config.capacity);

        inventory.normalize(&config);
        assert_eq!(inventory, once, "normalisation non idempotente");
    }

    #[test]
    fn test_missing_field_falls_back_to_the_initial_stock() {
        let inventory: Inventory = serde_json::from_str("{}").unwrap();
        assert_eq!(inventory, Inventory::default());
    }

    #[test]
    fn test_effect_is_meaningful_only_when_a_gauge_moves() {
        assert!(!ConsumableEffect::default().is_meaningful());
        assert!(ConsumableEffect {
            energy: 0.5,
            ..ConsumableEffect::default()
        }
        .is_meaningful());
    }

    #[test]
    fn test_each_consumable_touches_only_its_own_gauges() {
        let config = InventoryConfig::default();
        let empty = PetStats::new(0.0, 0.0, 0.0);

        let coffee = ConsumableKind::Coffee.potential_effect(empty, &config);
        assert_eq!(coffee.energy, config.coffee_energy);
        assert_eq!(coffee.satiety, 0.0);
        assert_eq!(coffee.happiness, 0.0);

        let snack = ConsumableKind::Snack.potential_effect(empty, &config);
        assert_eq!(snack.satiety, config.snack_satiety);
        assert_eq!(snack.energy, 0.0);

        let potion = ConsumableKind::DebugPotion.potential_effect(empty, &config);
        assert_eq!(potion.energy, config.debug_potion_amount);
        assert_eq!(potion.satiety, config.debug_potion_amount);
        assert_eq!(potion.happiness, config.debug_potion_amount);
    }

    #[test]
    fn test_effect_is_clipped_by_the_remaining_headroom() {
        let config = InventoryConfig::default();
        let nearly_full = PetStats::new(95.0, 100.0, 90.0);

        let coffee = ConsumableKind::Coffee.potential_effect(nearly_full, &config);
        assert_eq!(coffee.energy, 5.0, "le plafond de jauge n'est pas respecté");

        let snack = ConsumableKind::Snack.potential_effect(nearly_full, &config);
        assert!(!snack.is_meaningful(), "satiété pleine mais effet annoncé");

        let potion = ConsumableKind::DebugPotion.potential_effect(nearly_full, &config);
        assert_eq!(potion.energy, 5.0);
        assert_eq!(potion.satiety, 0.0);
        assert_eq!(potion.happiness, 10.0);
        assert!(potion.is_meaningful());
    }

    #[test]
    fn test_a_potion_on_three_full_gauges_has_no_effect() {
        let config = InventoryConfig::default();
        let full = PetStats::new(100.0, 100.0, 100.0);
        assert!(!ConsumableKind::DebugPotion
            .potential_effect(full, &config)
            .is_meaningful());
    }
}
