use std::collections::{HashMap, HashSet};
use std::fs;

use ai::skills::{ParsedSkill, SkillProvider, SkillReference, SkillScope};
use repo_metadata::repositories::DetectedRepositories;
use repo_metadata::{DirectoryWatcher, RepoMetadataModel};
use tempfile::TempDir;
use warp_core::channel::ChannelState;
use warp_util::local_or_remote_path::LocalOrRemotePath;
use warpui::App;
use watcher::HomeDirectoryWatcher;

use super::*;
use crate::settings::AISettings;
use crate::warp_managed_paths_watcher::WarpManagedPathsWatcher;

// ============================================================================
// Tests for get_skills_for_working_directory subdirectory scoping
// ============================================================================

#[test]
fn get_skills_for_working_directory_scopes_subdirectory_skills() {
    // This test verifies the key scoping behavior:
    // - Root skills are visible from anywhere in the repo
    // - Subdirectory skills are only visible when working_directory is within that subdirectory

    // Use real temp directories so DetectedRepositories can canonicalize paths
    // and correctly report repo_root, which controls ancestor-vs-descendant scoping.
    // Canonicalize the temp base to avoid macOS /var -> /private/var symlink mismatches.
    let temp = TempDir::new().unwrap();
    let base = dunce::canonicalize(temp.path()).unwrap();
    let repo = base.join("repo");
    let frontend_dir = repo.join("packages/frontend");
    let backend_dir = repo.join("packages/backend");
    fs::create_dir_all(&frontend_dir).unwrap();
    fs::create_dir_all(&backend_dir).unwrap();

    // Create mock skills
    let root_skill_path = LocalOrRemotePath::Local(repo.join(".agents/skills/root-skill/SKILL.md"));
    let frontend_skill_path =
        LocalOrRemotePath::Local(frontend_dir.join(".agents/skills/frontend-skill/SKILL.md"));

    let root_skill = ParsedSkill {
        name: "root-skill".to_string(),
        description: "A root skill".to_string(),
        path: root_skill_path.clone(),
        content: "# Root skill".to_string(),
        line_range: None,
        provider: SkillProvider::Agents,
        scope: SkillScope::Project,
    };

    let frontend_skill = ParsedSkill {
        name: "frontend-skill".to_string(),
        description: "A frontend skill".to_string(),
        path: frontend_skill_path.clone(),
        content: "# Frontend skill".to_string(),
        line_range: None,
        provider: SkillProvider::Agents,
        scope: SkillScope::Project,
    };

    // Build the internal state manually
    let mut directory_skills: HashMap<LocalOrRemotePath, HashSet<LocalOrRemotePath>> =
        HashMap::new();
    directory_skills
        .entry(LocalOrRemotePath::Local(repo.clone()))
        .or_default()
        .insert(root_skill_path.clone());
    directory_skills
        .entry(LocalOrRemotePath::Local(frontend_dir.clone()))
        .or_default()
        .insert(frontend_skill_path.clone());

    let mut skills_by_path: HashMap<LocalOrRemotePath, ParsedSkill> = HashMap::new();
    skills_by_path.insert(root_skill_path.clone(), root_skill);
    skills_by_path.insert(frontend_skill_path.clone(), frontend_skill);

    App::test((), |mut app| async move {
        app.add_singleton_model(DirectoryWatcher::new);
        app.add_singleton_model(AISettings::new_with_defaults);
        let repo_handle = app.add_singleton_model(|_| DetectedRepositories::default());
        app.add_singleton_model(RepoMetadataModel::new);
        app.add_singleton_model(HomeDirectoryWatcher::new_for_test);
        app.add_singleton_model(WarpManagedPathsWatcher::new_for_testing);
        let skill_manager_handle = app.add_singleton_model(SkillManager::new);

        // Register the repo root so get_root_for_path returns Some.
        let canonical_repo =
            warp_util::standardized_path::StandardizedPath::from_local_canonicalized(&repo)
                .unwrap();
        repo_handle.update(&mut app, |repos, _ctx| {
            repos.insert_test_repo_root(canonical_repo);
        });

        // Inject the test state
        skill_manager_handle.update(&mut app, |manager, _ctx| {
            manager.directory_skills = directory_skills;
            manager.skills_by_path = skills_by_path;
        });

        // Test 1: From frontend directory, should see both root and frontend skills
        let skills_from_frontend = skill_manager_handle.read(&app, |manager, ctx| {
            manager.get_skills_for_working_directory(
                Some(&LocalOrRemotePath::Local(frontend_dir.clone())),
                ctx,
            )
        });
        let names_from_frontend: Vec<&str> = skills_from_frontend
            .iter()
            .map(|s| s.name.as_str())
            .collect();
        assert!(
            names_from_frontend.contains(&"root-skill"),
            "Root skill should be visible from frontend dir"
        );
        assert!(
            names_from_frontend.contains(&"frontend-skill"),
            "Frontend skill should be visible from frontend dir"
        );

        // Test 2: From backend directory, should only see root skill (not frontend skill)
        let skills_from_backend = skill_manager_handle.read(&app, |manager, ctx| {
            manager.get_skills_for_working_directory(
                Some(&LocalOrRemotePath::Local(backend_dir.clone())),
                ctx,
            )
        });
        let names_from_backend: Vec<&str> = skills_from_backend
            .iter()
            .map(|s| s.name.as_str())
            .collect();
        assert!(
            names_from_backend.contains(&"root-skill"),
            "Root skill should be visible from backend dir"
        );
        assert!(
            !names_from_backend.contains(&"frontend-skill"),
            "Frontend skill should NOT be visible from backend dir"
        );

        // Test 3: From repo root, should only see root skill (not frontend skill)
        let skills_from_root = skill_manager_handle.read(&app, |manager, ctx| {
            manager.get_skills_for_working_directory(
                Some(&LocalOrRemotePath::Local(repo.clone())),
                ctx,
            )
        });
        let names_from_root: Vec<&str> = skills_from_root.iter().map(|s| s.name.as_str()).collect();
        assert!(
            names_from_root.contains(&"root-skill"),
            "Root skill should be visible from repo root"
        );
        assert!(
            !names_from_root.contains(&"frontend-skill"),
            "Frontend skill should NOT be visible from repo root"
        );
    });
}

