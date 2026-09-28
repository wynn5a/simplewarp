use std::time::{Duration, SystemTime};

use ai::skills::{ParsedSkill, SkillProvider, SkillReference, SkillScope};
use repo_metadata::repositories::DetectedRepositories;
use repo_metadata::{DirectoryWatcher, RepoMetadataModel};
use warp_util::host_id::HostId;
use warp_util::local_or_remote_path::LocalOrRemotePath;
use warp_util::remote_path::RemotePath;
use warp_util::standardized_path::StandardizedPath;
use warpui::App;
use watcher::HomeDirectoryWatcher;

use super::{
    RecordingCardText, parsed_skill_for_common_locations, read_skill_display_text,
    start_recording_card_text, stop_recording_card_text,
};
use crate::ai::agent::{
    RecordingStarted, RecordingStopped, StartRecordingResult, StopRecordingResult,
};
use crate::ai::skills::SkillManager;
use crate::settings::AISettings;
use crate::warp_managed_paths_watcher::WarpManagedPathsWatcher;

#[test]
fn start_recording_card_text_uses_static_title_and_description_subtext() {
    let result = StartRecordingResult::Success(RecordingStarted {
        recording_id: "rec-1".to_string(),
        started_at: SystemTime::UNIX_EPOCH,
        width_px: 1280,
        height_px: 720,
    });

    let text = start_recording_card_text("Demo checkout flow", Some(&result));

    assert_eq!(
        text,
        RecordingCardText {
            primary: "Recording started".to_string(),
            subtext: Some("Demo checkout flow".to_string()),
        }
    );
}

#[test]
fn start_recording_card_text_includes_failure_copy() {
    let result = StartRecordingResult::Error("unsupported platform".to_string());

    let text = start_recording_card_text("Demo checkout flow", Some(&result));

    assert_eq!(
        text,
        RecordingCardText {
            primary: "Recording failed to start".to_string(),
            subtext: Some("unsupported platform".to_string()),
        }
    );
}

#[test]
fn stop_recording_card_text_includes_complete_duration() {
    let result = StopRecordingResult::Success(RecordingStopped {
        artifact_uid: "artifact-1".to_string(),
        duration: Duration::from_secs(2),
        width_px: 1280,
        height_px: 720,
        size_bytes: 42,
        completion_status: computer_use::RecordingCompletionStatus::Completed,
        termination_reason: "Stopped by agent".to_string(),
    });

    let text = stop_recording_card_text(Some(&result));

    assert_eq!(
        text,
        RecordingCardText {
            primary: "Recording saved".to_string(),
            subtext: Some("0:02".to_string()),
        }
    );
}

#[test]
fn stop_recording_card_text_includes_partial_duration_without_raw_reason() {
    let result = StopRecordingResult::Success(RecordingStopped {
        artifact_uid: "artifact-1".to_string(),
        duration: Duration::from_secs(12),
        width_px: 1280,
        height_px: 720,
        size_bytes: 42,
        completion_status: computer_use::RecordingCompletionStatus::StoppedEarly,
        termination_reason: "internal raw reason".to_string(),
    });

    let text = stop_recording_card_text(Some(&result));

    assert_eq!(
        text,
        RecordingCardText {
            primary: "Recording saved".to_string(),
            subtext: Some("Partial recording • 0:12".to_string()),
        }
    );
}

fn make_skill(name: &str) -> ParsedSkill {
    ParsedSkill {
        name: name.to_string(),
        description: String::new(),
        path: LocalOrRemotePath::Local(
            std::path::PathBuf::from("/home/user/.agents/skills")
                .join(name)
                .join("SKILL.md"),
        ),
        content: String::new(),
        line_range: None,
        provider: SkillProvider::Agents,
        scope: SkillScope::Home,
    }
}

#[test]
fn read_skill_display_text_shows_slash_command_when_skill_found() {
    let skill = make_skill("hello-world");
    let reference = SkillReference::Path(skill.path.clone());
    assert_eq!(
        read_skill_display_text(Some(&skill), &reference),
        "/hello-world"
    );
}

