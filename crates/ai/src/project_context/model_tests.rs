use std::path::PathBuf;

use warp_util::local_or_remote_path::LocalOrRemotePath;
use warp_util::standardized_path::StandardizedPath;

fn local_path(path: &str) -> LocalOrRemotePath {
    LocalOrRemotePath::Local(PathBuf::from(path))
}

use super::*;

#[test]
fn test_find_applicable_rules_empty_rules() {
    let rules = ProjectRules { rules: vec![] };
    let path = local_path("/a/b/c/file.rs");

    let result = rules.find_active_or_applicable_rules(&path).active_rules;
    assert!(result.is_empty());
}

#[test]
fn test_find_applicable_rules_no_matching_rules() {
    let mut rules = ProjectRules::default();

    rules.upsert_rule(&local_path("/x/y/WARP.md"), "content1".to_string());
    rules.upsert_rule(&local_path("/z/AGENTS.md"), "content2".to_string());

    let path = local_path("/a/b/c/file.rs");

    let result = rules.find_active_or_applicable_rules(&path).active_rules;
    assert!(result.is_empty());
}

#[test]
fn test_find_applicable_rules_single_matching_rule() {
    let mut rules = ProjectRules::default();

    rules.upsert_rule(&local_path("/a/WARP.md"), "content1".to_string());
    rules.upsert_rule(&local_path("/x/AGENTS.md"), "content2".to_string());

    let path = local_path("/a/b/c/file.rs");

    let result = rules.find_active_or_applicable_rules(&path).active_rules;
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].path, local_path("/a/WARP.md"));
}

#[test]
fn test_find_applicable_rules_includes_all_ancestor_rules() {
    let mut rules = ProjectRules::default();

    rules.upsert_rule(&local_path("/a/WARP.md"), "root_warp".to_string());
    rules.upsert_rule(&local_path("/a/b/WARP.md"), "nested_warp".to_string());
    rules.upsert_rule(&local_path("/a/b/c/WARP.md"), "deep_warp".to_string());

    let path = local_path("/a/b/c/d/file.rs");

    let result = rules.find_active_or_applicable_rules(&path).active_rules;
    assert_eq!(result.len(), 3);

    // All should be WARP.md files (same priority), order is not specified by depth
    // Just verify all expected rules are present
    let paths: Vec<LocalOrRemotePath> = result.iter().map(|r| r.path.clone()).collect();
    assert!(paths.contains(&local_path("/a/WARP.md")));
    assert!(paths.contains(&local_path("/a/b/WARP.md")));
    assert!(paths.contains(&local_path("/a/b/c/WARP.md")));
}

#[test]
fn test_find_applicable_rules_multiple_patterns() {
    let mut rules = ProjectRules::default();

    rules.upsert_rule(&local_path("/a/b/AGENTS.md"), "agents_content".to_string());
    rules.upsert_rule(&local_path("/a/WARP.md"), "warp_content".to_string());

    let path = local_path("/a/b/file.rs");

    let result = rules.find_active_or_applicable_rules(&path).active_rules;
    assert_eq!(result.len(), 2);

    assert_eq!(result[0].path, local_path("/a/b/AGENTS.md"));
    assert_eq!(result[0].content, "agents_content");
    assert_eq!(result[1].path, local_path("/a/WARP.md"));
    assert_eq!(result[1].content, "warp_content");
}

#[test]
fn test_find_applicable_rules_exact_path_match() {
    let mut rules = ProjectRules::default();

    rules.upsert_rule(&local_path("/a/b/WARP.md"), "exact_match".to_string());

    let path = local_path("/a/b/file.rs");

    let result = rules.find_active_or_applicable_rules(&path).active_rules;
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].path, local_path("/a/b/WARP.md"));
    assert_eq!(result[0].content, "exact_match");
}

#[test]
fn test_find_applicable_rules_ignores_deeper_paths() {
    let mut rules = ProjectRules::default();

    rules.upsert_rule(&local_path("/a/WARP.md"), "applicable".to_string());
    rules.upsert_rule(&local_path("/a/b/c/d/e/WARP.md"), "too_deep".to_string()); // Path doesn't contain /a/b

    let path = local_path("/a/b/file.rs");

    let result = rules.find_active_or_applicable_rules(&path).active_rules;
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].path, local_path("/a/WARP.md"));
    assert_eq!(result[0].content, "applicable");
}

#[test]
fn test_find_applicable_rules_handles_root_path() {
    let mut rules = ProjectRules::default();

    rules.upsert_rule(&local_path("/WARP.md"), "root_rule".to_string());

    let path = local_path("/a/b/file.rs");

    let result = rules.find_active_or_applicable_rules(&path).active_rules;
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].path, local_path("/WARP.md"));
    assert_eq!(result[0].content, "root_rule");
}

