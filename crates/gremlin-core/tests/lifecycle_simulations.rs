//! Tests d'intégration et simulations de cycle de vie pour `gremlin-core`.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::float_cmp)]

use gremlin_core::{
    ActionKind, CoreConfig, CoreError, CoreEvent, EvolutionStage, PetMood, PetProgression,
    PetState, PetStats, MAX_CATCHUP_DURATION_SECS,
};
use std::time::Duration;

#[test]
fn test_prolonged_neglect_leads_to_death_timeline() {
    let mut pet = PetState::new("TestGremlin");
    assert_eq!(pet.mood(), PetMood::Happy);
    assert!(pet.is_alive());

    // Après 30 minutes : forme nominale (satiété 70 %, énergie 85 %).
    pet.tick(Duration::from_secs(30 * 60));
    assert_eq!(pet.mood(), PetMood::Happy);
    assert!((pet.stats().satiety() - 70.0).abs() < 1.0);

    // Après 40 minutes supplémentaires (total 70 min) : satiété 30 % -> Hungry.
    pet.tick(Duration::from_secs(40 * 60));
    assert_eq!(pet.mood(), PetMood::Hungry);

    // Après 20 minutes supplémentaires (total 90 min) : satiété 10 % -> Sick.
    pet.tick(Duration::from_secs(20 * 60));
    assert_eq!(pet.mood(), PetMood::Sick);

    // Après 60 minutes supplémentaires (total 150 min) : satiété à 0 %, énergie à 25 %.
    pet.tick(Duration::from_secs(60 * 60));
    assert_eq!(pet.stats().satiety(), 0.0);
    assert!((pet.stats().energy() - 25.0).abs() < 1.0);
    assert_eq!(pet.mood(), PetMood::Sick);
    assert!(pet.is_alive());

    // Après 60 minutes supplémentaires (total 210 min) : énergie et satiété à 0 -> mort.
    let events = pet.tick(Duration::from_secs(60 * 60));
    assert_eq!(pet.mood(), PetMood::Dead);
    assert!(!pet.is_alive());
    assert!(events.contains(&CoreEvent::Died));
}

#[test]
fn test_regular_commit_flow_progression() {
    let mut pet = PetState::new("WorkaholicGremlin");

    // Simulation d'une semaine de dev : 5 jours, 10 commits par jour.
    let mut total_commits_emitted = 0;
    let mut evolutions_reached = Vec::new();

    for _day in 1..=5 {
        for _commit in 1..=10 {
            let events = pet
                .handle_commit("gremlin-repo", "feature-branch")
                .expect("le familier doit rester vivant sur ce scénario");
            total_commits_emitted += 1;

            for event in events {
                if let CoreEvent::EvolutionUnlocked { new_stage } = event {
                    evolutions_reached.push(new_stage);
                }
            }

            pet.tick(Duration::from_secs(10 * 60));
        }

        let _ = pet.feed(Some(30.0));
        let _ = pet.pet(Some(20.0));

        let _ = pet.sleep();
        pet.tick(Duration::from_secs(8 * 3600));
        let _ = pet.wake_up();
    }

    assert_eq!(pet.progression().total_commits(), total_commits_emitted);
    assert_eq!(total_commits_emitted, 50);

    // 50 commits * 50 XP = 2500 XP -> niveau 7, palier Teen atteint à 1000 XP.
    assert_eq!(pet.progression().total_xp(), 2500);
    assert_eq!(pet.progression().level(), 7);
    assert_eq!(pet.progression().stage(), EvolutionStage::Teen);
    assert!(evolutions_reached.contains(&EvolutionStage::Teen));
    assert!(pet.is_alive());
}

#[test]
fn test_xp_progression_invariants_and_exact_thresholds() {
    // Invariance mathématique sur 100 niveaux.
    for level in 1..=100 {
        let total_xp = PetProgression::total_xp_for_level(level);
        let calculated_level = PetProgression::level_from_total_xp(total_xp);
        assert_eq!(
            calculated_level, level,
            "Échec au niveau {level} pour {total_xp} XP"
        );

        if total_xp > 0 {
            let just_before_level = PetProgression::level_from_total_xp(total_xp - 1);
            assert_eq!(
                just_before_level,
                level - 1,
                "Échec juste avant le niveau {level}"
            );
        }
    }

    // Équivalence entre ajouts unitaires (+1 XP) et ajouts par paquets (+50 XP).
    let mut prog_unitary = PetProgression::default();
    let mut prog_batch = PetProgression::default();

    for _ in 0..5000 {
        prog_unitary.add_xp(1);
    }
    for _ in 0..100 {
        prog_batch.add_xp(50);
    }

    assert_eq!(prog_unitary.total_xp(), 5000);
    assert_eq!(prog_batch.total_xp(), 5000);
    assert_eq!(prog_unitary.level(), prog_batch.level());
    assert_eq!(prog_unitary.stage(), prog_batch.stage());
    assert_eq!(
        prog_unitary.xp_in_current_level(),
        prog_batch.xp_in_current_level()
    );
}

#[test]
fn test_sleep_mode_slows_decay_overnight() {
    let mut pet_awake = PetState::new("Awake");
    let mut pet_asleep = PetState::new("Asleep");

    let _ = pet_asleep.sleep();

    let night_duration = Duration::from_secs(8 * 3600);
    pet_awake.tick(night_duration);
    pet_asleep.tick(night_duration);

    // Éveillé : 8 h * 60 min * 0.5 = 240 points d'énergie perdus -> plancher.
    assert_eq!(pet_awake.stats().energy(), 0.0);
    assert_eq!(pet_awake.stats().satiety(), 0.0);

    // Endormi : 10 % du taux normal.
    // Énergie : 100 - (8*60 * 0.5 * 0.1) = 76.0 ; satiété : 100 - 48 = 52.0.
    assert!((pet_asleep.stats().energy() - 76.0).abs() < 1.0);
    assert!((pet_asleep.stats().satiety() - 52.0).abs() < 1.0);
    assert!(pet_asleep.is_alive());
}

