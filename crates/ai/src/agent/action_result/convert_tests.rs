use super::*;

#[test]
fn read_files_partial_success_converts_failed_files() {
    let result =
        api::request::input::tool_call_result::Result::try_from(ReadFilesResult::Success {
            files: vec![FileContext::new(
                "/tmp/success.txt".to_string(),
                AnyFileContent::StringContent("hello".to_string()),
                None,
                None,
            )],
            failed_files: vec![ReadFilesFailedFile {
                path: "/tmp/missing.txt".to_string(),
                message: "File not found or could not be read".to_string(),
            }],
        })
        .expect("read_files success should convert");

    let api::request::input::tool_call_result::Result::ReadFiles(result) = result else {
        panic!("expected read_files result");
    };

    let Some(api::read_files_result::Result::AnyFilesSuccess(success)) = result.result else {
        panic!("expected any files success result");
    };

    assert_eq!(success.files.len(), 1);
    assert_eq!(success.failed_reads.len(), 1);
    assert_eq!(success.failed_reads[0].path, "/tmp/missing.txt");
    assert_eq!(
        success.failed_reads[0].message,
        "File not found or could not be read"
    );
}

#[test]
fn a_command_cancelled_before_execution_still_reports_a_result() {
    // The wire has no "cancelled" variant for a shell command, but the model must hear that the
    // call did not run; dropping the result made the agent loop invent one that said it did.
    let result = api::request::input::tool_call_result::Result::try_from(
        RequestCommandOutputResult::CancelledBeforeExecution,
    )
    .expect("a cancelled command converts");

    let api::request::input::tool_call_result::Result::RunShellCommand(shell) = result else {
        panic!("expected a shell command result");
    };

    assert!(matches!(
        shell.result,
        Some(api::run_shell_command_result::Result::PermissionDenied(_))
    ));
}

#[test]
fn remember_results_convert_to_the_proto() {
    use api::remember_result::{Error, Result as Remember, Success};

    let project = api::request::input::tool_call_result::Result::try_from(RememberResult::Saved {
        project_root: Some("/work/app".to_string()),
        already_saved: true,
    })
    .expect("a saved memory converts");
    assert_eq!(
        project,
        api::request::input::tool_call_result::Result::Remember(api::RememberResult {
            result: Some(Remember::Success(Success {
                project_root: "/work/app".to_string(),
                already_saved: true,
            })),
        })
    );

    let global = api::request::input::tool_call_result::Result::try_from(RememberResult::Saved {
        project_root: None,
        already_saved: false,
    })
    .expect("a saved memory converts");
    let api::request::input::tool_call_result::Result::Remember(global) = global else {
        panic!("expected a remember result");
    };
    assert_eq!(global.result, Some(Remember::Success(Success::default())));

    let error = api::request::input::tool_call_result::Result::try_from(RememberResult::Error(
        "The memory is empty.".to_string(),
    ))
    .expect("an error converts");
    let api::request::input::tool_call_result::Result::Remember(error) = error else {
        panic!("expected a remember result");
    };
    assert_eq!(
        error.result,
        Some(Remember::Error(Error {
            message: "The memory is empty.".to_string(),
        }))
    );

    assert!(matches!(
        api::request::input::tool_call_result::Result::try_from(RememberResult::Cancelled),
        Err(ConvertToAPITypeError::Ignore)
    ));
}

#[test]
fn a_remember_tool_call_becomes_a_remember_action() {
    use crate::agent::action::{AIAgentActionType, MemoryScope};

    let action = AIAgentActionType::from(api::message::tool_call::Remember {
        content: "Use cargo nextest.".to_string(),
        scope: api::message::tool_call::remember::Scope::Project as i32,
    });

    assert_eq!(
        action,
        AIAgentActionType::Remember {
            content: "Use cargo nextest.".to_string(),
            scope: MemoryScope::Project,
        }
    );
    assert!(!action.to_string().contains("nextest"));
}
