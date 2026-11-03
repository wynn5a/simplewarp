use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use chrono::Local;
use settings::Setting;
use warp_multi_agent_api::response_event;
use warpui::{App, SingletonEntity};

use crate::ai::agent::conversation::AIConversationId;
use crate::ai::agent::task::TaskId;
use crate::ai::agent::{
    AIAgentAttachment, AIAgentContext, AIAgentInput, CancellationReason, ImageContext,
    UserQueryMode,
};
use crate::ai::blocklist::queued_query::{QueuedQuery, QueuedQueryModel, QueuedQueryOrigin};
use crate::ai::blocklist::{
    BlocklistAIHistoryEvent, BlocklistAIHistoryModel, PendingAttachment, PendingFile, RequestInput,
    ResponseStream, ResponseStreamId,
};
use crate::ai::llms::LLMId;
use crate::test_util::terminal::{add_window_with_terminal, initialize_app_for_terminal_view};

fn image_attachment(file_name: &str) -> PendingAttachment {
    PendingAttachment::Image(ImageContext {
        data: String::new(),
        mime_type: "image/png".to_owned(),
        file_name: file_name.to_owned(),
        is_figma: false,
    })
}

fn file_attachment(file_name: &str) -> PendingAttachment {
    PendingAttachment::File(PendingFile {
        file_name: file_name.to_owned(),
        file_path: file_name.into(),
        mime_type: "text/plain".to_owned(),
    })
}

#[test]
fn input_for_query_converts_prompt_attachments_and_ignores_live_staging() {
    // `input_for_query` builds its image/file context purely from the explicitly-provided
    // attachment set (resolved by `send_query` from either the queued row or live staging),
    // never from the context model's pending attachments.
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);
        let terminal = add_window_with_terminal(&mut app, None);

        terminal.update(&mut app, |terminal, ctx| {
            let conversation_id =
                BlocklistAIHistoryModel::handle(ctx).update(ctx, |history_model, ctx| {
                    history_model.start_new_conversation(terminal.id(), false, false, ctx)
                });

            let controller = terminal.ai_controller();
            let context_model = controller.as_ref(ctx).context_model.clone();
            let active_session = controller.as_ref(ctx).active_session.clone();

            // Stage *live* attachments that must NOT leak into a query built from a different,
            // explicitly-provided attachment set.
            context_model.update(ctx, |m, ctx| {
                m.append_pending_attachments(
                    vec![image_attachment("live.png"), file_attachment("live.txt")],
                    ctx,
                );
            });

            let task_id = TaskId::new("test-task".to_owned());
            // Two files sharing a basename to exercise duplicate-basename suffixing.
            let prompt_attachments = vec![
                image_attachment("queued.png"),
                file_attachment("notes.txt"),
                file_attachment("notes.txt"),
            ];

            let input = super::input_for_query(
                "build a query".to_owned(),
                &task_id,
                conversation_id,
                UserQueryMode::Normal,
                None,
                HashMap::new(),
                prompt_attachments,
                context_model.as_ref(ctx),
                active_session.as_ref(ctx),
                ctx,
            );

            let AIAgentInput::UserQuery {
                context,
                referenced_attachments,
                ..
            } = input
            else {
                panic!("expected UserQuery");
            };

            // The provided image is attached as image context; the live-staged image is not.
            let image_names: Vec<&str> = context
                .iter()
                .filter_map(|c| match c {
                    AIAgentContext::Image(img) => Some(img.file_name.as_str()),
                    _ => None,
                })
                .collect();
            assert_eq!(image_names, vec!["queued.png"]);

            // The provided files are attached as FilePathReference with duplicate-basename
            // suffixing; the live-staged file is not.
            let mut file_names: Vec<String> = referenced_attachments
                .values()
                .filter_map(|a| match a {
                    AIAgentAttachment::FilePathReference { file_name, .. } => {
                        Some(file_name.clone())
                    }
                    _ => None,
                })
                .collect();
            file_names.sort();
            assert_eq!(
                file_names,
                vec!["notes.txt".to_owned(), "notes.txt".to_owned()]
            );
            assert!(referenced_attachments.contains_key("notes.txt"));
            assert!(referenced_attachments.contains_key("notes.txt (1)"));
            assert!(!referenced_attachments.contains_key("live.txt"));
        });
    });
}

#[test]
fn cancelling_conversation_aborts_pending_auto_resume() {
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);
        let terminal = add_window_with_terminal(&mut app, None);

        // An ID with no backing conversation: if the scheduled wait ever
        // completes, the resume is a harmless no-op.
        let conversation_id = AIConversationId::new();

        terminal.update(&mut app, |terminal, ctx| {
            terminal.ai_controller().update(ctx, |controller, ctx| {
                controller.schedule_auto_resume_after_error(conversation_id, ctx);
                assert!(
                    controller
                        .pending_auto_resume_handles
                        .contains_key(&conversation_id)
                );

                controller.cancel_conversation_progress(
                    conversation_id,
                    CancellationReason::ManuallyCancelled,
                    ctx,
                );
                assert!(
                    !controller
                        .pending_auto_resume_handles
                        .contains_key(&conversation_id)
                );
            });
        });
    });
}

