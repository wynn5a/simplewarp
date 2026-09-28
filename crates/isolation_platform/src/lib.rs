use std::sync::OnceLock;

use serde::Serialize;
use warp_core::channel::{Channel, ChannelState};

mod docker;
mod kubernetes;
mod namespace;

/// Environment variable set by the server to identify the isolation platform.
/// The value should match one of the `IsolationPlatformType` variants in snake_case.
const WARP_ISOLATION_PLATFORM_ENV: &str = "WARP_ISOLATION_PLATFORM";

/// A kind of isolation platform. For our usage, isolation platforms are different ways where Warp
/// can be sandboxed, such as VMs, containers, or cloud hosts. This may also include weaker forms
/// of sandboxing such as Git worktrees.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IsolationPlatformType {
    /// Warp is running within a Docker container. Note that this does *not* mean this is a Warp-hosted
    /// Docker Sandboxes environment. Instead, it's likely a self-hosted agent.
    Docker,
    /// Warp is running within a Docker Sandbox, likely as a Warp-hosted agent.
    DockerSandbox,
    /// Warp is running within a Kubernetes pod, likely as a self-hosted agent.
    Kubernetes,
    /// Warp is running within a Namespace instance, likely as a Warp-hosted agent.
    Namespace,
}

/// Detect the current isolation platform, if any.
///
/// Results are memoized for the lifetime of the process.
pub fn detect() -> Option<IsolationPlatformType> {
    static DETECTED_PLATFORM: OnceLock<Option<IsolationPlatformType>> = OnceLock::new();

    *DETECTED_PLATFORM.get_or_init(|| {
        // This never applies to integration tests.
        if ChannelState::channel() == Channel::Integration {
            return None;
        }

        // Use a closure so we can early-return.
        #[allow(clippy::redundant_closure_call)]
        let platform = (|| {
            // If the server explicitly told us which platform we're on, trust it.
            // This takes priority over all heuristic-based detection.
            if let Some(platform) = platform_from_env() {
                return Some(platform);
            }

            if namespace::is_in_namespace_instance() {
                return Some(IsolationPlatformType::Namespace);
            }

            if kubernetes::is_in_kubernetes() {
                return Some(IsolationPlatformType::Kubernetes);
            }

            if docker::is_in_docker() {
                return Some(IsolationPlatformType::Docker);
            }

            None
        })();

        match platform {
            Some(platform) => {
                log::debug!("Detected isolation platform: {:?}", platform);
            }
            None => {
                log::info!("No isolation platform detected");
            }
        }

        platform
    })
}

/// Parse the `WARP_ISOLATION_PLATFORM` environment variable into a platform type.
fn platform_from_env() -> Option<IsolationPlatformType> {
    let value = std::env::var(WARP_ISOLATION_PLATFORM_ENV).ok()?;
    match value.as_str() {
        "docker" => Some(IsolationPlatformType::Docker),
        "docker_sandbox" => Some(IsolationPlatformType::DockerSandbox),
        "kubernetes" => Some(IsolationPlatformType::Kubernetes),
        "namespace" => Some(IsolationPlatformType::Namespace),
        other => {
            log::warn!("Unknown {WARP_ISOLATION_PLATFORM_ENV} value: {other}");
            None
        }
    }
}
