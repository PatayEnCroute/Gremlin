//! Tests d'intégration et simulations de cycle de vie pour `gremlin-core`.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::float_cmp)]

use gremlin_core::{
    ActionKind, CivilDate, ConsumableKind, CoreConfig, CoreError, CoreEvent, EvolutionStage,
    PauseReason, PetMood, PetProgression, PetState, PetStats, PomodoroState, StreakReward,
    MAX_CATCHUP_DURATION_SECS,
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

// -----------------------------------------------------------------------------
// Phase 8 : série, inventaire et concentration, sans jamais lire l'horloge
// -----------------------------------------------------------------------------

/// Date de test, construite ou test échoué : aucune date approximative.
fn day(year: i32, month: u8, day: u8) -> CivilDate {
    CivilDate::new(year, month, day).expect("date de test valide")
}

#[test]
fn test_a_full_week_of_work_without_any_real_clock() {
    let mut pet = PetState::new("Gizmo");
    let start = day(2024, 5, 1);
    let mut unlocked = Vec::new();
    let mut granted = 0_usize;

    // Sept jours de travail : un commit par jour, et une journée simulée entre
    // deux. Aucune horloge n'est lue — les dates comme les durées sont injectées.
    for offset in 0..7 {
        let today = start.checked_add_days(offset).expect("jour de test");

        let commit = pet
            .handle_commit("gremlin", "main")
            .expect("le familier accepte un commit");
        assert!(commit
            .iter()
            .any(|event| matches!(event, CoreEvent::CommitReceived { .. })));

        for event in pet.record_commit_activity(today, today) {
            match event {
                CoreEvent::StreakRewardUnlocked { reward, .. } => unlocked.push(reward),
                CoreEvent::ConsumableGranted { .. } => granted += 1,
                _ => {}
            }
        }

        // Le jour civil et le temps simulé sont deux choses distinctes : le
        // premier est injecté, le second s'écoule. Simuler vingt-quatre heures
        // de négligence par jour tuerait le familier au troisième jour, ce qui
        // n'apprendrait rien sur la règle de série — le scénario garde donc des
        // sessions courtes et nourrit le familier au passage.
        pet.tick(Duration::from_secs(15 * 60));
        for kind in [ConsumableKind::Snack, ConsumableKind::Coffee] {
            // Un objet sans effet ou hors stock est refusé : c'est attendu, et
            // le refus ne consomme rien.
            let _ = pet.use_consumable(kind);
        }
        assert!(pet.is_alive(), "le familier doit survivre à la semaine");
    }

    let last_day = start.checked_add_days(6).expect("dernier jour");
    let streak = pet.productivity().streak();
    assert_eq!(streak.current_streak(last_day), 7);
    assert_eq!(streak.longest_days(), 7);
    assert_eq!(streak.total_productive_days(), 7);
    assert_eq!(
        unlocked,
        vec![StreakReward::LeafPin, StreakReward::FocusHeadphones],
        "les paliers 3 et 7 débloquent une seule récompense chacun"
    );
    assert_eq!(granted, 7, "une récompense quotidienne par jour actif");
    assert!(!streak.is_unlocked(StreakReward::AuroraAura));
}

#[test]
fn test_the_grace_day_then_the_break_without_touching_the_records() {
    let mut pet = PetState::new("Gizmo");
    let start = day(2024, 5, 1);
    for offset in 0..4 {
        let today = start.checked_add_days(offset).expect("jour de test");
        pet.record_commit_activity(today, today);
    }

    let last = start.checked_add_days(3).expect("dernier jour actif");
    assert_eq!(pet.productivity().streak().current_streak(last), 4);

    // Lendemain sans commit : la série reste affichée toute la journée.
    let grace = start.checked_add_days(4).expect("lendemain");
    assert_eq!(pet.productivity().streak().current_streak(grace), 4);
    assert!(
        pet.refresh_current_day(grace).is_empty(),
        "rien à annoncer tant que la série ne bouge pas"
    );

    // Deuxième jour manqué : elle tombe, les records survivent.
    let broken = start.checked_add_days(5).expect("surlendemain");
    let events = pet.refresh_current_day(broken);
    assert!(events.iter().any(|event| matches!(
        event,
        CoreEvent::StreakChanged {
            current_days: 0,
            longest_days: 4,
        }
    )));
    assert_eq!(pet.productivity().streak().longest_days(), 4);
    assert_eq!(pet.productivity().streak().total_productive_days(), 4);
}

#[test]
fn test_an_offline_catchup_never_advances_the_focus_timer() {
    let mut pet = PetState::new("Gizmo");
    pet.start_pomodoro().expect("minuteur démarrable");
    let started = pet
        .productivity()
        .pomodoro()
        .remaining()
        .expect("session en cours");

    // Un mois de rattrapage hors-ligne : les jauges plongent, le minuteur non.
    pet.tick(Duration::from_secs(MAX_CATCHUP_DURATION_SECS));
    assert_eq!(pet.productivity().pomodoro().remaining(), Some(started));
    assert_eq!(pet.productivity().pomodoro().completed_work_blocks(), 0);
}

#[test]
fn test_a_full_focus_cycle_grants_nothing_farmable() {
    let mut config = CoreConfig::new();
    config.pomodoro.work_secs = 60;
    config.pomodoro.short_break_secs = 30;
    config.normalize();

    let mut pet = PetState::with_config("Gizmo", config);
    let xp_before = pet.progression().total_xp();
    let stock_before = pet.productivity().inventory().total();
    pet.start_pomodoro().expect("minuteur démarrable");

    let events = pet.advance_live_productivity(Duration::from_secs(60));
    assert!(events.iter().any(|event| matches!(
        event,
        CoreEvent::PomodoroPhaseCompleted {
            completed_work_blocks: 1,
            ..
        }
    )));
    assert!(events
        .iter()
        .any(|event| matches!(event, CoreEvent::WellbeingReminder { .. })));

    assert_eq!(pet.progression().total_xp(), xp_before, "XP farmable");
    assert_eq!(
        pet.productivity().inventory().total(),
        stock_before,
        "objet farmable"
    );
    assert_eq!(
        pet.productivity().streak().total_productive_days(),
        0,
        "journée de série fabriquée par le minuteur"
    );
}

#[test]
fn test_a_phase_seven_save_loads_with_phase_eight_defaults() {
    // Sauvegarde telle qu'écrite avant la phase 8 : aucun champ de productivité.
    let legacy = r#"{
        "version": 1,
        "name": "Ancien Gremlin",
        "stats": { "energy": 62.0, "satiety": 48.0, "happiness": 71.0 },
        "mood": "Happy",
        "progression": { "level": 4, "total_xp": 640, "total_commits": 12 },
        "is_sleeping": false,
        "coding_timer_secs": 0.0
    }"#;

    let pet = PetState::from_json(legacy).expect("sauvegarde de phase 7 lisible");
    assert_eq!(pet.name(), "Ancien Gremlin");
    assert_eq!(pet.progression().total_commits(), 12);

    // La dotation de départ est attribuée exactement une fois, à la lecture du
    // champ absent — pas à chaque chargement.
    let inventory = pet.productivity().inventory();
    assert_eq!(inventory.quantity(ConsumableKind::Coffee), 1);
    assert_eq!(inventory.quantity(ConsumableKind::DebugPotion), 1);
    assert_eq!(inventory.quantity(ConsumableKind::Snack), 2);

    assert_eq!(pet.productivity().streak().longest_days(), 0);
    assert_eq!(pet.productivity().pomodoro().state(), PomodoroState::Idle);

    // Un aller-retour conserve l'inventaire au lieu de le redistribuer.
    let json = pet.to_json().expect("sérialisation");
    let reloaded = PetState::from_json(&json).expect("relecture");
    assert_eq!(reloaded.productivity().inventory(), inventory);
}