#[test]
fn mock_response_stream_updates_history_through_controller() {
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);
        let terminal = add_window_with_terminal(&mut app, None);
        let captured_events = Arc::new(Mutex::new(Vec::new()));
        let events_for_subscription = Arc::clone(&captured_events);
        app.update(|ctx| {
            ctx.subscribe_to_model(&BlocklistAIHistoryModel::handle(ctx), move |_, event, _| {
                events_for_subscription.lock().unwrap().push(event.clone())
            });
        });

        let (conversation_id, stream) = terminal.update(&mut app, |view, ctx| {
            let terminal_view_id = view.id();
            let stream_id = ResponseStreamId::new_for_test();
            let conversation_id =
                BlocklistAIHistoryModel::handle(ctx).update(ctx, |history, ctx| {
                    let conversation_id =
                        history.start_new_conversation(terminal_view_id, false, false, ctx);
                    let task_id = history
                        .conversation(&conversation_id)
                        .unwrap()
                        .get_root_task_id()
                        .clone();
                    history
                        .update_conversation_for_new_request_input(
                            RequestInput {
                                conversation_id,
                                input_messages: HashMap::from([(task_id, vec![])]),
                                working_directory: None,
                                model_id: LLMId::from("test-model"),
                                coding_model_id: LLMId::from("test-coding-model"),
                                cli_agent_model_id: LLMId::from("test-cli-agent-model"),
                                request_start_ts: Local::now(),
                                supported_tools_override: None,
                            },
                            stream_id.clone(),
                            terminal_view_id,
                            ctx,
                        )
                        .unwrap();
                    conversation_id
                });
            let stream = ctx.add_model(|_| ResponseStream::new_for_test(stream_id.clone()));
            view.ai_controller().update(ctx, |controller, ctx| {
                controller.register_mock_stream_for_test(
                    stream_id,
                    conversation_id,
                    stream.clone(),
                    ctx,
                );
            });
            (conversation_id, stream)
        });

        stream.update(&mut app, |stream, ctx| {
            stream.emit_response_event_for_test(
                warp_multi_agent_api::ResponseEvent {
                    r#type: Some(response_event::Type::Init(response_event::StreamInit {
                        request_id: "test-request".to_string(),
                        conversation_id: "test-server-conversation".to_string(),
                        run_id: String::new(),
                    })),
                },
                ctx,
            );
            stream.emit_response_event_for_test(
                warp_multi_agent_api::ResponseEvent {
                    r#type: Some(response_event::Type::Finished(
                        response_event::StreamFinished {
                            reason: Some(response_event::stream_finished::Reason::Done(
                                response_event::stream_finished::Done {},
                            )),
                            token_usage: vec![],
                            should_refresh_model_config: false,
                            request_cost: None,
                            ..Default::default()
                        },
                    )),
                },
                ctx,
            );
        });

        BlocklistAIHistoryModel::handle(&app).read(&app, |history, _| {
            assert_eq!(
                history.conversation(&conversation_id).map(|c| c.status()),
                Some(&crate::ai::agent::conversation::ConversationStatus::Success)
            );
        });
        let events = captured_events.lock().unwrap();
        assert!(events.iter().any(|event| matches!(
            event,
            BlocklistAIHistoryEvent::ConversationServerTokenAssigned {
                conversation_id: id,
                ..
            } if *id == conversation_id
        )));
        assert!(events.iter().any(|event| matches!(
            event,
            BlocklistAIHistoryEvent::UpdatedStreamingExchange {
                conversation_id: id,
                ..
            } if *id == conversation_id
        )));
    });
}

/// When an agent command exits the shell, the conversation must be finalized as
/// `Error` (not `Cancelled`), and a subsequent `ManuallyCancelled` (as fired by
/// the pane-close path) must not overwrite that failure.
#[test]
fn fail_conversation_due_to_shell_exit_reports_error_and_survives_manual_cancel() {
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);
        let terminal = add_window_with_terminal(&mut app, None);

        let conversation_id = terminal.update(&mut app, |view, ctx| {
            let terminal_view_id = view.id();
            let stream_id = ResponseStreamId::new_for_test();
            let conversation_id =
                BlocklistAIHistoryModel::handle(ctx).update(ctx, |history, ctx| {
                    let conversation_id =
                        history.start_new_conversation(terminal_view_id, false, false, ctx);
                    let task_id = history
                        .conversation(&conversation_id)
                        .unwrap()
                        .get_root_task_id()
                        .clone();
                    history
                        .update_conversation_for_new_request_input(
                            RequestInput {
                                conversation_id,
                                input_messages: HashMap::from([(task_id, vec![])]),
                                working_directory: None,
                                model_id: LLMId::from("test-model"),
                                coding_model_id: LLMId::from("test-coding-model"),
                                cli_agent_model_id: LLMId::from("test-cli-agent-model"),
                                request_start_ts: Local::now(),
                                supported_tools_override: None,
                            },
                            stream_id.clone(),
                            terminal_view_id,
                            ctx,
                        )
                        .unwrap();
                    conversation_id
                });
            let stream = ctx.add_model(|_| ResponseStream::new_for_test(stream_id.clone()));
            view.ai_controller().update(ctx, |controller, ctx| {
                controller.register_mock_stream_for_test(stream_id, conversation_id, stream, ctx);
                controller.fail_conversation_due_to_shell_exit(
                    conversation_id,
                    "exit 1".to_string(),
                    ctx,
                );
            });
            conversation_id
        });

        // The in-flight request is finalized as Error (with the shell-exit error
        // on its exchange), not Cancelled.
        BlocklistAIHistoryModel::handle(&app).read(&app, |history, _| {
            assert_eq!(
                history.conversation(&conversation_id).map(|c| c.status()),
                Some(&crate::ai::agent::conversation::ConversationStatus::Error)
            );
        });

        // The pane-close cancellation path must be a no-op now that the
        // conversation is terminal.
        terminal.update(&mut app, |view, ctx| {
            view.ai_controller().update(ctx, |controller, ctx| {
                controller.cancel_conversation_progress(
                    conversation_id,
                    CancellationReason::ManuallyCancelled,
                    ctx,
                );
            });
        });
        BlocklistAIHistoryModel::handle(&app).read(&app, |history, _| {
            assert_eq!(
                history.conversation(&conversation_id).map(|c| c.status()),
                Some(&crate::ai::agent::conversation::ConversationStatus::Error)
            );
        });
    });
}

