use warp_util::path::ShellFamily;

use super::{clone_command, target_directory};

#[test]
fn target_directory_follows_git_naming() {
    assert_eq!(
        target_directory("https://github.com/owner/repo.git"),
        Some("repo")
    );
    assert_eq!(
        target_directory("https://github.com/owner/repo"),
        Some("repo")
    );
    assert_eq!(
        target_directory("https://github.com/owner/repo/"),
        Some("repo")
    );
    assert_eq!(
        target_directory("git@github.com:owner/repo.git"),
        Some("repo")
    );
    assert_eq!(target_directory("git@host:repo.git"), Some("repo"));
    assert_eq!(target_directory("/tmp/some/repo/.git"), Some("repo"));
    assert_eq!(target_directory(""), None);
    assert_eq!(target_directory("https://host/.."), None);
}

#[test]
fn posix_command_clones_then_enters_the_clone() {
    assert_eq!(
        clone_command(" https://github.com/owner/repo.git ", ShellFamily::Posix),
        "git clone -- https://github.com/owner/repo.git && cd repo"
    );
}

#[test]
fn posix_command_escapes_url_and_directory() {
    assert_eq!(
        clone_command("/tmp/my repo", ShellFamily::Posix),
        r"git clone -- /tmp/my\ repo && cd my\ repo"
    );
}

#[test]
fn powershell_command_avoids_double_ampersand() {
    assert_eq!(
        clone_command("https://github.com/owner/repo.git", ShellFamily::PowerShell),
        "git clone -- https://github.com/owner/repo.git; if ($?) { cd repo }"
    );
}

#[test]
fn command_without_a_directory_only_clones() {
    assert_eq!(
        clone_command("https://host/..", ShellFamily::Posix),
        "git clone -- https://host/.."
    );
}
