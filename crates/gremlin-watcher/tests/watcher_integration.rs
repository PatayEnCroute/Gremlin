//! Tests d'intégration pour la surveillance passive des dépôts Git (`gremlin-watcher`).
//!
//! Règle de conception de cette suite : **aucune assertion ne repose sur une durée
//! fixe**. Chaque test attend un signal précis avec un délai généreux, ou observe
//! l'absence d'un signal pendant une période de calme. Une machine chargée ralentit
//! les tests, elle ne les fait pas échouer.

// Un test peut paniquer : c'est même sa façon d'échouer. La règle du workspace
// qui bannit `expect` vise le code de production, pas les assertions d'une suite.
#![allow(clippy::expect_used)]

mod common;

use common::{
    assert_no_signal, create_dir, init_repo, ref_file, remove_tree, simulate_commit, test_config,
    tracked_config, wait_for, wait_for_all, wait_for_discoveries, wait_for_discovery, write_file,
    write_reflog, TempDirGuard,
};
use crossbeam_channel::unbounded;
use gremlin_watcher::{DevSignal, RepoWatcher, WatcherConfig, WatcherStatus};

/// Raccourci : construit un watcher ou fait échouer le test.
fn new_watcher(
    sender: crossbeam_channel::Sender<DevSignal>,
    config: &WatcherConfig,
) -> RepoWatcher {
    match RepoWatcher::new_with_config(sender, config) {
        Ok(watcher) => watcher,
        Err(e) => panic!("impossible de démarrer le RepoWatcher : {e}"),
    }
}

/// Raccourci : enregistre un dépôt ou fait échouer le test.
fn watch_repo(watcher: &mut RepoWatcher, repo: &std::path::Path) {
    if let Err(e) = watcher.watch_repo(repo) {
        panic!("l'enregistrement de {} doit réussir : {e}", repo.display());
    }
}

/// Raccourci : monte les dépôts déclarés, en exigeant qu'aucun n'échoue.
fn arm(watcher: &mut RepoWatcher) {
    match watcher.arm_tracked_repos() {
        Ok(failures) if failures.is_empty() => {}
        Ok(failures) => panic!("dépôts configurés non montés : {failures:?}"),
        Err(e) => panic!("l'armement des dépôts configurés doit réussir : {e}"),
    }
}

#[test]
fn test_end_to_end_git_commit_detection() {
    let guard = TempDirGuard::new("commit_detect");
    let repo = guard.path().to_path_buf();
    init_repo(&repo, "main", &"1".repeat(40));

    let (tx, rx) = unbounded();
    let mut watcher = new_watcher(tx, &test_config(100));
    watch_repo(&mut watcher, &repo);
    wait_for_discovery(&rx, &repo);

    let new_sha = "a".repeat(40);
    simulate_commit(
        &repo,
        "main",
        &"1".repeat(40),
        &new_sha,
        "feat: level up gremlin",
    );

    let signal = wait_for(&rx, "CommitCreated", |signal| {
        matches!(signal, DevSignal::CommitCreated { .. })
    });
    match signal {
        DevSignal::CommitCreated {
            commit_sha,
            message,
            branch,
            repo_path,
            ..
        } => {
            assert_eq!(commit_sha, Some(new_sha));
            assert_eq!(message, Some("feat: level up gremlin".to_string()));
            assert_eq!(branch, "main");
            assert_eq!(repo_path, repo);
        }
        other => panic!("signal inattendu : {other:?}"),
    }
}

#[test]
fn test_end_to_end_branch_switch_detection() {
    let guard = TempDirGuard::new("branch_switch");
    let repo = guard.path().to_path_buf();
    let sha = "1".repeat(40);
    init_repo(&repo, "main", &sha);

    let (tx, rx) = unbounded();
    let mut watcher = new_watcher(tx, &test_config(100));
    watch_repo(&mut watcher, &repo);
    wait_for_discovery(&rx, &repo);

    // Bascule de branche : nouvelle référence + réécriture de HEAD.
    write_file(&ref_file(&repo, "feature-skin"), &format!("{sha}\n"));
    write_reflog(
        &repo,
        &sha,
        &sha,
        "checkout: moving from main to feature-skin",
    );
    write_file(
        &repo.join(".git").join("HEAD"),
        "ref: refs/heads/feature-skin\n",
    );

    let signal = wait_for(&rx, "BranchChanged", |signal| {
        matches!(signal, DevSignal::BranchChanged { .. })
    });
    match signal {
        DevSignal::BranchChanged {
            old_branch,
            new_branch,
            ..
        } => {
            assert_eq!(old_branch, "main");
            assert_eq!(new_branch, "feature-skin");
        }
        other => panic!("signal inattendu : {other:?}"),
    }
}