#[test]
fn get_skills_for_working_directory_name_collision_returns_both() {
    // When the same skill name exists at root and subdirectory, both should be returned.
    // The caller (agent) is responsible for precedence based on path proximity.

    // Use real temp directories so DetectedRepositories can canonicalize paths.
    // Canonicalize the temp base to avoid macOS /var -> /private/var symlink mismatches.
    let temp = TempDir::new().unwrap();
    let base = dunce::canonicalize(temp.path()).unwrap();
    let repo = base.join("repo");
    let subdir = repo.join("packages/frontend");
    fs::create_dir_all(&subdir).unwrap();

    let root_skill_path = LocalOrRemotePath::Local(repo.join(".agents/skills/deploy/SKILL.md"));
    let subdir_skill_path = LocalOrRemotePath::Local(subdir.join(".agents/skills/deploy/SKILL.md"));

    let root_skill = ParsedSkill {
        name: "deploy".to_string(),
        description: "Root deploy".to_string(),
        path: root_skill_path.clone(),
        content: "# Root deploy".to_string(),
        line_range: None,
        provider: SkillProvider::Agents,
        scope: SkillScope::Project,
    };

    let subdir_skill = ParsedSkill {
        name: "deploy".to_string(),
        description: "Subdir deploy".to_string(),
        path: subdir_skill_path.clone(),
        content: "# Subdir deploy".to_string(),
        line_range: None,
        provider: SkillProvider::Agents,
        scope: SkillScope::Project,
    };

    let mut directory_skills: HashMap<LocalOrRemotePath, HashSet<LocalOrRemotePath>> =
        HashMap::new();
    directory_skills
        .entry(LocalOrRemotePath::Local(repo.clone()))
        .or_default()
        .insert(root_skill_path.clone());
    directory_skills
        .entry(LocalOrRemotePath::Local(subdir.clone()))
        .or_default()
        .insert(subdir_skill_path.clone());

    let mut skills_by_path: HashMap<LocalOrRemotePath, ParsedSkill> = HashMap::new();
    skills_by_path.insert(root_skill_path.clone(), root_skill);
    skills_by_path.insert(subdir_skill_path.clone(), subdir_skill);

    App::test((), |mut app| async move {
        app.add_singleton_model(DirectoryWatcher::new);
        app.add_singleton_model(AISettings::new_with_defaults);
        let repo_handle = app.add_singleton_model(|_| DetectedRepositories::default());
        app.add_singleton_model(RepoMetadataModel::new);
        app.add_singleton_model(HomeDirectoryWatcher::new_for_test);
        app.add_singleton_model(WarpManagedPathsWatcher::new_for_testing);
        let skill_manager_handle = app.add_singleton_model(SkillManager::new);

        // Register the repo root so get_root_for_path returns Some.
        let canonical_repo =
            warp_util::standardized_path::StandardizedPath::from_local_canonicalized(&repo)
                .unwrap();
        repo_handle.update(&mut app, |repos, _ctx| {
            repos.insert_test_repo_root(canonical_repo);
        });

        skill_manager_handle.update(&mut app, |manager, _ctx| {
            manager.directory_skills = directory_skills;
            manager.skills_by_path = skills_by_path;
        });

        // From subdir: should see both "deploy" skills (root + subdir)
        let skills = skill_manager_handle.read(&app, |manager, ctx| {
            manager.get_skills_for_working_directory(
                Some(&LocalOrRemotePath::Local(subdir.clone())),
                ctx,
            )
        });
        let deploy_skills: Vec<_> = skills.iter().filter(|s| s.name == "deploy").collect();
        assert_eq!(
            deploy_skills.len(),
            2,
            "Both deploy skills should be visible from subdir"
        );

        // From repo root: should only see root "deploy"
        let skills = skill_manager_handle.read(&app, |manager, ctx| {
            manager.get_skills_for_working_directory(
                Some(&LocalOrRemotePath::Local(repo.clone())),
                ctx,
            )
        });
        let deploy_skills: Vec<_> = skills.iter().filter(|s| s.name == "deploy").collect();
        assert_eq!(
            deploy_skills.len(),
            1,
            "Only root deploy should be visible from repo root"
        );
        assert_eq!(deploy_skills[0].description, "Root deploy");
    });
}

