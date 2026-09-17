// TODO(roland): Delete all of this once agent mode fully replaces the AI assistant panel.
use warpui::{Entity, ModelContext};

use super::utils::{FormattedTranscriptMessage, TranscriptPart, markdown_segments_from_text};
use crate::ai_assistant::utils::{AssistantTranscriptPart, TranscriptPartSubType};
use crate::send_telemetry_from_ctx;
use crate::server::telemetry::{TelemetryEvent, WarpAIRequestResult};

#[derive(Default)]
pub struct Requests {
    /// The currently displayed transcript.
    current_transcript: Vec<TranscriptPart>,

    /// When a user Restarts their transcript, we still remember
    /// the previous transcript parts for things like suggestions.
    /// This list is mutually exclusive from current_transcript.
    old_transcript_parts: Vec<TranscriptPart>,
}

impl Entity for Requests {
    type Event = Event;
}

pub enum Event {
    RequestFinished { succeeded: bool },
}

/// Public interface.
impl Requests {
    pub fn new() -> Self {
        Self::default()
    }

    /// Records the prompt and the failure answer every request is guaranteed to
    /// receive: the assistant panel's server call is a guaranteed wall in this
    /// fork, so the failure message is the only response there is.
    pub fn issue_request(&mut self, request: String, ctx: &mut ModelContext<Self>) {
        let raw_request = request.trim();
        let transcript_part_index = self.current_transcript.len();

        let request_in_markdown = markdown_segments_from_text(
            transcript_part_index,
            TranscriptPartSubType::Question,
            raw_request,
        );
        let response =
            "We're experiencing technical difficulties right now. Please try again later.";
        let response_in_markdown = markdown_segments_from_text(
            transcript_part_index,
            TranscriptPartSubType::Answer,
            response,
        );

        self.current_transcript.push(TranscriptPart {
            user: FormattedTranscriptMessage {
                markdown: request_in_markdown,
                raw: raw_request.to_string(),
            },
            assistant: AssistantTranscriptPart {
                is_error: true,
                copy_all_tooltip_and_button_mouse_handles: None,
                formatted_message: FormattedTranscriptMessage {
                    markdown: response_in_markdown,
                    raw: response.to_string(),
                },
            },
        });

        send_telemetry_from_ctx!(
            TelemetryEvent::WarpAIRequestIssued {
                result: WarpAIRequestResult::Failed
            },
            ctx
        );

        ctx.emit(Event::RequestFinished { succeeded: false });
        ctx.notify();
    }

    pub fn reset(&mut self, ctx: &mut ModelContext<Self>) {
        let mut old_transcript = Vec::new();
        std::mem::swap(&mut old_transcript, &mut self.current_transcript);
        self.old_transcript_parts.extend(old_transcript);
        ctx.notify();
    }

    #[cfg(test)]
    pub fn new_with_transcript(transcript: Vec<TranscriptPart>) -> Self {
        Self {
            current_transcript: transcript,
            old_transcript_parts: Vec::new(),
        }
    }

    pub fn transcript(&self) -> &[TranscriptPart] {
        self.current_transcript.as_slice()
    }

    /// Includes the old transcript parts appended with the current
    /// transcript parts. You likely want to just be using the current transcript parts
    /// (exposed by the `Requests::transcript` API) in most use cases.
    fn total_transcript_history(&self) -> impl Iterator<Item = &TranscriptPart> {
        self.old_transcript_parts
            .iter()
            .chain(self.current_transcript.iter())
    }

    pub fn all_past_transcript_prompts(&self) -> Vec<String> {
        self.total_transcript_history()
            .map(|p| p.raw_user_prompt().to_string())
            .collect()
    }
}
