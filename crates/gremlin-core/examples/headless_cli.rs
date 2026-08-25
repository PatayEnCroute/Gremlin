//! # Simulateur Headless en Ligne de Commande pour `gremlin-core`
//!
//! Permet d'interagir et d'exécuter des simulations du familier sans aucune interface graphique.

#![allow(
    clippy::too_many_lines,
    clippy::significant_drop_tightening,
    clippy::print_stdout,
    clippy::use_debug
)]

use gremlin_core::{CoreEvent, PetState};
use std::io::{self, BufRead, Write};
use std::time::Duration;

fn render_progress_bar(percentage: f32, width: usize) -> String {
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    let filled = ((percentage * width as f32).round() as usize).min(width);
    let empty = width.saturating_sub(filled);
    let mut bar = String::with_capacity(width + 2);
    bar.push('[');
    bar.push_str(&"=".repeat(filled));
    bar.push_str(&" ".repeat(empty));
    bar.push(']');
    bar
}

fn print_status(pet: &PetState) {
    println!("\n╔═══════════════════════════════════════════════════════════════╗");
    let progression = pet.progression();
    let stats = pet.stats();

    println!(
        "║  FAMILIER : {:<15}   HUMEUR : {:<20} ║",
        pet.name(),
        pet.mood().display_name()
    );
    println!(
        "║  STADE : {:<17}   NIVEAU : {:<20} ║",
        progression.stage().display_name(),
        progression.level()
    );
    println!(
        "║  COMMITS : {:<15}   XP TOTALE : {:<17} ║",
        progression.total_commits(),
        progression.total_xp()
    );
    println!("╠═══════════════════════════════════════════════════════════════╣");

    let pct_xp = progression.progress_percentage_to_next_level();
    let bar_xp = render_progress_bar(pct_xp, 20);
    println!(
        "║  XP Suivante : {:<22} {:>3.0}% (reste {} XP)  ║",
        bar_xp,
        pct_xp * 100.0,
        progression.xp_remaining_for_next_level()
    );

    let bar_energy = render_progress_bar(stats.energy() / 100.0, 15);
    let bar_satiety = render_progress_bar(stats.satiety() / 100.0, 15);
    let bar_happiness = render_progress_bar(stats.happiness() / 100.0, 15);

    println!(
        "║  Énergie  : {:<17} {:>5.1} / 100.0                     ║",
        bar_energy,
        stats.energy()
    );
    println!(
        "║  Satiété  : {:<17} {:>5.1} / 100.0                     ║",
        bar_satiety,
        stats.satiety()
    );
    println!(
        "║  Bonheur  : {:<17} {:>5.1} / 100.0                     ║",
        bar_happiness,
        stats.happiness()
    );
    println!(
        "║  Sommeil  : {:<10}          Timer Code : {:>4.1}s           ║",
        if pet.is_sleeping() {
            "Actif"
        } else {
            "Inactif"
        },
        pet.coding_timer_secs()
    );
    println!("╚═══════════════════════════════════════════════════════════════╝\n");
}

fn print_events(events: &[CoreEvent]) {
    for event in events {
        match event {
            CoreEvent::CommitReceived {
                repo,
                branch,
                xp_gained,
            } => {
                println!("  [GIT] Commit reçu sur {repo}:{branch} (+{xp_gained} XP)");
            }
            CoreEvent::TestRunReceived {
                repo,
                summary,
                xp_gained,
                ..
            } => println!(
                "  [TEST] {repo}: {} réussis, {} échoués (+{xp_gained} XP)",
                summary.passed(),
                summary.failed()
            ),
            CoreEvent::BuildCompleted {
                repo,
                summary,
                xp_gained,
                ..
            } => println!(
                "  [BUILD] {repo}: {} (+{xp_gained} XP)",
                if summary.success() {
                    "réussi"
                } else {
                    "échoué"
                }
            ),
            CoreEvent::FocusMilestoneReached { duration, bonus_xp } => println!(
                "  [FOCUS] Palier de {} min (+{bonus_xp} XP)",
                duration.as_secs() / 60
            ),
            CoreEvent::BreakRecommended { .. } => {
                println!("  [FOCUS] Une pause est recommandée.");
            }
            CoreEvent::IdleStateChanged { is_idle } => println!(
                "  [ACTIVITÉ] {}",
                if *is_idle { "inactif" } else { "de retour" }
            ),
            CoreEvent::MoodChanged { from, to } => {
                println!(
                    "  [HUMEUR] Transition : {} -> {}",
                    from.display_name(),
                    to.display_name()
                );
            }
            CoreEvent::LevelUp {
                new_level,
                total_xp,
            } => {
                println!(
                    "  ★ [NIVEAU SUPÉRIEUR] Nouveau niveau : {new_level} (Total XP: {total_xp})"
                );
            }
            CoreEvent::EvolutionUnlocked { new_stage } => {
                println!(
                    "  ★ [ÉVOLUTION] Le Gremlin a évolué en : {} !",
                    new_stage.display_name()
                );
            }
            CoreEvent::Fed { amount } => {
                println!("  [ACTION] Nourri (+{amount:.1} satiété)");
            }
            CoreEvent::Petted { amount } => {
                println!("  [ACTION] Caressé (+{amount:.1} bonheur)");
            }
            CoreEvent::Healed { amount } => {
                println!("  [ACTION] Soigné (+{amount:.1} vitalité)");
            }
            CoreEvent::Rested { amount } => {
                println!("  [ACTION] Reposé (+{amount:.1} énergie)");
            }
            CoreEvent::FellAsleep => {
                println!("  [SOMMEIL] Le Gremlin s'est endormi zZz...");
            }
            CoreEvent::WokeUp => {
                println!("  [RÉVEIL] Le Gremlin est réveillé !");
            }
            CoreEvent::Died => {
                println!("  ☠ [MORT] Le Gremlin s'est éteint par négligence.");
            }
            CoreEvent::Revived => {
                println!("  ❤ [RENAISSANCE] Le Gremlin a été ressuscité !");
            }
            CoreEvent::StatsDecayed { .. } => {}
        }
    }
}