/// An optimistic long-running-command completion that cancels an in-flight
/// stream must finalize the conversation as `Success`, not `Cancelled`. This is
/// a regression test for the reason -> status mapping living in a single place
/// (`CancellationReason::conversation_outcome`).
#[test]
fn optimistic_cli_subagent_completion_with_in_flight_stream_reports_success() {
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);
        let terminal = add_window_with_terminal(&mut app, None);

        let conversation_id = terminal.update(&mut app, |view, ctx| {
            let terminal_view_id = view.id();
            let stream_id = ResponseStreamId::new_for_test();
            let conversation_id =
                BlocklistAIHistoryModel::handle(ctx).update(ctx, |history, ctx| {
                    let conversation_id =
                        history.start_new_conversation(terminal_view_id, false, false, ctx);
                    let task_id = history
                        .conversation(&conversation_id)
                        .unwrap()
                        .get_root_task_id()
                        .clone();
                    history
                        .update_conversation_for_new_request_input(
                            RequestInput {
                                conversation_id,
                                input_messages: HashMap::from([(task_id, vec![])]),
                                working_directory: None,
                                model_id: LLMId::from("test-model"),
                                coding_model_id: LLMId::from("test-coding-model"),
                                cli_agent_model_id: LLMId::from("test-cli-agent-model"),
                                request_start_ts: Local::now(),
                                supported_tools_override: None,
                            },
                            stream_id.clone(),
                            terminal_view_id,
                            ctx,
                        )
                        .unwrap();
                    conversation_id
                });
            let stream = ctx.add_model(|_| ResponseStream::new_for_test(stream_id.clone()));
            view.ai_controller().update(ctx, |controller, ctx| {
                controller.register_mock_stream_for_test(stream_id, conversation_id, stream, ctx);
                // The long-running command finished while the agent was still
                // streaming, cancelling the in-flight stream optimistically.
                controller.cancel_conversation_progress(
                    conversation_id,
                    CancellationReason::CommandFinishedDuringInlineAgentView,
                    ctx,
                );
            });
            conversation_id
        });

        BlocklistAIHistoryModel::handle(&app).read(&app, |history, _| {
            assert_eq!(
                history.conversation(&conversation_id).map(|c| c.status()),
                Some(&crate::ai::agent::conversation::ConversationStatus::Success)
            );
        });
    });
}

