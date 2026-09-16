use super::*;

#[test]
fn test_parse_github_pr_url() {
    assert_eq!(
        parse_github_pr_url("https://github.com/owner/repo/pull/123"),
        Some(("repo".to_string(), 123))
    );
    assert_eq!(
        parse_github_pr_url("https://github.com/my-org/my-repo/pull/456"),
        Some(("my-repo".to_string(), 456))
    );
    assert_eq!(
        parse_github_pr_url("https://github.com/my-org/my-repo"),
        None
    );
    assert_eq!(parse_github_pr_url("not a url"), None);
}

#[test]
fn file_button_label_prefers_filename() {
    assert_eq!(
        file_button_label("report.txt", "outputs/other.txt"),
        "report.txt"
    );
}

#[test]
fn file_button_label_falls_back_to_filepath_basename() {
    assert_eq!(file_button_label("", "outputs/report.txt"), "report.txt");
}

#[test]
fn file_button_label_falls_back_to_generic_label() {
    assert_eq!(file_button_label("", ""), "File");
}

#[test]
fn converts_graphql_file_artifact() {
    let artifact = Artifact::try_from(warp_graphql::ai::AIConversationArtifact::FileArtifact(
        warp_graphql::ai::FileArtifact {
            artifact_uid: "artifact-file-1".into(),
            filepath: "outputs/report.txt".to_string(),
            mime_type: "text/plain".to_string(),
            description: Some("Daily summary".to_string()),
            size_bytes: Some(42),
        },
    ))
    .expect("expected file artifact conversion");

    assert_eq!(
        artifact,
        Artifact::File {
            artifact_uid: "artifact-file-1".to_string(),
            filepath: "outputs/report.txt".to_string(),
            filename: "report.txt".to_string(),
            mime_type: "text/plain".to_string(),
            description: Some("Daily summary".to_string()),
            size_bytes: Some(42),
        }
    );
}
