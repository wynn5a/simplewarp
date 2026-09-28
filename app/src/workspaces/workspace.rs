use serde::{Deserialize, Serialize};

use super::team::Team;
use crate::server::ids::ServerId;

#[derive(Clone, Copy, Hash, Debug, PartialEq, Eq)]
pub struct WorkspaceUid(ServerId);
impl From<String> for WorkspaceUid {
    fn from(uid: String) -> Self {
        WorkspaceUid(ServerId::from_string_lossy(uid))
    }
}
impl From<WorkspaceUid> for String {
    fn from(workspace_uid: WorkspaceUid) -> String {
        workspace_uid.0.to_string()
    }
}
impl From<ServerId> for WorkspaceUid {
    fn from(uid: ServerId) -> Self {
        WorkspaceUid(uid)
    }
}

#[derive(Clone, Debug)]
pub struct Workspace {
    pub uid: WorkspaceUid,
    pub teams: Vec<Team>,
    pub billing_metadata: BillingMetadata,
}

impl Workspace {
    pub fn from_local_cache(uid: WorkspaceUid, teams: Option<Vec<Team>>) -> Self {
        // Derive the workspace billing metadata from the first team's cached billing
        // metadata, if available. This ensures the workspace-level billing info is
        // consistent with team-level data loaded from the cache.
        let billing_metadata = teams
            .as_ref()
            .and_then(|t| t.first())
            .map(|team| team.billing_metadata.clone())
            .unwrap_or_default();
        Self {
            uid,
            teams: teams.unwrap_or_default(),
            billing_metadata,
        }
    }
}

/// This enum is the rust representation of `CustomerType` from the GraphQL Schema.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum CustomerType {
    #[default]
    Free,
    Turbo,
    SelfServe,
    Prosumer,
    Legacy,
    Enterprise,
    Business,
    Lightspeed,
    Build,
    BuildMax,
    Unknown,
}

/// This enum is the rust representation of `DelinquencyStatus` from the GraphQL Schema.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum DelinquencyStatus {
    #[default]
    NoDelinquency,
    PastDue,
    Unpaid,
    TeamLimitExceeded,
    Unknown,
}

/// Rust representation of feature policies from the GraphQL Schema.
#[derive(Clone, Debug, Copy, Serialize, Deserialize)]
pub struct WarpAiPolicy {
    pub limit: i64,
    pub is_code_suggestions_toggleable: bool,
    pub is_prompt_suggestions_toggleable: bool,
    pub is_next_command_enabled: bool,
    pub is_git_operations_ai_enabled: bool,
    pub is_voice_enabled: bool,
}
#[derive(Clone, Debug, Copy, Serialize, Deserialize)]
pub struct WorkspaceSizePolicy {
    pub is_unlimited: bool,
    pub limit: i64,
}
#[derive(Clone, Debug, Copy, Serialize, Deserialize)]
pub struct SharedNotebooksPolicy {
    pub is_unlimited: bool,
    pub limit: i64,
}
#[derive(Clone, Debug, Copy, Serialize, Deserialize)]
pub struct SharedWorkflowsPolicy {
    pub is_unlimited: bool,
    pub limit: i64,
}

#[derive(Clone, Debug, Copy, Serialize, Deserialize)]
pub struct SessionSharingPolicy {
    pub is_enabled: bool,
    pub max_session_size: u64,
}

#[derive(Clone, Debug, Copy, Serialize, Deserialize)]
pub struct AIAutonomyPolicy {
    pub is_enabled: bool,
    pub toggleable: bool,
}