#[test]
fn test_checkout_to_another_commit_is_not_a_commit() {
    let guard = TempDirGuard::new("checkout_no_xp");
    let repo = guard.path().to_path_buf();
    let main_sha = "1".repeat(40);
    let feature_sha = "2".repeat(40);
    init_repo(&repo, "main", &main_sha);
    write_file(&ref_file(&repo, "feature"), &format!("{feature_sha}\n"));
    write_reflog(&repo, &"0".repeat(40), &main_sha, "commit: initial");

    let (tx, rx) = unbounded();
    let mut watcher = new_watcher(tx, &test_config(100));
    watch_repo(&mut watcher, &repo);
    wait_for_discovery(&rx, &repo);

    // `git checkout feature` : HEAD change de branche ET de SHA, sans nouveau commit.
    write_reflog(
        &repo,
        &main_sha,
        &feature_sha,
        "checkout: moving from main to feature",
    );
    write_file(&repo.join(".git").join("HEAD"), "ref: refs/heads/feature\n");

    let _ = wait_for(
        &rx,
        "BranchChanged",
        |signal| matches!(signal, DevSignal::BranchChanged { new_branch, .. } if new_branch == "feature"),
    );
    assert_no_signal(&rx, "CommitCreated après un checkout", |signal| {
        matches!(signal, DevSignal::CommitCreated { .. })
    });
}

#[test]
fn test_unreadable_head_does_not_fabricate_a_branch() {
    let guard = TempDirGuard::new("head_transient");
    let repo = guard.path().to_path_buf();
    let sha = "1".repeat(40);
    init_repo(&repo, "develop", &sha);

    let (tx, rx) = unbounded();
    let mut watcher = new_watcher(tx, &test_config(100));
    watch_repo(&mut watcher, &repo);
    wait_for_discovery(&rx, &repo);

    // Git remplace brièvement HEAD pendant un checkout : contenu transitoirement vide.
    let head = repo.join(".git").join("HEAD");
    write_file(&head, "");
    assert_no_signal(&rx, "BranchChanged fabriqué vers main", |signal| {
        matches!(
            signal,
            DevSignal::BranchChanged { .. } | DevSignal::CommitCreated { .. }
        )
    });

    // Une fois HEAD relisible, la surveillance reprend normalement.
    write_file(&head, "ref: refs/heads/develop\n");
    let new_sha = "b".repeat(40);
    simulate_commit(
        &repo,
        "develop",
        &sha,
        &new_sha,
        "feat: retour à la normale",
    );
    let _ = wait_for(
        &rx,
        "CommitCreated après HEAD restauré",
        |signal| matches!(signal, DevSignal::CommitCreated { commit_sha, .. } if commit_sha.as_deref() == Some(new_sha.as_str())),
    );
}

#[test]
fn test_rapid_burst_writes_are_aggregated() {
    let guard = TempDirGuard::new("burst_debounce");
    let repo = guard.path().to_path_buf();
    init_repo(&repo, "main", &"0".repeat(40));

    let (tx, rx) = unbounded();
    let mut watcher = new_watcher(tx, &test_config(150));
    watch_repo(&mut watcher, &repo);
    wait_for_discovery(&rx, &repo);

    // Rafale d'écritures portant toutes sur le même commit.
    let sha = "3".repeat(40);
    for i in 0..10 {
        write_file(
            &repo.join(".git").join("COMMIT_EDITMSG"),
            &format!("wip {i}\n"),
        );
        simulate_commit(&repo, "main", &"0".repeat(40), &sha, "burst commit");
    }

    // Invariant : un seul signal pour ce SHA, quel que soit le découpage des flushes.
    let _ = wait_for(
        &rx,
        "CommitCreated (rafale)",
        |signal| matches!(signal, DevSignal::CommitCreated { commit_sha, .. } if commit_sha.as_deref() == Some(sha.as_str())),
    );
    assert_no_signal(
        &rx,
        "CommitCreated dupliqué",
        |signal| matches!(signal, DevSignal::CommitCreated { commit_sha, .. } if commit_sha.as_deref() == Some(sha.as_str())),
    );
}