fn print_help() {
    println!("\nCommandes disponibles :");
    println!("  status                        - Afficher l'état détaillé");
    println!("  feed [quantite]               - Nourrir le familier (défaut: 30)");
    println!("  pet [quantite]                - Caresser / réjouir (défaut: 15)");
    println!("  heal [quantite]               - Soigner le familier (défaut: 40)");
    println!("  rest [quantite]               - Donner de l'énergie (défaut: 25)");
    println!("  sleep                         - Mettre en mode sommeil");
    println!("  wake                          - Réveiller");
    println!("  commit [repo] [branche]       - Simuler un commit Git");
    println!("  tick <minutes>                - Faire avancer le temps de N minutes");
    println!("  simulate <jours> [commits/j]  - Simuler plusieurs jours accélérés");
    println!("  revive                        - Ressusciter un familier décédé");
    println!("  export                        - Exporter l'état en JSON");
    println!("  help                          - Afficher ce menu");
    println!("  exit / quit                   - Quitter\n");
}

fn main() -> io::Result<()> {
    println!("=====================================================");
    println!("   GREMLIN — Simulateur Headless CLI (Phase 1)       ");
    println!("=====================================================");

    let mut pet = PetState::new("Gizmo");
    print_status(&pet);
    print_help();

    let stdin = io::stdin();
    let mut reader = stdin.lock();

    loop {
        print!("gremlin> ");
        io::stdout().flush()?;

        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            break;
        }

        let input = line.trim();
        if input.is_empty() {
            continue;
        }

        let parts: Vec<&str> = input.split_whitespace().collect();
        let cmd = parts[0].to_lowercase();

        match cmd.as_str() {
            "status" => {
                print_status(&pet);
            }
            "feed" => {
                let amount = parts.get(1).and_then(|s| s.parse::<f32>().ok());
                match pet.feed(amount) {
                    Ok(events) => print_events(&events),
                    Err(e) => println!("  Erreur : {e}"),
                }
            }
            "pet" => {
                let amount = parts.get(1).and_then(|s| s.parse::<f32>().ok());
                match pet.pet(amount) {
                    Ok(events) => print_events(&events),
                    Err(e) => println!("  Erreur : {e}"),
                }
            }
            "heal" => {
                let amount = parts.get(1).and_then(|s| s.parse::<f32>().ok());
                match pet.heal(amount) {
                    Ok(events) => print_events(&events),
                    Err(e) => println!("  Erreur : {e}"),
                }
            }
            "rest" => {
                let amount = parts.get(1).and_then(|s| s.parse::<f32>().ok());
                match pet.rest(amount) {
                    Ok(events) => print_events(&events),
                    Err(e) => println!("  Erreur : {e}"),
                }
            }
            "sleep" => match pet.sleep() {
                Ok(events) => print_events(&events),
                Err(e) => println!("  Erreur : {e}"),
            },
            "wake" => match pet.wake_up() {
                Ok(events) => print_events(&events),
                Err(e) => println!("  Erreur : {e}"),
            },
            "commit" => {
                let repo = parts.get(1).copied().unwrap_or("my-project");
                let branch = parts.get(2).copied().unwrap_or("main");
                match pet.handle_commit(repo, branch) {
                    Ok(events) => print_events(&events),
                    Err(e) => println!("  Erreur : {e}"),
                }
            }
            "tick" => {
                let minutes = parts
                    .get(1)
                    .and_then(|s| s.parse::<u64>().ok())
                    .unwrap_or(60);
                // `minutes * 60` débordait sur une saisie extrême.
                let seconds = minutes.saturating_mul(60);
                println!("  Avance rapide de {minutes} minutes...");
                let events = pet.tick(Duration::from_secs(seconds));
                print_events(&events);
            }
            "simulate" => {
                let days = parts
                    .get(1)
                    .and_then(|s| s.parse::<u32>().ok())
                    .unwrap_or(7);
                let commits_per_day = parts
                    .get(2)
                    .and_then(|s| s.parse::<u32>().ok())
                    .unwrap_or(5);
                println!(
                    "  Simulation accélérée : {days} jours avec {commits_per_day} commits/jour..."
                );

                for day in 1..=days {
                    if !pet.is_alive() {
                        println!("  [JOUR {day}] Le familier est décédé.");
                        break;
                    }
                    for _ in 0..commits_per_day {
                        match pet.handle_commit("gremlin-sim", "main") {
                            Ok(events) => print_events(&events),
                            Err(e) => println!("  Erreur : {e}"),
                        }
                        let _ = pet.tick(Duration::from_secs(30 * 60));
                    }
                    let _ = pet.feed(Some(30.0));
                    let _ = pet.sleep();
                    let _ = pet.tick(Duration::from_secs(8 * 3600));
                    let _ = pet.wake_up();
                }
                print_status(&pet);
            }
            "revive" => match pet.revive() {
                Ok(events) => print_events(&events),
                Err(e) => println!("  Erreur : {e}"),
            },
            "export" => match pet.to_json() {
                Ok(json) => println!("{json}"),
                Err(e) => println!("  Erreur d'exportation : {e}"),
            },
            "help" => {
                print_help();
            }
            "exit" | "quit" => {
                println!("Fermeture du simulateur.");
                break;
            }
            _ => {
                println!("Commande inconnue: '{cmd}'. Tapez 'help' pour la liste des commandes.");
            }
        }
    }

    Ok(())
}