/// How the controller marks the given stream's conversation, and whether it owes a compaction,
/// after the stream finishes for exceeding the context window.
fn finish_with_overflow(
    auto_compaction: bool,
    prepare: fn(&mut super::BlocklistAIController, ResponseStreamId),
) -> (crate::ai::agent::conversation::ConversationStatus, bool) {
    let result = Arc::new(Mutex::new(None));
    let result_ref = Arc::clone(&result);
    App::test((), move |mut app| async move {
        initialize_app_for_terminal_view(&mut app);
        app.update(|ctx| {
            crate::settings::AISettings::handle(ctx).update(ctx, |settings, ctx| {
                settings
                    .auto_compaction_enabled
                    .set_value(auto_compaction, ctx)
                    .unwrap();
            });
        });
        let terminal = add_window_with_terminal(&mut app, None);

        let (conversation_id, stream_id, stream) = terminal.update(&mut app, |view, ctx| {
            let terminal_view_id = view.id();
            let stream_id = ResponseStreamId::new_for_test();
            let conversation_id =
                BlocklistAIHistoryModel::handle(ctx).update(ctx, |history, ctx| {
                    let conversation_id =
                        history.start_new_conversation(terminal_view_id, false, false, ctx);
                    let task_id = history
                        .conversation(&conversation_id)
                        .unwrap()
                        .get_root_task_id()
                        .clone();
                    history
                        .update_conversation_for_new_request_input(
                            RequestInput {
                                conversation_id,
                                input_messages: HashMap::from([(task_id, vec![])]),
                                working_directory: None,
                                model_id: LLMId::from("test-model"),
                                coding_model_id: LLMId::from("test-coding-model"),
                                cli_agent_model_id: LLMId::from("test-cli-agent-model"),
                                request_start_ts: Local::now(),
                                supported_tools_override: None,
                            },
                            stream_id.clone(),
                            terminal_view_id,
                            ctx,
                        )
                        .unwrap();
                    conversation_id
                });
            let stream = ctx.add_model(|_| ResponseStream::new_for_test(stream_id.clone()));
            view.ai_controller().update(ctx, |controller, ctx| {
                controller.register_mock_stream_for_test(
                    stream_id.clone(),
                    conversation_id,
                    stream.clone(),
                    ctx,
                );
                prepare(controller, stream_id.clone());
            });
            (conversation_id, stream_id, stream)
        });

        stream.update(&mut app, |stream, ctx| {
            stream.emit_response_event_for_test(
                warp_multi_agent_api::ResponseEvent {
                    r#type: Some(response_event::Type::Init(response_event::StreamInit {
                        request_id: "test-request".to_string(),
                        conversation_id: "test-server-conversation".to_string(),
                        run_id: String::new(),
                    })),
                },
                ctx,
            );
            stream.emit_response_event_for_test(
                warp_multi_agent_api::ResponseEvent {
                    r#type: Some(response_event::Type::Finished(
                        response_event::StreamFinished {
                            reason: Some(
                                response_event::stream_finished::Reason::ContextWindowExceeded(
                                    Default::default(),
                                ),
                            ),
                            ..Default::default()
                        },
                    )),
                },
                ctx,
            );
        });

        let status = BlocklistAIHistoryModel::handle(&app).read(&app, |history, _| {
            history
                .conversation(&conversation_id)
                .map(|conversation| conversation.status().clone())
                .unwrap()
        });
        let owes_compaction = terminal.read(&app, |view, ctx| {
            view.ai_controller()
                .as_ref(ctx)
                .owes_compaction_for_test(&stream_id)
        });
        *result_ref.lock().unwrap() = Some((status, owes_compaction));
    });
    let result = result.lock().unwrap().take();
    result.unwrap()
}

#[test]
fn an_overflow_compacts_once_and_resumes() {
    let (status, owes_compaction) = finish_with_overflow(true, |_, _| {});
    assert_eq!(
        status,
        crate::ai::agent::conversation::ConversationStatus::TransientError
    );
    assert!(owes_compaction);
}

#[test]
fn an_overflow_after_a_compaction_is_an_error() {
    let (status, owes_compaction) = finish_with_overflow(true, |controller, stream_id| {
        controller.mark_overflow_retry_for_test(stream_id);
    });
    assert_eq!(
        status,
        crate::ai::agent::conversation::ConversationStatus::Error
    );
    assert!(!owes_compaction);
}

#[test]
fn an_overflowed_summary_is_not_compacted_again() {
    let (status, owes_compaction) = finish_with_overflow(true, |controller, stream_id| {
        controller.mark_compaction_for_test(stream_id);
    });
    assert_eq!(
        status,
        crate::ai::agent::conversation::ConversationStatus::Error
    );
    assert!(!owes_compaction);
}

#[test]
fn auto_compaction_off_reports_the_overflow() {
    let (status, owes_compaction) = finish_with_overflow(false, |_, _| {});
    assert_eq!(
        status,
        crate::ai::agent::conversation::ConversationStatus::Error
    );
    assert!(!owes_compaction);
}

fn held_request_input(conversation_id: AIConversationId, task_id: TaskId) -> RequestInput {
    RequestInput {
        conversation_id,
        input_messages: HashMap::from([(
            task_id,
            vec![AIAgentInput::UserQuery {
                query: "fix the build".to_string(),
                context: Arc::from(vec![AIAgentContext::Image(ImageContext {
                    data: String::new(),
                    mime_type: "image/png".to_owned(),
                    file_name: "shot.png".to_owned(),
                    is_figma: false,
                })]),
                referenced_attachments: HashMap::from([(
                    "notes.md".to_string(),
                    AIAgentAttachment::FilePathReference {
                        file_id: "id".to_string(),
                        file_name: "notes.md".to_string(),
                        file_path: "/tmp/notes.md".to_string(),
                    },
                )]),
                user_query_mode: UserQueryMode::Normal,
                running_command: None,
                intended_agent: None,
            }],
        )]),
        working_directory: None,
        model_id: LLMId::from("test-model"),
        coding_model_id: LLMId::from("test-coding-model"),
        cli_agent_model_id: LLMId::from("test-cli-agent-model"),
        request_start_ts: Local::now(),
        supported_tools_override: None,
    }
}

#[test]
fn every_held_request_and_resume_has_one_next_step() {
    use super::compaction::{Continuation, DeferredRequest, Step, StreamOutcome, next_step};

    let held = || {
        Continuation::Send(Box::new(DeferredRequest::for_test(held_request_input(
            AIConversationId::new(),
            TaskId::new("task".to_string()),
        ))))
    };
    assert!(matches!(
        next_step(held(), StreamOutcome::Succeeded),
        Step::Send(_)
    ));
    assert!(matches!(
        next_step(held(), StreamOutcome::Failed),
        Step::Send(_)
    ));
    assert!(matches!(
        next_step(held(), StreamOutcome::Cancelled),
        Step::Requeue(_)
    ));
    assert!(matches!(
        next_step(Continuation::Resume, StreamOutcome::Succeeded),
        Step::Resume
    ));
    for outcome in [StreamOutcome::Failed, StreamOutcome::Cancelled] {
        assert!(matches!(
            next_step(Continuation::Resume, outcome),
            Step::Nothing
        ));
    }
    for outcome in [
        StreamOutcome::Succeeded,
        StreamOutcome::Failed,
        StreamOutcome::Cancelled,
    ] {
        assert!(matches!(
            next_step(Continuation::Done, outcome),
            Step::Nothing
        ));
    }
}

