//! Local-to-cloud handoff snapshot upload pipeline, invoked from
//! [`upload_snapshot_for_handoff`].
//!
//! Given a set of repo and orphan-file paths, gathers git-diff patches or file contents for
//! each, and uploads them (plus a `snapshot_state.json` manifest) to presigned GCS URLs.
//! Transient upload failures retry through the shared [`with_bounded_retry`] helper.
//!
//! All failures are logged and absorbed so the caller continues regardless.
use std::collections::{HashMap, HashSet};
use std::ffi::{OsStr, OsString};
#[cfg(unix)]
use std::os::unix::ffi::OsStrExt as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context as _, Result};
use command::Stdio;
use command::r#async::Command;
use futures::future::join_all;
use warp_errors::report_error;
use warpui::r#async::FutureExt as _;

use crate::ai::agent_sdk::retry::with_bounded_retry;
use crate::server::server_api::ai::{
    AIClient, InitialSnapshotToken, SnapshotUploadFileInfo as AiSnapshotUploadFileInfo,
    UploadLocalHandoffSnapshotRequest,
};
use crate::server::server_api::harness_support::{
    SnapshotFileInfo, UploadTarget, upload_to_target,
};

/// Upper bound for each git subprocess spawned during the gather phase.
const GIT_COMMAND_TIMEOUT: Duration = Duration::from_secs(30);

/// Total cap on files (blobs + manifest) uploaded per run. Blobs beyond the cap are dropped
/// from upload and marked `skipped` in the manifest so consumers can distinguish capped
/// entries from real upload failures.
const MAX_SNAPSHOT_FILES_PER_RUN: usize = 100;

/// Per-file ceiling, mirroring the server's `handoff_snapshots.max_file_upload_size_bytes`.
/// The presigned URL is signed with this limit, so a larger blob's PUT is rejected by storage.
///
/// Oversized blobs are dropped from the upload plan and marked `skipped` rather than left to
/// fail: a checkpoint attempt refuses to commit when any required blob failed, so one
/// too-large file would otherwise block every future attempt for the run rather than costing
/// just itself.
const MAX_SNAPSHOT_FILE_SIZE_BYTES: u64 = 25 * 1024 * 1024;

// --- Declarations file parsing ---

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum EntryKind {
    Repo,
    File,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DeclarationEntry {
    kind: EntryKind,
    path: String,
}

// --- Gather phase: upload blobs and per-entry results ---

struct SnapshotUploadFile {
    filename: String,
    content: Vec<u8>,
    mime_type: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EntryStatus {
    Uploaded,
    Failed,
    /// Deliberately dropped to honor [`MAX_SNAPSHOT_FILES_PER_RUN`]. A policy decision, not a
    /// failure, so a checkpoint attempt may still commit the kept subset.
    Skipped,
    /// The server returned no presigned target for this blob, violating `upload-snapshot`'s
    /// positional alignment. Distinct from [`EntryStatus::Skipped`] because nothing
    /// intentional happened: committing here would silently shrink the object set.
    NoTarget,
    GatherFailed,
    ReadFailed,
}

impl EntryStatus {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Uploaded => "uploaded",
            Self::Failed => "failed",
            Self::Skipped => "skipped",
            Self::NoTarget => "no_target",
            Self::GatherFailed => "gather_failed",
            Self::ReadFailed => "read_failed",
        }
    }
}

#[derive(Debug)]
struct EntryResult {
    /// Label for log output — prefers the snapshot filename and falls back to the source path.
    label: String,
    status: EntryStatus,
    error: Option<String>,
}

struct SnapshotSummary {
    uploaded: usize,
    failed: usize,
    skipped: usize,
    no_target: usize,
    gather_failed: usize,
    read_failed: usize,
    total: usize,
    manifest_uploaded: bool,
}

impl SnapshotSummary {
    fn from_entries(entries: &[EntryResult], manifest_uploaded: bool) -> Self {
        let mut s = Self {
            uploaded: 0,
            failed: 0,
            skipped: 0,
            no_target: 0,
            gather_failed: 0,
            read_failed: 0,
            total: entries.len(),
            manifest_uploaded,
        };
        for e in entries {
            match e.status {
                EntryStatus::Uploaded => s.uploaded += 1,
                EntryStatus::Failed => s.failed += 1,
                EntryStatus::Skipped => s.skipped += 1,
                EntryStatus::NoTarget => s.no_target += 1,
                EntryStatus::GatherFailed => s.gather_failed += 1,
                EntryStatus::ReadFailed => s.read_failed += 1,
            }
        }
        s
    }