#[test]
fn test_find_applicable_rules_complex_scenario() {
    // This test covers the example from the original request:
    // For path /a/b/c/file.rs with rules:
    // - /a/WARP.md
    // - /a/AGENTS.md
    // - /a/b/WARP.md
    // - /a/b/AGENTS.md
    let mut rules = ProjectRules::default();

    rules.upsert_rule(&local_path("/a/WARP.md"), "a_warp".to_string());
    rules.upsert_rule(&local_path("/a/AGENTS.md"), "a_agents".to_string());
    rules.upsert_rule(&local_path("/a/b/WARP.md"), "ab_warp".to_string());
    rules.upsert_rule(&local_path("/a/b/AGENTS.md"), "ab_agents".to_string());
    rules.upsert_rule(&local_path("/x/WARP.md"), "irrelevant".to_string()); // Should be ignored

    let path = local_path("/a/b/c/file.rs");

    let result = rules.find_active_or_applicable_rules(&path).active_rules;
    assert_eq!(result.len(), 2);

    // Expect only WARP.md files to be included as they have higher priority.
    assert_eq!(result[0].path, local_path("/a/WARP.md"));
    assert_eq!(result[0].content, "a_warp");
    assert_eq!(result[1].path, local_path("/a/b/WARP.md"));
    assert_eq!(result[1].content, "ab_warp");
}

#[test]
fn test_find_applicable_rules_handles_unknown_file_patterns() {
    let mut rules = ProjectRules::default();

    rules.upsert_rule(&local_path("/a/WARP.md"), "known_pattern".to_string());
    rules.upsert_rule(&local_path("/a/UNKNOWN.md"), "unknown_pattern".to_string());
    let path = local_path("/a/file.rs");

    let result = rules.find_active_or_applicable_rules(&path).active_rules;
    assert_eq!(result.len(), 1);

    assert_eq!(result[0].path, local_path("/a/WARP.md"));
    assert_eq!(result[0].content, "known_pattern");
}

#[test]
fn test_find_applicable_rules_with_relative_paths() {
    let mut rules = ProjectRules::default();

    rules.upsert_rule(&local_path("src/WARP.md"), "src_warp".to_string());
    rules.upsert_rule(
        &local_path("src/components/WARP.md"),
        "components_warp".to_string(),
    );

    let path = local_path("src/components/Button.tsx");

    let result = rules.find_active_or_applicable_rules(&path).active_rules;
    assert_eq!(result.len(), 2);

    // Both are WARP.md files (same priority), order within same priority is not guaranteed
    // Just verify both rules are present
    let paths: Vec<LocalOrRemotePath> = result.iter().map(|r| r.path.clone()).collect();
    assert!(paths.contains(&local_path("src/WARP.md")));
    assert!(paths.contains(&local_path("src/components/WARP.md")));
}

fn make_rule_path(path: &str) -> ProjectRulePath {
    ProjectRulePath {
        path: PathBuf::from(path),
        project_root: PathBuf::from("/project"),
    }
}

#[test]
fn test_merge_independent_deltas() {
    let mut delta = RulesDelta {
        discovered_rules: vec![make_rule_path("/a/WARP.md")],
        deleted_rules: vec![],
    };
    delta.merge(RulesDelta {
        discovered_rules: vec![],
        deleted_rules: vec![PathBuf::from("/b/WARP.md")],
    });

    assert_eq!(delta.discovered_rules.len(), 1);
    assert_eq!(delta.discovered_rules[0].path, PathBuf::from("/a/WARP.md"));
    assert_eq!(delta.deleted_rules, vec![PathBuf::from("/b/WARP.md")]);
}

#[test]
fn test_merge_add_then_delete_yields_delete() {
    let mut delta = RulesDelta {
        discovered_rules: vec![make_rule_path("/a/WARP.md")],
        deleted_rules: vec![],
    };
    delta.merge(RulesDelta {
        discovered_rules: vec![],
        deleted_rules: vec![PathBuf::from("/a/WARP.md")],
    });

    assert!(delta.discovered_rules.is_empty());
    assert_eq!(delta.deleted_rules, vec![PathBuf::from("/a/WARP.md")]);
}

