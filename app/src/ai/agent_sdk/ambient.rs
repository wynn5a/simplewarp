//! Commands to interact with ambient agents on Warp's platform.
use warp_cli::agent::OutputFormat;
use warp_cli::json_filter::JsonOutput;
use warp_cli::task::{
    ArtifactTypeArg, ExecutionLocationArg, ListTasksArgs, RunSortByArg, RunSourceArg, RunStateArg,
    TaskGetArgs,
};
use warp_cli::{GlobalOptions, SortOrderArg};
use warp_core::channel::ChannelState;
use warpui::r#async::Spawnable;
use warpui::platform::TerminationMode;
use warpui::{AppContext, ModelContext, SingletonEntity};

use crate::ServerApiProvider;
use crate::ai::ambient_agents::spawn::SessionJoinInfo;
use crate::ai::ambient_agents::{AmbientAgentTask, AmbientAgentTaskState};
use crate::ai::artifacts::Artifact;
use crate::server::server_api::ai::{
    AgentSource, ArtifactType, ExecutionLocation, RunSortBy, RunSortOrder, TaskListFilter,
};
use crate::util::time_format::format_approx_duration_from_now_utc;

const MAX_LINE_WIDTH: usize = 90;

/// Singleton model that runs async work for ambient agent CLI commands.
struct AmbientAgentRunner;

/// List ambient agent tasks.
pub fn list_ambient_agent_tasks(
    ctx: &mut AppContext,
    global_options: GlobalOptions,
    args: ListTasksArgs,
) -> anyhow::Result<()> {
    let runner = ctx.add_singleton_model(|_ctx| AmbientAgentRunner);
    let filter = filter_from_args(&args);
    let json_output = args.json_output.clone();
    let output_format = global_options.output_format;
    runner.update(ctx, |runner, ctx| {
        runner.list_tasks(args.limit, filter, output_format, json_output, ctx)
    })
}

/// Get status of a specific ambient agent task.
pub fn get_ambient_agent_task_status(
    ctx: &mut AppContext,
    global_options: GlobalOptions,
    args: TaskGetArgs,
) -> anyhow::Result<()> {
    let runner = ctx.add_singleton_model(|_ctx| AmbientAgentRunner);
    let output_format = global_options.output_format;
    runner.update(ctx, |runner, ctx| {
        runner.get_task_status(args, output_format, ctx)
    })
}

/// Translate CLI-level `ListTasksArgs` into the server-facing `TaskListFilter`.
pub(super) fn filter_from_args(args: &ListTasksArgs) -> TaskListFilter {
    let states = if args.state.is_empty() {
        None
    } else {
        Some(
            args.state
                .iter()
                .map(|s| run_state_from_arg(*s))
                .collect::<Vec<_>>(),
        )
    };

    TaskListFilter {
        creator_uid: args.creator.clone(),
        updated_after: args.updated_after,
        created_after: args.created_after,
        created_before: args.created_before,
        states,
        source: args.source.map(run_source_from_arg),
        execution_location: args.execution_location.map(execution_location_from_arg),
        environment_id: args.environment.clone(),
        skill_spec: args.skill.clone(),
        schedule_id: args.schedule.clone(),
        ancestor_run_id: args.ancestor_run.clone(),
        config_name: args.name.clone(),
        model_id: args.model.clone(),
        artifact_type: args.artifact_type.map(artifact_type_from_arg),
        search_query: args.query.clone(),
        sort_by: args.sort_by.map(sort_by_from_arg),
        sort_order: args.sort_order.map(sort_order_from_arg),
        cursor: args.cursor.clone(),
    }
}

fn run_state_from_arg(arg: RunStateArg) -> AmbientAgentTaskState {
    match arg {
        RunStateArg::Queued => AmbientAgentTaskState::Queued,
        RunStateArg::Pending => AmbientAgentTaskState::Pending,
        RunStateArg::Claimed => AmbientAgentTaskState::Claimed,
        RunStateArg::InProgress => AmbientAgentTaskState::InProgress,
        RunStateArg::Succeeded => AmbientAgentTaskState::Succeeded,
        RunStateArg::Failed => AmbientAgentTaskState::Failed,
        RunStateArg::Error => AmbientAgentTaskState::Error,
        RunStateArg::Blocked => AmbientAgentTaskState::Blocked,
        RunStateArg::Cancelled => AmbientAgentTaskState::Cancelled,
    }
}

fn run_source_from_arg(arg: RunSourceArg) -> AgentSource {
    match arg {
        RunSourceArg::Api => AgentSource::AgentWebhook,
        RunSourceArg::Cli => AgentSource::Cli,
        RunSourceArg::Slack => AgentSource::Slack,
        RunSourceArg::Linear => AgentSource::Linear,
        RunSourceArg::ScheduledAgent => AgentSource::ScheduledAgent,
        RunSourceArg::WebApp => AgentSource::WebApp,
        RunSourceArg::CloudMode => AgentSource::CloudMode,
        RunSourceArg::GitHubAction => AgentSource::GitHubAction,
        RunSourceArg::Interactive => AgentSource::Interactive,
    }
}