    fn all_uploaded(&self) -> bool {
        self.manifest_uploaded && self.uploaded == self.total
    }
}

#[derive(Debug)]
struct SnapshotOutcome {
    entries: Vec<EntryResult>,
    manifest_uploaded: bool,
}

// --- Manifest schema ---

#[derive(serde::Serialize)]
struct RepoManifestEntry {
    path: String,
    repo_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    branch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    head_sha: Option<String>,
    patch_file: Option<String>,
    status: &'static str,
    uploaded: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(serde::Serialize)]
struct FileManifestEntry {
    path: String,
    snapshot_file: Option<String>,
    status: &'static str,
    uploaded: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(serde::Serialize)]
struct SnapshotManifest {
    version: u32,
    repos: Vec<RepoManifestEntry>,
    files: Vec<FileManifestEntry>,
}

// --- Upload helpers ---

/// Upload `body` to `target` through the shared retry helper, re-cloning `body` per attempt.
async fn upload_with_retry(
    http: &http_client::Client,
    target: &UploadTarget,
    body: Vec<u8>,
    operation: &str,
) -> Result<()> {
    with_bounded_retry(operation, || upload_to_target(http, target, body.clone())).await
}

// --- Entry point ---

/// Build the snapshot for a local-to-cloud handoff: gather repo patches and orphan file
/// contents, allocate an initial snapshot token plus presigned upload URLs via
/// `AIClient::upload_local_handoff_snapshot`, and upload the artifacts.
///
/// Returns:
/// - `Ok(Some(initial_snapshot_token))` when a token was minted **and the manifest landed in GCS**.
///   Individual blob uploads may still have failed; the manifest catalogues their status so the
///   cloud agent rehydrates against whatever did land, matching the cloud→cloud best-effort
///   posture.
/// - `Ok(None)` when the workspace was empty (no repos, no orphan files) **or** when the
///   manifest itself failed to upload. Without the manifest the snapshot is unusable, so
///   callers should spawn the cloud agent without an initial snapshot token instead of pointing
///   it at an incomplete prefix. Manifest-upload failures are also routed through
///   `report_error!` so on-call alerting catches the silent regression.
/// - `Err(_)` only for hard failures of `upload_local_handoff_snapshot` itself (auth, etc.).
pub(crate) async fn upload_snapshot_for_handoff(
    repo_paths: Vec<PathBuf>,
    orphan_file_paths: Vec<PathBuf>,
    client: Arc<dyn AIClient>,
    http: &http_client::Client,
) -> Result<Option<InitialSnapshotToken>> {
    if repo_paths.is_empty() && orphan_file_paths.is_empty() {
        log::info!("Handoff snapshot has no declarations; skipping upload");
        return Ok(None);
    }

    let declarations: Vec<DeclarationEntry> = repo_paths
        .into_iter()
        .map(|path| DeclarationEntry {
            kind: EntryKind::Repo,
            path: path.display().to_string(),
        })
        .chain(orphan_file_paths.into_iter().map(|path| DeclarationEntry {
            kind: EntryKind::File,
            path: path.display().to_string(),
        }))
        .collect();

    let GatheredSnapshot {
        manifest_filename,
        mut upload_files,
        mut repos,
        mut files,
        mut pre_upload_entries,
    } = gather_snapshot_entries(declarations).await;

    apply_per_run_cap(
        &mut upload_files,
        &mut repos,
        &mut files,
        &mut pre_upload_entries,
    );

    let mut file_infos: Vec<SnapshotFileInfo> = upload_files
        .iter()
        .map(|file| SnapshotFileInfo {
            filename: file.filename.clone(),
            mime_type: file.mime_type.clone(),
        })
        .collect();
    file_infos.push(SnapshotFileInfo {
        filename: manifest_filename.clone(),
        mime_type: "application/json".to_string(),
    });

    let upload_request = UploadLocalHandoffSnapshotRequest {
        files: file_infos
            .iter()
            .map(|file| AiSnapshotUploadFileInfo {
                filename: file.filename.clone(),
                mime_type: file.mime_type.clone(),
            })
            .collect(),
    };
    let response = client
        .upload_local_handoff_snapshot(upload_request)
        .await
        .context("failed to allocate initial snapshot token")?;
    log::info!(
        "Initial snapshot token allocated; expires_at={}, uploads={}",
        response.expires_at,
        response.uploads.len(),
    );
    let initial_snapshot_token = response.initial_snapshot_token;

    // Server returns `uploads` aligned by index with the request `files` array (and does
    // not echo per-entry filenames), so we zip them positionally into a filename-keyed map.
    // Any request file the server omits lands in `upload_entry` with no target and is
    // marked `skipped` downstream.
    if response.uploads.len() != file_infos.len() {
        log::warn!(
            "Handoff snapshot upload-target response length {} does not match request length {}; \
             extras will be marked skipped",
            response.uploads.len(),
            file_infos.len(),
        );
    }
    let mut target_map: HashMap<String, UploadTarget> = HashMap::new();
    for (file, target) in file_infos.iter().zip(response.uploads.into_iter()) {
        target_map.insert(file.filename.clone(), target);
    }

    let Some(outcome) = upload_prepared_snapshot_files(
        http,
        manifest_filename,
        upload_files,
        repos,
        files,
        pre_upload_entries,
        target_map,
    )
    .await
    else {
        // Manifest serialization failed (already reported via `report_error!` inside
        // the helper). Without a manifest the snapshot is unusable, so refuse the token.
        return Ok(None);
    };

    let summary = SnapshotSummary::from_entries(&outcome.entries, outcome.manifest_uploaded);
    log_snapshot_outcome(&outcome);
    if !summary.manifest_uploaded {
        // Without the manifest the cloud agent has no catalogue to rehydrate from, even
        // when individual blobs landed. Alert on-call and refuse the token so we don't
        // silently spawn a cloud agent with no recoverable state.
        report_error!(
            "Handoff snapshot manifest failed to upload; cloud agent will start with no rehydration content",
            extra: { "uploaded" => %summary.uploaded, "total" => %summary.total }
        );
        return Ok(None);
    }

    Ok(Some(initial_snapshot_token))
}

