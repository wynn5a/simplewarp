use clap::{Args, Subcommand};

/// Model-related subcommands.
#[derive(Debug, Clone, Subcommand)]
pub enum ModelCommand {
    /// List available models.
    List,
}

impl ModelCommand {
    pub(crate) fn as_str_for_tracing(&self) -> &'static str {
        match self {
            ModelCommand::List => "model list",
        }
    }
}

/// Shared CLI args for selecting a base model.
#[derive(Debug, Clone, Args, Default)]
pub struct ModelArgs {
    /// Override the base model used by this command.
    ///
    /// Use the `model list` command to see available model IDs.
    #[arg(long = "model", value_name = "MODEL_ID")]
    pub model: Option<String>,
}
