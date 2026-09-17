use chrono::{DateTime, Utc};
use clap::{Args, Subcommand, ValueEnum};

use crate::SortOrderArg;
use crate::date_time::parse_rfc3339;
use crate::json_filter::JsonOutput;

/// Task-related subcommands.
#[derive(Debug, Clone, Subcommand)]
pub enum TaskCommand {
    /// List ambient agent tasks.
    List(Box<ListTasksArgs>),
    /// Get status of a specific ambient agent task.
    Get(TaskGetArgs),
}

impl TaskCommand {
    pub(crate) fn as_str_for_tracing(&self) -> &'static str {
        match self {
            TaskCommand::List(_) => "run list",
            TaskCommand::Get(_) => "run get",
        }
    }
}

#[derive(Debug, Clone, Args)]
pub struct ListTasksArgs {
    /// Maximum number of tasks to return (default: 10).
    #[arg(short = 'L', long = "limit", default_value = "10")]
    pub limit: i32,

    /// Filter by run state. Repeat the flag to match any of multiple states.
    #[arg(long = "state", value_enum, value_name = "STATE")]
    pub state: Vec<RunStateArg>,

    /// Filter by run source.
    #[arg(long = "source", value_enum, value_name = "SOURCE")]
    pub source: Option<RunSourceArg>,

    /// Filter by where the run executed.
    #[arg(long = "execution-location", value_enum, value_name = "LOC")]
    pub execution_location: Option<ExecutionLocationArg>,

    /// Filter by creator ID.
    #[arg(long = "creator", value_name = "UID")]
    pub creator: Option<String>,

    /// Filter by environment ID.
    #[arg(long = "environment", value_name = "ENV_ID")]
    pub environment: Option<String>,

    /// Filter by skill (e.g. `owner/repo:path/to/SKILL.md`).
    #[arg(long = "skill", value_name = "SKILL")]
    pub skill: Option<String>,

    /// Filter to runs created by a specific scheduled agent.
    #[arg(long = "schedule", value_name = "SCHEDULE_ID")]
    pub schedule: Option<String>,

    /// Filter to descendants of a specific run.
    #[arg(long = "ancestor-run", value_name = "RUN_ID")]
    pub ancestor_run: Option<String>,

    /// Filter by agent config name.
    #[arg(long = "name", value_name = "NAME")]
    pub name: Option<String>,

    /// Filter by model ID.
    #[arg(long = "model", value_name = "MODEL_ID")]
    pub model: Option<String>,

    /// Filter by produced artifact type.
    #[arg(long = "artifact-type", value_enum, value_name = "TYPE")]
    pub artifact_type: Option<ArtifactTypeArg>,

    /// Only include runs created after the given timestamp.
    #[arg(long = "created-after", value_name = "RFC3339", value_parser = parse_rfc3339)]
    pub created_after: Option<DateTime<Utc>>,

    /// Only include runs created before the given timestamp.
    #[arg(long = "created-before", value_name = "RFC3339", value_parser = parse_rfc3339)]
    pub created_before: Option<DateTime<Utc>>,

    /// Only include runs updated after the given timestamp.
    #[arg(long = "updated-after", value_name = "RFC3339", value_parser = parse_rfc3339)]
    pub updated_after: Option<DateTime<Utc>>,

    /// Fuzzy search across run title, prompt, and skill spec.
    #[arg(short = 'q', long = "query", value_name = "TEXT")]
    pub query: Option<String>,

    /// Sort field.
    #[arg(long = "sort-by", value_enum, value_name = "FIELD")]
    pub sort_by: Option<RunSortByArg>,

    /// Sort direction.
    #[arg(long = "sort-order", value_enum, value_name = "DIR")]
    pub sort_order: Option<SortOrderArg>,

    /// Opaque pagination cursor from a previous list response.
    ///
    /// When using `--cursor`, `--sort-by` and `--sort-order` must match the
    /// values used to obtain the cursor.
    #[arg(long = "cursor", value_name = "CURSOR")]
    pub cursor: Option<String>,

    /// JSON formatting configuration.
    #[command(flatten)]
    pub json_output: JsonOutput,
}

/// Run state values accepted by `--state`. Repeatable; multiple values match any of them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum RunStateArg {
    #[value(name = "queued")]
    Queued,
    #[value(name = "pending")]
    Pending,
    #[value(name = "claimed")]
    Claimed,
    #[value(name = "in-progress")]
    InProgress,
    #[value(name = "succeeded")]
    Succeeded,
    #[value(name = "failed")]
    Failed,
    #[value(name = "error")]
    Error,
    #[value(name = "blocked")]
    Blocked,
    #[value(name = "cancelled")]
    Cancelled,
}

/// Run source values accepted by `--source`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum RunSourceArg {
    #[value(name = "api")]
    Api,
    #[value(name = "cli")]
    Cli,
    #[value(name = "slack")]
    Slack,
    #[value(name = "linear")]
    Linear,
    #[value(name = "scheduled-agent")]
    ScheduledAgent,
    #[value(name = "web-app")]
    WebApp,
    #[value(name = "cloud-mode")]
    CloudMode,
    #[value(name = "github-action")]
    GitHubAction,
    #[value(name = "interactive")]
    Interactive,
}

/// Execution-location values accepted by `--execution-location`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ExecutionLocationArg {
    #[value(name = "local")]
    Local,
    #[value(name = "remote")]
    Remote,
}

/// Artifact-type values accepted by `--artifact-type`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ArtifactTypeArg {
    #[value(name = "plan")]
    Plan,
    #[value(name = "pull-request")]
    PullRequest,
    #[value(name = "screenshot")]
    Screenshot,
    #[value(name = "file")]
    File,
}

/// Sort-by values accepted by `--sort-by`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum RunSortByArg {
    #[value(name = "updated-at")]
    UpdatedAt,
    #[value(name = "created-at")]
    CreatedAt,
    #[value(name = "title")]
    Title,
    #[value(name = "agent")]
    Agent,
}

#[derive(Debug, Clone, Args)]
pub struct TaskGetArgs {
    /// The task ID to get status for.
    pub task_id: String,

    /// JSON formatting configuration.
    #[command(flatten)]
    pub json_output: JsonOutput,
}

#[cfg(test)]
#[path = "task_tests.rs"]
mod tests;