struct GatheredSnapshot {
    manifest_filename: String,
    upload_files: Vec<SnapshotUploadFile>,
    repos: Vec<RepoManifestEntry>,
    files: Vec<FileManifestEntry>,
    pre_upload_entries: Vec<EntryResult>,
}

async fn gather_snapshot_entries(declarations: Vec<DeclarationEntry>) -> GatheredSnapshot {
    let mut used_filenames = HashSet::new();
    let manifest_filename = unique_filename("snapshot_state.json", &mut used_filenames);

    // Gather phase: produce upload blobs and per-entry manifest stubs.
    // Gather/read failures are captured as EntryResult entries and surfaced in the log output.
    let mut upload_files: Vec<SnapshotUploadFile> = Vec::new();
    let mut repos: Vec<RepoManifestEntry> = Vec::new();
    let mut files: Vec<FileManifestEntry> = Vec::new();
    let mut pre_upload_entries: Vec<EntryResult> = Vec::new();

    let mut repo_index: usize = 0;
    for entry in &declarations {
        match entry.kind {
            EntryKind::Repo => {
                repo_index += 1;
                gather_repo(
                    &entry.path,
                    repo_index,
                    &mut used_filenames,
                    &mut upload_files,
                    &mut repos,
                    &mut pre_upload_entries,
                )
                .await;
            }
            EntryKind::File => {
                gather_file(
                    &entry.path,
                    &mut used_filenames,
                    &mut upload_files,
                    &mut files,
                    &mut pre_upload_entries,
                )
                .await;
            }
        }
    }

    GatheredSnapshot {
        manifest_filename,
        upload_files,
        repos,
        files,
        pre_upload_entries,
    }
}

