use std::collections::HashMap;

use warp_multi_agent_api as api;
use warp_multi_agent_api::input_context as ic;

use super::*;

fn context() -> api::InputContext {
    api::InputContext {
        directory: Some(ic::Directory {
            pwd: "/work/app".to_string(),
            home: "/Users/me".to_string(),
            ..Default::default()
        }),
        operating_system: Some(ic::OperatingSystem {
            platform: "MacOS".to_string(),
            distribution: String::new(),
        }),
        shell: Some(ic::Shell {
            name: "zsh".to_string(),
            version: "5.9".to_string(),
        }),
        current_time: Some(prost_types::Timestamp {
            seconds: 1_790_000_000,
            nanos: 0,
        }),
        ..Default::default()
    }
}

#[test]
fn no_context_renders_nothing() {
    assert_eq!(environment(None), "");
    assert_eq!(attachments(None, &HashMap::new()), "");
}

#[test]
fn the_environment_names_the_directory_os_shell_and_time() {
    let text = environment(Some(&context()));
    assert!(text.contains("Working directory: /work/app"));
    assert!(text.contains("Operating system: MacOS"));
    assert!(text.contains("Shell: zsh 5.9"));
    assert!(text.contains("Current time: 2026-09-21 "), "{text}");
}

#[test]
fn git_state_and_the_pull_request_are_described() {
    let mut context = context();
    context.git = Some(ic::Git {
        branch: "feature".to_string(),
        repository: Some(ic::git::Repository {
            name: "app".to_string(),
            owner: "me".to_string(),
            host: "github.com".to_string(),
        }),
        pull_request: Some(ic::git::PullRequest {
            number: 7,
            base_branch: "main".to_string(),
            url: "https://github.com/me/app/pull/7".to_string(),
            ..Default::default()
        }),
        ..Default::default()
    });
    let text = environment(Some(&context));
    assert!(text.contains("Git repository: me/app on github.com"));
    assert!(text.contains("Git branch: feature"));
    assert!(text.contains("#7 into main"));
}

#[test]
fn project_rules_are_included_with_their_path() {
    let mut context = context();
    context.project_rules = vec![ic::ProjectRules {
        root_path: "/work/app".to_string(),
        active_rule_files: vec![api::FileContent {
            file_path: "/work/app/AGENTS.md".to_string(),
            content: "Always run the tests.".to_string(),
            ..Default::default()
        }],
        additional_rule_file_paths: vec!["/work/app/sub/AGENTS.md".to_string()],
    }];
    let text = environment(Some(&context));
    assert!(text.contains("# Project rules"));
    assert!(text.contains("/work/app/AGENTS.md"));
    assert!(text.contains("Always run the tests."));
    assert!(text.contains("- /work/app/sub/AGENTS.md"));
}

#[test]
fn empty_project_rules_add_no_section() {
    let mut context = context();
    context.project_rules = vec![ic::ProjectRules::default()];
    assert!(!environment(Some(&context)).contains("Project rules"));
}

#[test]
fn available_skills_are_listed() {
    let mut context = context();
    context.updated_skills_context = Some(ic::SkillsContext {
        available_skills: vec![api::SkillDescriptor {
            name: "deploy".to_string(),
            description: "Ship it".to_string(),
            skill_reference: Some(api::skill_descriptor::SkillReference::Path(
                "/home/.agents/skills/deploy/SKILL.md".to_string(),
            )),
            ..Default::default()
        }],
    });
    let text = environment(Some(&context));
    assert!(text.contains("- deploy: Ship it (/home/.agents/skills/deploy/SKILL.md)"));
}

fn plain_text(text: &str) -> api::Attachment {
    api::Attachment {
        value: Some(AttachmentValue::PlainText(text.to_string())),
    }
}

#[test]
fn referenced_attachments_are_named_by_their_key() {
    let referenced = HashMap::from([
        ("b".to_string(), plain_text("two")),
        ("a".to_string(), plain_text("one")),
    ]);
    let text = attachments(None, &referenced);
    let a = text.find("Attachment `a`").expect("a");
    let b = text.find("Attachment `b`").expect("b");
    assert!(a < b, "attachments are ordered by key");
    assert!(text.contains("one") && text.contains("two"));
}

#[test]
fn executed_commands_selected_text_and_files_are_rendered() {
    let mut context = context();
    #[allow(deprecated)]
    context
        .executed_shell_commands
        .push(api::ExecutedShellCommand {
            command: "cargo test".to_string(),
            output: "1 failed".to_string(),
            exit_code: 101,
            ..Default::default()
        });
    context.selected_text.push(ic::SelectedText {
        text: "selected".to_string(),
    });
    context.files.push(ic::File {
        content: Some(api::FileContent {
            file_path: "src/lib.rs".to_string(),
            content: "fn main() {}".to_string(),
            line_range: Some(api::FileContentLineRange { start: 3, end: 9 }),
        }),
    });
    let text = attachments(Some(&context), &HashMap::new());
    assert!(text.contains("Terminal command `cargo test` (exit code 101)"));
    assert!(text.contains("1 failed"));
    assert!(text.contains("Text the user selected"));
    assert!(text.contains("File `src/lib.rs`, lines 3-9"));
}

#[test]
fn images_are_reported_as_unseen() {
    let mut context = context();
    context.images.push(ic::Image::default());
    let text = attachments(Some(&context), &HashMap::new());
    assert!(text.contains("1 image(s)"));
    assert!(text.contains("cannot see images"));
}

#[test]
fn a_fence_is_longer_than_any_backtick_run_in_the_text() {
    let text = fenced("before ```` after", 1000);
    assert!(text.starts_with("`````\n"), "{text}");
    assert!(text.ends_with("\n`````"));
}

#[test]
fn long_text_is_cut_and_says_so() {
    let text = fenced(&"x".repeat(100), 10);
    assert!(text.contains("xxxxxxxxxx\n(cut"));
    assert!(!text.contains(&"x".repeat(11)));
}

#[test]
fn text_is_cut_on_a_character_boundary() {
    // Each character here is several bytes, so a byte cut would split one.
    let text = fenced(&"é".repeat(20), 5);
    assert!(text.contains(&"é".repeat(5)));
    assert!(!text.contains(&"é".repeat(6)));
}

#[test]
fn too_many_attachments_are_dropped_with_a_note() {
    let big = "y".repeat(MAX_ITEM_CHARS);
    let referenced: HashMap<_, _> = (0..6)
        .map(|index| (format!("k{index}"), plain_text(&big)))
        .collect();
    let text = attachments(None, &referenced);
    assert!(text.contains("More attachments were left out"));
}

#[test]
fn rules_with_no_root_path_are_the_users_own() {
    let mut context = context();
    context.project_rules = vec![ic::ProjectRules {
        active_rule_files: vec![api::FileContent {
            file_path: "Tabs".to_string(),
            content: "Use tabs.".to_string(),
            ..Default::default()
        }],
        ..Default::default()
    }];
    let text = environment(Some(&context));
    assert!(text.contains("# User rules"));
    assert!(text.contains("Use tabs."));
    assert!(!text.contains("# Project rules"));
}