#[test]
fn test_interleaved_repositories_are_debounced_independently() {
    let guard = TempDirGuard::new("interleaved");
    let first = guard.child("alpha");
    let second = guard.child("beta");
    init_repo(&first, "main", &"1".repeat(40));
    init_repo(&second, "main", &"2".repeat(40));

    let (tx, rx) = unbounded();
    let mut watcher = new_watcher(tx, &test_config(100));
    watch_repo(&mut watcher, &first);
    watch_repo(&mut watcher, &second);
    wait_for_discoveries(&rx, &[&first, &second]);

    let first_sha = "a".repeat(40);
    let second_sha = "b".repeat(40);
    simulate_commit(&first, "main", &"1".repeat(40), &first_sha, "alpha avance");
    simulate_commit(&second, "main", &"2".repeat(40), &second_sha, "beta avance");

    // Chaque dépôt est stabilisé indépendamment : les deux commits doivent sortir,
    // quel que soit l'ordre des flushes.
    let expectations: Vec<(String, _)> = [(first.clone(), first_sha), (second.clone(), second_sha)]
        .into_iter()
        .map(|(repo, sha)| {
            (
                format!("CommitCreated({})", repo.display()),
                move |signal: &DevSignal| {
                    matches!(
                        signal,
                        DevSignal::CommitCreated { repo_path, commit_sha, .. }
                            if *repo_path == repo && commit_sha.as_deref() == Some(sha.as_str())
                    )
                },
            )
        })
        .collect();
    wait_for_all(&rx, "commits entrelacés", expectations);
}

#[test]
fn test_adding_a_repo_being_cloned_emits_no_fake_commit() {
    let guard = TempDirGuard::new("clone_detect");

    // `git clone` : le dossier apparaît d'abord, puis `.git`, puis HEAD, puis la
    // ref. L'utilisateur peut déclarer le dépôt à n'importe quel moment de cette
    // séquence — ici au plus tôt, sur un `.git` encore vide.
    let cloned = guard.child("cloned_project");
    create_dir(&cloned.join(".git"));

    let (tx, rx) = unbounded();
    let mut watcher = new_watcher(tx, &test_config(100));
    watch_repo(&mut watcher, &cloned);
    wait_for_discovery(&rx, &cloned);

    let sha = "c".repeat(40);
    write_file(&cloned.join(".git").join("HEAD"), "ref: refs/heads/main\n");
    write_reflog(&cloned, &"0".repeat(40), &sha, "clone: from git@github.com");
    write_file(&ref_file(&cloned, "main"), &format!("{sha}\n"));

    // Un clone n'est pas un commit : aucune récompense ne doit être accordée.
    assert_no_signal(&rx, "CommitCreated pendant un clone", |signal| {
        matches!(
            signal,
            DevSignal::CommitCreated { .. } | DevSignal::BranchChanged { .. }
        )
    });

    // Le dépôt cloné reste pleinement surveillé ensuite.
    let commit_sha = "d".repeat(40);
    simulate_commit(
        &cloned,
        "main",
        &sha,
        &commit_sha,
        "feat: premier commit local",
    );
    let _ = wait_for(
        &rx,
        "CommitCreated après le clone",
        |signal| matches!(signal, DevSignal::CommitCreated { commit_sha: got, .. } if got.as_deref() == Some(commit_sha.as_str())),
    );
}

#[test]
fn test_repo_deletion_emits_repo_removed() {
    let guard = TempDirGuard::new("repo_deleted");
    let repo = guard.child("doomed_project");
    init_repo(&repo, "main", &"1".repeat(40));

    let (tx, rx) = unbounded();
    let mut watcher = new_watcher(tx, &test_config(100));
    watch_repo(&mut watcher, &repo);
    wait_for_discovery(&rx, &repo);

    remove_tree(&repo);

    let signal = wait_for(
        &rx,
        "RepoRemoved après suppression",
        |signal| matches!(signal, DevSignal::RepoRemoved { path, .. } if *path == repo),
    );
    match signal {
        DevSignal::RepoRemoved { repo_name, .. } => assert_eq!(repo_name, "doomed_project"),
        other => panic!("signal inattendu : {other:?}"),
    }

    // L'état interne est purgé : plus aucun watch ni entrée résiduelle.
    match watcher.watched_repos() {
        Ok(repos) => assert!(repos.is_empty(), "état résiduel : {repos:?}"),
        Err(e) => panic!("interrogation du worker impossible : {e}"),
    }
}