async fn upload_prepared_snapshot_files(
    http: &http_client::Client,
    manifest_filename: String,
    upload_files: Vec<SnapshotUploadFile>,
    mut repos: Vec<RepoManifestEntry>,
    mut files: Vec<FileManifestEntry>,
    pre_upload_entries: Vec<EntryResult>,
    target_map: HashMap<String, UploadTarget>,
) -> Option<SnapshotOutcome> {
    // Upload non-manifest blobs concurrently, each with bounded retries on transient errors.
    let upload_futures = upload_files
        .iter()
        .map(|file| upload_entry(http, file, &target_map));
    let upload_entries: Vec<EntryResult> = join_all(upload_futures).await;
    fold_upload_results(&mut repos, &mut files, &upload_entries);

    // Build and upload the manifest last, with the real outcomes baked in.
    let manifest = SnapshotManifest {
        version: 1,
        repos,
        files,
    };
    let manifest_bytes = match serde_json::to_vec_pretty(&manifest) {
        Ok(b) => b,
        Err(e) => {
            // Pipeline-abort: route through report_error! so Sentry captures it.
            report_error!(
                anyhow::Error::from(e)
                    .context("Failed to serialize snapshot manifest; skipping upload")
            );
            return None;
        }
    };
    let (manifest_uploaded, manifest_error) = match target_map.get(&manifest_filename) {
        Some(target) => {
            let upload_target = merge_content_type(target, "application/json");
            let operation = format!("snapshot upload '{manifest_filename}'");
            match upload_with_retry(http, &upload_target, manifest_bytes, &operation).await {
                Ok(()) => (true, None),
                Err(e) => {
                    // Capture the full chain for the manifest's `error` field, then surface it
                    // to Sentry via report_error!.
                    let e = e.context(format!("Failed to upload manifest '{manifest_filename}'"));
                    let msg = format!("{e:#}");
                    report_error!(e);
                    (false, Some(msg))
                }
            }
        }
        None => (
            false,
            Some(String::from("no upload target returned by server")),
        ),
    };

    // Assemble the final entries list in a stable order: pre-upload failures, upload results,
    // then the manifest itself.
    let mut entries = pre_upload_entries;
    entries.extend(upload_entries);
    entries.push(EntryResult {
        label: manifest_filename,
        status: if manifest_uploaded {
            EntryStatus::Uploaded
        } else {
            EntryStatus::Failed
        },
        error: manifest_error,
    });

    Some(SnapshotOutcome {
        entries,
        manifest_uploaded,
    })
}

/// Message recorded for a blob dropped for exceeding [`MAX_SNAPSHOT_FILE_SIZE_BYTES`].
fn oversized_error(size_bytes: u64) -> String {
    format!(
        "exceeds the per-file snapshot limit of {MAX_SNAPSHOT_FILE_SIZE_BYTES} bytes ({size_bytes} bytes)"
    )
}

/// Gather a repo entry: run `build_repo_patch` and append an upload blob + manifest stub.
async fn gather_repo(
    repo_path: &str,
    repo_index: usize,
    used_filenames: &mut HashSet<String>,
    upload_files: &mut Vec<SnapshotUploadFile>,
    repos: &mut Vec<RepoManifestEntry>,
    pre_upload_entries: &mut Vec<EntryResult>,
) {
    let repo = Path::new(repo_path);
    let metadata = repo_metadata(repo).await;
    match build_repo_patch(repo).await {
        Ok(patch) if patch.len() as u64 > MAX_SNAPSHOT_FILE_SIZE_BYTES => {
            let err_str = oversized_error(patch.len() as u64);
            log::warn!("Skipping repo '{repo_path}': {err_str}");
            repos.push(RepoManifestEntry {
                path: repo_path.to_string(),
                repo_name: metadata.repo_name,
                branch: metadata.branch,
                head_sha: metadata.head_sha,
                patch_file: None,
                status: "skipped",
                uploaded: Some(false),
                error: Some(err_str.clone()),
            });
            pre_upload_entries.push(EntryResult {
                label: format!("[repo] {repo_path}"),
                status: EntryStatus::Skipped,
                error: Some(err_str),
            });
        }
        Ok(patch) if patch.is_empty() => {
            repos.push(RepoManifestEntry {
                path: repo_path.to_string(),
                repo_name: metadata.repo_name,
                branch: metadata.branch,
                head_sha: metadata.head_sha,
                patch_file: None,
                status: "clean",
                uploaded: None,
                error: None,
            });
        }
        Ok(patch) => {
            let preferred = format!(
                "{}_{}.patch",
                repo_index,
                sanitize_filename_component(&metadata.repo_name)
            );
            let filename = unique_filename(&preferred, used_filenames);
            upload_files.push(SnapshotUploadFile {
                filename: filename.clone(),
                content: patch,
                mime_type: "text/x-diff".to_string(),
            });
            repos.push(RepoManifestEntry {
                path: repo_path.to_string(),
                repo_name: metadata.repo_name,
                branch: metadata.branch,
                head_sha: metadata.head_sha,
                patch_file: Some(filename),
                status: "dirty",
                uploaded: None,
                error: None,
            });
        }
        Err(e) => {
            let err_str = format!("{e:#}");
            log::warn!("Failed to snapshot repo '{repo_path}': {err_str}");
            repos.push(RepoManifestEntry {
                path: repo_path.to_string(),
                repo_name: metadata.repo_name,
                branch: metadata.branch,
                head_sha: metadata.head_sha,
                patch_file: None,
                status: "gather_failed",
                uploaded: None,
                error: Some(err_str.clone()),
            });
            pre_upload_entries.push(EntryResult {
                label: format!("[repo] {repo_path}"),
                status: EntryStatus::GatherFailed,
                error: Some(err_str),
            });
        }
    }
}