#[test]
fn test_read_bundled_skills_with_variable_substitution() {
    let temp_dir = TempDir::new().unwrap();
    let resources_dir = temp_dir.path();
    let skills_dir = resources_dir.join("bundled/skills");

    // Create a test skill with variables
    let skill_dir = skills_dir.join("test-skill");
    fs::create_dir_all(&skill_dir).unwrap();
    let skill_file = skill_dir.join("SKILL.md");
    fs::write(
        &skill_file,
        r#"---
name: test-skill
description: Test skill with variables
---

Run `{{warp_cli_binary_name}}` from {{warp_url_scheme}}.
"#,
    )
    .unwrap();

    let skills = futures::executor::block_on(read_bundled_skills(&skills_dir, temp_dir.path()));

    assert_eq!(skills.len(), 1);
    let skill = skills.get("test-skill").unwrap();

    let expected_cli = ChannelState::cli_command_name();
    let expected_scheme = ChannelState::url_scheme();
    assert!(
        skill
            .content
            .contains(&format!("Run `{expected_cli}` from {expected_scheme}."))
    );
}

#[test]
fn test_read_bundled_skills_renders_host_paths() {
    let temp_dir = TempDir::new().unwrap();
    let resources_dir = temp_dir.path();
    let skills_dir = resources_dir.join("bundled/skills");
    let skill_dir = skills_dir.join("test-skill");
    fs::create_dir_all(&skill_dir).unwrap();
    fs::write(
        skill_dir.join("SKILL.md"),
        r#"---
name: test-skill
description: Test path rendering
---

Use {{skill_dir}} and {{settings_schema_path}}.
"#,
    )
    .unwrap();

    let skills = futures::executor::block_on(read_bundled_skills(&skills_dir, resources_dir));

    let skill = skills.get("test-skill").unwrap();
    // The skill's reported path and rendered variables are anchored to the
    // resources root the skills were read from.
    assert_eq!(
        skill.path,
        LocalOrRemotePath::Local(skill_dir.join("SKILL.md"))
    );
    assert!(skill.content.contains(&skill_dir.display().to_string()));
    assert!(
        skill.content.contains(
            &resources_dir
                .join("settings_schema.json")
                .display()
                .to_string()
        )
    );
}

#[test]
fn test_read_bundled_skills_preserves_other_content() {
    let temp_dir = TempDir::new().unwrap();
    let resources_dir = temp_dir.path();
    let skills_dir = resources_dir.join("bundled/skills");

    // Create a test skill with both warp and non-warp variables
    let skill_dir = skills_dir.join("test-skill");
    fs::create_dir_all(&skill_dir).unwrap();
    let skill_file = skill_dir.join("SKILL.md");
    fs::write(
        &skill_file,
        r#"---
name: test-skill
description: Test skill with mixed variables
---

Use {{other_var}}, {{warp_cli_binary_name}}, and {{skill_dir}} together.
"#,
    )
    .unwrap();

    let skills = futures::executor::block_on(read_bundled_skills(&skills_dir, resources_dir));

    assert_eq!(skills.len(), 1);
    let skill = skills.get("test-skill").unwrap();

    let expected_cli = ChannelState::cli_command_name();
    assert!(skill.content.contains(&format!(
        "Use {{{{other_var}}}}, {expected_cli}, and {} together.",
        skill_dir.display()
    )));
}