#[test]
fn test_unwatch_explicitly_watched_repo() {
    let guard = TempDirGuard::new("unwatch_explicit");
    let repo = guard.child("explicit_project");
    init_repo(&repo, "main", &"1".repeat(40));

    let (tx, rx) = unbounded();
    let mut watcher = new_watcher(tx, &test_config(100));
    watch_repo(&mut watcher, &repo);
    wait_for_discovery(&rx, &repo);

    if let Err(e) = watcher.unwatch_repo(&repo) {
        panic!("la désinscription doit réussir : {e}");
    }
    let _ = wait_for(
        &rx,
        "RepoRemoved",
        |signal| matches!(signal, DevSignal::RepoRemoved { path, .. } if *path == repo),
    );

    // Plus aucun signal après désinscription, malgré une activité Git réelle.
    simulate_commit(&repo, "main", &"1".repeat(40), &"e".repeat(40), "ignoré");
    assert_no_signal(&rx, "signal après désinscription", |signal| {
        matches!(
            signal,
            DevSignal::CommitCreated { .. } | DevSignal::BranchChanged { .. }
        )
    });
}

#[test]
fn test_unwatch_tracked_repo() {
    // Un dépôt monté depuis la configuration se retire exactement comme un dépôt
    // ajouté à la volée : c'est la même liste, pas deux origines distinctes.
    let guard = TempDirGuard::new("unwatch_tracked");
    let repo = guard.child("configured_project");
    init_repo(&repo, "main", &"1".repeat(40));

    let (tx, rx) = unbounded();
    let mut watcher = new_watcher(tx, &tracked_config(100, &[&repo]));
    arm(&mut watcher);
    wait_for_discovery(&rx, &repo);

    if let Err(e) = watcher.unwatch_repo(&repo) {
        panic!("la désinscription doit réussir : {e}");
    }
    let _ = wait_for(
        &rx,
        "RepoRemoved d'un dépôt configuré",
        |signal| matches!(signal, DevSignal::RepoRemoved { path, .. } if *path == repo),
    );

    match watcher.watched_repos() {
        Ok(repos) => assert!(
            !repos.contains(&repo),
            "le dépôt configuré doit avoir disparu de l'état : {repos:?}"
        ),
        Err(e) => panic!("interrogation du worker impossible : {e}"),
    }
}

#[test]
fn test_tracked_repos_are_armed_at_startup() {
    let guard = TempDirGuard::new("tracked_startup");
    let first = guard.child("projet_a");
    let second = guard.child("imbrique/projet_b");
    for repo in [&first, &second] {
        init_repo(repo, "main", &"1".repeat(40));
    }

    let (tx, rx) = unbounded();
    let mut watcher = new_watcher(tx, &tracked_config(100, &[&first, &second]));
    arm(&mut watcher);

    // Les deux dépôts déclarés sont montés, quelle que soit leur profondeur :
    // aucun parcours d'arborescence n'intervient, donc aucune limite de niveau.
    wait_for_discoveries(&rx, &[&first, &second]);

    // Et l'activité Git y est bien suivie.
    simulate_commit(
        &second,
        "main",
        &"1".repeat(40),
        &"2".repeat(40),
        "feat: dépôt configuré",
    );
    let _ = wait_for(
        &rx,
        "CommitCreated sur un dépôt configuré",
        |signal| matches!(signal, DevSignal::CommitCreated { repo_path, .. } if *repo_path == second),
    );
}

#[test]
fn test_a_repo_created_next_to_a_tracked_one_stays_ignored() {
    // Anti-régression du retrait du scanner : ni un dépôt voisin, ni un dépôt
    // imbriqué dans un dépôt suivi ne doivent s'enregistrer d'eux-mêmes.
    let guard = TempDirGuard::new("no_hot_discovery");
    let tracked = guard.child("suivi");
    init_repo(&tracked, "main", &"1".repeat(40));

    let (tx, rx) = unbounded();
    let mut watcher = new_watcher(tx, &tracked_config(100, &[&tracked]));
    arm(&mut watcher);
    wait_for_discovery(&rx, &tracked);

    let sibling = guard.path().join("intrus");
    create_dir(&sibling);
    init_repo(&sibling, "main", &"2".repeat(40));

    let nested = tracked.join("sous_projet");
    create_dir(&nested);
    init_repo(&nested, "main", &"3".repeat(40));

    assert_no_signal(&rx, "découverte automatique d'un dépôt", |signal| {
        matches!(signal, DevSignal::RepoDiscovered { .. })
    });

    match watcher.watched_repos() {
        Ok(repos) => assert_eq!(
            repos,
            vec![tracked],
            "seul le dépôt explicitement déclaré doit être surveillé"
        ),
        Err(e) => panic!("interrogation du worker impossible : {e}"),
    }
}

