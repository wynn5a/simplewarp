use warp_multi_agent_api as api;
use warp_multi_agent_api::client_action::Action;

use super::*;

fn request(will_retry: bool, input: api::request::input::Type) -> api::Request {
    api::Request {
        task_context: Some(api::request::TaskContext {
            tasks: vec![api::Task {
                id: "task-1".to_string(),
                ..Default::default()
            }],
        }),
        input: Some(api::request::Input {
            r#type: Some(input),
            ..Default::default()
        }),
        settings: Some(api::request::Settings {
            will_retry_failures_before_reply: will_retry,
            ..Default::default()
        }),
        ..Default::default()
    }
}

fn question(will_retry: bool) -> api::Request {
    use api::request::input::user_inputs::{UserInput, user_input};

    request(
        will_retry,
        api::request::input::Type::UserInputs(api::request::input::UserInputs {
            inputs: vec![UserInput {
                input: Some(user_input::Input::UserQuery(
                    api::request::input::UserQuery {
                        query: "hello".to_string(),
                        ..Default::default()
                    },
                )),
            }],
        }),
    )
}

fn reply_to(request: &api::Request) -> Reply {
    Reply::new(Emitter::new(request), Schema::AnthropicMessages, request)
}

fn status(status: u16) -> Error {
    Error::ProviderStatus {
        status,
        body: String::new(),
        retry_after: None,
    }
}

/// Everything the reply hands over, in order: `Ok` events, then the failure if there is one.
fn drain(reply: &mut Reply) -> (Vec<api::ResponseEvent>, Option<Failure>) {
    let mut events = Vec::new();
    while let Some(item) = reply.next() {
        match item {
            Ok(event) => events.push(event),
            Err(failure) => return (events, Some(failure)),
        }
    }
    (events, None)
}

fn is_init(event: &api::ResponseEvent) -> bool {
    matches!(event.r#type, Some(api::response_event::Type::Init(_)))
}

fn actions(event: &api::ResponseEvent) -> Vec<Action> {
    match event.r#type.as_ref() {
        Some(api::response_event::Type::ClientActions(actions)) => actions
            .actions
            .iter()
            .filter_map(|action| action.action.clone())
            .collect(),
        _ => Vec::new(),
    }
}

fn is_rollback(event: &api::ResponseEvent) -> bool {
    matches!(actions(event).as_slice(), [Action::RollbackTransaction(_)])
}

fn stores_the_question(event: &api::ResponseEvent) -> bool {
    actions(event).iter().any(|action| {
        matches!(action, Action::AddMessagesToTask(add) if add.messages.iter().any(|message| {
            matches!(message.message, Some(api::message::Message::UserQuery(_)))
        }))
    })
}

const USAGE: &str =
    r#"{"type":"message_start","message":{"usage":{"input_tokens":10,"output_tokens":1}}}"#;
const TEXT: &str =
    r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hi"}}"#;
const OVERLOADED: &str =
    r#"{"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}"#;

/// A request cancelled while the provider is still thinking keeps its question: the opening that
/// stores it goes out before the provider says anything.
#[test]
fn the_opening_is_sent_before_the_provider_replies() {
    let mut reply = reply_to(&question(true));
    let (events, failure) = drain(&mut reply);
    assert!(failure.is_none());
    assert!(is_init(&events[0]));
    assert!(stores_the_question(&events[1]));

    reply.on_message("message_start", USAGE);
    assert!(drain(&mut reply).0.is_empty(), "usage is not a reply yet");
    reply.on_message("content_block_delta", TEXT);
    assert_eq!(drain(&mut reply).0.len(), 1, "the text");
}

#[test]
fn a_summary_shows_as_in_progress_before_the_provider_replies() {
    let mut reply = reply_to(&request(
        false,
        api::request::input::Type::SummarizeConversation(Default::default()),
    ));
    let (events, _) = drain(&mut reply);
    assert!(
        events
            .iter()
            .any(|event| actions(event).iter().any(|action| matches!(
                action,
                Action::AddMessagesToTask(add) if add.messages.iter().any(|message| matches!(
                    message.message,
                    Some(api::message::Message::Summarization(_))
                ))
            )))
    );
}

#[test]
fn a_retryable_failure_before_the_reply_rolls_back_what_it_stored() {
    let mut reply = reply_to(&question(true));
    drain(&mut reply);
    reply.fail(status(503));

    let (events, failure) = drain(&mut reply);
    assert_eq!(events.len(), 1);
    assert!(is_rollback(&events[0]), "the retry stores the inputs again");
    let failure = failure.expect("the failure is handed over");
    assert!(failure.before_reply);
    assert!(failure.error.is_retryable());
}

#[test]
fn a_failure_the_client_will_not_retry_keeps_the_inputs() {
    for (will_retry, error) in [(false, status(503)), (true, status(400))] {
        let mut reply = reply_to(&question(will_retry));
        drain(&mut reply);
        reply.fail(error);

        let (events, failure) = drain(&mut reply);
        assert!(events.is_empty(), "no rollback");
        assert!(failure.expect("the failure follows").before_reply);
    }
}

#[test]
fn a_failed_summary_request_is_rolled_back() {
    for will_retry in [false, true] {
        let mut reply = reply_to(&request(
            will_retry,
            api::request::input::Type::SummarizeConversation(Default::default()),
        ));
        drain(&mut reply);
        reply.fail(status(400));

        let (events, failure) = drain(&mut reply);
        assert_eq!(events.len(), 1);
        assert!(is_rollback(&events[0]));
        assert!(failure.is_some());
    }
}

#[test]
fn a_failure_after_the_reply_started_is_not_before_the_reply() {
    let mut reply = reply_to(&question(true));
    drain(&mut reply);
    reply.on_message("content_block_delta", TEXT);
    reply.on_message("error", OVERLOADED);

    let (events, failure) = drain(&mut reply);
    assert_eq!(events.len(), 1, "the text, and no rollback");
    let failure = failure.expect("the failure follows the reply");
    assert!(!failure.before_reply);
    assert!(
        failure.error.is_retryable(),
        "still transient, but too late to retry"
    );
}

#[test]
fn an_overloaded_stream_before_the_reply_is_retried() {
    let mut reply = reply_to(&question(true));
    drain(&mut reply);
    reply.on_message("error", OVERLOADED);

    let (events, failure) = drain(&mut reply);
    assert!(is_rollback(&events[0]));
    assert!(failure.expect("a failure").before_reply);
}
