//! Wire format of the OSC 7501 program status protocol.
//!
//! Reference: <https://mitchellh.com/writing/program-status-osc7501>

use std::{fmt, str};

use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD, STANDARD_NO_PAD};

pub const MAX_SEQUENCE_BYTES: usize = 4096;
pub const MAX_RECORDS: usize = 256;
const MAX_KEY_BYTES: usize = 16;
const MAX_ID_BYTES: usize = 128;
const MAX_ID_SEGMENT_BYTES: usize = 32;
const MAX_ID_DEPTH: usize = 8;
const MAX_APP_BYTES: usize = 32;
const MAX_TITLE_ENCODED_BYTES: usize = 256;
const MAX_TITLE_BYTES: usize = 192;
const MAX_MESSAGE_ENCODED_BYTES: usize = 2732;
const MAX_MESSAGE_BYTES: usize = 2048;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProgramState {
    Idle,
    Working,
    Done,
    Blocked,
    Error,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockedKind {
    Permission,
    Question,
    Auth,
}

/// Slash-separated hierarchical record id. The empty path is the root record.
#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RecordPath(String);

impl RecordPath {
    pub fn root() -> Self {
        Self::default()
    }

    pub fn is_root(&self) -> bool {
        self.0.is_empty()
    }

    pub(super) fn parse(id: &str) -> Option<Self> {
        if id.len() > MAX_ID_BYTES {
            return None;
        }
        let mut depth = 0;
        for segment in id.split('/') {
            depth += 1;
            if depth > MAX_ID_DEPTH || !is_valid_name(segment, MAX_ID_SEGMENT_BYTES) {
                return None;
            }
        }
        Some(Self(id.to_owned()))
    }

    /// Whether `self` is `ancestor` itself or lies beneath it. Segment-wise, so `build` is not
    /// an ancestor of `buildx`.
    pub fn is_within(&self, ancestor: &RecordPath) -> bool {
        if ancestor.is_root() || self == ancestor {
            return true;
        }
        self.0
            .strip_prefix(ancestor.0.as_str())
            .is_some_and(|rest| rest.starts_with('/'))
    }
}

/// A report that sets (and completely replaces) one record.
#[derive(Clone, PartialEq, Eq)]
pub struct ProgramStatusUpdate {
    pub id: RecordPath,
    pub state: ProgramState,
    pub kind: Option<BlockedKind>,
    pub progress: Option<u8>,
    pub app: Option<String>,
    pub title: Option<String>,
    pub message: Option<String>,
}

// `title` and `message` are untrusted text from any process on the PTY and must not reach logs.
impl fmt::Debug for ProgramStatusUpdate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProgramStatusUpdate")
            .field("id", &self.id)
            .field("state", &self.state)
            .field("kind", &self.kind)
            .field("progress", &self.progress)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProgramStatusReport {
    Set(ProgramStatusUpdate),
    /// Removes the addressed record and every record beneath it.
    Clear {
        id: RecordPath,
    },
}

/// Parses the body of an `OSC 7501` sequence (everything after `7501;`). Returns `None` when the
/// report must be ignored.
pub fn parse(body: &[u8]) -> Option<ProgramStatusReport> {
    if body.len() > MAX_SEQUENCE_BYTES {
        return None;
    }
    let body = str::from_utf8(body).ok()?;

    let mut state = None;
    let mut id = None;
    let mut kind = None;
    let mut progress = None;
    let mut app = None;
    let mut title = None;
    let mut message = None;
    for pair in body.split(':') {
        let Some((key, value)) = pair.split_once('=') else {
            continue;
        };
        if key.len() > MAX_KEY_BYTES {
            continue;
        }
        match key {
            "state" => state = Some(value),
            "id" => id = Some(value),
            "kind" => kind = Some(value),
            "progress" => progress = Some(value),
            "app" => app = Some(value),
            "title" => title = Some(value),
            "msg" => message = Some(value),
            _ => (),
        }
    }

    let state = state?;
    let id = match id {
        Some(id) => RecordPath::parse(id)?,
        None => RecordPath::root(),
    };
    let state = match state {
        "clear" => return Some(ProgramStatusReport::Clear { id }),
        "idle" => ProgramState::Idle,
        "working" => ProgramState::Working,
        "done" => ProgramState::Done,
        "blocked" => ProgramState::Blocked,
        "error" => ProgramState::Error,
        _ => return None,
    };

    let kind = match (state, kind) {
        (ProgramState::Blocked, Some("permission")) => Some(BlockedKind::Permission),
        (ProgramState::Blocked, Some("question")) => Some(BlockedKind::Question),
        (ProgramState::Blocked, Some("auth")) => Some(BlockedKind::Auth),
        (
            ProgramState::Idle
            | ProgramState::Working
            | ProgramState::Done
            | ProgramState::Blocked
            | ProgramState::Error,
            _,
        ) => None,
    };
    let progress = match state {
        ProgramState::Working | ProgramState::Blocked => progress.and_then(parse_progress),
        ProgramState::Idle | ProgramState::Done | ProgramState::Error => None,
    };
    let app = app
        .filter(|app| is_valid_name(app, MAX_APP_BYTES))
        .map(str::to_owned);
    let title = match title {
        Some(title) => decode_text(title, MAX_TITLE_ENCODED_BYTES, MAX_TITLE_BYTES)?,
        None => None,
    };
    let message = match message {
        Some(message) => decode_text(message, MAX_MESSAGE_ENCODED_BYTES, MAX_MESSAGE_BYTES)?,
        None => None,
    };

    Some(ProgramStatusReport::Set(ProgramStatusUpdate {
        id,
        state,
        kind,
        progress,
        app,
        title,
        message,
    }))
}

/// Maps the parameters after `9;4` of a ConEmu progress report (`<state>[;<progress>]`) onto the
/// root record. States with no equivalent here are ignored.
pub fn parse_conemu_progress(params: &[&[u8]]) -> Option<ProgramStatusReport> {
    let number = |param: Option<&&[u8]>| {
        let param = param?;
        if param.is_empty() || !param.iter().all(u8::is_ascii_digit) {
            return None;
        }
        Some(
            str::from_utf8(param)
                .ok()?
                .parse::<u32>()
                .unwrap_or(u32::MAX),
        )
    };
    let progress = number(params.get(1)).map(|progress| progress.min(100) as u8);
    let (state, progress) = match number(params.first())? {
        0 => {
            return Some(ProgramStatusReport::Clear {
                id: RecordPath::root(),
            });
        }
        1 => (ProgramState::Working, progress),
        2 => (ProgramState::Error, None),
        3 | 4 => (ProgramState::Working, None),
        _ => return None,
    };
    Some(ProgramStatusReport::Set(ProgramStatusUpdate {
        id: RecordPath::root(),
        state,
        kind: None,
        progress,
        app: None,
        title: None,
        message: None,
    }))
}

fn is_valid_name(name: &str, max_bytes: usize) -> bool {
    !name.is_empty()
        && name.len() <= max_bytes
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'+' | b'-'))
}

