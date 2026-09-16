use ai::agent::action_result::StopRecordingResult;
use computer_use::RecordingHandle;
use instant::Instant;
use warpui::App;

use super::super::recording_controller::ActiveRecording;
use super::*;
use crate::test_util::terminal::initialize_app_for_terminal_view;

/// Conversation cancellation must not upload the recording: it kills ffmpeg (by
/// dropping the handle) and resolves as `Cancelled`.
#[test]
fn cancellation_finalization_skips_upload_even_without_actions() {
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);

        let (handle, _exit_state) = RecordingHandle::new_test(1, 1);
        let recording = ActiveRecording {
            id: "recording".to_string(),
            conversation_id: AIConversationId::new(),
            handle,
            started_at: Instant::now(),
            target: computer_use::Target::Screen,
            pointer_session: computer_use::PointerSession::new(),
            actions: Vec::new(),
            pending_group: None,
        };

        let result = finalize_recording(recording, FinalizeReason::RunCancelled, false);

        assert_eq!(result, StopRecordingResult::Cancelled);
    });
}

/// An agent-initiated stop with `should_persist=false` discards the recording:
/// it kills ffmpeg (by dropping the handle) and resolves as `Discarded`, so the
/// agent's turn continues.
#[test]
fn agent_discard_finalization_skips_upload() {
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);

        let (handle, _exit_state) = RecordingHandle::new_test(1, 1);
        let recording = ActiveRecording {
            id: "recording".to_string(),
            conversation_id: AIConversationId::new(),
            handle,
            started_at: Instant::now(),
            target: computer_use::Target::Screen,
            pointer_session: computer_use::PointerSession::new(),
            actions: Vec::new(),
            pending_group: None,
        };

        let result = finalize_recording(recording, FinalizeReason::StoppedByAgent, false);

        assert_eq!(result, StopRecordingResult::Discarded);
    });
}

/// A recording with no committed meaningful action group is an explicit
/// finalization error: the handle is dropped (which kill-on-drops ffmpeg and
/// removes the partial capture). This returns before `recorder.stop`, so it
/// needs no live recorder.
#[test]
fn empty_actions_finalization_is_an_error_without_upload() {
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);

        let (handle, _exit_state) = RecordingHandle::new_test(1, 1);
        let recording = ActiveRecording {
            id: "recording".to_string(),
            conversation_id: AIConversationId::new(),
            handle,
            started_at: Instant::now(),
            target: computer_use::Target::Screen,
            pointer_session: computer_use::PointerSession::new(),
            actions: Vec::new(),
            pending_group: None,
        };

        let result = finalize_recording(recording, FinalizeReason::StoppedByAgent, true);

        assert!(
            matches!(result, StopRecordingResult::Error(ref msg) if msg.contains("no committed actions")),
            "empty actions should finalize as an error, got {result:?}"
        );
    });
}
