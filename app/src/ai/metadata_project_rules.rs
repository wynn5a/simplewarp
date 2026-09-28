use ai::project_context::model::ProjectRuleContents;
use futures::future::{BoxFuture, FutureExt as _};
use warp_util::local_or_remote_path::LocalOrRemotePath;

pub(crate) fn read_project_rule_contents(
    rule_paths: Vec<LocalOrRemotePath>,
) -> BoxFuture<'static, anyhow::Result<ProjectRuleContents>> {
    async move {
        let mut contents = Vec::new();
        for path in rule_paths {
            let local_path = std::path::PathBuf::from(&path);
            match async_fs::read_to_string(&local_path).await {
                Ok(content) => contents.push((path, content)),
                Err(error) => log::debug!(
                    "Failed to read project rule file {}: {error}",
                    local_path.display()
                ),
            }
        }
        Ok(contents)
    }
    .boxed()
}
