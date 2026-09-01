use crate::schema;

/// The type of a managed secret's value, shared with `warp_managed_secrets::ManagedSecretValue`
/// for local typed-secret env-var injection into spawned harness processes.
#[derive(cynic::Enum, Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ManagedSecretType {
    AnthropicApiKey,
    AnthropicBedrockAccessKey,
    AnthropicBedrockApiKey,
    Dotenvx,
    OpenaiApiKey,
    RawValue,
}
