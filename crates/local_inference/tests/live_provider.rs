//! End-to-end tests against a real provider endpoint.
//!
//! Every test here is `#[ignore]`d, so a normal `cargo test` stays offline. Give the endpoint in
//! the environment and run them on purpose:
//!
//! ```sh
//! export LOCAL_INFERENCE_BASE_URL=https://example.com/v1
//! export LOCAL_INFERENCE_API_KEY=sk-...
//! export LOCAL_INFERENCE_MODEL=some-model
//! cargo test -p local_inference --test live_provider -- --ignored --nocapture
//! ```
//!
//! `LOCAL_INFERENCE_SCHEMA` picks the protocol: `anthropic` for Anthropic Messages, anything
//! else (or unset) for OpenAI Chat Completions.
//!
//! These tests send real requests and cost real tokens. They assert on the shape of the reply,
//! never on its exact words, because a model is free to word an answer as it likes.

use futures::StreamExt;
use warp_multi_agent_api as api;
use warp_multi_agent_api::message::{tool_call, tool_call_result};
use warp_multi_agent_api::request::settings::custom_model_providers::{
    CustomEndpointSchema, CustomModel, CustomModelProvider,
};

/// The endpoint under test, read from the environment.
struct Endpoint {
    base_url: String,
    api_key: String,
    model: String,
    schema: CustomEndpointSchema,
}

/// Reads the endpoint, or returns `None` when the environment does not name one.
fn endpoint() -> Option<Endpoint> {
    let base_url = std::env::var("LOCAL_INFERENCE_BASE_URL").ok()?;
    let api_key = std::env::var("LOCAL_INFERENCE_API_KEY").ok()?;
    let model = std::env::var("LOCAL_INFERENCE_MODEL").ok()?;
    let schema = match std::env::var("LOCAL_INFERENCE_SCHEMA").as_deref() {
        Ok("anthropic") => CustomEndpointSchema::AnthropicMessages,
        _ => CustomEndpointSchema::OpenaiChatCompletions,
    };
    Some(Endpoint {
        base_url,
        api_key,
        model,
        schema,
    })
}

/// Announces a skip when the environment names no endpoint.
macro_rules! endpoint_or_skip {
    () => {
        match endpoint() {
            Some(endpoint) => endpoint,
            None => {
                eprintln!("skipped: set LOCAL_INFERENCE_BASE_URL, _API_KEY and _MODEL to run");
                return;
            }
        }
    };
}

fn message(inner: api::message::Message) -> api::Message {
    api::Message {
        id: uuid::Uuid::new_v4().to_string(),
        message: Some(inner),
        ..Default::default()
    }
}

fn user_message(text: &str) -> api::Message {
    message(api::message::Message::UserQuery(api::message::UserQuery {
        query: text.to_string(),
        ..Default::default()
    }))
}

fn agent_message(text: &str) -> api::Message {
    message(api::message::Message::AgentOutput(
        api::message::AgentOutput {
            text: text.to_string(),
        },
    ))
}

fn shell_call(id: &str, command: &str) -> api::Message {
    message(api::message::Message::ToolCall(api::message::ToolCall {
        tool_call_id: id.to_string(),
        tool: Some(tool_call::Tool::RunShellCommand(
            tool_call::RunShellCommand {
                command: command.to_string(),
                is_read_only: true,
                ..Default::default()
            },
        )),
    }))
}

fn shell_result(id: &str, output: &str) -> api::Message {
    message(api::message::Message::ToolCallResult(
        api::message::ToolCallResult {
            tool_call_id: id.to_string(),
            result: Some(tool_call_result::Result::RunShellCommand(
                api::RunShellCommandResult {
                    result: Some(api::run_shell_command_result::Result::CommandFinished(
                        api::ShellCommandFinished {
                            output: output.to_string(),
                            exit_code: 0,
                            ..Default::default()
                        },
                    )),
                    ..Default::default()
                },
            )),
            ..Default::default()
        },
    ))
}

/// Builds a request that routes to the endpoint under test.
fn request(endpoint: &Endpoint, messages: Vec<api::Message>) -> api::Request {
    api::Request {
        settings: Some(api::request::Settings {
            model_config: Some(api::request::settings::ModelConfig {
                base: endpoint.model.clone(),
                ..Default::default()
            }),
            custom_model_providers: Some(api::request::settings::CustomModelProviders {
                providers: vec![CustomModelProvider {
                    base_url: endpoint.base_url.clone(),
                    api_key: endpoint.api_key.clone(),
                    models: vec![CustomModel {
                        slug: endpoint.model.clone(),
                        config_key: endpoint.model.clone(),
                    }],
                    schema: endpoint.schema as i32,
                }],
            }),
            ..Default::default()
        }),
        task_context: Some(api::request::TaskContext {
            tasks: vec![api::Task {
                id: uuid::Uuid::new_v4().to_string(),
                messages,
                ..Default::default()
            }],
        }),
        ..Default::default()
    }
}

