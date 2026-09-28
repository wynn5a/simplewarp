use crate::local_or_remote_path::LocalOrRemotePath;
use crate::standardized_path::StandardizedPath;

fn local_repo_path() -> std::path::PathBuf {
    #[cfg(unix)]
    let path = "/repo";
    #[cfg(windows)]
    let path = r"C:\repo";

    path.into()
}

fn local_file_path() -> std::path::PathBuf {
    local_repo_path().join("file.txt")
}

fn local_absolute_file_path() -> std::path::PathBuf {
    #[cfg(unix)]
    let path = "/server/repo/src/foo.rs";
    #[cfg(windows)]
    let path = r"C:\server\repo\src\foo.rs";

    path.into()
}

#[test]
fn local_or_remote_path_helpers_return_local_path_components() {
    let local_file = local_file_path();
    let path = LocalOrRemotePath::Local(local_file.clone());

    assert_eq!(path.display_name(), "file.txt");
    assert_eq!(
        path.path_component(),
        StandardizedPath::try_from_local(&local_file).unwrap()
    );
    assert_eq!(
        path.display_path(),
        local_file.to_string_lossy().into_owned()
    );
    assert_eq!(path.to_local_path(), Some(local_file.as_path()));
}

#[test]
fn local_or_remote_path_join() {
    let local_repo = local_repo_path();
    let local = LocalOrRemotePath::Local(local_repo.clone());

    let local_joined = local.join("src/foo.rs");
    let expected_local_joined = local_repo.join("src/foo.rs");
    assert_eq!(
        local_joined.path_component(),
        StandardizedPath::try_from_local(&expected_local_joined).unwrap()
    );
}

#[test]
fn local_or_remote_path_join_with_absolute_replaces_prefix() {
    let local = LocalOrRemotePath::Local(local_repo_path());
    let local_abs = local_absolute_file_path();
    let local_abs_str = local_abs.to_string_lossy().into_owned();

    // Path::join replacement semantics on absolute argument.
    let local_joined = local.join(&local_abs_str);
    assert_eq!(
        local_joined.path_component(),
        StandardizedPath::try_from_local(&local_abs).unwrap()
    );
}

#[test]
fn local_or_remote_path_strip_repo_prefix_local_local() {
    let repo = LocalOrRemotePath::Local(local_repo_path());
    let inside = LocalOrRemotePath::Local(local_repo_path().join("src").join("foo.rs"));
    let outside = LocalOrRemotePath::Local(local_absolute_file_path());
    let expected_relative = std::path::Path::new("src")
        .join("foo.rs")
        .to_string_lossy()
        .into_owned();

    assert_eq!(repo.strip_repo_prefix(&inside), Some(expected_relative));
    assert_eq!(repo.strip_repo_prefix(&outside), None);
}
