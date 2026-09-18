use warp_cli::CliCommand;

use super::command_requires_auth;

#[test]
fn logout_does_not_require_auth() {
    assert!(!command_requires_auth(&CliCommand::Logout));
}