#[test]
fn test_merge_delete_then_add_yields_add() {
    let mut delta = RulesDelta {
        discovered_rules: vec![],
        deleted_rules: vec![PathBuf::from("/a/WARP.md")],
    };
    delta.merge(RulesDelta {
        discovered_rules: vec![make_rule_path("/a/WARP.md")],
        deleted_rules: vec![],
    });

    assert_eq!(delta.discovered_rules.len(), 1);
    assert_eq!(delta.discovered_rules[0].path, PathBuf::from("/a/WARP.md"));
    assert!(delta.deleted_rules.is_empty());
}

#[test]
fn test_merge_add_delete_add_yields_add() {
    let mut delta = RulesDelta::default();
    delta.merge(RulesDelta {
        discovered_rules: vec![make_rule_path("/a/WARP.md")],
        deleted_rules: vec![],
    });
    delta.merge(RulesDelta {
        discovered_rules: vec![],
        deleted_rules: vec![PathBuf::from("/a/WARP.md")],
    });
    delta.merge(RulesDelta {
        discovered_rules: vec![make_rule_path("/a/WARP.md")],
        deleted_rules: vec![],
    });

    assert_eq!(delta.discovered_rules.len(), 1);
    assert_eq!(delta.discovered_rules[0].path, PathBuf::from("/a/WARP.md"));
    assert!(delta.deleted_rules.is_empty());
}

#[test]
fn test_merge_delete_add_delete_yields_delete() {
    let mut delta = RulesDelta::default();
    delta.merge(RulesDelta {
        discovered_rules: vec![],
        deleted_rules: vec![PathBuf::from("/a/WARP.md")],
    });
    delta.merge(RulesDelta {
        discovered_rules: vec![make_rule_path("/a/WARP.md")],
        deleted_rules: vec![],
    });
    delta.merge(RulesDelta {
        discovered_rules: vec![],
        deleted_rules: vec![PathBuf::from("/a/WARP.md")],
    });

    assert!(delta.discovered_rules.is_empty());
    assert_eq!(delta.deleted_rules, vec![PathBuf::from("/a/WARP.md")]);
}

#[test]
fn test_merge_rediscovery_keeps_latest() {
    let mut delta = RulesDelta {
        discovered_rules: vec![make_rule_path("/a/WARP.md")],
        deleted_rules: vec![],
    };
    // A second discovery of the same path (content update) should deduplicate.
    delta.merge(RulesDelta {
        discovered_rules: vec![make_rule_path("/a/WARP.md")],
        deleted_rules: vec![],
    });

    assert_eq!(delta.discovered_rules.len(), 1);
    assert!(delta.deleted_rules.is_empty());
}

#[test]
fn test_missing_rule_content_preserves_cached_content_while_path_is_standing() {
    let rule_path = local_path("/unavailable/project/WARP.md");
    let mut existing_rules = ProjectRules::default();
    existing_rules.upsert_rule(&rule_path, "cached content".to_string());

    let rules = ProjectContextModel::reconcile_project_rules(
        vec![rule_path.clone()],
        Vec::new(),
        existing_rules,
    );
    let result = rules.find_active_or_applicable_rules(&local_path("/unavailable/project/main.rs"));

    assert_eq!(result.active_rules.len(), 1);
    assert_eq!(result.active_rules[0].path, rule_path);
    assert_eq!(result.active_rules[0].content, "cached content");
}

#[test]
fn test_rule_missing_from_standing_results_is_removed_from_cached_content() {
    let rule_path = local_path("/unavailable/project/WARP.md");
    let mut existing_rules = ProjectRules::default();
    existing_rules.upsert_rule(&rule_path, "cached content".to_string());

    let rules =
        ProjectContextModel::reconcile_project_rules(Vec::new(), Vec::new(), existing_rules);
    assert!(rules.rule_paths().next().is_none());
}

// Helper for global-rules tests: inserts a synthetic global rule directly into
// the model. Bypasses the watcher infrastructure (which requires the warpui
// runtime) so we can exercise `find_applicable_rules`'s layering logic.
fn insert_global_rule(model: &mut ProjectContextModel, path: &Path, content: &str) {
    model.global_rules.rules.insert(
        path.to_path_buf(),
        ProjectRule {
            path: LocalOrRemotePath::Local(path.to_path_buf()),
            content: content.to_string(),
        },
    );
}

fn insert_project_rule(
    model: &mut ProjectContextModel,
    project_root: &Path,
    rule_path: &Path,
    content: &str,
) {
    let rules = model
        .path_to_rules
        .entry(LocalOrRemotePath::Local(project_root.to_path_buf()))
        .or_default();
    rules.upsert_rule(
        &LocalOrRemotePath::Local(rule_path.to_path_buf()),
        content.to_string(),
    );
}