#[test]
fn a_cancelled_compaction_queues_the_held_question_instead_of_sending_it() {
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);
        let terminal = add_window_with_terminal(&mut app, None);

        terminal.update(&mut app, |view, ctx| {
            let conversation_id = BlocklistAIHistoryModel::handle(ctx)
                .update(ctx, |history, ctx| {
                    history.start_new_conversation(view.id(), false, false, ctx)
                });
            let task_id = BlocklistAIHistoryModel::as_ref(ctx)
                .conversation(&conversation_id)
                .unwrap()
                .get_root_task_id()
                .clone();
            QueuedQueryModel::handle(ctx).update(ctx, |model, ctx| {
                model.append(
                    conversation_id,
                    QueuedQuery::new(
                        "queued later".to_string(),
                        QueuedQueryOrigin::QueueSlashCommand,
                    ),
                    ctx,
                );
            });
            let stream_id = ResponseStreamId::new_for_test();

            view.ai_controller().update(ctx, |controller, ctx| {
                controller.hold_for_compaction_for_test(
                    stream_id.clone(),
                    held_request_input(conversation_id, task_id),
                );
                controller.continue_after_stream(
                    &stream_id,
                    conversation_id,
                    super::compaction::StreamOutcome::Cancelled,
                    ctx,
                );
                assert!(!controller.is_tracking_compaction_for_test(&stream_id));
                assert!(!controller.has_active_stream_for_conversation(conversation_id, ctx));
            });

            let queue = QueuedQueryModel::as_ref(ctx).queue(conversation_id);
            assert_eq!(queue.len(), 2);
            assert_eq!(queue[0].text(), "fix the build");
            assert_eq!(queue[0].origin(), QueuedQueryOrigin::HeldForCompaction);
            assert_eq!(queue[1].text(), "queued later");
            let attachments = queue[0].attachments();
            assert_eq!(attachments.len(), 2);
            assert!(attachments.iter().any(|attachment| matches!(
                attachment,
                PendingAttachment::Image(image) if image.file_name == "shot.png"
            )));
            assert!(attachments.iter().any(|attachment| matches!(
                attachment,
                PendingAttachment::File(file) if file.file_name == "notes.md"
                    && file.file_path == std::path::Path::new("/tmp/notes.md")
            )));
        });
    });
}

#[test]
fn a_failed_overflow_compaction_records_the_failure_and_does_not_resume() {
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);
        let terminal = add_window_with_terminal(&mut app, None);

        terminal.update(&mut app, |view, ctx| {
            let conversation_id =
                BlocklistAIHistoryModel::handle(ctx).update(ctx, |history, ctx| {
                    let id = history.start_new_conversation(view.id(), false, false, ctx);
                    let conversation = history.conversation_mut(&id).unwrap();
                    conversation.record_context_usage(
                        &warp_multi_agent_api::response_event::stream_finished::ConversationUsageMetadata {
                            total_input_tokens: 190_000,
                            ..Default::default()
                        },
                    );
                    id
                });
            let stream_id = ResponseStreamId::new_for_test();

            view.ai_controller().update(ctx, |controller, ctx| {
                controller.mark_resume_after_compaction_for_test(stream_id.clone());
                controller.continue_after_stream(
                    &stream_id,
                    conversation_id,
                    super::compaction::StreamOutcome::Failed,
                    ctx,
                );
                assert!(!controller.is_tracking_compaction_for_test(&stream_id));
                assert!(!controller.has_active_stream_for_conversation(conversation_id, ctx));
            });

            let conversation = BlocklistAIHistoryModel::as_ref(ctx)
                .conversation(&conversation_id)
                .unwrap();
            assert_eq!(conversation.last_compacted_to(), Some(190_000));
        });
    });
}

fn action_result_input(task_id: TaskId) -> AIAgentInput {
    use ai::agent::action_result::{AnyFileContent, FileContext, ReadFilesResult};

    use crate::ai::agent::{AIAgentActionResult, AIAgentActionResultType};

    AIAgentInput::ActionResult {
        result: AIAgentActionResult {
            id: "call-1".to_string().into(),
            task_id,
            result: AIAgentActionResultType::ReadFiles(ReadFilesResult::Success {
                files: vec![FileContext::new(
                    "lib.rs".to_string(),
                    AnyFileContent::StringContent("fn main() {}".to_string()),
                    None,
                    None,
                )],
                failed_files: Vec::new(),
            }),
        },
        context: Arc::new([]),
    }
}

fn results_request_input(conversation_id: AIConversationId, task_id: TaskId) -> RequestInput {
    let mut request_input = held_request_input(conversation_id, task_id.clone());
    request_input.input_messages =
        HashMap::from([(task_id.clone(), vec![action_result_input(task_id)])]);
    request_input
}

