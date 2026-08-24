//! Tests d'intégration pour la surveillance passive des dépôts Git (`gremlin-watcher`).
//!
//! Règle de conception de cette suite : **aucune assertion ne repose sur une durée
//! fixe**. Chaque test attend un signal précis avec un délai généreux, ou observe
//! l'absence d'un signal pendant une période de calme. Une machine chargée ralentit
//! les tests, elle ne les fait pas échouer.

mod common;

use common::{
    assert_no_signal, create_dir, init_repo, ref_file, remove_tree, simulate_commit, test_config,
    wait_for, wait_for_all, wait_for_discoveries, wait_for_discovery, write_file, write_reflog,
    TempDirGuard,
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

/// Raccourci : enregistre une racine de projets ou fait échouer le test.
fn watch_root(watcher: &mut RepoWatcher, root: &std::path::Path) {
    if let Err(e) = watcher.watch_workspace_root(root) {
        panic!("la racine {} doit être surveillée : {e}", root.display());
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
fn test_hot_discovery_of_new_repo() {
    let guard = TempDirGuard::new("hot_discovery");
    let workspace_root = guard.path().to_path_buf();

    let (tx, rx) = unbounded();
    let mut watcher = new_watcher(tx, &test_config(100));
    // L'enregistrement est confirmé par le worker : aucune temporisation nécessaire.
    watch_root(&mut watcher, &workspace_root);

    let new_repo = workspace_root.join("super_project");
    create_dir(&new_repo);
    init_repo(&new_repo, "main", &"7".repeat(40));

    let signal = wait_for(
        &rx,
        "RepoDiscovered à chaud",
        |signal| matches!(signal, DevSignal::RepoDiscovered { path, .. } if *path == new_repo),
    );
    match signal {
        DevSignal::RepoDiscovered { repo_name, .. } => assert_eq!(repo_name, "super_project"),
        other => panic!("signal inattendu : {other:?}"),
    }

    // Le dépôt découvert à chaud est bien suivi par la source de vérité unique.
    match watcher.watched_repos() {
        Ok(repos) => assert!(repos.contains(&new_repo)),
        Err(e) => panic!("interrogation du worker impossible : {e}"),
    }
}

#[test]
fn test_git_clone_progressive_creation_emits_no_fake_commit() {
    let guard = TempDirGuard::new("clone_detect");
    let workspace_root = guard.path().to_path_buf();

    let (tx, rx) = unbounded();
    let mut watcher = new_watcher(tx, &test_config(100));
    watch_root(&mut watcher, &workspace_root);

    // `git clone` : le dossier apparaît d'abord, puis `.git`, puis HEAD, puis la ref.
    let cloned = workspace_root.join("cloned_project");
    create_dir(&cloned);
    create_dir(&cloned.join(".git"));
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
fn test_unwatch_auto_discovered_repo() {
    let guard = TempDirGuard::new("unwatch_discovered");
    let workspace_root = guard.path().to_path_buf();

    let (tx, rx) = unbounded();
    let mut watcher = new_watcher(tx, &test_config(100));
    watch_root(&mut watcher, &workspace_root);

    // Dépôt jamais passé à `watch_repo` : découvert à chaud par la racine.
    let discovered = workspace_root.join("auto_project");
    create_dir(&discovered);
    init_repo(&discovered, "main", &"1".repeat(40));
    wait_for_discovery(&rx, &discovered);

    // Un dépôt auto-découvert doit pouvoir être retiré comme un autre.
    if let Err(e) = watcher.unwatch_repo(&discovered) {
        panic!("la désinscription doit réussir : {e}");
    }
    let _ = wait_for(
        &rx,
        "RepoRemoved d'un dépôt auto-découvert",
        |signal| matches!(signal, DevSignal::RepoRemoved { path, .. } if *path == discovered),
    );

    match watcher.watched_repos() {
        Ok(repos) => assert!(
            !repos.contains(&discovered),
            "le dépôt auto-découvert doit avoir disparu de l'état : {repos:?}"
        ),
        Err(e) => panic!("interrogation du worker impossible : {e}"),
    }
}

#[test]
fn test_unwatch_workspace_root_stops_hot_discovery() {
    let guard = TempDirGuard::new("unwatch_root");
    let workspace_root = guard.path().to_path_buf();

    let (tx, rx) = unbounded();
    let mut watcher = new_watcher(tx, &test_config(100));
    watch_root(&mut watcher, &workspace_root);
    if let Err(e) = watcher.unwatch_workspace_root(&workspace_root) {
        panic!("le retrait de la racine doit réussir : {e}");
    }

    let late_repo = workspace_root.join("late_project");
    create_dir(&late_repo);
    init_repo(&late_repo, "main", &"1".repeat(40));

    assert_no_signal(&rx, "découverte après retrait de la racine", |signal| {
        matches!(signal, DevSignal::RepoDiscovered { .. })
    });
}

#[test]
fn test_background_scan_registers_existing_repos() {
    let guard = TempDirGuard::new("background_scan");
    let first = guard.child("scanned_a");
    let second = guard.child("nested/scanned_b");
    let ignored = guard.child("node_modules/scanned_c");
    for repo in [&first, &second, &ignored] {
        init_repo(repo, "main", &"1".repeat(40));
    }

    let (tx, rx) = unbounded();
    let mut watcher = new_watcher(tx, &test_config(100));
    watcher.start_background_scan(vec![guard.path().to_path_buf()], 4);

    wait_for_discoveries(&rx, &[&first, &second]);

    match watcher.watched_repos() {
        Ok(repos) => assert!(
            !repos.contains(&ignored),
            "les dossiers ignorés ne doivent pas être scannés : {repos:?}"
        ),
        Err(e) => panic!("interrogation du worker impossible : {e}"),
    }
}

#[test]
fn test_auto_discovery_consumes_custom_roots() {
    let guard = TempDirGuard::new("custom_roots");
    let repo = guard.child("configured_project");
    init_repo(&repo, "main", &"1".repeat(40));

    let config = WatcherConfig {
        debounce_duration_ms: 100,
        auto_discovery: false,
        custom_roots: vec![guard.path().to_path_buf()],
        max_scan_depth: 3,
        ..WatcherConfig::default()
    };

    let (tx, rx) = unbounded();
    let mut watcher = new_watcher(tx, &config);
    if let Err(e) = watcher.start_auto_discovery() {
        panic!("la découverte configurée doit démarrer : {e}");
    }

    // Le dépôt existant est trouvé par le scan des racines personnalisées...
    wait_for_discovery(&rx, &repo);

    // ...et la racine personnalisée est aussi surveillée à chaud.
    let hot = guard.path().join("hot_project");
    create_dir(&hot);
    init_repo(&hot, "main", &"2".repeat(40));
    wait_for_discovery(&rx, &hot);
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
