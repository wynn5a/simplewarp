use std::future::Future;

use futures::FutureExt as _;
use tracing::Instrument as _;

#[derive(Clone)]
pub(crate) struct SetupClientEventReporter;

impl SetupClientEventReporter {
    /// Constructs a reporter for setup paths. Previously this also carried the
    /// Oz run id and API client for posting setup metrics to the server; the
    /// only remaining job is timing setup steps into local tracing spans.
    pub(crate) fn new() -> Self {
        Self
    }

    pub(crate) async fn record_result<T, E: std::error::Error>(
        &self,
        step: SetupStep,
        future: impl Future<Output = Result<T, E>>,
    ) -> Result<T, E> {
        let (_, span) = step.to_event_name_and_span();

        future
            .map(|result| {
                result.inspect_err(|err| {
                    tracing::error!(error = %err);
                })
            })
            .instrument(span)
            .await
    }

    pub(crate) async fn record_value<T>(
        &self,
        step: SetupStep,
        future: impl Future<Output = T>,
    ) -> T {
        let (_, span) = step.to_event_name_and_span();

        future.instrument(span).await
    }
}

#[derive(Clone, Copy)]
pub(crate) enum SetupStep {
    EnvironmentResolution,
    SkillRepoClone,
    TerminalBootstrap,
    McpServerStartup,
    AgentProfileConfiguration,
    ProfileMcpServerStartup,
    SharedSessionEstablishment,
    GlobalSkillResolution,
    GlobalSkillRepoClone,
    EnvironmentRepoClone,
    CacheSetup,
    EnvironmentSetupCommands,
    FileBasedMcpDiscovery,
    FileBasedMcpReadiness,
    EnvironmentSkillLoading,
    GlobalSkillLoading,
    SkillsDirsLoading,
    ThirdPartyHarnessPreparation,
    /// Sub-steps of [`SetupStep::ThirdPartyHarnessPreparation`] that track plugin
    /// install/update latency and reliability individually.
    ThirdPartyHarnessPreparationNotificationPluginInstall,
    ThirdPartyHarnessPreparationNotificationPluginUpdate,
    ThirdPartyHarnessPreparationPlatformPluginInstall,
    ThirdPartyHarnessPreparationPlatformPluginUpdate,
}

macro_rules! span_and_name {
    ($name:literal) => {
        ($name, tracing::info_span!($name, tags.cloud_agent = true))
    };
}

impl SetupStep {
    fn to_event_name_and_span(self) -> (&'static str, tracing::Span) {
        match self {
            Self::EnvironmentResolution => {
                span_and_name!("setup_environment_resolution")
            }
            Self::SkillRepoClone => {
                span_and_name!("setup_skill_repo_clone")
            }
            Self::TerminalBootstrap => {
                span_and_name!("setup_terminal_bootstrap")
            }
            Self::McpServerStartup => {
                span_and_name!("setup_mcp_server_startup")
            }
            Self::AgentProfileConfiguration => {
                span_and_name!("setup_agent_profile_configuration")
            }
            Self::ProfileMcpServerStartup => {
                span_and_name!("setup_profile_mcp_server_startup")
            }
            Self::SharedSessionEstablishment => {
                span_and_name!("setup_shared_session_establishment")
            }
            Self::GlobalSkillResolution => {
                span_and_name!("setup_global_skill_resolution")
            }
            Self::GlobalSkillRepoClone => {
                span_and_name!("setup_global_skill_repo_clone")
            }
            Self::EnvironmentRepoClone => {
                span_and_name!("setup_environment_repo_clone")
            }
            Self::CacheSetup => {
                span_and_name!("setup_caches")
            }
            Self::EnvironmentSetupCommands => {
                span_and_name!("setup_environment_setup_commands")
            }
            Self::FileBasedMcpDiscovery => {
                span_and_name!("setup_file_based_mcp_discovery")
            }
            Self::FileBasedMcpReadiness => {
                span_and_name!("setup_file_based_mcp_readiness")
            }
            Self::EnvironmentSkillLoading => {
                span_and_name!("setup_environment_skill_loading")
            }
            Self::GlobalSkillLoading => {
                span_and_name!("setup_global_skill_loading")
            }
            Self::SkillsDirsLoading => {
                span_and_name!("setup_skills_dirs_loading")
            }
            Self::ThirdPartyHarnessPreparation => {
                span_and_name!("setup_third_party_harness_preparation")
            }
            Self::ThirdPartyHarnessPreparationNotificationPluginInstall => {
                span_and_name!("setup_third_party_harness_preparation_notification_plugin_install")
            }
            Self::ThirdPartyHarnessPreparationNotificationPluginUpdate => {
                span_and_name!("setup_third_party_harness_preparation_notification_plugin_update")
            }
            Self::ThirdPartyHarnessPreparationPlatformPluginInstall => {
                span_and_name!("setup_third_party_harness_preparation_platform_plugin_install")
            }
            Self::ThirdPartyHarnessPreparationPlatformPluginUpdate => {
                span_and_name!("setup_third_party_harness_preparation_platform_plugin_update")
            }
        }
    }
}