fn parse_progress(value: &str) -> Option<u8> {
    if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    value.parse().ok().filter(|progress| *progress <= 100)
}

/// Decodes a base64 text field. The outer `None` discards the whole report (over a limit, bad
/// base64, or unsafe text); `Some(None)` is an empty field, treated as absent.
fn decode_text(value: &str, max_encoded: usize, max_decoded: usize) -> Option<Option<String>> {
    if value.len() > max_encoded {
        return None;
    }
    let bytes = STANDARD
        .decode(value)
        .or_else(|_| STANDARD_NO_PAD.decode(value))
        .ok()?;
    if bytes.len() > max_decoded {
        return None;
    }
    let text = String::from_utf8(bytes).ok()?;
    if text.chars().any(is_unsafe_char) {
        return None;
    }
    Some((!text.is_empty()).then_some(text))
}

/// Control characters, line/paragraph separators, and bidi controls that can spoof rendered text.
fn is_unsafe_char(c: char) -> bool {
    c.is_control()
        || matches!(
            c,
            '\u{200e}' | '\u{200f}' | '\u{2028}' | '\u{2029}' | '\u{202a}'..='\u{202e}'
                | '\u{2066}'..='\u{2069}'
        )
}

#[cfg(test)]
#[path = "protocol_tests.rs"]
mod tests;
