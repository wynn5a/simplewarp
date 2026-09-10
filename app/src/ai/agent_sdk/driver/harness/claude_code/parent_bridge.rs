//! On-disk staging of lead-agent mailbox messages for Claude wake runs.
//!
//! The per-session state directory is a three-stage mailbox:
//! - `staged/` holds message records waiting to be surfaced to Claude.
//! - `surfaced/` holds records Claude has already been shown.
//! - `pending-hook-output.json` plus `pending-hook-output.ack` coordinate the
//!   handoff between Warp and the Claude hook process.
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;

use crate::ai::agent_events::{AgentMessageEventMetadata, MessageHydrator};
use crate::ai::agent_sdk::driver::OZ_MESSAGE_LISTENER_STATE_ROOT_ENV;

const LEGACY_MESSAGE_LISTENER_STATE_ROOT_ENV: &str = "OZ_PARENT_STATE_ROOT";
const PARENT_BRIDGE_DEFAULT_STATE_ROOT: &str = ".claude-code/oz-parent-bridge";
const PARENT_BRIDGE_SURFACED_DIR_NAME: &str = "surfaced";
const PARENT_BRIDGE_EVENT_CURSOR_FILE_NAME: &str = "event-cursor.json";
const PARENT_BRIDGE_HOOK_OUTPUT_FILE_NAME: &str = "pending-hook-output.json";
const PARENT_BRIDGE_HOOK_OUTPUT_ACK_FILE_NAME: &str = "pending-hook-output.ack";

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct MessageBridgeEventCursor {
    since_sequence: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(super) struct MessageBridgeMessageRecord {
    pub sequence: i64,
    pub message_id: String,
    #[serde(default)]
    pub sender_run_id: String,
    #[serde(default)]
    pub subject: String,
    #[serde(default)]
    pub body: String,
    pub occurred_at: String,
}

pub(super) fn parent_bridge_root() -> Result<PathBuf> {
    for env_name in [
        OZ_MESSAGE_LISTENER_STATE_ROOT_ENV,
        LEGACY_MESSAGE_LISTENER_STATE_ROOT_ENV,
    ] {
        if let Ok(dir) = std::env::var(env_name)
            && !dir.is_empty()
        {
            return Ok(PathBuf::from(dir));
        }
    }
    dirs::home_dir()
        .map(|home| home.join(PARENT_BRIDGE_DEFAULT_STATE_ROOT))
        .ok_or_else(|| anyhow!("could not determine home directory"))
}

fn parent_bridge_staged_dir(state_dir: &Path) -> PathBuf {
    state_dir.join("staged")
}

fn parent_bridge_surfaced_dir(state_dir: &Path) -> PathBuf {
    state_dir.join(PARENT_BRIDGE_SURFACED_DIR_NAME)
}

pub(super) fn parent_bridge_hook_output_file(state_dir: &Path) -> PathBuf {
    state_dir.join(PARENT_BRIDGE_HOOK_OUTPUT_FILE_NAME)
}

pub(super) fn parent_bridge_hook_output_ack_file(state_dir: &Path) -> PathBuf {
    state_dir.join(PARENT_BRIDGE_HOOK_OUTPUT_ACK_FILE_NAME)
}

pub(super) fn parent_bridge_event_cursor_file(state_dir: &Path) -> PathBuf {
    state_dir.join(PARENT_BRIDGE_EVENT_CURSOR_FILE_NAME)
}

fn parent_bridge_message_path(dir: &Path, sequence: i64, message_id: &str) -> PathBuf {
    dir.join(format!("{sequence:020}-{message_id}.json"))
}

pub(super) fn parent_bridge_staged_message_path(
    state_dir: &Path,
    sequence: i64,
    message_id: &str,
) -> PathBuf {
    parent_bridge_message_path(&parent_bridge_staged_dir(state_dir), sequence, message_id)
}

// Reader/staging helpers exercised by tests so the state-dir layout can't drift.
#[cfg_attr(not(test), allow(dead_code))]
pub(super) fn parent_bridge_surfaced_message_path(
    state_dir: &Path,
    sequence: i64,
    message_id: &str,
) -> PathBuf {
    parent_bridge_message_path(&parent_bridge_surfaced_dir(state_dir), sequence, message_id)
}

pub(super) fn ensure_parent_bridge_state_dir(state_dir: &Path) -> Result<()> {
    fs::create_dir_all(parent_bridge_staged_dir(state_dir))
        .with_context(|| format!("Failed to create {}", state_dir.display()))?;
    fs::create_dir_all(parent_bridge_surfaced_dir(state_dir))
        .with_context(|| format!("Failed to create {}", state_dir.display()))?;
    Ok(())
}

#[cfg_attr(not(test), allow(dead_code))]
pub(super) fn read_parent_bridge_event_cursor(state_dir: &Path) -> Result<i64> {
    let path = parent_bridge_event_cursor_file(state_dir);
    if !path.exists() {
        return Ok(0);
    }

    let cursor = serde_json::from_slice::<MessageBridgeEventCursor>(
        &fs::read(&path).with_context(|| format!("Failed to read {}", path.display()))?,
    )
    .with_context(|| format!("Failed to parse {}", path.display()))?;
    Ok(cursor.since_sequence)
}

pub(super) fn write_parent_bridge_event_cursor(state_dir: &Path, sequence: i64) -> Result<()> {
    write_parent_bridge_json_atomically(
        &parent_bridge_event_cursor_file(state_dir),
        &MessageBridgeEventCursor {
            since_sequence: sequence,
        },
    )
}

pub(super) fn stage_parent_bridge_message(
    state_dir: &Path,
    record: &MessageBridgeMessageRecord,
) -> Result<()> {
    let target = parent_bridge_staged_message_path(state_dir, record.sequence, &record.message_id);
    if !target.exists() {
        write_parent_bridge_json_atomically(&target, record)?;
    }
    Ok(())
}

pub(super) async fn prime_parent_bridge_staged_for_self_managed_wake(
    hydrator: &MessageHydrator,
    state_dir: &Path,
    wake_message: Option<&AgentMessageEventMetadata>,
) -> Result<()> {
    remove_file_if_exists(&parent_bridge_hook_output_file(state_dir))?;
    remove_file_if_exists(&parent_bridge_hook_output_ack_file(state_dir))?;
    move_parent_bridge_surfaced_messages_to_staged(state_dir)?;

    let Some(wake_message) = wake_message else {
        return Ok(());
    };

    let record = hydrate_parent_bridge_message_record(
        hydrator,
        &MessageBridgeMessageRecord {
            sequence: wake_message.sequence,
            message_id: wake_message.message_id.clone(),
            sender_run_id: String::new(),
            subject: String::new(),
            body: String::new(),
            occurred_at: wake_message.occurred_at.clone(),
        },
    )
    .await?;
    stage_parent_bridge_message(state_dir, &record)?;
    write_parent_bridge_event_cursor(state_dir, wake_message.sequence)
}

fn move_parent_bridge_surfaced_messages_to_staged(state_dir: &Path) -> Result<()> {
    let surfaced_records = parent_bridge_message_records(&parent_bridge_surfaced_dir(state_dir))?;
    for (path, record) in surfaced_records {
        let target =
            parent_bridge_staged_message_path(state_dir, record.sequence, &record.message_id);
        if target.exists() {
            write_parent_bridge_json_atomically(&target, &record)?;
            remove_file_if_exists(&path)?;
        } else {
            fs::rename(&path, &target).with_context(|| {
                format!(
                    "Failed to move message bridge record {} back to {}",
                    path.display(),
                    target.display()
                )
            })?;
        }
    }
    Ok(())
}

fn parent_bridge_sorted_message_paths(dir: &Path) -> Result<Vec<PathBuf>> {
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut paths = fs::read_dir(dir)
        .with_context(|| format!("Failed to read {}", dir.display()))?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("json"))
        .collect::<Vec<_>>();
    paths.sort();
    Ok(paths)
}