#[test]
fn test_read_bundled_skills_no_variables() {
    let temp_dir = TempDir::new().unwrap();
    let resources_dir = temp_dir.path();
    let skills_dir = resources_dir.join("bundled/skills");

    // Create a test skill with no variables
    let skill_dir = skills_dir.join("test-skill");
    fs::create_dir_all(&skill_dir).unwrap();
    let skill_file = skill_dir.join("SKILL.md");
    fs::write(
        &skill_file,
        r#"---
name: test-skill
description: Test skill without variables
---

Plain content with no variables.
"#,
    )
    .unwrap();

    let skills = futures::executor::block_on(read_bundled_skills(&skills_dir, resources_dir));

    assert_eq!(skills.len(), 1);
    let skill = skills.get("test-skill").unwrap();
    assert!(skill.content.contains("Plain content with no variables."));
}

#[test]
fn test_build_bundled_skill_context() {
    let temp_dir = TempDir::new().unwrap();
    let resources_dir = temp_dir.path();
    let skill_dir = resources_dir.join("bundled/skills/test-skill");
    let context = build_bundled_skill_context(resources_dir, &skill_dir);

    assert_eq!(context.len(), 8);
    assert!(context.contains_key("warp_cli_binary_name"));
    assert!(context.contains_key("warp_url_scheme"));
    assert!(context.contains_key("settings_file_path"));
    assert!(context.contains_key("keybindings_file_path"));
    assert_eq!(
        context.get("gui_settings_file_path").unwrap(),
        &warp_core::paths::gui_config_local_dir()
            .map(|path| path.join("settings.toml"))
            .unwrap_or_default()
            .display()
            .to_string()
    );
    assert_eq!(
        context.get("gui_mcp_config_file_path").unwrap(),
        &warp_core::paths::gui_mcp_config_file_path()
            .unwrap_or_default()
            .display()
            .to_string()
    );
    assert_eq!(
        context.get("settings_schema_path").unwrap(),
        &resources_dir
            .join("settings_schema.json")
            .display()
            .to_string()
    );
    assert_eq!(
        context.get("skill_dir").unwrap(),
        &skill_dir.display().to_string()
    );

    assert_eq!(
        context.get("warp_cli_binary_name").unwrap(),
        ChannelState::cli_command_name()
    );
    assert_eq!(
        context.get("warp_url_scheme").unwrap(),
        ChannelState::url_scheme()
    );
    assert_eq!(
        context.get("settings_file_path").unwrap(),
        &crate::settings::user_preferences_toml_file_path()
            .display()
            .to_string()
    );
    assert_eq!(
        context.get("keybindings_file_path").unwrap(),
        &crate::keyboard::keybinding_file_path()
            .display()
            .to_string()
    );
}

// Origin-aware lookup reports why an active bundled skill could not be resolved.
#[test]
fn active_skill_by_reference_with_origin_returns_typed_lookup_errors() {
    App::test((), |app| async move {
        app.add_singleton_model(DirectoryWatcher::new);
        app.add_singleton_model(AISettings::new_with_defaults);
        app.add_singleton_model(|_| DetectedRepositories::default());
        app.add_singleton_model(RepoMetadataModel::new);
        app.add_singleton_model(HomeDirectoryWatcher::new_for_test);
        app.add_singleton_model(WarpManagedPathsWatcher::new_for_testing);
        let handle = app.add_singleton_model(SkillManager::new);
        let reference = SkillReference::BundledSkillId("missing".to_string());

        let unavailable_error = handle.read(&app, |manager, ctx| {
            manager
                .active_skill_by_reference_with_origin(
                    &reference,
                    &SkillPathOrigin::Unavailable,
                    ctx,
                )
                .unwrap_err()
        });
        assert_eq!(
            unavailable_error,
            ActiveSkillLookupError::BundledSkillsUnavailable
        );

        let not_found_error = handle.read(&app, |manager, ctx| {
            manager
                .active_skill_by_reference_with_origin(&reference, &SkillPathOrigin::Local, ctx)
                .unwrap_err()
        });
        assert_eq!(
            not_found_error,
            ActiveSkillLookupError::NotFound { reference }
        );
    });
}
