//! Agent SDK entry points for invoking Agent-related functionality from the app.
//! For now this provides a simple runner that echoes the received command.

use std::fmt::Write;

use anyhow::Context;
pub use driver::AgentDriver;
use driver::AgentDriverError;
use tracing::Instrument as _;
use warp_cli::agent::{AgentCommand, Harness, OutputFormat, Prompt, RunAgentArgs};
use warp_cli::{CliCommand, GlobalOptions};
use warp_core::features::FeatureFlag;
use warp_logging::log_file_path;
use warpui::platform::TerminationMode;
use warpui::{AppContext, ModelSpawner, SingletonEntity};

use crate::ai::agent_sdk::driver::harness::{HarnessKind, harness_kind};
use crate::ai::agent_sdk::driver::{AgentDriverOptions, Task};
use crate::ai::agent_sdk::mcp_config::build_mcp_servers_from_specs;
use crate::ai::ambient_agents::AgentConfigSnapshot;
use crate::ai::ambient_agents::task::HarnessConfig;
use crate::ai::llms::LLMId;
use crate::cloud_object::model::persistence::CloudModel;
use crate::workflows::workflow::Workflow;

mod common;
mod config_file;
pub(crate) mod driver;
mod mcp;
mod mcp_config;
mod model;
pub mod output;
mod profiles;
pub(crate) mod setup_observability;

/// Run a Warp CLI command.
#[tracing::instrument(name = "agent_sdk::run", skip_all, err)]
pub fn run(
    ctx: &mut AppContext,
    command: CliCommand,
    global_options: GlobalOptions,
) -> anyhow::Result<()> {
    dispatch_command(ctx, command, global_options)
}

/// Dispatch a CLI command to its handler.
fn dispatch_command(
    ctx: &mut AppContext,
    command: CliCommand,
    global_options: GlobalOptions,
) -> anyhow::Result<()> {
    match command {
        CliCommand::Agent(agent_cmd) => run_agent(ctx, global_options, agent_cmd),
        CliCommand::MCP(mcp_cmd) => mcp::run(ctx, global_options, mcp_cmd),
        CliCommand::Model(model_cmd) => model::run(ctx, global_options, model_cmd),
    }
}

/// Run the agent with the provided command.
fn run_agent(
    ctx: &mut AppContext,
    global_options: GlobalOptions,
    command: AgentCommand,
) -> anyhow::Result<()> {
    match command {
        AgentCommand::Run(args) => {
            if args.harness != Harness::Oz && !FeatureFlag::AgentHarness.is_enabled() {
                return Err(anyhow::anyhow!("unexpected argument '--harness' found"));
            }
            if args.harness == Harness::OpenCode {
                return Err(anyhow::anyhow!(
                    "The opencode harness is only supported for local child agent launches."
                ));
            }

            // Start the agent driver runner, which will handle the rest of the setup steps
            // (managing both sync and async steps) as well as triggering the driver.
            let runner = ctx.add_singleton_model(|_| AgentDriverRunner);
            runner.update(ctx, move |_, ctx| {
                let spawner = ctx.spawner();
                ctx.spawn(
                    AgentDriverRunner::setup_and_run_driver(
                        spawner,
                        args,
                        global_options.output_format,
                    ),
                    |_, result, _ctx| {
                        if let Err(e) = result {
                            report_fatal_error(e.into(), _ctx);
                        }
                    },
                );
            });

            Ok(())
        }
        AgentCommand::Profile(sub) => profiles::run(ctx, global_options, sub),
    }
}

