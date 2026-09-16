use super::{
    AgentMessageHeader, AgentRunEvent, Artifact, ArtifactDownloadResponse,
    ForkConversationResponse, ReadAgentMessageResponse, RunFollowupRequest, SpawnAgentRequest,
    UserQueryMode,
};
use crate::notebooks::NotebookId;

#[test]
fn spawn_agent_request_serializes_agent_uid_as_agent_identity_uid() {
    let request = SpawnAgentRequest {
        prompt: Some("hello".to_string()),
        mode: UserQueryMode::Normal,
        config: None,
        title: None,
        team: None,
        agent_identity_uid: Some("agent_123".to_string()),
        skill: None,
        attachments: vec![],
        interactive: None,
        parent_run_id: None,
        runtime_skills: vec![],
        referenced_attachments: vec![],
        conversation_id: None,
        snapshot_disabled: None,
        orchestration_handoff: None,
    };

    let value = serde_json::to_value(&request).unwrap();

    assert_eq!(
        value.get("agent_identity_uid").and_then(|v| v.as_str()),
        Some("agent_123")
    );
    assert!(value.get("agent_uid").is_none());
}

#[test]
fn spawn_agent_request_omits_prompt_when_none() {
    let request = SpawnAgentRequest {
        prompt: None,
        mode: UserQueryMode::Normal,
        config: None,
        title: None,
        team: None,
        agent_identity_uid: None,
        skill: None,
        attachments: vec![],
        interactive: None,
        parent_run_id: None,
        runtime_skills: vec![],
        referenced_attachments: vec![],
        conversation_id: None,
        snapshot_disabled: None,
        orchestration_handoff: None,
    };

    let value = serde_json::to_value(&request).unwrap();

    assert!(value.get("prompt").is_none());
}

#[test]
fn test_deserialize_file_artifact_download_response() {
    let json = r#"{
        "artifact_uid": "artifact-123",
        "artifact_type": "FILE",
        "created_at": "2024-01-15T10:30:00Z",
        "data": {
            "download_url": "https://storage.example.com/report.txt",
            "expires_at": "2024-01-15T11:30:00Z",
            "content_type": "text/plain",
            "filepath": "outputs/report.txt",
            "filename": "report.txt",
            "description": "daily summary",
            "size_bytes": 42
        }
    }"#;

    let artifact: ArtifactDownloadResponse = serde_json::from_str(json).unwrap();

    let ArtifactDownloadResponse::File { common, data } = artifact else {
        panic!("expected File artifact download response");
    };
    assert_eq!(common.artifact_uid, "artifact-123");
    assert_eq!(common.created_at.to_rfc3339(), "2024-01-15T10:30:00+00:00");
    assert_eq!(data.download_url, "https://storage.example.com/report.txt");
    assert_eq!(data.expires_at.to_rfc3339(), "2024-01-15T11:30:00+00:00");
    assert_eq!(data.content_type, "text/plain");
    assert_eq!(data.filepath, "outputs/report.txt");
    assert_eq!(data.filename, "report.txt");
    assert_eq!(data.description.as_deref(), Some("daily summary"));
    assert_eq!(data.size_bytes, Some(42));
}

#[test]
fn test_deserialize_screenshot_artifact_download_response() {
    let json = r#"{
        "artifact_uid": "screenshot-123",
        "artifact_type": "SCREENSHOT",
        "created_at": "2024-01-15T10:30:00Z",
        "data": {
            "download_url": "https://storage.example.com/screenshot.png",
            "expires_at": "2024-01-15T11:30:00Z",
            "content_type": "image/png",
            "description": "dashboard screenshot"
        }
    }"#;

    let artifact: ArtifactDownloadResponse = serde_json::from_str(json).unwrap();

    let ArtifactDownloadResponse::Screenshot { common, data } = artifact else {
        panic!("expected Screenshot artifact download response");
    };
    assert_eq!(common.artifact_uid, "screenshot-123");
    assert_eq!(common.created_at.to_rfc3339(), "2024-01-15T10:30:00+00:00");
    assert_eq!(
        data.download_url,
        "https://storage.example.com/screenshot.png"
    );
    assert_eq!(data.expires_at.to_rfc3339(), "2024-01-15T11:30:00+00:00");
    assert_eq!(data.content_type, "image/png");
    assert_eq!(data.description.as_deref(), Some("dashboard screenshot"));
}