/// Gather a file entry: read the file and append an upload blob + manifest stub.
async fn gather_file(
    file_path: &str,
    used_filenames: &mut HashSet<String>,
    upload_files: &mut Vec<SnapshotUploadFile>,
    files: &mut Vec<FileManifestEntry>,
    pre_upload_entries: &mut Vec<EntryResult>,
) {
    let path = Path::new(file_path);
    // Stat before reading so an oversized file is skipped without pulling it into memory.
    if let Ok(metadata) = tokio::fs::metadata(path).await
        && metadata.len() > MAX_SNAPSHOT_FILE_SIZE_BYTES
    {
        let err_str = oversized_error(metadata.len());
        log::warn!("Skipping file '{file_path}': {err_str}");
        files.push(FileManifestEntry {
            path: file_path.to_string(),
            snapshot_file: None,
            status: "skipped",
            uploaded: Some(false),
            error: Some(err_str.clone()),
        });
        pre_upload_entries.push(EntryResult {
            label: format!("[file] {file_path}"),
            status: EntryStatus::Skipped,
            error: Some(err_str),
        });
        return;
    }
    match tokio::fs::read(path).await {
        Ok(content) => {
            // Sanitize before uniquifying so the de-duplication suffix cannot break the
            // invariants; see `sanitize_name_component`.
            let preferred = sanitize_name_component(
                &path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| file_path.to_string()),
                FALLBACK_SNAPSHOT_FILENAME,
            );
            let filename = unique_filename(&preferred, used_filenames);
            let mime = mime_guess::from_path(path)
                .first_or_octet_stream()
                .to_string();
            upload_files.push(SnapshotUploadFile {
                filename: filename.clone(),
                content,
                mime_type: mime,
            });
            files.push(FileManifestEntry {
                path: file_path.to_string(),
                snapshot_file: Some(filename),
                // Placeholder; rewritten by fold_upload_results once uploads settle.
                status: "uploaded",
                uploaded: None,
                error: None,
            });
        }
        Err(e) => {
            let err_str = format!("Failed to read file '{file_path}': {e:#}");
            log::warn!("{err_str}");
            files.push(FileManifestEntry {
                path: file_path.to_string(),
                snapshot_file: None,
                status: "read_failed",
                uploaded: None,
                error: Some(err_str.clone()),
            });
            pre_upload_entries.push(EntryResult {
                label: format!("[file] {file_path}"),
                status: EntryStatus::ReadFailed,
                error: Some(err_str),
            });
        }
    }
}

/// Upload a single prepared file through the retry helper.
/// Produces an [`EntryResult`] labelled with the file's filename, or marked
/// [`EntryStatus::NoTarget`] if the server did not return a target for it.
async fn upload_entry(
    http: &http_client::Client,
    file: &SnapshotUploadFile,
    target_map: &HashMap<String, UploadTarget>,
) -> EntryResult {
    let Some(target) = target_map.get(&file.filename) else {
        log::warn!(
            "No upload target returned by the server for file '{}'; it will not be uploaded",
            file.filename
        );
        return EntryResult {
            label: file.filename.clone(),
            status: EntryStatus::NoTarget,
            error: Some("no upload target returned by server".to_string()),
        };
    };

    let upload_target = merge_content_type(target, &file.mime_type);
    let operation = format!("snapshot upload '{}'", file.filename);
    match upload_with_retry(http, &upload_target, file.content.clone(), &operation).await {
        Ok(()) => EntryResult {
            label: file.filename.clone(),
            status: EntryStatus::Uploaded,
            error: None,
        },
        Err(e) => {
            let msg = format!("{e:#}");
            log::warn!("Failed to upload '{}': {msg}", file.filename);
            EntryResult {
                label: file.filename.clone(),
                status: EntryStatus::Failed,
                error: Some(msg),
            }
        }
    }
}