/// Build the merged agent configuration from all sources and the Task for the driver.
/// Merge precedence: file < CLI
fn build_merged_config_and_task(
    args: &RunAgentArgs,
    prompt: &Option<Prompt>,
    ctx: &mut AppContext,
) -> anyhow::Result<(AgentConfigSnapshot, Task)> {
    let loaded_file = match args.config_file.file.as_deref() {
        Some(path) => Some(config_file::load_config_file(path)?),
        None => None,
    };

    let cli_mcp_servers = build_mcp_servers_from_specs(&args.all_mcp_specs())?;

    // Merge precedence: file < CLI
    let file_merged = config_file::merge_with_precedence(loaded_file.as_ref(), Default::default());

    // When a non-Oz harness is active, --model targets the harness rather than the Oz model.
    let harness_model_id = if args.harness != Harness::Oz {
        args.model.model.clone()
    } else {
        None
    };
    let harness_override = (args.harness != Harness::Oz).then_some(HarnessConfig {
        harness_type: args.harness,
        model_id: harness_model_id,
        reasoning_level: None,
    });

    let oz_model = if args.harness == Harness::Oz {
        args.model.model.clone().or(file_merged.model_id)
    } else {
        None
    };

    let mut merged_config = AgentConfigSnapshot {
        // CLI name > file name
        name: args.name.clone().or(file_merged.name),
        environment_id: file_merged.environment_id,
        model_id: oz_model,
        base_prompt: file_merged.base_prompt,
        mcp_servers: config_file::merge_mcp_servers(file_merged.mcp_servers, cli_mcp_servers),
        profile_id: args.profile.clone(),
        worker_host: file_merged.worker_host,
        skill_spec: file_merged.skill_spec,
        harness: harness_override,
        harness_auth_secrets: None,
        additional_source_repos: None,
    };

    let runtime_mcp_specs = match merged_config.mcp_servers.as_ref() {
        Some(mcp_servers) => config_file::mcp_specs_from_mcp_servers(mcp_servers)?,
        None => Vec::new(),
    };

    let model_override: Option<LLMId> = merged_config
        .model_id
        .as_deref()
        .filter(|_| args.harness == Harness::Oz)
        .map(|model_id| common::validate_agent_mode_base_model_id(model_id, ctx))
        .transpose()?;

    // Keep the task config snapshot aligned with the effective model selection.
    merged_config.model_id = model_override.clone().map(|id| id.to_string());

    // Combine base_prompt with user prompt locally.
    let local_prompt = match (merged_config.base_prompt.as_deref(), prompt) {
        (Some(base_prompt), Some(Prompt::PlainText(user_prompt))) => {
            Prompt::PlainText(format!("{base_prompt}\n\n{user_prompt}"))
        }
        (Some(base_prompt), None) => Prompt::PlainText(base_prompt.to_string()),
        (_, Some(p)) => p.clone(),
        (None, None) => {
            return Err(anyhow::anyhow!(AgentDriverError::InvalidRuntimeState));
        }
    };

    let task = Task {
        prompt: resolve_prompt(&local_prompt, ctx)?,
        model: model_override,
        profile: args.profile.clone(),
        mcp_specs: runtime_mcp_specs,
        harness: harness_kind(args.harness)?,
    };

    Ok((merged_config, task))
}

/// Resolve a `Prompt` to a plain string.
fn resolve_prompt(prompt: &Prompt, ctx: &AppContext) -> Result<String, AgentDriverError> {
    match prompt {
        Prompt::PlainText(prompt_str) => Ok(prompt_str.to_string()),
        Prompt::SavedPrompt(workflow_id) => {
            let Some(workflow) = CloudModel::as_ref(ctx).get_workflow_by_uid(workflow_id) else {
                return Err(AgentDriverError::AIWorkflowNotFound(workflow_id.to_owned()));
            };

            let Workflow::AgentMode { query, .. } = &workflow.model().data else {
                return Err(AgentDriverError::AIWorkflowNotFound(workflow_id.to_owned()));
            };
            Ok(query.to_owned())
        }
    }
}

/// Singleton model that provides a ModelContext for spawning async operations
/// when starting the agent driver. This is needed because conversation fetching
/// requires spawning an async task, which requires a ModelContext.
struct AgentDriverRunner;

impl warpui::Entity for AgentDriverRunner {
    type Event = ();
}

impl warpui::SingletonEntity for AgentDriverRunner {}