#[test]
fn test_deserialize_plan_artifact() {
    let json = r#"{
        "created_at": "2024-01-15T10:30:00Z",
        "artifact_type": "PLAN",
        "data": {
            "document_uid": "doc-uid-123",
            "notebook_uid": "1234567890123456789012",
            "title": "My Plan"
        }
    }"#;

    let artifact: Artifact = serde_json::from_str(json).unwrap();

    let Artifact::Plan {
        document_uid,
        notebook_uid,
        title,
    } = &artifact
    else {
        panic!("expected Plan artifact");
    };
    assert_eq!(document_uid, "doc-uid-123");
    assert_eq!(
        notebook_uid.as_ref().map(|n| n.to_string()),
        Some("1234567890123456789012".to_string())
    );
    assert_eq!(*title, Some("My Plan".to_string()));
}

#[test]
fn test_deserialize_pull_request_artifact() {
    let json = r#"{
        "created_at": "2024-01-15T10:30:00Z",
        "artifact_type": "PULL_REQUEST",
        "data": {
            "url": "https://github.com/org/repo/pull/42",
            "branch": "feature-branch"
        }
    }"#;

    let artifact: Artifact = serde_json::from_str(json).unwrap();

    let Artifact::PullRequest {
        url,
        branch,
        repo,
        number,
    } = &artifact
    else {
        panic!("expected PullRequest artifact");
    };
    assert_eq!(url, "https://github.com/org/repo/pull/42");
    assert_eq!(branch, "feature-branch");
    assert_eq!(*repo, Some("repo".to_string()));
    assert_eq!(*number, Some(42));
}

#[test]
fn test_deserialize_pull_request_non_github_url() {
    let json = r#"{
        "created_at": "2024-01-15T10:30:00Z",
        "artifact_type": "PULL_REQUEST",
        "data": {
            "url": "https://gitlab.com/org/repo/merge_requests/42",
            "branch": "feature-branch"
        }
    }"#;

    let artifact: Artifact = serde_json::from_str(json).unwrap();

    let Artifact::PullRequest { repo, number, .. } = &artifact else {
        panic!("expected PullRequest artifact");
    };
    assert_eq!(*repo, None);
    assert_eq!(*number, None);
}

#[test]
fn test_deserialize_plan_artifact_with_optional_fields_missing() {
    let json = r#"{
        "created_at": "2024-01-15T10:30:00Z",
        "artifact_type": "PLAN",
        "data": {
            "document_uid": "doc-uid-123",
            "notebook_uid": "abcdefghijklmnopqrstuv"
        }
    }"#;

    let artifact: Artifact = serde_json::from_str(json).unwrap();

    let Artifact::Plan {
        document_uid,
        notebook_uid,
        title,
    } = &artifact
    else {
        panic!("expected Plan artifact");
    };
    assert_eq!(document_uid, "doc-uid-123");
    assert_eq!(
        notebook_uid.as_ref().map(|n| n.to_string()),
        Some("abcdefghijklmnopqrstuv".to_string())
    );
    assert!(title.is_none());
}

#[test]
fn test_deserialize_artifact_missing_data_field() {
    let json = r#"{
        "created_at": "2024-01-15T10:30:00Z",
        "artifact_type": "PLAN"
    }"#;

    let result = serde_json::from_str::<Artifact>(json);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("missing field"));
}

#[test]
fn test_deserialize_artifact_invalid_plan_data() {
    // Missing required `document_uid` field should fail deserialization
    let json = r#"{
        "created_at": "2024-01-15T10:30:00Z",
        "artifact_type": "PLAN",
        "data": {
            "title": "Only title, no document_uid"
        }
    }"#;

    let result = serde_json::from_str::<Artifact>(json);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("missing field"));
}