fn parent_bridge_message_records(dir: &Path) -> Result<Vec<(PathBuf, MessageBridgeMessageRecord)>> {
    parent_bridge_sorted_message_paths(dir)?
        .into_iter()
        .map(|path| {
            let record = serde_json::from_slice::<MessageBridgeMessageRecord>(
                &fs::read(&path).with_context(|| format!("Failed to read {}", path.display()))?,
            )
            .with_context(|| format!("Failed to parse {}", path.display()))?;
            Ok((path, record))
        })
        .collect()
}

fn remove_file_if_exists(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => {
            Err(anyhow::Error::from(err).context(format!("Failed to remove {}", path.display())))
        }
    }
}

async fn hydrate_parent_bridge_message_record(
    hydrator: &MessageHydrator,
    record: &MessageBridgeMessageRecord,
) -> Result<MessageBridgeMessageRecord> {
    if !record.sender_run_id.is_empty() {
        return Ok(record.clone());
    }

    let message = hydrator
        .read_message_with_timeout(&record.message_id)
        .await
        .with_context(|| format!("Failed to read lead-agent message {}", record.message_id))?;
    Ok(MessageBridgeMessageRecord {
        sequence: record.sequence,
        message_id: message.message_id,
        sender_run_id: message.sender_run_id,
        subject: message.subject,
        body: message.body,
        occurred_at: record.occurred_at.clone(),
    })
}

fn write_parent_bridge_json_atomically<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    write_parent_bridge_bytes_atomically(path, &serde_json::to_vec(value)?)
}

fn write_parent_bridge_bytes_atomically(path: &Path, bytes: &[u8]) -> Result<()> {
    let Some(parent) = path.parent() else {
        return Err(anyhow!("{} has no parent directory", path.display()));
    };
    fs::create_dir_all(parent).with_context(|| format!("Failed to create {}", parent.display()))?;

    let prefix = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("parent-bridge");
    let mut temp_file = NamedTempFile::new_in(parent)
        .with_context(|| format!("Failed to create temp file for {}", path.display()))?;
    temp_file
        .write_all(bytes)
        .with_context(|| format!("Failed to write temp file for {}", path.display()))?;
    temp_file
        .flush()
        .with_context(|| format!("Failed to flush temp file for {}", path.display()))?;
    temp_file
        .persist(path)
        .map(|_| ())
        .map_err(|err| {
            anyhow::Error::from(err.error).context(format!("Failed to write {}", path.display()))
        })
        .with_context(|| format!("Failed to persist temporary {prefix} file"))?;
    Ok(())
}
