use chrono::{DateTime, FixedOffset};
use serde::{Deserialize, Serialize};

/// A representation of a Block for the server.
#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
pub struct Block {
    pub id: Option<String>,

    /// The input lines for a block.
    pub command: Option<String>,

    /// The output lines for a block.
    pub output: Option<String>,

    /// The input lines with their corresponding escape sequences so it can be rendered outside of
    /// the terminal.
    pub stylized_command: Option<String>,

    /// The output lines with their corresponding escape sequences so it can be rendered outside of
    /// the terminal.
    pub stylized_output: Option<String>,

    /// The prompt lines with their corresponding escape sequences so it can be rendered outside of
    /// the terminal.
    pub stylized_prompt: Option<String>,

    /// The prompt and command (combined) lines with their corresponding escape sequences so it can
    /// be rendered outside of the terminal. Only non-null if using PS1 with the combined grid.
    pub stylized_prompt_and_command: Option<String>,

    /// The current working directory of the block.
    pub pwd: Option<String>,

    /// The terminal's timestamp of block completion.
    pub time_started_term: DateTime<FixedOffset>,

    /// The terminal's timestamp of block completion.
    pub time_completed_term: DateTime<FixedOffset>,
}
