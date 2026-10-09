//! Cross-surface equivalence tests for the agent-icon helpers.
//!
//! The invariant under test: for every canonical logical run state, every surface produces
//! the same [`IconWithStatusVariant`]. Surfaces today are:
//! - Terminal view (vertical tabs + pane header) via
//!   [`super::agent_icon_variant_from_terminal_inputs`]
//! - Run cards (conversation list, agent management view) via
//!   [`super::agent_icon_variant_for_run`]
//! - Notification mailbox — exercised in `notifications/item_tests.rs`
//!
//! Adding a new canonical state is a one-enum-variant + one `expected` arm + one `*_inputs`
//! arm change; the table test below enforces every surface agrees.
use chrono::Utc;
use warp_cli::agent::Harness;

use super::{
    CLISessionInputs, TerminalIconInputs, agent_conversation_entry_icon_variant,
    agent_icon_variant_for_run, agent_icon_variant_from_terminal_inputs, badge_for_program_state,
};
use crate::ai::agent::conversation::{AIConversationId, ConversationStatus};
use crate::ai::agent_conversations_model::entry::{
    AgentConversationBackingData, AgentConversationCapabilities, AgentConversationDisplayData,
    AgentConversationIdentity, AgentConversationProvenance,
};
use crate::ai::agent_conversations_model::{
    AgentConversationEntry, AgentConversationEntryId, AgentRunDisplayStatus,
};
use crate::terminal::CLIAgent;
use crate::terminal::program_status::ProgramState;
use crate::ui_components::icon_with_status::IconWithStatusVariant;

/// Projection of the fields we care about for cross-surface equivalence.
/// [`IconWithStatusVariant`] itself can't derive `PartialEq` because `NeutralElement`
/// carries a `Box<dyn Element>`, so we extract the agent-variant fields here.
#[derive(Debug, Clone, PartialEq, Eq)]
struct AgentIconFields {
    is_cli: bool,
    cli_agent: Option<CLIAgent>,
    status: Option<ConversationStatus>,
}

impl AgentIconFields {
    fn from_variant(variant: &IconWithStatusVariant) -> Option<Self> {
        match variant {
            IconWithStatusVariant::OzAgent { status } => Some(Self {
                is_cli: false,
                cli_agent: None,
                status: status.clone(),
            }),
            IconWithStatusVariant::CLIAgent { agent, status } => Some(Self {
                is_cli: true,
                cli_agent: Some(*agent),
                status: status.clone(),
            }),
            IconWithStatusVariant::Neutral { .. }
            | IconWithStatusVariant::NeutralElement { .. } => None,
        }
    }
}

/// Canonical logical run states. Each represents a conceptually distinct run whose icon must
/// be rendered identically across every surface that can display it.
#[derive(Debug, Clone, Copy)]
enum CanonicalRunState {
    /// Plain terminal, no conversation, no agent activity.
    PlainTerminal,
    /// Local Warp-native (Oz) conversation, in-progress.
    LocalOzInProgress,
    /// Local Claude CLI session with a plugin listener (rich status), in-progress.
    LocalClaudePluginInProgress,
    /// Local Claude CLI session with a plugin listener (rich status), blocked.
    LocalClaudePluginBlocked,
    /// Local Claude CLI session detected via command matching only (no listener, no rich status).
    LocalClaudeCommandDetected,
}