fn execution_location_from_arg(arg: ExecutionLocationArg) -> ExecutionLocation {
    match arg {
        ExecutionLocationArg::Local => ExecutionLocation::Local,
        ExecutionLocationArg::Remote => ExecutionLocation::Remote,
    }
}

fn artifact_type_from_arg(arg: ArtifactTypeArg) -> ArtifactType {
    match arg {
        ArtifactTypeArg::Plan => ArtifactType::Plan,
        ArtifactTypeArg::PullRequest => ArtifactType::PullRequest,
        ArtifactTypeArg::Screenshot => ArtifactType::Screenshot,
        ArtifactTypeArg::File => ArtifactType::File,
    }
}

fn sort_by_from_arg(arg: RunSortByArg) -> RunSortBy {
    match arg {
        RunSortByArg::UpdatedAt => RunSortBy::UpdatedAt,
        RunSortByArg::CreatedAt => RunSortBy::CreatedAt,
        RunSortByArg::Title => RunSortBy::Title,
        RunSortByArg::Agent => RunSortBy::Agent,
    }
}

fn sort_order_from_arg(arg: SortOrderArg) -> RunSortOrder {
    match arg {
        SortOrderArg::Asc => RunSortOrder::Asc,
        SortOrderArg::Desc => RunSortOrder::Desc,
    }
}

impl AmbientAgentRunner {
    fn spawn_command(
        &self,
        future: impl Spawnable<Output = anyhow::Result<()>>,
        ctx: &mut ModelContext<Self>,
    ) {
        ctx.spawn(future, |_, result, ctx| match result {
            Ok(()) => {
                ctx.terminate_app(TerminationMode::ForceTerminate, None);
            }
            Err(err) => {
                super::report_fatal_error(err, ctx);
            }
        });
    }
    fn list_tasks(
        &self,
        limit: i32,
        filter: TaskListFilter,
        output_format: OutputFormat,
        json_output: JsonOutput,
        ctx: &mut ModelContext<Self>,
    ) -> anyhow::Result<()> {
        let ai_client = ServerApiProvider::as_ref(ctx).get_ai_client();

        let list_future = async move {
            if matches!(output_format, OutputFormat::Json) || json_output.force_json_output() {
                let response = ai_client.list_agent_runs_raw(limit, filter).await?;
                super::output::print_raw_json(response, &json_output)?;
            } else if matches!(output_format, OutputFormat::Ndjson) {
                let tasks = ai_client.list_ambient_agent_tasks(limit, filter).await?;
                for task in tasks {
                    super::output::write_json_line(&task, std::io::stdout())?;
                }
            } else {
                let tasks = ai_client.list_ambient_agent_tasks(limit, filter).await?;
                Self::print_tasks_table(&tasks);
            }
            Ok(())
        };
        self.spawn_command(list_future, ctx);

        Ok(())
    }

    fn get_task_status(
        &self,
        args: TaskGetArgs,
        output_format: OutputFormat,
        ctx: &mut ModelContext<Self>,
    ) -> anyhow::Result<()> {
        let ai_client = ServerApiProvider::as_ref(ctx).get_ai_client();

        let status_future = async move {
            let task_id = args.task_id.parse()?;
            let json_output = args.json_output;
            if matches!(output_format, OutputFormat::Json) || json_output.force_json_output() {
                let response = ai_client.get_agent_run_raw(&task_id).await?;
                super::output::print_raw_json(response, &json_output)?;
            } else if matches!(output_format, OutputFormat::Ndjson) {
                let task = ai_client.get_ambient_agent_task(&task_id).await?;
                super::output::write_json_line(&task, std::io::stdout())?;
            } else {
                let task = ai_client.get_ambient_agent_task(&task_id).await?;
                Self::print_tasks_table(&[task]);
            }
            Ok(())
        };
        self.spawn_command(status_future, ctx);

        Ok(())
    }