impl AgentDriverRunner {
    #[tracing::instrument(skip_all, err, fields(args.sandboxed = args.sandboxed))]
    async fn setup_and_run_driver(
        foreground: ModelSpawner<Self>,
        args: RunAgentArgs,
        output_format: OutputFormat,
    ) -> Result<(), AgentDriverError> {
        // Build driver options and task.
        let (driver_options, task) = Self::build_driver_options_and_task(&foreground, args).await?;

        match &task.harness {
            HarnessKind::Unsupported(harness) => {
                return Err(AgentDriverError::HarnessSetupFailed {
                    harness: harness.to_string(),
                    reason: format!(
                        "The {harness} harness is only supported for local child agent launches."
                    ),
                });
            }
            HarnessKind::Oz | HarnessKind::ThirdParty(_) => {}
        }

        // Validate that the third-party harness is installed and authed.
        if let HarnessKind::ThirdParty(harness) = &task.harness {
            harness.validate()?;
        }

        // Run the driver
        foreground
            .spawn(move |_, ctx| {
                Self::create_and_run_driver(ctx, driver_options, output_format, task);
            })
            .await?;

        Ok(())
    }

    /// Build the AgentDriverOptions and Task for a fresh local run.
    async fn build_driver_options_and_task(
        foreground: &ModelSpawner<Self>,
        args: RunAgentArgs,
    ) -> Result<(AgentDriverOptions, Task), AgentDriverError> {
        // Get the working directory
        let working_dir = match args.cwd.as_ref() {
            Some(dir) => dunce::canonicalize(dir)
                .with_context(|| format!("Unable to resolve {}", dir.display())),
            None => std::env::current_dir().context("Unable to determine working directory"),
        }
        .map_err(AgentDriverError::ConfigBuildFailed)?;

        let prompt = args.prompt_arg.to_prompt();

        // Build the AgentConfigSnapshot, Task, and AgentDriverOptions
        let prompt_clone = prompt.clone();
        let (merged_config, task, driver_options) = foreground
            .spawn(move |_, ctx| -> anyhow::Result<_> {
                let (merged_config, task) =
                    build_merged_config_and_task(&args, &prompt_clone, ctx)?;

                let third_party_harness_model_config = merged_config
                    .harness
                    .as_ref()
                    .and_then(|h| h.model_config());
                let driver_options = driver::AgentDriverOptions {
                    working_dir: working_dir.clone(),
                    idle_on_complete: args.idle_on_complete.map(|d| d.into()),
                    selected_harness: args.harness,
                    third_party_harness_model_config,
                    strict_mcp_startup: args.strict_mcp_startup,
                    mcp_startup_timeout: args.mcp_startup_timeout.map(|duration| duration.into()),
                };

                Ok((merged_config, task, driver_options))
            })
            .await?
            .map_err(AgentDriverError::ConfigBuildFailed)?;

        if merged_config.environment_id.is_some() {
            log::warn!(
                "Ignoring the config file's environment_id: cloud environments are not available"
            );
        }

        Ok((driver_options, task))
    }

    /// Create the AgentDriver and start running the task.
    #[tracing::instrument(skip_all)]
    fn create_and_run_driver(
        ctx: &mut AppContext,
        driver_options: driver::AgentDriverOptions,
        output_format: OutputFormat,
        task: driver::Task,
    ) {
        // It's difficult to fallibly instantiate a UI framework model, so a driver that fails to
        // initialize panics here.
        let driver = ctx.add_singleton_model(|ctx| {
            AgentDriver::new(driver_options, ctx).expect("Could not initialize driver")
        });

        driver.update(ctx, |driver, ctx| {
            driver.set_output_format(output_format);
            let span = tracing::info_span!("AgentDriver::run", ?task.model, ?task.harness);
            let agent_future = span.in_scope(|| driver.run(task, ctx)).instrument(span);

            ctx.spawn(agent_future, |_, result, ctx| match result {
                Ok(()) => {
                    ctx.terminate_app(TerminationMode::ForceTerminate, None);
                }
                Err(err) => {
                    report_fatal_error(err.into(), ctx);
                }
            });
        });
    }
}

/// Report a fatal error and terminate the app.
fn report_fatal_error(err: anyhow::Error, ctx: &mut AppContext) {
    let mut message = err.to_string();
    for cause in err.chain().skip(1) {
        let _ = write!(&mut message, "\n=> {cause}");
    }

    tracing::event!(tracing::Level::ERROR, message);

    {
        if let Ok(path) = log_file_path() {
            let _ = write!(
                message,
                "\n\nFor more information, check Warp logs at {}",
                path.display()
            );
        }
    }

    let error = anyhow::anyhow!(message);
    ctx.terminate_app(TerminationMode::ForceTerminate, Some(Err(error)));
}
