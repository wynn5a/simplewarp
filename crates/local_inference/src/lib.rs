//! Local inference for SimpleWarp.
//!
//! This crate is the drop-in replacement for `warp_multi_agent_client`. That client posts every
//! agent request to `{warp_server}/ai/multi-agent`, with the user API keys inside the request
//! body, and lets the server run the agent loop. This crate keeps the whole loop on the machine:
//! it reads the conversation out of the request, calls the provider direct, and turns the reply
//! back into the same [`api::ResponseEvent`] stream that the client already knows how to apply.
//!
//! The client keeps the parts it always owned: it runs the tools, it renders the blocks, and it
//! decides what to send next. Only the model call moves.
//!
//! ```text
//!   before:  client ──► Warp server ──► provider
//!   after:   client ──────────────────► provider
//! ```

use futures::stream::BoxStream;
use warp_multi_agent_api as api;

pub mod config;
pub mod convert;
pub mod models;
pub mod prompt;
pub mod provider;
pub mod tools;

mod emit;
mod stream;

pub use config::{ProviderTarget, Schema};
pub use models::{Provider, ProviderModel, list_models};
pub use stream::generate_local_output;

/// A response event stream, in the shape that the client already consumes.
pub type OutputStream = BoxStream<'static, Result<api::ResponseEvent, Error>>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("the request carried no settings")]
    NoSettings,

    #[error("the request named no base model")]
    NoModel,

    #[error("no API key is set for model `{model}`")]
    NoApiKey { model: String },

    #[error("no model is configured; add an API key or a custom endpoint in Settings > AI")]
    NoModelConfigured,

    #[error(
        "`{model}` belongs to an aggregator such as OpenRouter. Add it as a custom endpoint in \
         Settings > AI, with its base URL and this exact model name."
    )]
    AggregatorModel { model: String },

    #[error("the request carried no user input")]
    NoInput,

    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("the provider answered {status}: {body}")]
    ProviderStatus { status: u16, body: String },

    #[error("could not read the provider stream: {0}")]
    EventSource(Box<reqwest_eventsource::Error>),

    #[error("could not decode the provider reply: {0}")]
    Decode(#[from] serde_json::Error),
}