/// What one reply produced, gathered from the event stream.
#[derive(Default, Debug)]
struct Reply {
    saw_init: bool,
    began_transaction: bool,
    committed_transaction: bool,
    text: String,
    reasoning: String,
    /// Every tool call, as `(name, arguments summary)`.
    tool_calls: Vec<(String, String)>,
    finish: Option<String>,
}

/// Runs one request to the end and gathers what came back.
async fn run(request: &api::Request) -> Reply {
    let mut stream = local_inference::generate_local_output(request)
        .await
        .expect("the request should resolve to an endpoint");

    let mut reply = Reply::default();
    while let Some(event) = stream.next().await {
        let event = event.expect("the provider stream should not fail");
        match event.r#type.expect("every event carries a type") {
            api::response_event::Type::Init(_) => reply.saw_init = true,
            api::response_event::Type::ClientActions(actions) => {
                for action in actions.actions {
                    collect_action(&mut reply, action);
                }
            }
            api::response_event::Type::Finished(finished) => {
                reply.finish = Some(format!("{:?}", finished.reason));
            }
        }
    }
    reply
}

fn collect_action(reply: &mut Reply, action: api::ClientAction) {
    use api::client_action::Action;

    match action.action {
        Some(Action::BeginTransaction(_)) => reply.began_transaction = true,
        Some(Action::CommitTransaction(_)) => reply.committed_transaction = true,
        Some(Action::AddMessagesToTask(add)) => {
            for message in add.messages {
                collect_message(reply, message);
            }
        }
        Some(Action::AppendToMessageContent(append)) => {
            if let Some(message) = append.message {
                collect_message(reply, message);
            }
        }
        _ => {}
    }
}

fn collect_message(reply: &mut Reply, message: api::Message) {
    match message.message {
        Some(api::message::Message::AgentOutput(output)) => reply.text.push_str(&output.text),
        Some(api::message::Message::AgentReasoning(reasoning)) => {
            reply.reasoning.push_str(&reasoning.reasoning)
        }
        Some(api::message::Message::ToolCall(call)) => {
            let (name, summary) = match call.tool {
                Some(tool_call::Tool::RunShellCommand(shell)) => {
                    ("run_shell_command".to_string(), shell.command)
                }
                Some(tool_call::Tool::ReadFiles(read)) => {
                    ("read_files".to_string(), format!("{:?}", read.files))
                }
                Some(tool_call::Tool::Grep(grep)) => ("grep".to_string(), format!("{grep:?}")),
                Some(tool_call::Tool::FileGlobV2(glob)) => {
                    ("file_glob".to_string(), format!("{:?}", glob.patterns))
                }
                other => ("other".to_string(), format!("{other:?}")),
            };
            reply.tool_calls.push((name, summary));
        }
        _ => {}
    }
}

/// The base case: a question that needs no tool must come back as streamed text.
#[tokio::test]
#[ignore = "sends a real request to a provider"]
async fn a_plain_question_streams_text_back() {
    let endpoint = endpoint_or_skip!();
    let request = request(
        &endpoint,
        vec![user_message(
            "Answer in one short sentence, and call no tools: what does the `pwd` command print?",
        )],
    );

    let reply = run(&request).await;
    println!("text: {}", reply.text);
    println!("reasoning length: {}", reply.reasoning.len());
    println!("finish: {:?}", reply.finish);

    assert!(reply.saw_init, "the reply must open with Init");
    assert!(reply.began_transaction, "the reply must open a transaction");
    assert!(
        reply.committed_transaction,
        "the reply must commit its transaction"
    );
    assert!(!reply.text.is_empty(), "the reply carried no text");
    assert!(
        reply.tool_calls.is_empty(),
        "no tool was needed, but got {:?}",
        reply.tool_calls
    );
    assert_eq!(reply.finish.as_deref(), Some("Some(Done(Done))"));
}

