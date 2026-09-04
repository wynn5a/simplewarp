#[cfg(all(unix, not(target_os = "macos")))]
use std::fs;
#[cfg(all(unix, not(target_os = "macos")))]
use std::os::unix::ffi::OsStringExt as _;

#[cfg(all(unix, not(target_os = "macos")))]
use command::blocking::Command as BlockingCommand;
#[cfg(all(unix, not(target_os = "macos")))]
use tempfile::{Builder as TempDirBuilder, TempDir};
#[cfg(all(unix, not(target_os = "macos")))]
use tokio::runtime::Runtime;

#[cfg(all(unix, not(target_os = "macos")))]
use super::*;

/// Initialize a fresh git repo in `dir` with one committed file. Leaves the working tree dirty
/// (uncommitted edit) when `dirty` is true so `git diff --binary HEAD` has something to emit.
#[cfg(all(unix, not(target_os = "macos")))]
fn init_git_repo(dir: &Path, dirty: bool) {
    let run = |args: &[&str]| {
        let output = BlockingCommand::new("git")
            .current_dir(dir)
            .args(args)
            .output()
            .expect("failed to spawn git");
        assert!(
            output.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    };
    run(&["init", "-q"]);
    run(&["config", "user.email", "test@example.com"]);
    run(&["config", "user.name", "Test"]);
    fs::write(dir.join("README.md"), "initial\n").unwrap();
    run(&["add", "."]);
    run(&["commit", "-q", "-m", "init"]);
    if dirty {
        fs::write(dir.join("README.md"), "modified\n").unwrap();
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
fn snaptest_tempdir() -> TempDir {
    TempDirBuilder::new().prefix("snaptest").tempdir().unwrap()
}

#[cfg(all(unix, not(target_os = "macos")))]
#[test]
fn build_repo_patch_preserves_non_utf8_untracked_paths() {
    let tempdir = snaptest_tempdir();
    init_git_repo(tempdir.path(), false);
    let filename = std::ffi::OsString::from_vec(b"non-utf8-\xFF.txt".to_vec());
    let file_path = tempdir.path().join(filename);
    fs::write(&file_path, b"content from non-utf8 path\n").unwrap();

    let patch = Runtime::new()
        .unwrap()
        .block_on(build_repo_patch(tempdir.path()))
        .unwrap();
    let patch = String::from_utf8_lossy(&patch);

    assert!(
        patch.contains("content from non-utf8 path"),
        "patch should include non-UTF-8 untracked file contents: {patch}"
    );
}