#[test]
fn a_steer_is_sent_with_the_tool_results() {
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);
        let terminal = add_window_with_terminal(&mut app, None);

        terminal.update(&mut app, |view, ctx| {
            let conversation_id = BlocklistAIHistoryModel::handle(ctx)
                .update(ctx, |history, ctx| {
                    history.start_new_conversation(view.id(), false, false, ctx)
                });
            let task_id = BlocklistAIHistoryModel::as_ref(ctx)
                .conversation(&conversation_id)
                .unwrap()
                .get_root_task_id()
                .clone();
            QueuedQueryModel::handle(ctx).update(ctx, |model, ctx| {
                model.add_steer(
                    conversation_id,
                    "use snake_case".to_string(),
                    vec![image_attachment("shot.png")],
                    ctx,
                );
                model.append(
                    conversation_id,
                    QueuedQuery::new(
                        "after the task".to_string(),
                        QueuedQueryOrigin::QueueSlashCommand,
                    ),
                    ctx,
                );
            });
            let mut request_input = results_request_input(conversation_id, task_id.clone());

            let steers = view.ai_controller().update(ctx, |controller, ctx| {
                controller.add_steers(&mut request_input, Arc::new([]), ctx)
            });

            assert_eq!(steers.len(), 1);
            let inputs = &request_input.input_messages[&task_id];
            assert!(matches!(inputs[0], AIAgentInput::ActionResult { .. }));
            let AIAgentInput::UserQuery { query, context, .. } = &inputs[1] else {
                panic!("expected the steer after the results: {inputs:?}");
            };
            assert_eq!(query, "use snake_case");
            assert!(context.iter().any(|context| matches!(
                context,
                AIAgentContext::Image(image) if image.file_name == "shot.png"
            )));
            let queue = QueuedQueryModel::as_ref(ctx).queue(conversation_id);
            assert_eq!(queue.len(), 1);
            assert_eq!(queue[0].text(), "after the task");
        });
    });
}

#[test]
fn a_steer_waits_when_results_belong_to_a_subagent() {
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);
        let terminal = add_window_with_terminal(&mut app, None);

        terminal.update(&mut app, |view, ctx| {
            let conversation_id = BlocklistAIHistoryModel::handle(ctx)
                .update(ctx, |history, ctx| {
                    history.start_new_conversation(view.id(), false, false, ctx)
                });
            QueuedQueryModel::handle(ctx).update(ctx, |model, ctx| {
                model.add_steer(conversation_id, "use snake_case".to_string(), vec![], ctx);
            });
            let subagent_task = TaskId::new("subagent".to_string());
            let mut request_input = results_request_input(conversation_id, subagent_task.clone());

            let steers = view.ai_controller().update(ctx, |controller, ctx| {
                controller.add_steers(&mut request_input, Arc::new([]), ctx)
            });

            assert!(steers.is_empty());
            assert_eq!(request_input.input_messages[&subagent_task].len(), 1);
            assert_eq!(
                QueuedQueryModel::as_ref(ctx).queue(conversation_id).len(),
                1
            );
        });
    });
}

#[test]
fn a_cancelled_compaction_requeues_a_held_steer() {
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);
        let terminal = add_window_with_terminal(&mut app, None);

        terminal.update(&mut app, |view, ctx| {
            let conversation_id = BlocklistAIHistoryModel::handle(ctx)
                .update(ctx, |history, ctx| {
                    history.start_new_conversation(view.id(), false, false, ctx)
                });
            let task_id = BlocklistAIHistoryModel::as_ref(ctx)
                .conversation(&conversation_id)
                .unwrap()
                .get_root_task_id()
                .clone();
            QueuedQueryModel::handle(ctx).update(ctx, |model, ctx| {
                model.add_steer(conversation_id, "use snake_case".to_string(), vec![], ctx);
            });
            let mut request_input = results_request_input(conversation_id, task_id);
            let stream_id = ResponseStreamId::new_for_test();

            view.ai_controller().update(ctx, |controller, ctx| {
                let steers = controller.add_steers(&mut request_input, Arc::new([]), ctx);
                assert_eq!(steers.len(), 1);
                controller.hold_for_compaction_for_test(stream_id.clone(), request_input);
                controller.continue_after_stream(
                    &stream_id,
                    conversation_id,
                    super::compaction::StreamOutcome::Cancelled,
                    ctx,
                );
            });

            let queue = QueuedQueryModel::as_ref(ctx).queue(conversation_id);
            assert_eq!(queue.len(), 1);
            assert_eq!(queue[0].text(), "use snake_case");
            assert_eq!(queue[0].origin(), QueuedQueryOrigin::HeldForCompaction);
        });
    });
}