/// Fold upload outcomes into the per-entry manifest stubs so the uploaded manifest reflects
/// what actually landed in GCS.
fn fold_upload_results(
    repos: &mut [RepoManifestEntry],
    files: &mut [FileManifestEntry],
    upload_entries: &[EntryResult],
) {
    for entry in upload_entries {
        if let Some(repo_entry) = repos
            .iter_mut()
            .find(|r| r.patch_file.as_deref() == Some(entry.label.as_str()))
        {
            match entry.status {
                EntryStatus::Uploaded => {
                    repo_entry.uploaded = Some(true);
                    repo_entry.status = "uploaded";
                }
                EntryStatus::Failed => {
                    repo_entry.uploaded = Some(false);
                    repo_entry.status = "failed";
                    repo_entry.error = entry.error.clone();
                }
                // Both surface as `skipped` to keep the manifest's status vocabulary stable
                // for rehydration consumers; the distinguishing detail lives in `error`.
                EntryStatus::Skipped | EntryStatus::NoTarget => {
                    repo_entry.uploaded = Some(false);
                    repo_entry.status = "skipped";
                    repo_entry.error = entry.error.clone();
                }
                EntryStatus::GatherFailed | EntryStatus::ReadFailed => {
                    report_error!(
                        "fold_upload_results: unexpected pre-upload status for repo patch",
                        extra: { "status" => ?entry.status, "label" => %entry.label }
                    );
                }
            }
        } else if let Some(file_entry) = files
            .iter_mut()
            .find(|f| f.snapshot_file.as_deref() == Some(entry.label.as_str()))
        {
            match entry.status {
                EntryStatus::Uploaded => {
                    file_entry.uploaded = Some(true);
                    file_entry.status = "uploaded";
                }
                EntryStatus::Failed => {
                    file_entry.uploaded = Some(false);
                    file_entry.status = "failed";
                    file_entry.error = entry.error.clone();
                }
                EntryStatus::Skipped | EntryStatus::NoTarget => {
                    file_entry.uploaded = Some(false);
                    file_entry.status = "skipped";
                    file_entry.error = entry.error.clone();
                }
                EntryStatus::GatherFailed | EntryStatus::ReadFailed => {
                    report_error!(
                        "fold_upload_results: unexpected pre-upload status for file",
                        extra: { "status" => ?entry.status, "label" => %entry.label }
                    );
                }
            }
        }
    }
}

/// Enforce [`MAX_SNAPSHOT_FILES_PER_RUN`] by truncating the upload blob list.
/// Reserves one slot for the `snapshot_state.json` manifest, so blobs share the remaining
/// budget. For each dropped blob, rewrites the matching manifest entry to `skipped` with a
/// cap-reason error and records a pre-upload [`EntryResult`] so the summary count is honest.
fn apply_per_run_cap(
    upload_files: &mut Vec<SnapshotUploadFile>,
    repos: &mut [RepoManifestEntry],
    files: &mut [FileManifestEntry],
    pre_upload_entries: &mut Vec<EntryResult>,
) {
    let blob_limit = MAX_SNAPSHOT_FILES_PER_RUN.saturating_sub(1);
    if upload_files.len() <= blob_limit {
        return;
    }
    let total_including_manifest = upload_files.len() + 1;
    let dropped = upload_files.split_off(blob_limit);
    log::warn!(
        "Snapshot exceeds per-run cap of {MAX_SNAPSHOT_FILES_PER_RUN} files ({total_including_manifest} declared); dropping {} blob(s) from upload",
        dropped.len(),
    );
    let err_msg = format!("exceeded per-run snapshot cap of {MAX_SNAPSHOT_FILES_PER_RUN} files");
    for dropped_file in dropped {
        mark_capped_manifest_entry(repos, files, &dropped_file.filename, &err_msg);
        pre_upload_entries.push(EntryResult {
            label: dropped_file.filename,
            status: EntryStatus::Skipped,
            error: Some(err_msg.clone()),
        });
    }
}

/// Rewrite the manifest entry matching `filename` (by `patch_file` or `snapshot_file`) to
/// `skipped` with the given error message. Used when blobs are dropped to honor the per-run cap.
fn mark_capped_manifest_entry(
    repos: &mut [RepoManifestEntry],
    files: &mut [FileManifestEntry],
    filename: &str,
    err_msg: &str,
) {
    if let Some(repo_entry) = repos
        .iter_mut()
        .find(|r| r.patch_file.as_deref() == Some(filename))
    {
        repo_entry.status = "skipped";
        repo_entry.uploaded = Some(false);
        repo_entry.error = Some(err_msg.to_string());
    } else if let Some(file_entry) = files
        .iter_mut()
        .find(|f| f.snapshot_file.as_deref() == Some(filename))
    {
        file_entry.status = "skipped";
        file_entry.uploaded = Some(false);
        file_entry.error = Some(err_msg.to_string());
    }
}

