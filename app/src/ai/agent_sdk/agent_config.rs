//! Commands to interact with available agents via the public API.

use warp_cli::agent::ListAgentSkillsArgs;
use warpui::platform::TerminationMode;
use warpui::{AppContext, ModelContext, SingletonEntity};

use crate::ai::cloud_environments::GithubRepo;
use crate::server::server_api::ServerApiProvider;
use crate::server::server_api::ai::AgentSkillItem;

const MAX_LINE_WIDTH: usize = 90;

/// Singleton model that runs async work for agent CLI commands.
struct AgentConfigRunner;

/// List all available agent skills.
pub fn list_skills(ctx: &mut AppContext, args: ListAgentSkillsArgs) -> anyhow::Result<()> {
    let runner = ctx.add_singleton_model(|_ctx| AgentConfigRunner);
    runner.update(ctx, |runner, ctx| runner.list(args.repo.clone(), ctx))
}

/// Parse a repo spec string (owner/repo or GitHub URL) into a GithubRepo.
fn parse_repo_spec(spec: &str) -> anyhow::Result<GithubRepo> {
    let spec = spec.trim();

    // Try URL format: https://github.com/owner/repo or https://github.com/owner/repo.git
    if spec.starts_with("https://github.com/") || spec.starts_with("http://github.com/") {
        let path = spec
            .trim_start_matches("https://github.com/")
            .trim_start_matches("http://github.com/")
            .trim_end_matches(".git")
            .trim_end_matches('/');

        let parts: Vec<&str> = path.split('/').collect();
        if parts.len() >= 2 && !parts[0].is_empty() && !parts[1].is_empty() {
            return Ok(GithubRepo::new(parts[0].to_string(), parts[1].to_string()));
        }
    }

    // Try slug format: owner/repo
    let parts: Vec<&str> = spec.split('/').collect();
    if parts.len() == 2 && !parts[0].is_empty() && !parts[1].is_empty() {
        return Ok(GithubRepo::new(parts[0].to_string(), parts[1].to_string()));
    }

    Err(anyhow::anyhow!(
        "Invalid repo format: '{}'. Expected 'owner/repo' or 'https://github.com/owner/repo'",
        spec
    ))
}

impl AgentConfigRunner {
    fn list(&self, repo: Option<String>, ctx: &mut ModelContext<Self>) -> anyhow::Result<()> {
        // A named repo used to be checked against the user's GitHub App installation first,
        // through the server. There is no server here, so the spec is only validated for shape
        // and the listing proceeds; it will report on its own if it cannot reach the repo.
        if let Some(ref repo_spec) = repo {
            parse_repo_spec(repo_spec)?;
        }
        self.fetch_and_display_agents(repo, ctx);
        Ok(())
    }

    fn fetch_and_display_agents(&self, repo: Option<String>, ctx: &mut ModelContext<Self>) {
        let ai_client = ServerApiProvider::handle(ctx).as_ref(ctx).get_ai_client();

        if repo.is_some() {
            println!("Fetching agent skills from the specified repository...");
        } else {
            println!("Fetching agent skills from your Warp environments...");
        }

        let list_future = async move { ai_client.list_skills(repo).await };

        ctx.spawn(list_future, |_, result, ctx| match result {
            Ok(agents) => {
                Self::print_agents_table(&agents);
                ctx.terminate_app(TerminationMode::ForceTerminate, None);
            }
            Err(err) => {
                super::report_fatal_error(err, ctx);
            }
        });
    }

    /// Print a list of agents in a card-style format.
    fn print_agents_table(agents: &[AgentSkillItem]) {
        if agents.is_empty() {
            println!("No skills found.");
            return;
        }

        if agents.len() == 1 {
            println!("\nAgent:");
        } else {
            println!("\nAgents ({}):", agents.len());
        }

        for agent in agents {
            println!("\n{}", agent.name);

            for variant in &agent.variants {
                let mut table = super::output::standard_table();

                // ID
                table.add_row(vec![format!("ID: {}", variant.id)]);

                // Description
                if !variant.description.is_empty() {
                    let description_cell = super::text_layout::render_labeled_wrapped_field(
                        "Description",
                        &variant.description,
                        MAX_LINE_WIDTH,
                    );
                    table.add_row(vec![description_cell]);
                }

                // Base prompt (truncated)
                if !variant.base_prompt.is_empty() {
                    let mut chars = variant.base_prompt.chars();
                    let truncated: String = chars.by_ref().take(100).collect();
                    let truncated_prompt = if chars.next().is_some() {
                        format!("{truncated}...")
                    } else {
                        truncated
                    };
                    let prompt_cell = super::text_layout::render_labeled_wrapped_field(
                        "Base Prompt",
                        &truncated_prompt,
                        MAX_LINE_WIDTH,
                    );
                    table.add_row(vec![prompt_cell]);
                }

                // Source
                table.add_row(vec![format!(
                    "Source: {}/{}",
                    variant.source.owner, variant.source.name
                )]);

                // Environments
                if !variant.environments.is_empty() {
                    let env_entries: Vec<_> = variant
                        .environments
                        .iter()
                        .map(|e| format!("{} ({})", e.name, e.uid))
                        .collect();
                    table.add_row(vec![format!("Environments: {}", env_entries.join(", "))]);
                }

                println!("{table}");
            }
        }
    }
}

impl warpui::Entity for AgentConfigRunner {
    type Event = ();
}

impl SingletonEntity for AgentConfigRunner {}