/// Starts a request on a new conversation through a mock stream, returning the conversation, its
/// root task, the stream's id and the stream.
fn start_mock_request(
    app: &mut App,
    terminal: &warpui::ViewHandle<crate::terminal::TerminalView>,
) -> (
    AIConversationId,
    TaskId,
    ResponseStreamId,
    warpui::ModelHandle<ResponseStream>,
) {
    terminal.update(app, |view, ctx| {
        let terminal_view_id = view.id();
        let stream_id = ResponseStreamId::new_for_test();
        let (conversation_id, task_id) =
            BlocklistAIHistoryModel::handle(ctx).update(ctx, |history, ctx| {
                let conversation_id =
                    history.start_new_conversation(terminal_view_id, false, false, ctx);
                // A later turn: the root task already exists, so the reply adds to it.
                let conversation = history.conversation_mut(&conversation_id).unwrap();
                conversation.upgrade_optimistic_root_to_server_task_for_test(
                    warp_multi_agent_api::Task {
                        id: "root".to_string(),
                        ..Default::default()
                    },
                );
                let task_id = conversation.get_root_task_id().clone();
                let mut request_input = held_request_input(conversation_id, task_id.clone());
                request_input.input_messages = HashMap::from([(task_id.clone(), vec![])]);
                history
                    .update_conversation_for_new_request_input(
                        request_input,
                        stream_id.clone(),
                        terminal_view_id,
                        ctx,
                    )
                    .unwrap();
                (conversation_id, task_id)
            });
        let stream = ctx.add_model(|_| ResponseStream::new_for_test(stream_id.clone()));
        view.ai_controller().update(ctx, |controller, ctx| {
            controller.register_mock_stream_for_test(
                stream_id.clone(),
                conversation_id,
                stream.clone(),
                ctx,
            );
        });
        (conversation_id, task_id, stream_id, stream)
    })
}

/// The events `local_inference` sends before the provider replies: `Init`, then a transaction
/// that stores the question.
fn emit_opening(app: &mut App, stream: &warpui::ModelHandle<ResponseStream>, task_id: &TaskId) {
    use warp_multi_agent_api::client_action::Action;

    let task_id = task_id.to_string();
    stream.update(app, |stream, ctx| {
        stream.emit_response_event_for_test(
            warp_multi_agent_api::ResponseEvent {
                r#type: Some(response_event::Type::Init(response_event::StreamInit {
                    request_id: "test-request".to_string(),
                    conversation_id: "test-server-conversation".to_string(),
                    run_id: String::new(),
                })),
            },
            ctx,
        );
        let actions = [
            Action::BeginTransaction(Default::default()),
            Action::AddMessagesToTask(warp_multi_agent_api::client_action::AddMessagesToTask {
                task_id: task_id.clone(),
                messages: vec![warp_multi_agent_api::Message {
                    id: "question".to_string(),
                    task_id,
                    message: Some(warp_multi_agent_api::message::Message::UserQuery(
                        warp_multi_agent_api::message::UserQuery {
                            query: "fix the build".to_string(),
                            ..Default::default()
                        },
                    )),
                    ..Default::default()
                }],
            }),
        ];
        stream.emit_response_event_for_test(
            warp_multi_agent_api::ResponseEvent {
                r#type: Some(response_event::Type::ClientActions(
                    response_event::ClientActions {
                        actions: actions
                            .into_iter()
                            .map(|action| warp_multi_agent_api::ClientAction {
                                action: Some(action),
                            })
                            .collect(),
                    },
                )),
            },
            ctx,
        );
    });
}

fn stored_questions(app: &App, conversation_id: AIConversationId, task_id: &TaskId) -> usize {
    BlocklistAIHistoryModel::handle(app).read(app, |history, _| {
        history
            .conversation(&conversation_id)
            .and_then(|conversation| conversation.get_task(task_id))
            .map_or(0, |task| {
                task.messages()
                    .filter(|message| {
                        matches!(
                            message.message,
                            Some(warp_multi_agent_api::message::Message::UserQuery(_))
                        )
                    })
                    .count()
            })
    })
}

/// A request stopped while the provider is still thinking keeps the question it sent.
#[test]
fn a_request_cancelled_before_the_reply_keeps_its_question() {
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);
        let terminal = add_window_with_terminal(&mut app, None);
        let (conversation_id, task_id, stream_id, stream) = start_mock_request(&mut app, &terminal);

        emit_opening(&mut app, &stream, &task_id);
        terminal.update(&mut app, |view, ctx| {
            view.ai_controller().update(ctx, |controller, ctx| {
                controller.cancel_request(&stream_id, CancellationReason::ManuallyCancelled, ctx);
            });
        });

        assert_eq!(stored_questions(&app, conversation_id, &task_id), 1);
    });
}

/// A failure the client retries rolls back what the attempt stored, so the retry stores the
/// question once.
#[test]
fn a_retried_failure_rolls_back_the_stored_question() {
    use warp_multi_agent_api::client_action::Action;

    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);
        let terminal = add_window_with_terminal(&mut app, None);
        let (conversation_id, task_id, _, stream) = start_mock_request(&mut app, &terminal);

        emit_opening(&mut app, &stream, &task_id);
        assert_eq!(stored_questions(&app, conversation_id, &task_id), 1);
        stream.update(&mut app, |stream, ctx| {
            stream.emit_response_event_for_test(
                warp_multi_agent_api::ResponseEvent {
                    r#type: Some(response_event::Type::ClientActions(
                        response_event::ClientActions {
                            actions: vec![warp_multi_agent_api::ClientAction {
                                action: Some(Action::RollbackTransaction(Default::default())),
                            }],
                        },
                    )),
                },
                ctx,
            );
        });
        assert_eq!(stored_questions(&app, conversation_id, &task_id), 0);

        // The retried attempt opens again and stores the question once.
        emit_opening(&mut app, &stream, &task_id);
        assert_eq!(stored_questions(&app, conversation_id, &task_id), 1);
    });
}