#[test]
fn test_global_rule_alone_no_project_rules() {
    let mut model = ProjectContextModel::default();
    insert_global_rule(
        &mut model,
        Path::new("/home/u/.agents/AGENTS.md"),
        "global_content",
    );

    let result = model
        .find_applicable_rules(&local_path("/some/project/file.rs"))
        .expect("global rule should produce a result");

    assert_eq!(result.active_rules.len(), 1);
    assert_eq!(
        result.active_rules[0].path,
        local_path("/home/u/.agents/AGENTS.md")
    );
    assert_eq!(result.active_rules[0].content, "global_content");
    assert!(result.additional_rule_paths.is_empty());
}

#[test]
fn test_global_rule_layered_with_project_warp() {
    let mut model = ProjectContextModel::default();
    insert_global_rule(&mut model, Path::new("/home/u/.agents/AGENTS.md"), "global");
    insert_project_rule(
        &mut model,
        Path::new("/repo"),
        Path::new("/repo/WARP.md"),
        "project_warp",
    );

    let result = model
        .find_applicable_rules(&local_path("/repo/src/main.rs"))
        .expect("layered rules should produce a result");

    // Layered precedence: global first, then project rules.
    assert_eq!(result.active_rules.len(), 2);
    assert_eq!(result.active_rules[0].content, "global");
    assert_eq!(result.active_rules[1].content, "project_warp");
    assert_eq!(result.root_path, local_path("/repo"));
}

#[test]
fn test_in_dir_warp_shadows_agents_with_global() {
    let mut model = ProjectContextModel::default();
    insert_global_rule(&mut model, Path::new("/home/u/.agents/AGENTS.md"), "global");
    // Both WARP.md and AGENTS.md in the same project directory: WARP.md should
    // shadow AGENTS.md (existing in-directory behavior preserved).
    insert_project_rule(
        &mut model,
        Path::new("/repo"),
        Path::new("/repo/WARP.md"),
        "project_warp",
    );
    insert_project_rule(
        &mut model,
        Path::new("/repo"),
        Path::new("/repo/AGENTS.md"),
        "project_agents",
    );

    let result = model
        .find_applicable_rules(&local_path("/repo/src/main.rs"))
        .expect("layered rules should produce a result");

    // Expect: [global, project WARP.md]. project AGENTS.md is shadowed.
    assert_eq!(result.active_rules.len(), 2);
    assert_eq!(result.active_rules[0].content, "global");
    assert_eq!(result.active_rules[1].content, "project_warp");
}

#[test]
fn test_no_rules_returns_none() {
    let model = ProjectContextModel::default();
    let result = model.find_applicable_rules(&local_path("/some/path/file.rs"));
    assert!(result.is_none());
}

#[test]
fn test_global_rule_root_path_falls_back_to_parent() {
    let mut model = ProjectContextModel::default();
    insert_global_rule(&mut model, Path::new("/home/u/.agents/AGENTS.md"), "global");

    let result = model
        .find_applicable_rules(&local_path("/some/file.rs"))
        .expect("global rule should produce a result");

    // No project root indexed; root_path falls back to parent of the global rule.
    assert_eq!(result.root_path, local_path("/home/u/.agents"));
}

#[test]
fn test_multiple_global_rules_all_contribute() {
    let mut model = ProjectContextModel::default();
    insert_global_rule(
        &mut model,
        Path::new("/home/u/.agents/AGENTS.md"),
        "agents_global",
    );
    insert_global_rule(
        &mut model,
        Path::new("/home/u/.warp/WARP.md"),
        "warp_global",
    );

    let result = model
        .find_applicable_rules(&local_path("/repo/src/main.rs"))
        .expect("globals should produce a result");

    assert_eq!(result.active_rules.len(), 2);
    let contents: Vec<&str> = result
        .active_rules
        .iter()
        .map(|r| r.content.as_str())
        .collect();
    assert!(contents.contains(&"agents_global"));
    assert!(contents.contains(&"warp_global"));
}