#[derive(Clone, Debug, Copy, Serialize, Deserialize)]
pub struct TelemetryDataCollectionPolicy {
    pub default: bool,
    pub toggleable: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub enum UgcCollectionEnablementSetting {
    Disable,
    Enable,
    #[default]
    RespectUserSetting,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UgcDataCollectionPolicy {
    pub default_setting: UgcCollectionEnablementSetting,
    pub toggleable: bool,
}

#[derive(Clone, Debug, Copy, Serialize, Deserialize)]
pub struct UsageBasedPricingPolicy {
    pub toggleable: bool,
}

#[derive(Clone, Debug, Copy, Serialize, Deserialize)]
pub struct CodebaseContextPolicy {
    pub toggleable: bool,
    pub index_limit: Option<u32>,
    pub max_files_per_repo: u32,
}

#[derive(Clone, Debug, Copy, Serialize, Deserialize)]
pub struct ByoApiKeyPolicy {
    pub enabled: bool,
}

#[derive(Clone, Debug, Copy, Serialize, Deserialize)]
pub struct ByoEndpointPolicy {
    pub enabled: bool,
}

#[derive(Clone, Debug, Copy, Serialize, Deserialize)]
pub struct ManagedByokByoePolicy {
    pub enabled: bool,
}

#[derive(Clone, Debug, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PurchaseAddOnCreditsPolicy {
    pub enabled: bool,
    /// When `enabled` is false, allows purchasing add-on credit packs at a
    /// `price_premium_bps` surcharge over list price (e.g. on the Free plan).
    #[serde(default)]
    pub premium_enabled: bool,
    /// Surcharge in basis points applied to list prices when purchasing via
    /// the premium path (1000 bps = +10%). 0 for standard purchasing plans.
    #[serde(default)]
    pub price_premium_bps: i32,
}

#[derive(Clone, Debug, Copy, Serialize, Deserialize)]
pub struct EnterprisePayAsYouGoPolicy {
    pub enabled: bool,
}

#[derive(Clone, Debug, Copy, Serialize, Deserialize)]
pub struct EnterpriseCreditsAutoReloadPolicy {
    pub enabled: bool,
}

#[derive(Clone, Debug, Copy, Serialize, Deserialize)]
pub struct MultiAdminPolicy {
    pub enabled: bool,
}

#[derive(Clone, Debug, Copy, Serialize, Deserialize)]
pub struct NativeWorkspacesPolicy {
    pub enabled: bool,
}

#[derive(Clone, Debug, Copy, Serialize, Deserialize)]
pub struct AmbientAgentsPolicy {
    pub max_concurrent_agents: i32,
    pub instance_shape: Option<InstanceShape>,
}

#[derive(Clone, Debug, Copy, Serialize, Deserialize)]
pub struct InstanceShape {
    pub vcpus: i32,
    pub memory_gb: i32,
}

/// Granularity at which a viewer can see AI usage across their team.
/// Non-admins always collapse to `OwnOnly` regardless of tier.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum UsageVisibilityGranularity {
    #[default]
    OwnOnly,
    TeamAggregate,
    PerUserTotals,
    FullBreakdown,
}

/// Number of prior billing cycles a viewer can scroll back through, in
/// addition to the always-visible current cycle. Plan-wide; applies to
/// admins and non-admins alike.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum MaxPriorCycles {
    #[default]
    None,
    /// Current cycle plus `n` prior cycles (`n >= 1`).
    Limited(u32),
    Unlimited,
}

/// Rust representation of the `UsageVisibilityPolicy` tier policy from the
/// GraphQL schema.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct UsageVisibilityPolicy {
    pub admin_granularity: UsageVisibilityGranularity,
    pub max_prior_cycles: MaxPriorCycles,
}

/// This struct is the rust representation of `Tier` from the GraphQL Schema.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Tier {
    pub name: String,
    pub description: String,
    pub warp_ai_policy: Option<WarpAiPolicy>,
    pub workspace_size_policy: Option<WorkspaceSizePolicy>,
    pub shared_notebooks_policy: Option<SharedNotebooksPolicy>,
    pub shared_workflows_policy: Option<SharedWorkflowsPolicy>,
    pub session_sharing_policy: Option<SessionSharingPolicy>,
    pub ai_autonomy_policy: Option<AIAutonomyPolicy>,
    pub telemetry_data_collection_policy: Option<TelemetryDataCollectionPolicy>,
    pub ugc_data_collection_policy: Option<UgcDataCollectionPolicy>,
    pub usage_based_pricing_policy: Option<UsageBasedPricingPolicy>,
    pub codebase_context_policy: Option<CodebaseContextPolicy>,
    pub byo_api_key_policy: Option<ByoApiKeyPolicy>,
    pub byo_endpoint_policy: Option<ByoEndpointPolicy>,
    pub managed_byok_byoe_policy: Option<ManagedByokByoePolicy>,
    pub purchase_add_on_credits_policy: Option<PurchaseAddOnCreditsPolicy>,
    pub enterprise_pay_as_you_go_policy: Option<EnterprisePayAsYouGoPolicy>,
    pub enterprise_credits_auto_reload_policy: Option<EnterpriseCreditsAutoReloadPolicy>,
    pub multi_admin_policy: Option<MultiAdminPolicy>,
    pub native_workspaces_policy: Option<NativeWorkspacesPolicy>,
    pub ambient_agents_policy: Option<AmbientAgentsPolicy>,
    pub usage_visibility_policy: Option<UsageVisibilityPolicy>,
}

/// This struct is the rust representation of `BillingMetadata` from the GraphQL Schema.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct BillingMetadata {
    pub tier: Tier,
    pub customer_type: CustomerType,
    pub delinquency_status: DelinquencyStatus,
}

impl BillingMetadata {
    pub fn is_user_on_paid_plan(&self) -> bool {
        match self.customer_type {
            CustomerType::Turbo
            | CustomerType::SelfServe
            | CustomerType::Prosumer
            | CustomerType::Business
            | CustomerType::Lightspeed
            | CustomerType::Enterprise
            | CustomerType::Legacy
            | CustomerType::Build
            | CustomerType::BuildMax => true,
            CustomerType::Free | CustomerType::Unknown => false,
        }
    }

    pub fn is_managed_byok_byoe_enabled(&self) -> bool {
        self.tier
            .managed_byok_byoe_policy
            .is_some_and(|policy| policy.enabled)
    }
}
