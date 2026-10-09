//! This module contains traits and trait implementations for exposing helper methods for accessing
//! proto fields.
use warp_multi_agent_api as api;

pub trait TaskExt {
    fn parent_id(&self) -> Option<&str>;
}

impl TaskExt for api::Task {
    fn parent_id(&self) -> Option<&str> {
        self.dependencies
            .as_ref()
            .map(|deps| deps.parent_task_id.as_str())
            .filter(|id| !id.is_empty())
    }
}

pub trait MessageExt {
    fn tool_call(&self) -> Option<&api::message::ToolCall>;
    fn tool_call_mut(&mut self) -> Option<&mut api::message::ToolCall>;
    fn tool_call_result(&self) -> Option<&api::message::ToolCallResult>;
}

pub trait ToolCallExt {
    fn subagent(&self) -> Option<&api::message::tool_call::Subagent>;
    fn subagent_mut(&mut self) -> Option<&mut api::message::tool_call::Subagent>;
}

pub trait SubagentExt {
    fn is_cli(&self) -> bool;
    fn is_advice(&self) -> bool;
    fn is_summarization(&self) -> bool;
    fn is_conversation_search(&self) -> bool;
    fn is_warp_documentation_search(&self) -> bool;
}

impl MessageExt for api::Message {
    fn tool_call(&self) -> Option<&api::message::ToolCall> {
        self.message.as_ref().and_then(|message| {
            if let api::message::Message::ToolCall(tool_call) = message {
                Some(tool_call)
            } else {
                None
            }
        })
    }

    fn tool_call_mut(&mut self) -> Option<&mut api::message::ToolCall> {
        self.message.as_mut().and_then(|message| {
            if let api::message::Message::ToolCall(tool_call) = message {
                Some(tool_call)
            } else {
                None
            }
        })
    }

    fn tool_call_result(&self) -> Option<&api::message::ToolCallResult> {
        self.message.as_ref().and_then(|message| {
            if let api::message::Message::ToolCallResult(result) = message {
                Some(result)
            } else {
                None
            }
        })
    }
}

impl ToolCallExt for api::message::ToolCall {
    fn subagent(&self) -> Option<&api::message::tool_call::Subagent> {
        match self.tool.as_ref() {
            Some(api::message::tool_call::Tool::Subagent(subagent)) => Some(subagent),
            _ => None,
        }
    }

    fn subagent_mut(&mut self) -> Option<&mut api::message::tool_call::Subagent> {
        match self.tool.as_mut() {
            Some(api::message::tool_call::Tool::Subagent(subagent)) => Some(subagent),
            _ => None,
        }
    }
}

impl SubagentExt for api::message::tool_call::Subagent {
    fn is_cli(&self) -> bool {
        self.metadata.as_ref().is_some_and(|metadata| {
            matches!(
                metadata,
                api::message::tool_call::subagent::Metadata::Cli(_)
            )
        })
    }

    fn is_advice(&self) -> bool {
        self.metadata.as_ref().is_some_and(|metadata| {
            matches!(
                metadata,
                api::message::tool_call::subagent::Metadata::Advice(_)
            )
        })
    }

    fn is_summarization(&self) -> bool {
        self.metadata.as_ref().is_some_and(|metadata| {
            matches!(
                metadata,
                api::message::tool_call::subagent::Metadata::Summarization(_)
            )
        })
    }

    fn is_conversation_search(&self) -> bool {
        self.metadata.as_ref().is_some_and(|metadata| {
            matches!(
                metadata,
                api::message::tool_call::subagent::Metadata::ConversationSearch(_)
            )
        })
    }

    fn is_warp_documentation_search(&self) -> bool {
        self.metadata.as_ref().is_some_and(|metadata| {
            matches!(
                metadata,
                api::message::tool_call::subagent::Metadata::WarpDocumentationSearch(_)
            )
        })
    }
}