    /// Get the appropriate emoji for a task state.
    fn get_state_emoji(state: &AmbientAgentTaskState) -> &'static str {
        match state {
            AmbientAgentTaskState::Queued | AmbientAgentTaskState::Pending => "⏳",
            AmbientAgentTaskState::Claimed => "🔄",
            AmbientAgentTaskState::InProgress => "🔄",
            AmbientAgentTaskState::Succeeded => "✅",
            AmbientAgentTaskState::Failed
            | AmbientAgentTaskState::Error
            | AmbientAgentTaskState::Unknown => "❌",
            AmbientAgentTaskState::Blocked => "🛑",
            AmbientAgentTaskState::Cancelled => "🚫",
        }
    }

    /// Print runs in a beautifully formatted ASCII table with card-style layout.
    fn print_tasks_table(tasks: &[AmbientAgentTask]) {
        if tasks.is_empty() {
            println!("No runs found.");
            return;
        }

        if tasks.len() == 1 {
            println!("\nAgent Run:");
        } else {
            println!("\nAgent Runs ({}):", tasks.len());
        }

        let oz_root_url = ChannelState::oz_root_url();
        for task in tasks {
            let state_emoji = Self::get_state_emoji(&task.state);

            // Create a single-column table for each run (card-style)
            let mut table = crate::ai::agent_sdk::output::standard_table();

            // Run header with emoji and ID
            let header = format!("{} {} ({:?})", state_emoji, task.task_id, task.state);
            table.add_row(vec![header]);

            // Oz webapp link
            table.add_row(vec![format!("Oz: {oz_root_url}/runs/{}", task.task_id)]);

            // Title (wrapped, single cell)
            if !task.title.is_empty() {
                let title_cell = crate::ai::agent_sdk::text_layout::render_labeled_wrapped_field(
                    "Title",
                    &task.title,
                    MAX_LINE_WIDTH,
                );
                table.add_row(vec![title_cell]);
            }

            if let Some(executor) = task.executor_display_name() {
                table.add_row(vec![format!("Executed as: {executor}")]);
            }

            // Agent config snapshot (if available)
            if let Some(config) = task.agent_config_snapshot.as_ref() {
                let config_str =
                    serde_json::to_string_pretty(config).unwrap_or_else(|_| format!("{config:?}"));
                table.add_row(vec![format!("Config:\n{config_str}")]);
            }

            // Created time
            let created_formatted = format_approx_duration_from_now_utc(task.created_at);
            table.add_row(vec![format!("Created: {}", created_formatted)]);

            // Status message (if available) - single multi-line cell
            if let Some(status_msg) = &task.status_message {
                let status_cell = crate::ai::agent_sdk::text_layout::render_labeled_wrapped_field(
                    "Status",
                    &status_msg.message,
                    MAX_LINE_WIDTH,
                );
                table.add_row(vec![status_cell]);
            }

            // Artifacts (if available)
            if !task.artifacts.is_empty() {
                let artifacts_cell = Self::format_artifacts(&task.artifacts);
                table.add_row(vec![artifacts_cell]);
            }

            // Session link (if available)
            if let Some(session_join_info) = SessionJoinInfo::from_task(task) {
                table.add_row(vec![format!("Session: {}", session_join_info.session_link)]);
            }

            println!("{table}");
        }
    }

    /// Format artifacts for display.
    fn format_artifacts(artifacts: &[Artifact]) -> String {
        let mut lines = vec!["Artifacts:".to_string()];

        for artifact in artifacts {
            match artifact {
                Artifact::PullRequest {
                    url,
                    branch,
                    repo,
                    number,
                    ..
                } => {
                    let pr_display = match (repo, number) {
                        (Some(repo), Some(num)) => format!("  PR: {} #{}", repo, num),
                        _ => "  PR:".to_string(),
                    };
                    lines.push(pr_display);
                    lines.push(format!("    Branch: {}", branch));
                    lines.push(format!("    Link: {}", url));
                }
                Artifact::Plan {
                    notebook_uid,
                    title,
                    ..
                } => {
                    let plan_title = title.as_deref().unwrap_or("Untitled Plan");
                    lines.push(format!("  Plan: {}", plan_title));
                    if let Some(id) = notebook_uid {
                        lines.push(format!(
                            "    Link: {}/drive/notebook/{}",
                            ChannelState::server_root_url(),
                            id
                        ));
                    }
                }
                Artifact::Screenshot {
                    artifact_uid,
                    description,
                    ..
                } => {
                    let desc = description.as_deref().unwrap_or("No description");
                    lines.push(format!("  Screenshot: {} ({})", artifact_uid, desc));
                }
                Artifact::File {
                    filename,
                    filepath,
                    description,
                    ..
                } => {
                    let label = super::super::artifacts::file_button_label(filename, filepath);
                    lines.push(format!("  File: {}", label));
                    lines.push(format!("    Path: {}", filepath));
                    if let Some(description) = description {
                        lines.push(format!("    Description: {}", description));
                    }
                }
                Artifact::ExternalReference {
                    reference_type,
                    url,
                    title,
                    metadata,
                } => {
                    let title_str = title.as_deref().unwrap_or("Untitled reference");
                    lines.push(format!("  {reference_type}: {title_str}"));
                    lines.push(format!("    Link: {url}"));
                    if let Some(metadata) = metadata {
                        lines.push(format!("    Metadata: {metadata}"));
                    }
                }
            }
        }

        lines.join("\n")
    }
}

impl warpui::Entity for AmbientAgentRunner {
    type Event = ();
}

impl SingletonEntity for AmbientAgentRunner {}

#[cfg(test)]
#[path = "ambient_tests.rs"]
mod tests;
