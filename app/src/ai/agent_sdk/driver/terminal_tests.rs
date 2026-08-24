use regex::Regex;
use serial_test::serial;
use warpui::App;

use super::TerminalDriver;
use crate::ai::agent_sdk::driver::AgentDriverError;
use crate::terminal::model::secrets::set_user_and_enterprise_secret_regexes;
use crate::terminal::view::Event;
use crate::test_util::add_window_with_terminal;
use crate::test_util::terminal::initialize_app_for_terminal_view;

// #[serial] because the secret regexes configured below are global state.
#[test]
#[serial]
fn shell_exit_fails_in_flight_and_subsequent_commands() {
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);

        // Configure a secret pattern (a GitHub classic PAT). In production
        // these are populated from the user's/enterprise's privacy settings
        // via CustomSecretRegexUpdater.
        set_user_and_enterprise_secret_regexes(
            [&Regex::new(r"\bghp_[A-Za-z0-9_]{36}\b").expect("pattern should compile")],
            std::iter::empty(),
        );

        let terminal_view = add_window_with_terminal(&mut app, None);
        let terminal_driver =
            app.update(|ctx| TerminalDriver::create_from_existing_view(terminal_view.clone(), ctx));

        // A command containing a secret matching the configured pattern. The
        // attributed command in the shell-exit error must have the token
        // redacted, since the error flows into server task status and Sentry
        // reports.
        let token = "ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghij";
        let submitted = format!("echo {token}");
        let expected_redacted = format!("echo {}", "*".repeat(token.len()));

        // Start a command (e.g. an environment setup command) so the driver
        // has an in-flight command waiting on the terminal session.
        let command_future = terminal_driver
            .update(&mut app, |driver, ctx| {
                driver.execute_command(&submitted, ctx)
            })
            .expect("command should be accepted before the shell exits");

        // The shell process dies (e.g. the command ran `exit 1`).
        terminal_view.update(&mut app, |_, ctx| ctx.emit(Event::Exited));

        // The in-flight command must resolve with the shell-exit error
        // instead of hanging forever, regardless of whether it had already
        // started executing. The error must name the (redacted) command that
        // was running when the shell died.
        let result = match command_future.await {
            Ok(handle) => handle.await,
            Err(error) => Err(error),
        };
        match &result {
            Err(AgentDriverError::SetupCommandExitedShell { command }) => {
                assert_eq!(command, &expected_redacted);
            }
            other => {
                panic!("in-flight command should fail with SetupCommandExitedShell, got {other:?}")
            }
        }

        // Any further command must fail fast with the same error, still
        // attributing the (redacted) command that killed the shell (not the
        // newly attempted one).
        let fail_fast = terminal_driver.update(&mut app, |driver, ctx| {
            driver.execute_command("echo again", ctx).err()
        });
        match &fail_fast {
            Some(AgentDriverError::SetupCommandExitedShell { command }) => {
                assert_eq!(command, &expected_redacted);
            }
            other => panic!(
                "commands after shell exit should fail fast with SetupCommandExitedShell, got {other:?}"
            ),
        }
    });
}