#[test]
fn test_a_running_timer_in_a_save_is_repaired_at_load() {
    let mut pet = PetState::new("Gizmo");
    pet.start_pomodoro().expect("minuteur démarrable");
    pet.advance_live_productivity(Duration::from_secs(120));
    let remaining = pet.productivity().pomodoro().remaining();

    let json = pet.to_json().expect("sérialisation");
    let reloaded = PetState::from_json(&json).expect("relecture");

    // Le temps restant survit, la prétention d'avoir mesuré non.
    assert_eq!(reloaded.productivity().pomodoro().remaining(), remaining);
    assert!(!reloaded.productivity().pomodoro().is_running());
    assert_eq!(
        reloaded.productivity().pomodoro().pause_reason(),
        Some(PauseReason::Restarted)
    );
}

#[test]
fn test_a_hostile_productivity_save_is_normalised_without_losing_the_records() {
    // Jours dupliqués et hors calendrier, quantités absurdes, minuteur bloqué.
    let hostile = r#"{
        "version": 1,
        "name": "Bricolé",
        "productivity": {
            "streak": {
                "active_days": [19800, 19800, -12, 2147483647, 19801],
                "longest_days": 65535,
                "total_productive_days": 0,
                "unlocked_rewards_mask": 255
            },
            "inventory": { "quantities": [255, 255, 255] },
            "pomodoro": {
                "state": { "Running": { "phase": "Work", "remaining_millis": 0,
                                        "completed_work_blocks": 65535 } },
                "reminder_index": 0
            }
        }
    }"#;

    let pet = PetState::from_json(hostile).expect("sauvegarde hostile lisible");
    let productivity = pet.productivity();

    // Deux jours réels retenus, les valeurs hors calendrier écartées.
    assert_eq!(
        productivity
            .streak()
            .current_streak(CivilDate::from_day_number(19_801).expect("jour de test")),
        2
    );
    assert!(productivity.streak().longest_days() <= 36_500);
    assert!(productivity.streak().total_productive_days() >= 2);

    // Les stocks sont ramenés sous la capacité, le minuteur réarmé et suspendu.
    let capacity = pet.config().inventory.capacity;
    for kind in ConsumableKind::ALL {
        assert_eq!(productivity.inventory().quantity(kind), capacity);
    }
    assert!(productivity
        .pomodoro()
        .remaining()
        .is_some_and(|remaining| {
            remaining == Duration::from_secs(u64::from(pet.config().pomodoro.work_secs))
        }));
    assert_eq!(
        productivity.pomodoro().pause_reason(),
        Some(PauseReason::Restarted)
    );

    // La normalisation est idempotente : un second aller-retour ne change rien.
    let json = pet.to_json().expect("sérialisation");
    let twice = PetState::from_json(&json).expect("relecture");
    assert_eq!(twice.productivity(), productivity);
}
