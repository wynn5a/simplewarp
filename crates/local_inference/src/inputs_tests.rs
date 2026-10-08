use warp_multi_agent_api as api;

use super::*;

#[test]
fn summarize_adds_the_users_focus_only_when_given() {
    assert_eq!(summarize_conversation("  "), SUMMARIZE_CONVERSATION);
    let text = summarize_conversation("the migration");
    assert!(text.starts_with(SUMMARIZE_CONVERSATION));
    assert!(text.ends_with("the migration"));
}

#[test]
fn the_summarize_prompt_tells_the_model_not_to_use_tools() {
    // A tool call has no place in a summary, and the reply is stored as one.
    assert!(SUMMARIZE_CONVERSATION.contains("Do not call any tool"));
}

#[test]
fn a_skill_is_followed_by_the_users_query() {
    let skill = api::Skill {
        descriptor: Some(api::SkillDescriptor {
            name: "deploy".to_string(),
            ..Default::default()
        }),
        content: Some(api::FileContent {
            content: "Run ./deploy.sh".to_string(),
            ..Default::default()
        }),
    };
    let text = invoke_skill(Some(&skill), "to staging");
    assert!(text.contains("`deploy`"));
    assert!(text.contains("Run ./deploy.sh"));
    assert!(text.ends_with("to staging"));
    assert_eq!(invoke_skill(None, "just this"), "just this");
}

#[test]
fn review_comments_name_the_file_and_the_lines() {
    let comments = vec![
        api::ReviewComment {
            comment: "Rename this.".to_string(),
            comment_target: Some(api::review_comment::CommentTarget::CommentedLine(
                api::DiffHunk {
                    file_path: "src/a.rs".to_string(),
                    line_range: Some(api::FileContentLineRange { start: 4, end: 6 }),
                    diff_content: "+let x = 1;\n".to_string(),
                    ..Default::default()
                },
            )),
            ..Default::default()
        },
        api::ReviewComment {
            comment: "Too big.".to_string(),
            comment_target: Some(api::review_comment::CommentTarget::CommentedDiffset(
                Default::default(),
            )),
            ..Default::default()
        },
    ];
    let text = code_review(&comments, None);
    assert!(text.contains("1. In `src/a.rs`, lines 4-6"));
    assert!(text.contains("+let x = 1;"));
    assert!(text.contains("Rename this."));
    assert!(text.contains("2. On the changes as a whole"));
}

#[test]
fn the_summary_turn_carries_the_summary() {
    assert!(summary_turn("did X").contains("did X"));
}