/// Clone an [`UploadTarget`] and ensure its `Content-Type` header matches `mime_type`
/// (preserving any casing the server used if the header is already present).
fn merge_content_type(target: &UploadTarget, mime_type: &str) -> UploadTarget {
    let mut headers = target.headers.clone();
    if !headers
        .keys()
        .any(|name| name.eq_ignore_ascii_case("content-type"))
    {
        headers.insert("Content-Type".to_string(), mime_type.to_string());
    }
    UploadTarget {
        url: target.url.clone(),
        method: target.method.clone(),
        headers,
        fields: target.fields.clone(),
    }
}

/// Log the final outcome at INFO when everything uploaded, WARN otherwise. The log line
/// includes per-entry statuses so operators can diagnose partial state without parsing any
/// downstream logs.
fn log_snapshot_outcome(outcome: &SnapshotOutcome) {
    let summary = SnapshotSummary::from_entries(&outcome.entries, outcome.manifest_uploaded);
    let manifest_bit = if summary.manifest_uploaded {
        "manifest: uploaded"
    } else {
        "manifest: failed"
    };
    let header = format!(
        "Snapshot upload: {}/{} uploaded (failed: {}, skipped: {}, no_target: {}, \
         gather_failed: {}, read_failed: {}; {manifest_bit})",
        summary.uploaded,
        summary.total,
        summary.failed,
        summary.skipped,
        summary.no_target,
        summary.gather_failed,
        summary.read_failed,
    );
    if summary.all_uploaded() {
        log::info!("{header}");
        for e in &outcome.entries {
            log::info!("  {}: {}", e.label, e.status.as_str());
        }
    } else {
        log::warn!("{header}");
        for e in &outcome.entries {
            match &e.error {
                Some(err) => {
                    log::warn!("  {}: {} ({err})", e.label, e.status.as_str());
                }
                None => {
                    log::warn!("  {}: {}", e.label, e.status.as_str());
                }
            }
        }
    }
}

// --- Git-diff and filename helpers ---
struct RepoMetadata {
    repo_name: String,
    branch: Option<String>,
    head_sha: Option<String>,
}

async fn repo_metadata(repo_dir: &Path) -> RepoMetadata {
    RepoMetadata {
        repo_name: repo_dir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("repo")
            .to_string(),
        branch: git_output_string(repo_dir, &["symbolic-ref", "--quiet", "--short", "HEAD"]).await,
        head_sha: git_output_string(repo_dir, &["rev-parse", "HEAD"]).await,
    }
}

async fn build_repo_patch(repo_dir: &Path) -> Result<Vec<u8>> {
    let mut patch = git_output_bytes(repo_dir, ["diff", "--binary", "HEAD"], &[0]).await?;
    let untracked_listing = git_output_bytes(
        repo_dir,
        ["ls-files", "--others", "--exclude-standard", "-z"],
        &[0],
    )
    .await?;

    for raw_path in untracked_listing.split(|byte| *byte == 0) {
        if raw_path.is_empty() {
            continue;
        }
        let path = untracked_path_arg(raw_path);
        let args = [
            OsString::from("diff"),
            OsString::from("--binary"),
            OsString::from("--no-index"),
            OsString::from("--"),
            OsString::from("/dev/null"),
            path,
        ];
        let untracked_patch = git_output_bytes(repo_dir, args, &[0, 1]).await?;
        if untracked_patch.is_empty() {
            continue;
        }
        if !patch.is_empty() && !patch.ends_with(b"\n") {
            patch.push(b'\n');
        }
        patch.extend_from_slice(&untracked_patch);
    }

    Ok(patch)
}

fn untracked_path_arg(raw_path: &[u8]) -> OsString {
    #[cfg(unix)]
    {
        OsStr::from_bytes(raw_path).to_os_string()
    }
    #[cfg(not(unix))]
    {
        String::from_utf8_lossy(raw_path).into_owned().into()
    }
}

async fn git_output_string(repo_dir: &Path, args: &[&str]) -> Option<String> {
    let output = git_output_bytes(repo_dir, args, &[0]).await.ok()?;
    let value = String::from_utf8(output).ok()?;
    let value = value.trim().to_string();
    if value.is_empty() { None } else { Some(value) }
}

const FALLBACK_SNAPSHOT_FILENAME: &str = "snapshot_artifact";
const RESERVED_NAME_ESCAPE: &str = "snapshot-";

/// Longest logical filename we will mint. The server rejects names over 255 bytes; the
/// remainder is headroom for the `_<n>` de-duplication suffix [`unique_filename`] may append.
const MAX_SNAPSHOT_FILENAME_LEN: usize = 240;

