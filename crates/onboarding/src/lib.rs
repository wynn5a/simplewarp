// Onboarding library crate

pub mod callout;
pub mod telemetry;

/// The user's intention selected during onboarding slides.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OnboardingIntention {
    Terminal,
    AgentDrivenDevelopment,
}

impl std::fmt::Display for OnboardingIntention {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OnboardingIntention::AgentDrivenDevelopment => write!(f, "agent_driven"),
            OnboardingIntention::Terminal => write!(f, "terminal"),
        }
    }
}

pub use callout::{OnboardingCalloutView, OnboardingKeybindings};

/// User-facing descriptions of the AI features enabled when the agent intention is selected.
/// Shared by the intention slide's agent card checklist and the login slide's
/// skip-login confirmation dialog so the two always stay in sync.
pub const AI_FEATURES: &[&str] = &[
    "Use frontier and open-weight models with Warp Agent",
    "Hand off agent work to cloud agents",
    "Automatically diagnose and fix terminal errors",
    "Agentic control of long-running commands and TUIs",
    "Review code diffs and send comments directly to agents",
    "Remote control for Claude Code, Codex, and other agents",
];

/// User-facing names of the Warp Drive features enabled when the terminal
/// intention is selected with Warp Drive turned on. Shared by the login slide's
/// skip-login confirmation dialog so the list stays in sync with any future
/// surfaces that need it.
pub const WARP_DRIVE_FEATURES: &[&str] = &["Warp Drive", "Session Sharing"];

pub mod components;

pub use telemetry::OnboardingEvent;

pub fn init(app: &mut warpui_core::AppContext) {
    callout::init(app);
}
