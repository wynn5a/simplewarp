use super::*;

#[test]
fn the_prompt_names_every_tool_rule_the_client_depends_on() {
    // The client asks the user to approve a command that is not marked read-only, so the model
    // must be told what the mark decides.
    assert!(SYSTEM_PROMPT.contains("is_read_only"));
    // The client renders agent output as Markdown.
    assert!(SYSTEM_PROMPT.contains("Markdown"));
    // An exact-match search is the one way a diff can fail silently.
    assert!(SYSTEM_PROMPT.contains("apply_file_diffs"));
}