#[test]
fn test_deserialize_artifact_invalid_pr_data() {
    let json = r#"{
        "created_at": "2024-01-15T10:30:00Z",
        "artifact_type": "PULL_REQUEST",
        "data": {
            "url": "https://github.com/org/repo/pull/1"
        }
    }"#;

    let result = serde_json::from_str::<Artifact>(json);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("missing field"));
}

#[test]
fn test_deserialize_artifact_unknown_variant() {
    let json = r#"{
        "created_at": "2024-01-15T10:30:00Z",
        "artifact_type": "UNKNOWN_TYPE",
        "data": {
            "some_field": "value"
        }
    }"#;

    let result = serde_json::from_str::<Artifact>(json);
    assert!(result.is_err());
    let error_msg = result.unwrap_err().to_string();
    assert!(error_msg.contains("unknown variant"));
}

// ---------------------------------------------------------------------------------------------------------------------
//  Tests for resilient task list deserialization (skipping malformed tasks while tolerating unknown states)
// ---------------------------------------------------------------------------------------------------------------------

// ---------------------------------------------------------------------------------------------------------------------
//  We test roundtripping serialize and deserialize since we use this for persisting artifacts for local conversations.
// ---------------------------------------------------------------------------------------------------------------------

#[test]
fn test_artifact_plan_serialize_deserialize_roundtrip() {
    let original = Artifact::Plan {
        document_uid: "doc-123".to_string(),
        notebook_uid: Some(NotebookId::from("notebook12345678901234".to_string())),
        title: Some("My Plan".to_string()),
    };

    let serialized = serde_json::to_string(&original).unwrap();
    let deserialized: Artifact = serde_json::from_str(&serialized).unwrap();

    assert_eq!(original, deserialized);
}

#[test]
fn test_deserialize_agent_message_headers() {
    let json = r#"[
        {
            "message_id": "message-1",
            "sender_run_id": "run-1",
            "subject": "Build finished",
            "sent_at": "2026-04-09T20:00:00Z",
            "delivered_at": "2026-04-09T20:01:00Z",
            "read_at": null
        }
    ]"#;

    let headers: Vec<AgentMessageHeader> = serde_json::from_str(json).unwrap();

    assert_eq!(headers.len(), 1);
    assert_eq!(headers[0].message_id, "message-1");
    assert_eq!(headers[0].sender_run_id, "run-1");
    assert_eq!(headers[0].subject, "Build finished");
    assert_eq!(headers[0].sent_at, "2026-04-09T20:00:00Z");
    assert_eq!(
        headers[0].delivered_at.as_deref(),
        Some("2026-04-09T20:01:00Z")
    );
    assert_eq!(headers[0].read_at, None);
}

#[test]
fn test_deserialize_read_agent_message_response_with_timestamps() {
    let json = r#"{
        "message_id": "message-1",
        "sender_run_id": "run-1",
        "subject": "Build finished",
        "body": "Everything passed.",
        "sent_at": "2026-04-09T20:00:00Z",
        "delivered_at": "2026-04-09T20:01:00Z",
        "read_at": "2026-04-09T20:02:00Z"
    }"#;

    let response: ReadAgentMessageResponse = serde_json::from_str(json).unwrap();

    assert_eq!(response.message_id, "message-1");
    assert_eq!(response.sender_run_id, "run-1");
    assert_eq!(response.subject, "Build finished");
    assert_eq!(response.body, "Everything passed.");
    assert_eq!(response.sent_at, "2026-04-09T20:00:00Z");
    assert_eq!(
        response.delivered_at.as_deref(),
        Some("2026-04-09T20:01:00Z")
    );
    assert_eq!(response.read_at.as_deref(), Some("2026-04-09T20:02:00Z"));
}