#[test]
fn test_detached_head_and_packed_refs() {
    let guard = TempDirGuard::new("detached_packed");
    let repo = guard.path().to_path_buf();
    let sha = "1".repeat(40);

    // Dépôt sans référence loose : la branche est résolue via packed-refs.
    write_file(&repo.join(".git").join("HEAD"), "ref: refs/heads/main\n");
    write_file(
        &repo.join(".git").join("packed-refs"),
        &format!("# pack-refs with: peeled fully-peeled sorted\n{sha} refs/heads/main\n"),
    );

    let (tx, rx) = unbounded();
    let mut watcher = new_watcher(tx, &test_config(100));
    watch_repo(&mut watcher, &repo);
    wait_for_discovery(&rx, &repo);

    // Passage en HEAD détaché sur un autre commit.
    let detached_sha = "f".repeat(40);
    write_reflog(
        &repo,
        &sha,
        &detached_sha,
        &format!("checkout: moving from main to {detached_sha}"),
    );
    write_file(
        &repo.join(".git").join("HEAD"),
        &format!("{detached_sha}\n"),
    );

    let signal = wait_for(&rx, "BranchChanged vers HEAD détaché", |signal| {
        matches!(signal, DevSignal::BranchChanged { .. })
    });
    match signal {
        DevSignal::BranchChanged {
            old_branch,
            new_branch,
            ..
        } => {
            assert_eq!(old_branch, "main");
            assert_eq!(new_branch, format!("detached@{}", &detached_sha[..7]));
        }
        other => panic!("signal inattendu : {other:?}"),
    }
    assert_no_signal(&rx, "CommitCreated sur détachement de HEAD", |signal| {
        matches!(signal, DevSignal::CommitCreated { .. })
    });
}

#[test]
fn test_status_channel_reports_watch_failure() {
    let guard = TempDirGuard::new("status_channel");
    let (tx, _rx) = unbounded();
    let (status_tx, status_rx) = unbounded();

    let mut watcher = new_watcher(tx, &test_config(100));
    if let Err(e) = watcher.set_status_sender(status_tx) {
        panic!("le canal de statut doit être installé : {e}");
    }

    let missing = guard.path().join("absent_project");
    assert!(
        watcher.watch_repo(&missing).is_err(),
        "un dépôt inexistant doit être signalé comme une erreur"
    );

    let status = wait_for(&status_rx, "WatchFailed", |status| {
        matches!(status, WatcherStatus::WatchFailed { .. })
    });
    match status {
        WatcherStatus::WatchFailed { path, .. } => assert!(path.starts_with(&missing)),
        other @ (WatcherStatus::EventsLost { .. }
        | WatcherStatus::ReportRejected { .. }
        | WatcherStatus::HistoryUnreadable { .. }
        | WatcherStatus::ToolingStateChanged { .. }) => {
            panic!("statut inattendu : {other:?}")
        }
    }
}

#[test]
fn test_watcher_stops_when_consumer_disappears() {
    let guard = TempDirGuard::new("consumer_gone");
    let repo = guard.path().to_path_buf();
    init_repo(&repo, "main", &"1".repeat(40));

    let (tx, rx) = unbounded();
    let mut watcher = new_watcher(tx, &test_config(100));
    watch_repo(&mut watcher, &repo);
    wait_for_discovery(&rx, &repo);

    // Le consommateur disparaît : la surveillance doit cesser d'elle-même.
    drop(rx);
    simulate_commit(
        &repo,
        "main",
        &"1".repeat(40),
        &"9".repeat(40),
        "personne n'écoute",
    );

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while std::time::Instant::now() < deadline {
        match watcher.watched_repos() {
            Ok(_) => std::thread::sleep(std::time::Duration::from_millis(20)),
            Err(_) => return, // le worker s'est arrêté : le canal de contrôle ne répond plus
        }
    }
    panic!("le worker doit s'arrêter lorsque plus personne ne consomme les signaux");
}

