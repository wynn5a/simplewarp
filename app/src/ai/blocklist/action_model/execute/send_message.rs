use futures::FutureExt as _;
use futures::future::BoxFuture;
use warp_core::send_telemetry_from_ctx;
use warpui::{Entity, ModelContext};

use super::{ActionExecution, AnyActionExecution, ExecuteActionInput, PreprocessActionInput};
use crate::ai::agent::{
    AIAgentAction, AIAgentActionResultType, AIAgentActionType, SendMessageToAgentResult,
};
use crate::ai::ambient_agents::AmbientAgentTaskId;
use crate::ai::blocklist::telemetry::{
    BlocklistOrchestrationTelemetryEvent, TeamAgentCommunicationFailedEvent,
    TeamAgentCommunicationFailureReason, TeamAgentCommunicationKind,
    TeamAgentCommunicationTransport, TeamAgentOrchestrationVersion,
};

pub struct SendMessageToAgentExecutor {
    ambient_agent_task_id: Option<AmbientAgentTaskId>,
}

impl SendMessageToAgentExecutor {
    pub fn new() -> Self {
        Self {
            ambient_agent_task_id: None,
        }
    }

    pub fn set_ambient_agent_task_id(&mut self, id: Option<AmbientAgentTaskId>) {
        self.ambient_agent_task_id = id;
    }

    pub(super) fn should_autoexecute(
        &self,
        _input: ExecuteActionInput,
        _ctx: &mut ModelContext<Self>,
    ) -> bool {
        true
    }

    pub(super) fn execute(
        &mut self,
        input: ExecuteActionInput,
        ctx: &mut ModelContext<Self>,
    ) -> AnyActionExecution {
        let AIAgentAction {
            action:
                AIAgentActionType::SendMessageToAgent {
                    addresses,
                    subject,
                    message,
                },
            ..
        } = input.action
        else {
            return ActionExecution::<()>::InvalidAction.into();
        };

        let conversation_id = input.conversation_id;
        let error_message = "SimpleWarp is a local-only build; this operation needs Warp's \
                             servers"
            .to_owned();
        send_telemetry_from_ctx!(
            BlocklistOrchestrationTelemetryEvent::TeamAgentCommunicationFailed(
                TeamAgentCommunicationFailedEvent {
                    communication_kind: TeamAgentCommunicationKind::Message,
                    transport: TeamAgentCommunicationTransport::ServerApi,
                    orchestration_version: TeamAgentOrchestrationVersion::V2,
                    failure_reason: TeamAgentCommunicationFailureReason::RequestFailed,
                    source_conversation_id: conversation_id,
                    source_run_id: None,
                    target_count: Some(addresses.len()),
                    lifecycle_event_type: None,
                    error_message: Some(error_message.clone()),
                }
            ),
            ctx
        );
        log::warn!(
            "Failed to send child-agent message via server API: target_agent_ids={addresses:?} subject={subject:?} body_len={} error={error_message}",
            message.chars().count()
        );
        ActionExecution::<()>::Sync(AIAgentActionResultType::SendMessageToAgent(
            SendMessageToAgentResult::Error(error_message),
        ))
        .into()
    }

    pub(super) fn preprocess_action(
        &mut self,
        _action: PreprocessActionInput,
        _ctx: &mut ModelContext<Self>,
    ) -> BoxFuture<'static, ()> {
        futures::future::ready(()).boxed()
    }
}

impl Default for SendMessageToAgentExecutor {
    fn default() -> Self {
        Self::new()
    }
}

impl Entity for SendMessageToAgentExecutor {
    type Event = ();
}