#[test]
fn test_superseding_refresh_coalesces_without_overlapping_reads() {
    use std::sync::OnceLock;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::time::Duration;

    use repo_metadata::repositories::DetectedRepositories;
    use warpui_core::App;
    use warpui_core::r#async::Timer;

    /// Coordinates a background reader that blocks until released, so the test can observe a
    /// read that is genuinely in flight (rather than one that resolves before the next
    /// refresh request is even issued).
    struct ReadCoordinator {
        active_readers: AtomicUsize,
        max_active_readers: AtomicUsize,
        completed_rounds: AtomicUsize,
        release: AtomicBool,
    }

    fn coordinator() -> &'static ReadCoordinator {
        static INSTANCE: OnceLock<ReadCoordinator> = OnceLock::new();
        INSTANCE.get_or_init(|| ReadCoordinator {
            active_readers: AtomicUsize::new(0),
            max_active_readers: AtomicUsize::new(0),
            completed_rounds: AtomicUsize::new(0),
            release: AtomicBool::new(false),
        })
    }

    fn repo_root() -> StandardizedPath {
        let root = if cfg!(windows) { "C:/repo" } else { "/repo" };
        StandardizedPath::try_new(root).unwrap()
    }

    fn path_in_repo(name: &str) -> LocalOrRemotePath {
        local_path(repo_root().join(name).as_str())
    }

    // The first round reports a rule; the coalesced second round reports none, so the test can
    // confirm the latest (empty) result wins instead of leaving stale content behind.
    fn controlled_content_reader(
        _paths: Vec<LocalOrRemotePath>,
    ) -> BoxFuture<'static, anyhow::Result<ProjectRuleContents>> {
        Box::pin(async move {
            let coordinator = coordinator();
            let active = coordinator.active_readers.fetch_add(1, Ordering::SeqCst) + 1;
            coordinator
                .max_active_readers
                .fetch_max(active, Ordering::SeqCst);

            while !coordinator.release.load(Ordering::SeqCst) {
                Timer::after(Duration::from_millis(1)).await;
            }

            coordinator.active_readers.fetch_sub(1, Ordering::SeqCst);
            let round = coordinator.completed_rounds.fetch_add(1, Ordering::SeqCst);
            let contents = if round == 0 {
                vec![(path_in_repo("WARP.md"), "stale-round".to_string())]
            } else {
                Vec::new()
            };
            Ok(contents)
        })
    }

    App::test((), |mut app| async move {
        app.add_singleton_model(|_| DetectedRepositories::default());
        app.add_singleton_model(RepoMetadataModel::new);
        let model = app.add_model(|_| ProjectContextModel::default());
        let repo_id = RepositoryIdentifier(repo_root());
        let coordinator = coordinator();

        // First refresh: its read blocks on `coordinator.release` until we let it proceed
        // below, simulating a slow, uncancellable file read that is genuinely in flight.
        model.update(&mut app, |model, ctx| {
            model.refresh_project_rules_for_repo(repo_id.clone(), controlled_content_reader, ctx);
        });

        // Wait until the first read has actually started before superseding it, so the test
        // exercises real overlap rather than two sequential no-ops.
        for _ in 0..2000 {
            if coordinator.active_readers.load(Ordering::SeqCst) >= 1 {
                break;
            }
            Timer::after(Duration::from_millis(1)).await;
        }
        assert_eq!(
            coordinator.active_readers.load(Ordering::SeqCst),
            1,
            "first refresh should be blocked mid-read"
        );

        // Second refresh supersedes the first while it is still reading files. Coalescing must
        // record this request rather than spawn a second, overlapping read.
        model.update(&mut app, |model, ctx| {
            model.refresh_project_rules_for_repo(repo_id.clone(), controlled_content_reader, ctx);
        });
        // Give the executor a chance to (incorrectly) start a second overlapping read before
        // checking the invariant.
        Timer::after(Duration::from_millis(5)).await;
        assert_eq!(
            coordinator.active_readers.load(Ordering::SeqCst),
            1,
            "a superseding refresh must not start a second concurrent read"
        );

        // Let the in-flight (stale) read and its coalesced follow-up run to completion, then
        // wait for the model to finish applying both rounds' completion callbacks. The coalesced
        // (second, latest) refresh finds no rules, so the stale first-round rule must not
        // survive: coalescing must not silently leave `path_to_rules` stuck on stale content
        // applied by the superseded read.
        coordinator.release.store(true, Ordering::SeqCst);
        let mut observed_two_rounds = false;
        let mut has_stale_rule = true;
        for _ in 0..5000 {
            if coordinator.completed_rounds.load(Ordering::SeqCst) >= 2 {
                observed_two_rounds = true;
            }
            has_stale_rule = model.read(&app, |model, _ctx| {
                model
                    .find_applicable_project_rules(&path_in_repo("main.rs"))
                    .is_some()
            });
            if observed_two_rounds && !has_stale_rule {
                break;
            }
            Timer::after(Duration::from_millis(1)).await;
        }
        assert!(
            observed_two_rounds,
            "the coalesced refresh should run once the in-flight one completes"
        );
        assert_eq!(
            coordinator.max_active_readers.load(Ordering::SeqCst),
            1,
            "no two reads should ever have been active at the same time"
        );
        assert!(
            !has_stale_rule,
            "the latest (empty) refresh result must win over the stale in-flight one"
        );
    });
}
