use std::path::Path;

pub(crate) fn sanitized_basename(path_or_filename: &str) -> Option<String> {
    let file_name = Path::new(path_or_filename).file_name()?.to_str()?;
    if file_name.is_empty() {
        return None;
    }
    Some(file_name.to_string())
}

#[cfg(test)]
#[path = "artifact_download_tests.rs"]
mod tests;