#[test]
fn read_skill_display_text_no_double_slash_when_skill_not_found_with_path_reference() {
    // When the skill is not in the manager the fallback is skill_reference.to_string(),
    // which for a path reference is an absolute path starting with '/'.  The display
    // text must NOT prepend an extra '/' — doing so would produce '//home/…'.
    let path = LocalOrRemotePath::Local(std::path::PathBuf::from(
        "/home/devbox/.warp-local/skills/hello-world/SKILL.md",
    ));
    let reference = SkillReference::Path(path);
    let display = read_skill_display_text(None, &reference);
    assert!(
        !display.starts_with("//"),
        "display text must not start with '//': {display}"
    );
    assert!(
        display.starts_with('/'),
        "display text should start with '/': {display}"
    );
}

#[test]
fn read_skill_display_text_bundled_id_fallback_when_skill_not_found() {
    // The fallback uses the user-facing label (the bare id), not the canonical
    // `@warp-skill:<id>` reference form, so bundled-skill copy reads the same
    // way as path-based skill copy.
    let reference = SkillReference::BundledSkillId("create-pr".to_string());
    let display = read_skill_display_text(None, &reference);
    assert_eq!(display, "create-pr");
}

fn remote_location(host_id: &HostId, path: &str) -> LocalOrRemotePath {
    LocalOrRemotePath::Remote(RemotePath::new(
        host_id.clone(),
        StandardizedPath::try_new(path).unwrap(),
    ))
}

#[test]
fn parsed_skill_for_common_locations_resolves_cached_remote_skill() {
    let host_id = HostId::new("remote-host".to_string());
    let skill = ParsedSkill {
        name: "deploy".to_string(),
        description: "Deploy skill".to_string(),
        path: remote_location(&host_id, "/repo/.agents/skills/deploy/SKILL.md"),
        content: "# Deploy".to_string(),
        line_range: None,
        provider: SkillProvider::Agents,
        scope: SkillScope::Project,
    };
    let locations = vec![
        remote_location(&host_id, "/repo/.agents/skills/deploy/README.md"),
        remote_location(&host_id, "/repo/.agents/skills/deploy/scripts/run.sh"),
    ];

    App::test((), |mut app| async move {
        app.add_singleton_model(DirectoryWatcher::new);
        app.add_singleton_model(AISettings::new_with_defaults);
        app.add_singleton_model(|_| DetectedRepositories::default());
        app.add_singleton_model(RepoMetadataModel::new);
        app.add_singleton_model(HomeDirectoryWatcher::new_for_test);
        app.add_singleton_model(WarpManagedPathsWatcher::new_for_testing);
        let manager = app.add_singleton_model(SkillManager::new);
        manager.update(&mut app, |manager, _| {
            manager.add_skill_for_testing(skill.clone());
        });

        let resolved = manager.read(&app, |_, ctx| {
            parsed_skill_for_common_locations(locations, ctx).map(|skill| skill.path.clone())
        });
        assert_eq!(resolved, Some(skill.path));
    });
}

#[test]
fn parsed_skill_for_common_locations_does_not_mix_remote_hosts() {
    let first_host = HostId::new("first-host".to_string());
    let second_host = HostId::new("second-host".to_string());
    let skill = ParsedSkill {
        name: "deploy".to_string(),
        description: "Deploy skill".to_string(),
        path: remote_location(&first_host, "/repo/.agents/skills/deploy/SKILL.md"),
        content: "# Deploy".to_string(),
        line_range: None,
        provider: SkillProvider::Agents,
        scope: SkillScope::Project,
    };
    let locations = vec![
        remote_location(&first_host, "/repo/.agents/skills/deploy/README.md"),
        remote_location(&second_host, "/repo/.agents/skills/deploy/README.md"),
    ];

    App::test((), |mut app| async move {
        app.add_singleton_model(DirectoryWatcher::new);
        app.add_singleton_model(AISettings::new_with_defaults);
        app.add_singleton_model(|_| DetectedRepositories::default());
        app.add_singleton_model(RepoMetadataModel::new);
        app.add_singleton_model(HomeDirectoryWatcher::new_for_test);
        app.add_singleton_model(WarpManagedPathsWatcher::new_for_testing);
        let manager = app.add_singleton_model(SkillManager::new);
        manager.update(&mut app, |manager, _| {
            manager.add_skill_for_testing(skill);
        });

        assert!(manager.read(&app, |_, ctx| {
            parsed_skill_for_common_locations(locations, ctx).is_none()
        }));
    });
}