#[test]
fn test_revive_flow_resets_alive_state() {
    let mut pet = PetState::new("ZombieGremlin");
    pet.set_stats(PetStats::new(0.0, 0.0, 0.0));
    assert_eq!(pet.mood(), PetMood::Dead);

    // Actions interdites en état de mort.
    assert!(matches!(
        pet.feed(None),
        Err(CoreError::PetIsDead(ActionKind::Feed))
    ));
    assert!(matches!(
        pet.pet(None),
        Err(CoreError::PetIsDead(ActionKind::Pet))
    ));
    assert!(matches!(
        pet.sleep(),
        Err(CoreError::PetIsDead(ActionKind::Sleep))
    ));
    assert!(matches!(
        pet.handle_commit("repo", "main"),
        Err(CoreError::PetIsDead(ActionKind::Commit))
    ));

    let events = pet.revive();
    assert!(events.is_ok());
    assert_eq!(pet.mood(), PetMood::Happy);
    assert_eq!(pet.stats().energy(), 100.0);
    assert_eq!(pet.stats().satiety(), 100.0);
    assert_eq!(pet.stats().happiness(), 100.0);
    assert!(pet.is_alive());

    assert!(pet.feed(None).is_ok());
}

#[test]
fn test_offline_time_catchup_consistency() {
    // Comparaison sur une plage où aucune jauge n'a encore atteint son
    // plancher : sinon l'égalité serait trivialement vraie. La satiété est la
    // plus rapide (1.0/min), elle s'épuise en 100 minutes.
    let mut pet_single = PetState::new("Jump90m");
    let mut pet_stepped = PetState::new("Stepped90m");

    pet_single.tick(Duration::from_secs(90 * 60));
    for _ in 0..3 {
        pet_stepped.tick(Duration::from_secs(30 * 60));
    }

    assert!(pet_single.stats().energy() > 0.0);
    assert!(pet_single.stats().satiety() > 0.0);
    assert!(pet_single.stats().happiness() > 0.0);
    assert_eq!(pet_single.stats().energy(), pet_stepped.stats().energy());
    assert_eq!(pet_single.stats().satiety(), pet_stepped.stats().satiety());
    assert_eq!(
        pet_single.stats().happiness(),
        pet_stepped.stats().happiness()
    );
    assert_eq!(pet_single.mood(), pet_stepped.mood());
}

#[test]
fn test_catchup_is_capped_and_always_terminates() {
    // Un horodatage corrompu produit un delta absurde : la simulation doit
    // rester bornée et rendre la main.
    let mut pet = PetState::new("CorruptedTimestamp");
    let events = pet.tick(Duration::from_secs(u64::MAX));

    assert_eq!(pet.mood(), PetMood::Dead);
    assert!(events.contains(&CoreEvent::Died));

    // Le plafond doit produire exactement le même résultat que la durée maximale.
    let mut capped_pet = PetState::new("CorruptedTimestamp");
    capped_pet.tick(Duration::from_secs(MAX_CATCHUP_DURATION_SECS));
    assert_eq!(pet.stats(), capped_pet.stats());
}

#[test]
fn test_full_state_serde_roundtrip() {
    let mut pet = PetState::with_config("CustomGremlin", CoreConfig::new());
    pet.handle_commit("alpha", "main").unwrap();
    let _ = pet.pet(Some(25.0));

    let serialized = pet.to_json().expect("Serialization should succeed");
    let deserialized = PetState::from_json(&serialized).expect("Deserialization should succeed");

    assert_eq!(pet, deserialized);
    assert_eq!(deserialized.name(), "CustomGremlin");
    assert_eq!(deserialized.progression().total_commits(), 1);
}

#[test]
fn test_hostile_save_cannot_break_the_engine() {
    // Décroissance négative (jauges croissantes), pas de simulation nul,
    // niveau incohérent et jauges hors bornes réunis dans une seule sauvegarde.
    let hostile = r#"{
        "version": 1,
        "name": "",
        "stats": { "energy": 1e30, "satiety": -1e30, "happiness": 12.0 },
        "mood": "Happy",
        "progression": { "total_xp": 43500, "level": 1, "stage": "Baby", "total_commits": 0 },
        "config": {
            "decay": { "energy_decay_per_minute": -999.0, "satiety_decay_per_minute": 1.0,
                       "happiness_decay_per_minute": 0.8, "sleep_decay_multiplier": 42.0 },
            "actions": { "commit_xp_reward": 50, "coding_duration_secs": -5.0 },
            "catchup_step_secs": 0
        },
        "is_sleeping": false,
        "coding_timer_secs": 1e30
    }"#;

    let mut pet = PetState::from_json(hostile).expect("une sauvegarde abîmée doit être réparée");

    assert!(pet.config().validate().is_ok());
    assert_eq!(pet.progression().level(), 30);
    assert_eq!(pet.progression().stage(), EvolutionStage::CyberGremlin);
    assert_eq!(pet.stats().energy(), 100.0);
    assert_eq!(pet.stats().satiety(), 0.0);

    // Le moteur reste exploitable et les jauges ne croissent pas avec le temps.
    let energy_before = pet.stats().energy();
    pet.tick(Duration::from_secs(3600));
    assert!(pet.stats().energy() <= energy_before);
    assert!(pet.to_json().is_ok());
}