#[test]
fn test_junit_report_is_detected_end_to_end() {
    let guard = TempDirGuard::new("junit_report");
    let repo = guard.path().to_path_buf();
    init_repo(&repo, "main", &"1".repeat(40));

    let (tx, rx) = unbounded();
    let mut watcher = new_watcher(tx, &test_config(50));
    watch_repo(&mut watcher, &repo);
    wait_for_discovery(&rx, &repo);

    let report = repo.join("test-results").join("junit.xml");
    write_file(
        &report,
        r#"<testsuites tests="4" failures="1" skipped="1" time="2.5"></testsuites>"#,
    );

    let signal = wait_for(
        &rx,
        "TestCompleted JUnit",
        |signal| matches!(signal, DevSignal::TestCompleted { report_path, .. } if *report_path == report),
    );
    match signal {
        DevSignal::TestCompleted { summary, .. } => {
            assert_eq!(summary.passed, 2);
            assert_eq!(summary.failed, 1);
            assert_eq!(summary.skipped, 1);
        }
        other => panic!("signal inattendu : {other:?}"),
    }
}

#[test]
fn test_existing_report_is_only_a_baseline() {
    let guard = TempDirGuard::new("junit_baseline");
    let repo = guard.path().to_path_buf();
    init_repo(&repo, "main", &"1".repeat(40));
    let report = repo.join("test-results").join("junit.xml");
    write_file(
        &report,
        r#"<testsuites tests="1" failures="0"></testsuites>"#,
    );

    let (tx, rx) = unbounded();
    let mut watcher = new_watcher(tx, &test_config(50));
    watch_repo(&mut watcher, &repo);
    wait_for_discovery(&rx, &repo);
    assert_no_signal(&rx, "rapport historique", |signal| {
        matches!(signal, DevSignal::TestCompleted { .. })
    });

    write_file(
        &report,
        r#"<testsuites tests="2" failures="0"></testsuites>"#,
    );
    let _ = wait_for(
        &rx,
        "nouvelle écriture JUnit",
        |signal| matches!(signal, DevSignal::TestCompleted { summary, .. } if summary.passed == 2),
    );
}

#[test]
fn test_gremlin_build_contract_is_detected() {
    let guard = TempDirGuard::new("build_contract");
    let repo = guard.path().to_path_buf();
    init_repo(&repo, "main", &"1".repeat(40));

    let (tx, rx) = unbounded();
    let mut watcher = new_watcher(tx, &test_config(50));
    watch_repo(&mut watcher, &repo);
    wait_for_discovery(&rx, &repo);

    let report = repo.join(".gremlin").join("results").join("build.json");
    write_file(
        &report,
        r#"{"schema_version":1,"run_id":"build-42","kind":"build","tool":"cargo","outcome":"passed","duration_ms":1500}"#,
    );

    let signal = wait_for(
        &rx,
        "BuildCompleted",
        |signal| matches!(signal, DevSignal::BuildCompleted { run_id, .. } if run_id == "build-42"),
    );
    match signal {
        DevSignal::BuildCompleted { summary, .. } => assert!(summary.success),
        other => panic!("signal inattendu : {other:?}"),
    }
}

