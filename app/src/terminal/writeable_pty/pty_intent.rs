use std::borrow::Cow;

use async_channel::Sender;
use warp_completer::meta::Span;

use crate::ai::agent::AIAgentPtyWriteMode;
use crate::terminal::SizeUpdate;
use crate::terminal::model::completions::ShellCompletion;
use crate::terminal::view::ExecuteCommandEvent;

/// A request from the terminal view to the PTY controller: process control, byte writes, resizing,
/// command execution, and native shell completions.
pub enum PtyIntent {
    CtrlD,
    ShutdownPty,
    WriteBytes(Cow<'static, [u8]>),
    WriteAgentInput {
        bytes: Cow<'static, [u8]>,
        mode: AIAgentPtyWriteMode,
    },
    Resize(SizeUpdate),
    ExecuteCommand(ExecuteCommandEvent),
    RunNativeShellCompletions {
        buffer_text: String,
        results_tx: Sender<(Vec<ShellCompletion>, Option<Span>)>,
    },
}
