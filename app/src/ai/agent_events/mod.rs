//! Shared agent-event stream utilities used by orchestration consumers and
//! third-party harness bridges.

mod driver;

pub(crate) use driver::{
    AgentEventConsumer, AgentEventConsumerControlFlow, AgentEventDriverConfig, AgentEventFilter,
    AgentMessageEventMetadata, ServerApiAgentEventSource, run_agent_event_driver,
};
#[cfg(test)]
pub(crate) use driver::{
    AgentEventDriverState, AgentEventSource, AgentEventSourceItem,
    DEFAULT_AGENT_EVENT_FAILURES_BEFORE_ERROR_LOG, DEFAULT_AGENT_EVENT_RECONNECT_BACKOFF_STEPS,
    DEFAULT_PERMANENT_ERROR_BACKOFF_STEPS, agent_event_backoff,
    agent_event_failures_exceeded_threshold,
};
#[cfg(test)]
mod driver_tests;