fn emit_event(
    app: &mut App,
    stream: &warpui::ModelHandle<ResponseStream>,
    r#type: response_event::Type,
) {
    stream.update(app, |stream, ctx| {
        stream.emit_response_event_for_test(
            warp_multi_agent_api::ResponseEvent {
                r#type: Some(r#type),
            },
            ctx,
        );
    });
}

fn actions_event(
    actions: Vec<warp_multi_agent_api::client_action::Action>,
) -> response_event::Type {
    response_event::Type::ClientActions(response_event::ClientActions {
        actions: actions
            .into_iter()
            .map(|action| warp_multi_agent_api::ClientAction {
                action: Some(action),
            })
            .collect(),
    })
}

/// The first request of a new conversation fails before its reply and is retried: the rollback
/// keeps the root task it created (outside the transaction), so the retry, which carries the
/// conversation's tasks as they are now, adds to it.
#[test]
fn a_new_conversation_survives_a_retried_first_request() {
    use warp_multi_agent_api::client_action::{Action, AddMessagesToTask, CreateTask};

    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);
        let terminal = add_window_with_terminal(&mut app, None);
        let (conversation_id, stream) = terminal.update(&mut app, |view, ctx| {
            let terminal_view_id = view.id();
            let stream_id = ResponseStreamId::new_for_test();
            let conversation_id =
                BlocklistAIHistoryModel::handle(ctx).update(ctx, |history, ctx| {
                    let conversation_id =
                        history.start_new_conversation(terminal_view_id, false, false, ctx);
                    let task_id = history
                        .conversation(&conversation_id)
                        .unwrap()
                        .get_root_task_id()
                        .clone();
                    history
                        .update_conversation_for_new_request_input(
                            held_request_input(conversation_id, task_id),
                            stream_id.clone(),
                            terminal_view_id,
                            ctx,
                        )
                        .unwrap();
                    conversation_id
                });
            let stream = ctx.add_model(|_| ResponseStream::new_for_test(stream_id.clone()));
            view.ai_controller().update(ctx, |controller, ctx| {
                controller.register_mock_stream_for_test(
                    stream_id,
                    conversation_id,
                    stream.clone(),
                    ctx,
                );
            });
            (conversation_id, stream)
        });
        let init = || {
            response_event::Type::Init(response_event::StreamInit {
                request_id: "test-request".to_string(),
                conversation_id: "test-server-conversation".to_string(),
                run_id: String::new(),
            })
        };
        let store_question = || {
            actions_event(vec![
                Action::BeginTransaction(Default::default()),
                Action::AddMessagesToTask(AddMessagesToTask {
                    task_id: "root".to_string(),
                    messages: vec![warp_multi_agent_api::Message {
                        id: uuid::Uuid::new_v4().to_string(),
                        task_id: "root".to_string(),
                        message: Some(warp_multi_agent_api::message::Message::UserQuery(
                            warp_multi_agent_api::message::UserQuery {
                                query: "fix the build".to_string(),
                                ..Default::default()
                            },
                        )),
                        ..Default::default()
                    }],
                }),
            ])
        };

        // The first attempt creates the root, stores the question and is rolled back.
        emit_event(&mut app, &stream, init());
        emit_event(
            &mut app,
            &stream,
            actions_event(vec![Action::CreateTask(CreateTask {
                task: Some(warp_multi_agent_api::Task {
                    id: "root".to_string(),
                    ..Default::default()
                }),
            })]),
        );
        emit_event(&mut app, &stream, store_question());
        emit_event(
            &mut app,
            &stream,
            actions_event(vec![Action::RollbackTransaction(Default::default())]),
        );

        let root = TaskId::new("root".to_string());
        BlocklistAIHistoryModel::handle(&app).read(&app, |history, _| {
            let conversation = history.conversation(&conversation_id).unwrap();
            assert_eq!(conversation.get_root_task_id(), &root);
            let tasks = conversation.compute_active_tasks();
            assert_eq!(tasks.len(), 1, "the retry carries the created root");
            assert_eq!(tasks[0].id, "root");
            assert_eq!(conversation.exchange_count(), 1);
        });
        assert_eq!(stored_questions(&app, conversation_id, &root), 0);

        // The retry carries that task, so it opens without creating one and finishes.
        emit_event(&mut app, &stream, init());
        emit_event(&mut app, &stream, store_question());
        emit_event(
            &mut app,
            &stream,
            actions_event(vec![Action::CommitTransaction(Default::default())]),
        );
        emit_event(
            &mut app,
            &stream,
            response_event::Type::Finished(response_event::StreamFinished {
                reason: Some(response_event::stream_finished::Reason::Done(
                    response_event::stream_finished::Done {},
                )),
                ..Default::default()
            }),
        );

        assert_eq!(stored_questions(&app, conversation_id, &root), 1);
        BlocklistAIHistoryModel::handle(&app).read(&app, |history, _| {
            let conversation = history.conversation(&conversation_id).unwrap();
            assert_eq!(conversation.all_tasks().count(), 1);
            assert_eq!(conversation.exchange_count(), 1, "the block is intact");
            assert_eq!(
                conversation.status(),
                &crate::ai::agent::conversation::ConversationStatus::Success
            );
        });
    });
}
