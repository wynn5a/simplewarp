use comfy_table::Cell;
use serde::Serialize;
use warp_cli::GlobalOptions;
use warp_cli::agent::AgentProfileCommand;
use warpui::{AppContext, ModelContext, SingletonEntity};

use crate::ai::agent_sdk::output::{self, TableFormat};
use crate::ai::execution_profiles::profiles::AIExecutionProfilesModel;
use crate::cloud_object::model::generic_string_model::StringModel;

/// Handle Agent Profile-related CLI commands.
pub fn run(
    ctx: &mut AppContext,
    global_options: GlobalOptions,
    command: AgentProfileCommand,
) -> anyhow::Result<()> {
    let runner = ctx.add_singleton_model(|_ctx| ProfilesCommandRunner);
    match command {
        AgentProfileCommand::List => {
            runner.update(ctx, |runner, ctx| runner.list(global_options, ctx));
            Ok(())
        }
    }
}

/// Singleton model that runs async work for profile CLI commands.
struct ProfilesCommandRunner;

impl ProfilesCommandRunner {
    fn list(&self, global_options: GlobalOptions, ctx: &mut ModelContext<Self>) {
        let profiles: Vec<_> = AIExecutionProfilesModel::as_ref(ctx)
            .local_profiles(ctx)
            .profiles()
            .map(|(id, profile)| ProfileInfo {
                id: id.to_string(),
                name: profile.display_name(),
            })
            .collect();

        output::print_list(profiles, global_options.output_format);

        ctx.terminate_app(warpui::platform::TerminationMode::ForceTerminate, None);
    }
}

impl warpui::Entity for ProfilesCommandRunner {
    type Event = ();
}
impl SingletonEntity for ProfilesCommandRunner {}

/// Profile information that's shown in the `list` command.
#[derive(Serialize)]
struct ProfileInfo {
    id: String,
    name: String,
}

impl TableFormat for ProfileInfo {
    fn header() -> Vec<Cell> {
        vec![Cell::new("ID"), Cell::new("Name")]
    }

    fn row(&self) -> Vec<Cell> {
        vec![Cell::new(&self.id), Cell::new(&self.name)]
    }
}
