use warp_util::path::ShellFamily;

/// Builds the shell command that clones `url` into the working directory and then enters the
/// clone.
pub fn clone_command(url: &str, shell_family: ShellFamily) -> String {
    let url = url.trim();
    let clone = format!("git clone -- {}", shell_family.shell_escape(url));
    let Some(directory) = target_directory(url) else {
        return clone;
    };
    let directory = shell_family.shell_escape(directory);
    match shell_family {
        ShellFamily::Posix => format!("{clone} && cd {directory}"),
        // Windows PowerShell 5 has no `&&`.
        ShellFamily::PowerShell => format!("{clone}; if ($?) {{ cd {directory} }}"),
    }
}

/// The directory `git clone` creates for `url` when no directory is given: the last path
/// component, without a trailing `.git`.
fn target_directory(url: &str) -> Option<&str> {
    let path = url.trim_end_matches(['/', '\\']);
    let path = path.strip_suffix("/.git").unwrap_or(path);
    let name = path.rsplit(['/', '\\', ':']).next()?;
    let name = name.strip_suffix(".git").unwrap_or(name);
    (!name.is_empty() && name != "." && name != "..").then_some(name)
}

#[cfg(test)]
#[path = "clone_repo_tests.rs"]
mod tests;
