use std::sync::OnceLock;

use warp_core::channel::{Channel, ChannelState};

mod docker;
mod kubernetes;
mod namespace;

/// A kind of isolation platform. For our usage, isolation platforms are different ways where Warp
/// can be sandboxed, such as VMs, containers, or cloud hosts. This may also include weaker forms
/// of sandboxing such as Git worktrees.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IsolationPlatformType {
    /// Warp is running within a Docker container.
    Docker,
    /// Warp is running within a Kubernetes pod.
    Kubernetes,
    /// Warp is running within a Namespace instance.
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
