use serde_json::{Value, json};
use strum_macros::{EnumDiscriminants, EnumIter};
use warp_core::telemetry::{EnablementState, TelemetryEvent, TelemetryEventDesc};

#[derive(Debug, EnumDiscriminants)]
#[strum_discriminants(derive(EnumIter))]
pub(super) enum CliTelemetryEvent {
    /// Executing `warp agent run`
    AgentRun {
        gui: bool,
        requested_mcp_servers: usize,
        has_environment: bool,
        /// Which execution harness was selected (e.g. "oz", "claude").
        harness: String,
    },
    /// Executing `warp agent profile list`
    AgentProfileList,
    /// Executing `warp mcp list`
    MCPList,
    /// Executing `warp model list`
    ModelList,
    /// Executing `warp logout`
    Logout,
    /// Executing `warp whoami`
    Whoami,
    /// Executing `warp provider setup`
    ProviderSetup,
    /// Executing `warp provider list`
    ProviderList,
}

impl TelemetryEvent for CliTelemetryEvent {
    fn name(&self) -> &'static str {
        CliTelemetryEventDiscriminants::from(self).name()
    }

    fn payload(&self) -> Option<Value> {
        match self {
            CliTelemetryEvent::AgentRun {
                gui,
                requested_mcp_servers,
                has_environment,
                harness,
            } => Some(json!({
                "gui": gui,
                "requested_mcp_servers": requested_mcp_servers,
                "has_environment": has_environment,
                "harness": harness,
            })),
            CliTelemetryEvent::AgentProfileList => None,
            CliTelemetryEvent::MCPList => None,
            CliTelemetryEvent::ModelList => None,
            CliTelemetryEvent::Logout => None,
            CliTelemetryEvent::Whoami => None,
            CliTelemetryEvent::ProviderSetup => None,
            CliTelemetryEvent::ProviderList => None,
        }
    }

    fn description(&self) -> &'static str {
        CliTelemetryEventDiscriminants::from(self).description()
    }

    fn enablement_state(&self) -> EnablementState {
        CliTelemetryEventDiscriminants::from(self).enablement_state()
    }

    fn contains_ugc(&self) -> bool {
        false
    }

    fn event_descs() -> impl Iterator<Item = Box<dyn TelemetryEventDesc>> {
        warp_core::telemetry::enum_events::<Self>()
    }
}

impl TelemetryEventDesc for CliTelemetryEventDiscriminants {
    fn name(&self) -> &'static str {
        match self {
            CliTelemetryEventDiscriminants::AgentRun => "CLI.Execute.Agent.Run",
            CliTelemetryEventDiscriminants::AgentProfileList => "CLI.Execute.Agent.Profile.List",
            CliTelemetryEventDiscriminants::MCPList => "CLI.Execute.MCP.List",
            CliTelemetryEventDiscriminants::ModelList => "CLI.Execute.Model.List",
            CliTelemetryEventDiscriminants::Logout => "CLI.Execute.Logout",
            CliTelemetryEventDiscriminants::Whoami => "CLI.Execute.Whoami",
            CliTelemetryEventDiscriminants::ProviderSetup => "CLI.Execute.Provider.Setup",
            CliTelemetryEventDiscriminants::ProviderList => "CLI.Execute.Provider.List",
        }
    }

    fn description(&self) -> &'static str {
        match self {
            CliTelemetryEventDiscriminants::AgentRun => "Ran an agent from the Warp CLI",
            CliTelemetryEventDiscriminants::AgentProfileList => {
                "Listed agent profiles from the Warp CLI"
            }
            CliTelemetryEventDiscriminants::MCPList => "Listed MCP servers from the Warp CLI",
            CliTelemetryEventDiscriminants::ModelList => "Listed models from the Warp CLI",
            CliTelemetryEventDiscriminants::Logout => "Logged out via the Warp CLI",
            CliTelemetryEventDiscriminants::Whoami => "Printed current user info from the Warp CLI",
            CliTelemetryEventDiscriminants::ProviderSetup => "Set up a provider via the Warp CLI",
            CliTelemetryEventDiscriminants::ProviderList => "Listed providers from the Warp CLI",
        }
    }

    fn enablement_state(&self) -> EnablementState {
        let _ = self;
        EnablementState::Always
    }
}

warp_core::register_telemetry_event!(CliTelemetryEvent);
