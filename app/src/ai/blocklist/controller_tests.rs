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