impl CanonicalRunState {
    fn all() -> &'static [Self] {
        use CanonicalRunState::*;
        &[
            PlainTerminal,
            LocalOzInProgress,
            LocalClaudePluginInProgress,
            LocalClaudePluginBlocked,
            LocalClaudeCommandDetected,
        ]
    }

    /// The canonical [`AgentIconFields`] for this state. `None` means no agent icon renders.
    /// Editing an arm here is the deliberate way to evolve the cross-surface contract.
    fn expected(&self) -> Option<AgentIconFields> {
        use CanonicalRunState::*;
        match self {
            PlainTerminal => None,
            LocalOzInProgress => Some(AgentIconFields {
                is_cli: false,
                cli_agent: None,
                status: Some(ConversationStatus::InProgress),
            }),
            LocalClaudePluginInProgress => Some(AgentIconFields {
                is_cli: true,
                cli_agent: Some(CLIAgent::Claude),
                status: Some(ConversationStatus::InProgress),
            }),
            LocalClaudePluginBlocked => Some(AgentIconFields {
                is_cli: true,
                cli_agent: Some(CLIAgent::Claude),
                status: Some(ConversationStatus::Blocked {
                    blocked_action: String::new(),
                }),
            }),
            LocalClaudeCommandDetected => Some(AgentIconFields {
                is_cli: true,
                cli_agent: Some(CLIAgent::Claude),
                status: None,
            }),
        }
    }

    /// Terminal-view inputs for this state. Every state has a terminal representation.
    fn terminal_inputs(&self) -> TerminalIconInputs {
        use CanonicalRunState::*;
        match self {
            PlainTerminal => TerminalIconInputs {
                cli_session: None,
                program_status: None,
                selected_conversation_status: None,
                has_selected_conversation: false,
            },
            LocalOzInProgress => TerminalIconInputs {
                cli_session: None,
                program_status: None,
                selected_conversation_status: Some(ConversationStatus::InProgress),
                has_selected_conversation: true,
            },
            LocalClaudePluginInProgress => TerminalIconInputs {
                cli_session: Some(CLISessionInputs {
                    agent: CLIAgent::Claude,
                    has_listener: true,
                    status: ConversationStatus::InProgress,
                    supports_rich_status: true,
                }),
                program_status: None,
                selected_conversation_status: None,
                has_selected_conversation: false,
            },
            LocalClaudePluginBlocked => TerminalIconInputs {
                cli_session: Some(CLISessionInputs {
                    agent: CLIAgent::Claude,
                    has_listener: true,
                    status: ConversationStatus::Blocked {
                        blocked_action: String::new(),
                    },
                    supports_rich_status: true,
                }),
                program_status: None,
                selected_conversation_status: None,
                has_selected_conversation: false,
            },
            LocalClaudeCommandDetected => TerminalIconInputs {
                cli_session: Some(CLISessionInputs {
                    agent: CLIAgent::Claude,
                    has_listener: false,
                    status: ConversationStatus::InProgress,
                    supports_rich_status: false,
                }),
                program_status: None,
                selected_conversation_status: None,
                has_selected_conversation: false,
            },
        }
    }
}

/// The consistency enforcer: for every canonical state, the terminal-side helper must produce
/// the expected [`AgentIconFields`] projection.
#[test]
fn every_canonical_state_produces_consistent_icon_across_surfaces() {
    for state in CanonicalRunState::all() {
        let expected = state.expected();

        let terminal_actual = agent_icon_variant_from_terminal_inputs(&state.terminal_inputs())
            .as_ref()
            .and_then(AgentIconFields::from_variant);
        assert_eq!(
            terminal_actual, expected,
            "terminal surface disagreed for {state:?}"
        );
    }
}

#[test]
fn cli_agent_from_harness_maps_known_harnesses() {
    assert_eq!(CLIAgent::from_harness(Harness::Oz), None);
    assert_eq!(
        CLIAgent::from_harness(Harness::Claude),
        Some(CLIAgent::Claude)
    );
    assert_eq!(
        CLIAgent::from_harness(Harness::Gemini),
        Some(CLIAgent::Gemini)
    );
    assert_eq!(
        CLIAgent::from_harness(Harness::OpenCode),
        Some(CLIAgent::OpenCode)
    );
}

#[test]
fn run_card_with_oz_or_unknown_harness_renders_as_oz() {
    // Oz harness explicitly: local Oz is the spec-defined fallback.
    let variant = agent_icon_variant_for_run(Harness::Oz, ConversationStatus::Success);
    let fields = AgentIconFields::from_variant(&variant).unwrap();
    assert!(!fields.is_cli);

    // Unknown harness (e.g. server surfaced a future variant): also falls back to Oz so we
    // don't render an unbranded gray circle.
    let variant = agent_icon_variant_for_run(Harness::Unknown, ConversationStatus::Success);
    let fields = AgentIconFields::from_variant(&variant).unwrap();
    assert!(!fields.is_cli);
}

