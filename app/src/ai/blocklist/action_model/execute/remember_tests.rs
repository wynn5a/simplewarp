use super::*;

#[test]
fn content_is_trimmed() {
    assert_eq!(validate_content("  Use tabs.\n", true), Ok("Use tabs."));
}

#[test]
fn empty_or_long_content_is_rejected() {
    assert_eq!(validate_content(" \n ", true), Err("The memory is empty."));

    let longest = "x".repeat(MAX_MEMORY_CHARS);
    assert_eq!(validate_content(&longest, true), Ok(longest.as_str()));

    let too_long = "é".repeat(MAX_MEMORY_CHARS + 1);
    assert!(validate_content(&too_long, true).is_err());
}

#[test]
fn nothing_is_saved_while_memory_is_off() {
    assert_eq!(
        validate_content("Use tabs.", false),
        Err("Memory is turned off.")
    );
}

#[test]
fn a_remote_session_cannot_save_a_project_memory() {
    assert_eq!(
        remote_session_refusal(Some(&SessionType::WarpifiedRemote)),
        Some(REMOTE_SESSION_MESSAGE)
    );
    assert_eq!(remote_session_refusal(Some(&SessionType::Local)), None);
    assert_eq!(remote_session_refusal(None), None);
}
