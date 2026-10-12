#[derive(Copy, Clone, Debug)]
pub enum PromptSuggestionResolution {
    Accept,
    Reject { ctrl_c: bool },
}