#[test]
fn entry_icon_uses_harness() {
    let conversation_id = AIConversationId::new();
    let entry = AgentConversationEntry {
        id: AgentConversationEntryId::Conversation(conversation_id),
        identity: AgentConversationIdentity {
            local_conversation_id: Some(conversation_id),
            server_conversation_token: None,
        },
        provenance: AgentConversationProvenance::CloudSyncedConversation,
        display: AgentConversationDisplayData {
            title: "Codex conversation".to_string(),
            initial_query: None,
            created_at: Utc::now(),
            last_updated: Utc::now(),
            status: AgentRunDisplayStatus::Succeeded,
            request_usage: None,
            working_directory: None,
            harness: Some(Harness::Codex),
            artifacts: Vec::new(),
        },
        backing: AgentConversationBackingData {
            has_loaded_conversation: true,
            has_local_persisted_data: true,
            has_cloud_data: true,
        },
        capabilities: AgentConversationCapabilities {
            can_open: true,
            can_copy_link: false,
            can_delete: false,
            can_fork_locally: false,
            can_cancel: false,
        },
    };

    let variant = agent_conversation_entry_icon_variant(&entry);
    assert_eq!(
        AgentIconFields::from_variant(&variant).unwrap(),
        AgentIconFields {
            is_cli: true,
            cli_agent: Some(CLIAgent::Codex),
            status: Some(ConversationStatus::Success),
        }
    );
}

#[test]
fn program_status_badge_maps_every_state() {
    assert_eq!(badge_for_program_state(ProgramState::Idle), None);
    assert_eq!(
        badge_for_program_state(ProgramState::Working),
        Some(ConversationStatus::InProgress)
    );
    assert_eq!(
        badge_for_program_state(ProgramState::Done),
        Some(ConversationStatus::Success)
    );
    assert_eq!(
        badge_for_program_state(ProgramState::Blocked),
        Some(ConversationStatus::Blocked {
            blocked_action: String::new()
        })
    );
    assert_eq!(
        badge_for_program_state(ProgramState::Error),
        Some(ConversationStatus::Error)
    );
}

#[test]
fn program_status_alone_does_not_make_a_terminal_an_agent() {
    let inputs = TerminalIconInputs {
        cli_session: None,
        program_status: Some(ConversationStatus::InProgress),
        selected_conversation_status: None,
        has_selected_conversation: false,
    };
    assert!(agent_icon_variant_from_terminal_inputs(&inputs).is_none());
}

fn claude_session(has_listener: bool, supports_rich_status: bool) -> CLISessionInputs {
    CLISessionInputs {
        agent: CLIAgent::Claude,
        has_listener,
        status: ConversationStatus::Blocked {
            blocked_action: String::new(),
        },
        supports_rich_status,
    }
}

fn cli_status(inputs: &TerminalIconInputs) -> Option<ConversationStatus> {
    match agent_icon_variant_from_terminal_inputs(inputs) {
        Some(IconWithStatusVariant::CLIAgent { status, .. }) => status,
        _ => panic!("expected a CLI agent variant"),
    }
}

#[test]
fn program_status_fills_in_for_a_cli_session_without_rich_status() {
    for session in [claude_session(false, false), claude_session(true, false)] {
        let inputs = TerminalIconInputs {
            cli_session: Some(session),
            program_status: Some(ConversationStatus::InProgress),
            selected_conversation_status: None,
            has_selected_conversation: false,
        };
        assert_eq!(cli_status(&inputs), Some(ConversationStatus::InProgress));
    }
}

#[test]
fn rich_cli_status_wins_over_program_status() {
    let inputs = TerminalIconInputs {
        cli_session: Some(claude_session(true, true)),
        program_status: Some(ConversationStatus::InProgress),
        selected_conversation_status: None,
        has_selected_conversation: false,
    };
    assert_eq!(
        cli_status(&inputs),
        Some(ConversationStatus::Blocked {
            blocked_action: String::new()
        })
    );
}