#[test]
fn test_tooling_toggle_is_confirmed_and_rebaselines_reports() {
    let guard = TempDirGuard::new("tooling_toggle");
    let repo = guard.path().to_path_buf();
    init_repo(&repo, "main", &"a".repeat(40));

    let (tx, rx) = unbounded();
    let (status_tx, status_rx) = unbounded();
    let mut watcher = new_watcher(tx, &test_config(75));
    if let Err(error) = watcher.set_status_sender(status_tx) {
        panic!("canal de statut indisponible : {error}");
    }
    watch_repo(&mut watcher, &repo);
    wait_for_discovery(&rx, &repo);

    if let Err(error) = watcher.request_tooling_enabled(false) {
        panic!("désactivation refusée : {error}");
    }
    let _ = wait_for(&status_rx, "tooling disabled", |status| {
        matches!(
            status,
            WatcherStatus::ToolingStateChanged {
                enabled: false,
                error: None
            }
        )
    });
    let report = repo.join("junit.xml");
    write_file(&report, r#"<testsuite tests="1" failures="0" time="0.1"/>"#);
    assert_no_signal(&rx, "TestCompleted désactivé", |signal| {
        matches!(signal, DevSignal::TestCompleted { .. })
    });

    if let Err(error) = watcher.request_tooling_enabled(true) {
        panic!("réactivation refusée : {error}");
    }
    let _ = wait_for(&status_rx, "tooling enabled", |status| {
        matches!(
            status,
            WatcherStatus::ToolingStateChanged {
                enabled: true,
                error: None
            }
        )
    });
    assert_no_signal(&rx, "rapport de baseline", |signal| {
        matches!(signal, DevSignal::TestCompleted { .. })
    });

    write_file(&report, r#"<testsuite tests="2" failures="0" time="0.2"/>"#);
    let _ = wait_for(&rx, "TestCompleted", |signal| {
        matches!(signal, DevSignal::TestCompleted { .. })
    });
}

// -----------------------------------------------------------------------------
// Phase 8 : historique des jours de commits
// -----------------------------------------------------------------------------

/// Journal de références contenant plusieurs journées de commits.
///
/// Chaque ligne porte son propre horodatage : c'est l'unique preuve, sur cette
/// machine, qu'un commit y a été **créé** un jour donné.
fn write_history(repo_root: &std::path::Path, entries: &[(i64, &str)]) {
    use std::fmt::Write as _;

    let mut content = String::new();
    for (index, (unix_seconds, action)) in entries.iter().enumerate() {
        let old = format!("{index:040}");
        let new = format!("{:040}", index + 1);
        // L'écriture dans une `String` ne peut pas échouer : le résultat est
        // ignoré plutôt que déballé, pour ne pas introduire de panic de test.
        let _ = writeln!(
            content,
            "{old} {new} Dev Le Gremlin <dev@gremlin.rs> {unix_seconds} +0000\t{action}"
        );
    }
    write_file(&repo_root.join(".git").join("logs").join("HEAD"), &content);
}

/// Instant de midi UTC du `offset`-ième jour après le 2024-05-06.
const fn day_at(offset: i64) -> i64 {
    1_714_996_800 + offset * 86_400
}

#[test]
fn test_attaching_a_repository_seeds_its_commit_history_without_replaying_commits() {
    let guard = TempDirGuard::new("history_seed");
    let repo = guard.path().to_path_buf();
    init_repo(&repo, "main", &"1".repeat(40));
    write_history(
        &repo,
        &[
            (day_at(0), "commit (initial): premier"),
            (day_at(0), "commit: encore le même jour"),
            (day_at(1), "commit: lendemain"),
            (day_at(2), "commit (amend): correction"),
        ],
    );

    let (tx, rx) = unbounded();
    let mut watcher = new_watcher(tx, &test_config(100));
    watch_repo(&mut watcher, &repo);

    let signal = wait_for(&rx, "CommitHistorySeeded", |signal| {
        matches!(signal, DevSignal::CommitHistorySeeded { .. })
    });
    match signal {
        DevSignal::CommitHistorySeeded {
            stamps,
            truncated,
            repo_path,
            ..
        } => {
            assert_eq!(repo_path, repo);
            assert!(!truncated, "un journal court n'est pas tronqué");
            assert_eq!(
                stamps.len(),
                3,
                "trois journées distinctes, quatre commits : {stamps:?}"
            );
            assert!(
                stamps.windows(2).all(|pair| pair[0] <= pair[1]),
                "les horodatages doivent être triés"
            );
        }
        other => panic!("signal inattendu : {other:?}"),
    }

    // L'attachement ne rejoue aucun commit : le familier ne gagne pas d'XP pour
    // un historique qu'il n'a pas vu naître.
    assert_no_signal(&rx, "CommitCreated", |signal| {
        matches!(signal, DevSignal::CommitCreated { .. })
    });
}

#[test]
fn test_a_repository_without_journal_seeds_an_empty_history() {
    let guard = TempDirGuard::new("history_empty");
    let repo = guard.path().to_path_buf();
    init_repo(&repo, "main", &"1".repeat(40));

    let (tx, rx) = unbounded();
    let mut watcher = new_watcher(tx, &test_config(100));
    watch_repo(&mut watcher, &repo);

    let signal = wait_for(&rx, "CommitHistorySeeded", |signal| {
        matches!(signal, DevSignal::CommitHistorySeeded { .. })
    });
    match signal {
        DevSignal::CommitHistorySeeded {
            stamps, truncated, ..
        } => {
            // Un dépôt sans commit est une observation valide, pas un échec :
            // sans ce signal, l'orchestrateur ne saurait pas distinguer les deux.
            assert!(stamps.is_empty());
            assert!(!truncated);
        }
        other => panic!("signal inattendu : {other:?}"),
    }
}

#[test]
fn test_history_ignores_entries_that_created_no_local_commit() {
    let guard = TempDirGuard::new("history_actions");
    let repo = guard.path().to_path_buf();
    init_repo(&repo, "main", &"1".repeat(40));
    write_history(
        &repo,
        &[
            (day_at(0), "clone: from github.com"),
            (day_at(1), "checkout: moving from main to dev"),
            (day_at(2), "pull: Fast-forward"),
            (day_at(3), "reset: moving to HEAD~1"),
            (day_at(4), "commit: le seul vrai commit"),
        ],
    );

    let (tx, rx) = unbounded();
    let mut watcher = new_watcher(tx, &test_config(100));
    watch_repo(&mut watcher, &repo);

    let signal = wait_for(&rx, "CommitHistorySeeded", |signal| {
        matches!(signal, DevSignal::CommitHistorySeeded { .. })
    });
    match signal {
        DevSignal::CommitHistorySeeded { stamps, .. } => {
            assert_eq!(
                stamps.len(),
                1,
                "clone/checkout/pull/reset comptés : {stamps:?}"
            );
            assert_eq!(stamps[0].unix_seconds, day_at(4));
        }
        other => panic!("signal inattendu : {other:?}"),
    }
}

#[test]
fn test_a_live_commit_carries_its_stamp_only_when_the_reflog_is_authoritative() {
    let guard = TempDirGuard::new("live_stamp");
    let repo = guard.path().to_path_buf();
    init_repo(&repo, "main", &"1".repeat(40));

    let (tx, rx) = unbounded();
    let mut watcher = new_watcher(tx, &test_config(100));
    watch_repo(&mut watcher, &repo);
    wait_for_discovery(&rx, &repo);

    // Reflog à jour et daté : le commit porte son horodatage.
    let new_sha = "a".repeat(40);
    write_file(
        &repo.join(".git").join("logs").join("HEAD"),
        &format!(
            "{old} {new_sha} Dev Le Gremlin <dev@gremlin.rs> {stamp} +0200\tcommit: daté\n",
            old = "1".repeat(40),
            stamp = day_at(3),
        ),
    );
    write_file(&ref_file(&repo, "main"), &format!("{new_sha}\n"));

    let signal = wait_for(&rx, "CommitCreated", |signal| {
        matches!(signal, DevSignal::CommitCreated { .. })
    });
    match signal {
        DevSignal::CommitCreated { stamp, .. } => {
            let stamp = stamp.expect("un reflog daté et à jour doit fournir son horodatage");
            assert_eq!(stamp.unix_seconds, day_at(3));
            assert_eq!(stamp.utc_offset_minutes, 120);
        }
        other => panic!("signal inattendu : {other:?}"),
    }
}

#[test]
fn test_a_commit_without_a_usable_reflog_reacts_without_dating_the_streak() {
    let guard = TempDirGuard::new("live_nostamp");
    let repo = guard.path().to_path_buf();
    init_repo(&repo, "main", &"1".repeat(40));

    let (tx, rx) = unbounded();
    let mut watcher = new_watcher(tx, &test_config(100));
    watch_repo(&mut watcher, &repo);
    wait_for_discovery(&rx, &repo);

    // Aucun reflog : le changement de SHA fait bien réagir le familier, mais
    // aucune journée n'est attribuée faute de preuve temporelle.
    let new_sha = "b".repeat(40);
    write_file(&ref_file(&repo, "main"), &format!("{new_sha}\n"));

    let signal = wait_for(&rx, "CommitCreated", |signal| {
        matches!(signal, DevSignal::CommitCreated { .. })
    });
    match signal {
        DevSignal::CommitCreated { stamp, .. } => {
            assert!(stamp.is_none(), "journée inventée sans reflog : {stamp:?}");
        }
        other => panic!("signal inattendu : {other:?}"),
    }
}

#[test]
fn test_history_seeding_survives_a_burst_of_attachments_and_a_shutdown() {
    let guard = TempDirGuard::new("history_burst");
    let mut repos = Vec::new();
    for index in 0..6 {
        let repo = guard.path().join(format!("projet-{index}"));
        create_dir(&repo);
        init_repo(&repo, "main", &"1".repeat(40));
        write_history(&repo, &[(day_at(index), "commit: travail")]);
        repos.push(repo);
    }

    let (tx, rx) = unbounded();
    let mut watcher = new_watcher(tx, &test_config(100));
    for repo in &repos {
        watch_repo(&mut watcher, repo);
    }

    // Chaque dépôt reçoit son seed, malgré la rafale de rattachements.
    let mut seeded = 0;
    while seeded < repos.len() {
        wait_for(&rx, "CommitHistorySeeded", |signal| {
            matches!(signal, DevSignal::CommitHistorySeeded { .. })
        });
        seeded += 1;
    }

    // L'arrêt reste déterministe : le worker sert `Shutdown` entre deux
    // analyses, il n'est pas bloqué dans une boucle de lectures.
    drop(watcher);
}