/// Reshape `value` into a logical snapshot filename the server will accept, falling back to
/// `fallback` when nothing usable survives.
///
/// Logical names are agent-controlled and the server rejects the *entire* upload-targets
/// request if one is malformed, so a single awkward basename would otherwise cost the whole
/// snapshot. Its rules: `[A-Za-z0-9._-]` only, at most 255 bytes, not `.` or `..`, no leading
/// `-`, and — on the legacy path — nothing in the reserved `checkpoint_` namespace. Runs of
/// `_` are squashed on top of that, which keeps [`unique_filename`]'s `_<n>` suffix legible
/// and names conservative.
fn sanitize_name_component(value: &str, fallback: &str) -> String {
    let mut sanitized = String::with_capacity(value.len());
    for c in value.chars() {
        let c = if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') {
            c
        } else {
            '_'
        };
        if c == '_' && sanitized.ends_with('_') {
            continue;
        }
        sanitized.push(c);
    }

    let trimmed = sanitized
        .trim_start_matches(['_', '-'])
        .trim_end_matches('_');
    let mut name = match trimmed {
        "" | "." | ".." => fallback.to_string(),
        other => other.to_string(),
    };
    if is_reserved_snapshot_name(&name) {
        name.insert_str(0, RESERVED_NAME_ESCAPE);
    }
    // Sanitized names are pure ASCII, so this always lands on a char boundary.
    name.truncate(MAX_SNAPSHOT_FILENAME_LEN);
    name
}

/// Names owned by the checkpoint protocol, which the server refuses to sign legacy uploads for.
fn is_reserved_snapshot_name(name: &str) -> bool {
    name.starts_with("checkpoint_") || name == "latest-checkpoint.json"
}

fn sanitize_filename_component(value: &str) -> String {
    sanitize_name_component(value, "repo")
}

fn unique_filename(preferred: &str, used: &mut HashSet<String>) -> String {
    let preferred = Path::new(preferred)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| FALLBACK_SNAPSHOT_FILENAME.to_string());

    if used.insert(preferred.clone()) {
        return preferred;
    }

    let path = Path::new(&preferred);
    // Trailing `_` is trimmed so `a_.txt` de-duplicates to `a_2.txt` rather than reintroducing
    // the `__` run that sanitization just squashed out.
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().trim_end_matches('_').to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| FALLBACK_SNAPSHOT_FILENAME.to_string());
    let extension = path.extension().map(|e| e.to_string_lossy().to_string());

    for suffix in 2.. {
        let candidate = match &extension {
            Some(extension) if !extension.is_empty() => format!("{stem}_{suffix}.{extension}"),
            _ => format!("{stem}_{suffix}"),
        };
        if used.insert(candidate.clone()) {
            return candidate;
        }
    }

    unreachable!("unbounded suffix loop should always return");
}

/// Run `git <args>` in `repo_dir` and return stdout bytes. Fails when the process exits with an
/// exit code outside `allowed_exit_codes` or when it runs longer than [`GIT_COMMAND_TIMEOUT`].
///
/// The whole call is async: the `async_process` child is awaited via `Command::output()` with a
/// timeout composed on top, so no additional OS threads are spawned per git invocation and no
/// polling loop is needed. `kill_on_drop` ensures the child is reaped if the timeout elapses and
/// the future is dropped.
async fn git_output_bytes<I, S>(
    repo_dir: &Path,
    args: I,
    allowed_exit_codes: &[i32],
) -> Result<Vec<u8>>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let args = args
        .into_iter()
        .map(|arg| arg.as_ref().to_os_string())
        .collect::<Vec<_>>();
    let mut command = Command::new("git");
    command
        .args(&args)
        .current_dir(repo_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    let output = match command.output().with_timeout(GIT_COMMAND_TIMEOUT).await {
        Ok(Ok(output)) => output,
        Ok(Err(e)) => {
            return Err(anyhow::Error::new(e).context(format!(
                "Failed to run git {:?} in {}",
                args,
                repo_dir.display()
            )));
        }
        Err(_) => anyhow::bail!(
            "git {:?} timed out after {:?} in {}",
            args,
            GIT_COMMAND_TIMEOUT,
            repo_dir.display()
        ),
    };

    let status_code = output.status.code().unwrap_or(-1);
    if !allowed_exit_codes.contains(&status_code) {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git {:?} failed in {}: {stderr}", args, repo_dir.display());
    }
    Ok(output.stdout)
}

// Snapshot upload is cloud-agent-only and only ever runs inside a Linux Docker container, so
// skip the tests on Windows rather than teach every fixture to emit POSIX paths.
#[cfg(all(test, not(windows)))]
#[path = "snapshot_tests.rs"]
mod tests;
