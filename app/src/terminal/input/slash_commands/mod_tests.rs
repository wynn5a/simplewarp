use super::slash_command_is_submitted_as_prompt;
use crate::search::slash_command_menu::static_commands::{SlashCommandKind, commands};

/// The centralized classifier must mark only the prompt-submitting commands (/compact, /plan,
/// /orchestrate) as "submitted as a prompt". Every other slash command emits an immediate action
/// and must be treated as "run now" by the prompt-queue gate.
#[test]
fn slash_command_is_submitted_as_prompt_only_for_prompt_commands() {
    assert!(slash_command_is_submitted_as_prompt(&commands::COMPACT));
    assert!(slash_command_is_submitted_as_prompt(&commands::PLAN));
    assert!(slash_command_is_submitted_as_prompt(&commands::ORCHESTRATE));

    for command in [
        &*commands::FORK,
        &*commands::FORK_AND_COMPACT,
        &commands::FORK_FROM,
        &*commands::COMPACT_AND,
        &*commands::MODEL,
        &commands::REWIND,
        &commands::CONVERSATIONS,
        &*commands::QUEUE,
    ] {
        assert!(!slash_command_is_submitted_as_prompt(command));
    }
}

#[test]
fn commands_have_typed_identities_and_explicit_surface_support() {
    for (command, expected) in [
        (&*commands::AGENT, SlashCommandKind::Agent),
        (&*commands::NEW, SlashCommandKind::New),
        (&*commands::COMPACT, SlashCommandKind::Compact),
        (&commands::COST, SlashCommandKind::Cost),
        (&*commands::PLAN, SlashCommandKind::Plan),
        (&*commands::MODEL, SlashCommandKind::Model),
        (
            &*commands::CREATE_NEW_PROJECT,
            SlashCommandKind::CreateNewProject,
        ),
        (
            &commands::EXPORT_TO_CLIPBOARD,
            SlashCommandKind::ExportToClipboard,
        ),
        (&*commands::EXPORT_TO_FILE, SlashCommandKind::ExportToFile),
    ] {
        assert_eq!(
            command.kind, expected,
            "{} should have its typed command identity",
            command.name
        );
        assert!(command.supports_surface(settings::SettingsMode::Gui));
    }

    let command = &*commands::ORCHESTRATE;
    assert_eq!(command.kind, SlashCommandKind::Orchestrate);
    assert!(command.supports_surface(settings::SettingsMode::Gui));
}

#[test]
fn model_command_is_not_a_prompt_command() {
    assert_eq!(commands::MODEL.kind, SlashCommandKind::Model);
    assert!(!slash_command_is_submitted_as_prompt(&commands::MODEL));
    assert!(commands::MODEL.argument.is_none());
}

#[cfg(windows)]
mod windows {
    use std::sync::Arc;

    use super::super::*;
    use crate::terminal::ShellLaunchData;
    use crate::terminal::model::session::SessionInfo;
    use crate::terminal::model::session::command_executor::testing::TestCommandExecutor;
    use crate::terminal::shell::ShellType;

    fn wsl_session() -> Session {
        Session::new(
            SessionInfo::new_for_test().with_shell_type(ShellType::Bash),
            Arc::new(TestCommandExecutor::default()),
        )
        .with_shell_launch_data(ShellLaunchData::WSL {
            distro: "Ubuntu".to_owned(),
        })
    }

    #[test]
    fn open_file_command_converts_wsl_paths_to_host_paths() {
        let session = wsl_session();
        let cases = [
            (
                "/home/ubuntu",
                "subdir/test.txt",
                r"\\WSL$\Ubuntu\home\ubuntu\subdir\test.txt",
                None,
            ),
            (
                "/home/ubuntu/project",
                "../test.txt",
                r"\\WSL$\Ubuntu\home\ubuntu\test.txt",
                None,
            ),
            (
                "/home/ubuntu",
                "subdir/file\\ name.txt",
                r"\\WSL$\Ubuntu\home\ubuntu\subdir\file name.txt",
                None,
            ),
            (
                "/home/ubuntu",
                "subdir/test.txt:4:2",
                r"\\WSL$\Ubuntu\home\ubuntu\subdir\test.txt",
                Some(LineAndColumnArg {
                    line_num: 4,
                    column_num: Some(2),
                }),
            ),
        ];

        for (current_dir, raw_arg, expected_path, expected_line_col) in cases {
            let (path, line_col) = open_file_command_path(&session, current_dir, raw_arg);

            assert_eq!(path, PathBuf::from(expected_path));
            assert_eq!(line_col, expected_line_col);
        }
    }
}