#[test]
fn test_deserialize_agent_run_events_with_optional_fields() {
    let json = r#"[
        {
            "event_type": "run_started",
            "run_id": "run-1",
            "ref_id": null,
            "execution_id": "exec-1",
            "occurred_at": "2026-04-09T20:00:00Z",
            "sequence": 7
        },
        {
            "event_type": "new_message",
            "run_id": "run-2",
            "ref_id": "message-9",
            "execution_id": null,
            "occurred_at": "2026-04-09T20:05:00Z",
            "sequence": 8
        }
    ]"#;

    let events: Vec<AgentRunEvent> = serde_json::from_str(json).unwrap();

    assert_eq!(events.len(), 2);
    assert_eq!(events[0].event_type, "run_started");
    assert_eq!(events[0].execution_id.as_deref(), Some("exec-1"));
    assert_eq!(events[0].ref_id, None);
    assert_eq!(events[0].sequence, 7);
    assert_eq!(events[1].event_type, "new_message");
    assert_eq!(events[1].ref_id.as_deref(), Some("message-9"));
    assert_eq!(events[1].execution_id, None);
    assert_eq!(events[1].sequence, 8);
}

#[test]
fn test_artifact_plan_serialize_deserialize_roundtrip_no_notebook_uid() {
    let original = Artifact::Plan {
        document_uid: "doc-123".to_string(),
        notebook_uid: None,
        title: Some("My Plan".to_string()),
    };

    let serialized = serde_json::to_string(&original).unwrap();
    let deserialized: Artifact = serde_json::from_str(&serialized).unwrap();

    assert_eq!(original, deserialized);
}

#[test]
fn test_artifact_pr_serialize_deserialize_roundtrip() {
    let original = Artifact::PullRequest {
        url: "https://github.com/org/repo/pull/42".to_string(),
        branch: "feature-branch".to_string(),
        repo: Some("repo".to_string()),
        number: Some(42),
    };

    let serialized = serde_json::to_string(&original).unwrap();
    let deserialized: Artifact = serde_json::from_str(&serialized).unwrap();

    // repo/number are re-derived from URL on deserialize, so should match
    assert_eq!(original, deserialized);
}

#[test]
fn test_artifact_file_serialize_deserialize_roundtrip() {
    let original = Artifact::File {
        artifact_uid: "artifact-file-1".to_string(),
        filepath: "outputs/report.txt".to_string(),
        filename: "report.txt".to_string(),
        mime_type: "text/plain".to_string(),
        description: Some("Daily summary".to_string()),
        size_bytes: Some(42),
    };

    let serialized = serde_json::to_string(&original).unwrap();
    let deserialized: Artifact = serde_json::from_str(&serialized).unwrap();

    assert_eq!(original, deserialized);
}

#[test]
fn test_artifact_vec_serialize_deserialize_roundtrip() {
    let original = vec![
        Artifact::Plan {
            document_uid: "doc-1".to_string(),
            notebook_uid: None,
            title: Some("Plan 1".to_string()),
        },
        Artifact::PullRequest {
            url: "https://github.com/org/repo/pull/1".to_string(),
            branch: "main".to_string(),
            repo: Some("repo".to_string()),
            number: Some(1),
        },
        Artifact::File {
            artifact_uid: "artifact-file-1".to_string(),
            filepath: "outputs/report.txt".to_string(),
            filename: "report.txt".to_string(),
            mime_type: "text/plain".to_string(),
            description: Some("Daily summary".to_string()),
            size_bytes: Some(42),
        },
    ];

    let serialized = serde_json::to_string(&original).unwrap();
    let deserialized: Vec<Artifact> = serde_json::from_str(&serialized).unwrap();

    assert_eq!(original, deserialized);
}

#[test]
fn serialize_run_followup_request() {
    let request = RunFollowupRequest {
        message: "continue from here".to_string(),
    };

    let json = serde_json::to_value(request).unwrap();

    assert_eq!(
        json,
        serde_json::json!({
            "message": "continue from here",
        })
    );
}

#[test]
fn deserialize_fork_conversation_response() {
    let response: ForkConversationResponse = serde_json::from_value(serde_json::json!({
        "forked_conversation_id": "abcdef01-2345-6789-abcd-ef0123456789",
    }))
    .unwrap();
    assert_eq!(
        response.forked_conversation_id,
        "abcdef01-2345-6789-abcd-ef0123456789"
    );
}