/// The model must be able to call a tool, and the call must map onto the proto that the client
/// runs. This is the half of the loop that Warp's server used to own.
#[tokio::test]
#[ignore = "sends a real request to a provider"]
async fn a_question_about_the_machine_produces_a_tool_call() {
    let endpoint = endpoint_or_skip!();
    let request = request(
        &endpoint,
        vec![user_message(
            "How many files are in /tmp on this machine? Use the shell to find out.",
        )],
    );

    let reply = run(&request).await;
    println!("text: {}", reply.text);
    println!("tool calls: {:?}", reply.tool_calls);
    println!("finish: {:?}", reply.finish);

    assert!(
        !reply.tool_calls.is_empty(),
        "the model called no tool; text was {:?}",
        reply.text
    );
    let (name, command) = &reply.tool_calls[0];
    assert_eq!(name, "run_shell_command");
    assert!(!command.is_empty(), "the shell call carried no command");
    assert!(
        reply.committed_transaction,
        "the reply must commit its transaction"
    );
}

/// The other half of the loop: a tool result must go back to the model, and the model must
/// answer from it. This is what proves the round trip, not just one call.
#[tokio::test]
#[ignore = "sends a real request to a provider"]
async fn a_tool_result_comes_back_as_an_answer() {
    let endpoint = endpoint_or_skip!();
    let request = request(
        &endpoint,
        vec![
            user_message("How many lines does /etc/hosts have? Use the shell."),
            agent_message("I will count them."),
            shell_call("call-1", "wc -l < /etc/hosts"),
            shell_result("call-1", "     42\n"),
        ],
    );

    let reply = run(&request).await;
    println!("text: {}", reply.text);
    println!("tool calls: {:?}", reply.tool_calls);

    assert!(
        !reply.text.is_empty(),
        "the model gave no answer after the tool result"
    );
    assert!(
        reply.text.contains("42"),
        "the answer ignored the tool result: {:?}",
        reply.text
    );
}

fn reasoning_message(text: &str) -> api::Message {
    message(api::message::Message::AgentReasoning(
        api::message::AgentReasoning {
            reasoning: text.to_string(),
            ..Default::default()
        },
    ))
}

/// A second question in a conversation that already used a tool.
///
/// This is the exact history the client replays: `agent_tasks` rows hold the reasoning, the call,
/// and the agent text, and never a tool result. Before `pair_tool_calls`, the request went out
/// with two assistant messages in a row and the gateway answered 400 with "an assistant message
/// with 'tool_calls' must be followed by tool messages responding to each 'tool_call_id'".
#[tokio::test]
#[ignore = "sends a real request to a provider"]
async fn a_follow_up_question_after_a_tool_call_is_accepted() {
    let endpoint = endpoint_or_skip!();
    let request = request(
        &endpoint,
        vec![
            reasoning_message("The user wants a count of .rs files. I will use the shell."),
            shell_call("call-1", "find . -name '*.rs' -type f | wc -l"),
            agent_message("There are **4,053** `.rs` files in this directory."),
            user_message("Thanks. In one sentence, what kind of project is this?"),
        ],
    );

    let reply = run(&request).await;
    println!("text: {}", reply.text);
    println!("tool calls: {:?}", reply.tool_calls);

    // The point is that the provider accepts the conversation at all: `run` panics on the 400
    // this used to produce. Whether the model answers in prose or looks at a file first is its
    // own choice, so either counts as a reply.
    assert!(
        !reply.text.is_empty() || !reply.tool_calls.is_empty(),
        "the follow-up question produced nothing"
    );
    assert!(
        reply.committed_transaction,
        "the reply must commit its transaction"
    );
}

/// Proves the model is shown the earlier question, not just its own earlier answer.
///
/// The emitter now stores the question on the task, so a replayed history carries it. Before that
/// the model saw its own replies and tool calls with nothing that prompted them, and answered a
/// follow-up with half the conversation missing.
#[tokio::test]
#[ignore = "sends a real request to a provider"]
async fn the_model_can_see_the_earlier_question() {
    let endpoint = endpoint_or_skip!();
    let request = request(
        &endpoint,
        vec![
            user_message("How many .rs files are in this directory?"),
            shell_call("call-1", "find . -name '*.rs' -type f | wc -l"),
            agent_message("There are **4,053** `.rs` files in this directory."),
            user_message(
                "What did I ask you in my first message? Answer in one short sentence, and call \
                 no tools.",
            ),
        ],
    );

    let reply = run(&request).await;
    println!("text: {}", reply.text);

    let answer = reply.text.to_lowercase();
    assert!(
        answer.contains(".rs") || answer.contains("rust") || answer.contains("file"),
        "the model could not see the first question: {:?}",
        reply.text
    );
}
