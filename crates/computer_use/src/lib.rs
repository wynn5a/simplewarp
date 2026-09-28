#[cfg_attr(macos, path = "mac/mod.rs")]
#[cfg_attr(linux, path = "linux/mod.rs")]
#[cfg_attr(windows, path = "windows/mod.rs")]
#[cfg(not(noop))]
mod imp;
mod noop;
#[cfg(any(macos, linux, windows))]
mod screenshot_utils;

use std::borrow::Cow;

use async_trait::async_trait;
// Clippy doesn't like us pulling in a file as two different modules,
// so we add this alias instead of using another cfg_attr on the imp
// module definition.
#[cfg(noop)]
use noop as imp;
pub use pathfinder_geometry::vector::Vector2I;
use serde::{Deserialize, Serialize};
use serde_with::{DurationSecondsWithFrac, serde_as};

/// The platform that computer use is running on.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum Platform {
    Mac,
    Windows,
    LinuxX11,
    LinuxWayland,
}

pub fn is_supported_on_current_platform() -> bool {
    if cfg!(feature = "test-util") {
        noop::is_supported_on_current_platform()
    } else {
        imp::is_supported_on_current_platform()
    }
}
/// Returns an actor that can perform actions on the computer.
pub fn create_actor() -> Box<dyn Actor> {
    if cfg!(feature = "test-util") {
        Box::new(noop::Actor::new())
    } else {
        Box::new(imp::Actor::new())
    }
}

/// Returns whether background, per-window control (driving a specific window without raising it
/// or moving the cursor) is available on this client and OS. When false, callers should target
/// the whole screen / frontmost application.
///
/// How faithfully "background" holds varies by platform:
///
/// - macOS: events are posted directly to the owning process (`CGEventPostToPid`), so a window
///   can be driven even while fully covered, and nothing user-visible changes.
/// - Linux X11: events come from a dedicated second input seat (an XInput2/MPX master pair), so
///   the user's cursor, keyboard focus, and modifier state are untouched and applications see
///   real (non-synthetic) input. The trade-offs, inherent to X11's position-routed event
///   delivery, are:
///   - Pointer actions land on the topmost window at the target point. If the target window is
///     covered there, the actor first raises it *without* taking the user's focus; the action
///     fails if the raise does not take effect. Keyboard input needs no raise: it follows the
///     agent seat's own focus even while the window is covered.
///   - A second visible cursor appears on screen while window-targeted actions run.
///   - Under a click-to-focus window manager, the WM itself may react to an agent click by
///     focusing/raising the target for the user too. WM-less servers (e.g. Xvfb in cloud
///     environments) have no such side effect.
/// - Linux Wayland and Windows: unsupported (this returns false); only whole-screen control is
///   available.
pub fn background_supported() -> bool {
    if cfg!(feature = "test-util") {
        noop::background_supported()
    } else {
        imp::background_supported()
    }
}

/// Ends the background computer-use session owned by `owner` (the client conversation id),
/// releasing the session state that outlives individual action batches.
///
/// On macOS a background session activates the target window and installs focus-suppression
/// taps; this tears down only the windows owned by `owner`, deactivates them, and re-activates the
/// app that was frontmost before the session, so the user's keystrokes return to where they were.
/// On Linux X11 a background session drives a session-scoped agent seat (a second input seat
/// shared across the session's action batches so state like a held mouse button mid-drag
/// survives between batches); this removes `owner`'s seat, its on-screen cursor, and any input
/// state it still holds. Scoping by owner keeps concurrent background sessions (e.g. another
/// conversation driving a different window) intact. Idempotent and a no-op when `owner` has no
/// active session, and on platforms without background per-window control.
///
/// Call this whenever a computer-use session ends — normal completion, cancellation, or teardown.
pub fn end_background_session(owner: &str) {
    #[cfg(any(macos, linux))]
    {
        imp::end_background_session(owner);
    }
    #[cfg(not(any(macos, linux)))]
    {
        let _ = owner;
    }
}

/// Enumerates the on-screen windows, returning their metadata so a caller can pick one to
/// target. Returns an empty list on platforms where window enumeration is unsupported.
pub fn enumerate_windows() -> Vec<WindowInfo> {
    #[cfg(any(macos, linux))]
    {
        imp::enumerate_windows()
    }
    #[cfg(not(any(macos, linux)))]
    {
        Vec::new()
    }
}

/// Experimental: lists on-screen windows as a formatted diagnostic string. macOS and Linux
/// (X11) only.
///
/// Unlike [`enumerate_windows`], which returns slim [`WindowInfo`] records for window selection
/// and wire serialization, this function returns richer data including window bounds, formatted
/// as a human-readable table for CLI debugging. The two use separate types intentionally:
/// [`WindowInfo`] is kept wire-safe and bounds-free; the diagnostic output carries bounds that
/// are not part of the API representation.
#[cfg(macos)]
pub fn experimental_list_windows() -> Result<String, String> {
    Ok(imp::list_windows())
}

/// Experimental: lists on-screen windows as a formatted diagnostic string. macOS and Linux
/// (X11) only.
#[cfg(linux)]
pub fn experimental_list_windows() -> Result<String, String> {
    imp::list_windows()
}

/// Experimental: lists on-screen windows. Unsupported on this platform.
#[cfg(not(any(macos, linux)))]
pub fn experimental_list_windows() -> Result<String, String> {
    Err("Window listing is only supported on macOS and Linux (X11).".to_string())
}

/// The surface that a computer-use action or screenshot targets.
///
/// `Screen` reproduces the legacy behavior of acting on the whole screen / frontmost
/// application. `Window` drives a specific background window of a specific process without
/// moving the global cursor or taking the user's keyboard focus. On macOS the window is never
/// raised; on Linux X11 pointer events are routed by screen position, so a window that is
/// covered at the action point is raised (without focus) before clicks and scrolls — see
/// [`background_supported`] for the full per-platform semantics.
#[derive(Debug, Clone, Copy, Default, Eq, PartialEq, Serialize, Deserialize)]
pub enum Target {
    /// Target the whole screen / frontmost application (legacy behavior).
    #[default]
    Screen,
    /// Target a specific background window of a specific process.
    Window {
        /// The platform window id (a `CGWindowID` on macOS, an X window id on Linux X11). Must
        /// be a concrete, non-zero id selected from the enumerated window list. `0` is the
        /// "unknown" sentinel and is rejected by the actor, since coordinate remapping and
        /// window capture both require a known window.
        window_id: u32,
        /// The pid of the process that owns the window. Used for event delivery on macOS;
        /// informational on Linux X11, where events are addressed by window id.
        pid: i32,
    },
}

/// An action paired with the surface it targets.
///
/// The target is carried per-action so a single batch can, in principle, drive more than one
/// window. An absent / `Screen` target reproduces the legacy whole-screen behavior.
#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct TargetedAction {
    pub action: Action,
    #[serde(default)]
    pub target: Target,
}

impl TargetedAction {
    /// Builds a screen-targeted action (legacy behavior).
    pub fn screen(action: Action) -> Self {
        Self {
            action,
            target: Target::Screen,
        }
    }
}

/// Metadata about an on-screen window, so a caller can select a window to target.
/// Mirrors the fields of the `WindowInfo` API message.
#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct WindowInfo {
    /// The platform window id (a `CGWindowID` on macOS, an X window id on Linux X11).
    pub window_id: u32,
    /// The pid of the process that owns the window.
    pub pid: i32,
    /// The owning application's name (e.g. "Arc", "Notes").
    pub app_name: String,
    /// The window title, if available.
    pub title: String,
    /// The window layer (0 is a normal application window).
    pub layer: i32,
}
/// Metadata describing a captured window screenshot.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct CapturedWindow {
    /// The platform window id that was captured.
    pub window_id: u32,
    /// The width of the native captured image, in pixels.
    pub width_px: i32,
    /// The height of the native captured image, in pixels.
    pub height_px: i32,
}

#[async_trait]
pub trait Actor: Send + Sync + 'static {
    /// Returns the platform that this actor is running on, if known.
    fn platform(&self) -> Option<Platform>;

    /// Records the owner of the background computer-use session this actor drives (the client
    /// conversation id), so that when the session ends [`end_background_session`] tears down only
    /// this owner's background-activation state and leaves concurrent sessions untouched. Set it
    /// before performing actions. Default no-op; only the macOS actor tracks per-session ownership.
    fn set_background_session_owner(&mut self, _owner: Option<String>) {}

    async fn perform_actions(
        &mut self,
        actions: &[TargetedAction],
        options: Options,
    ) -> Result<ActionResult, String>;
}

/// Whether capture completed normally or stopped before an explicit stop.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum RecordingCompletionStatus {
    Completed,
    StoppedEarly,
}

/// A key that can be pressed or released.
#[derive(Debug, Clone, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub enum Key {
    /// A platform-specific keycode. On macOS and Windows, this is a virtual keycode.
    /// On Linux, this is an X11 keysym.
    Keycode(i32),
    /// A character key (e.g., 'a', '+'). On Windows, `Key::Char` only supports characters in
    /// the Basic Multilingual Plane (BMP, `U+0000`–`U+FFFF`). Supplementary-plane characters
    /// (emoji, some CJK extension blocks, etc.) will return an error; use `TypeText` instead for
    /// those.
    Char(char),
}

/// The actions that an actor can perform on the computer.
#[serde_as]
#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub enum Action {
    Wait(#[serde_as(as = "DurationSecondsWithFrac<f64>")] std::time::Duration),
    MouseDown {
        button: MouseButton,
        #[serde(with = "Vector2IDef")]
        at: Vector2I,
    },
    MouseUp {
        button: MouseButton,
    },
    MouseMove {
        #[serde(with = "Vector2IDef")]
        to: Vector2I,
    },
    MouseWheel {
        #[serde(with = "Vector2IDef")]
        at: Vector2I,
        direction: ScrollDirection,
        distance: ScrollDistance,
    },
    TypeText {
        text: String,
    },
    KeyDown {
        key: Key,
    },
    KeyUp {
        key: Key,
    },
}

/// The direction of a scroll action.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
pub enum ScrollDirection {
    Up,
    Down,
    Left,
    Right,
}

/// The distance of a scroll action.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
pub enum ScrollDistance {
    /// Scroll by a number of pixels.
    Pixels(i32),
    /// Scroll by a number of discrete "clicks" (wheel notches).
    Clicks(i32),
}

/// A rectangular region defined by top-left and bottom-right corners.
/// Coordinates are physical pixels relative to the selected screenshot target.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
pub struct ScreenshotRegion {
    #[serde(with = "Vector2IDef")]
    pub top_left: Vector2I,
    #[serde(with = "Vector2IDef")]
    pub bottom_right: Vector2I,
}

impl ScreenshotRegion {
    /// Validates that the region has valid coordinates for screenshot capture.
    ///
    /// Returns an error if:
    /// - `top_left` has negative coordinates
    /// - `bottom_right` is not strictly greater than `top_left` in both dimensions
    pub fn validate(&self) -> Result<(), String> {
        if self.top_left.x() < 0 || self.top_left.y() < 0 {
            return Err(format!(
                "Screenshot region top_left must be non-negative, got ({}, {})",
                self.top_left.x(),
                self.top_left.y()
            ));
        }
        if self.bottom_right.x() <= self.top_left.x() {
            return Err(format!(
                "Screenshot region must have positive width (bottom_right.x {} must be > top_left.x {})",
                self.bottom_right.x(),
                self.top_left.x()
            ));
        }
        if self.bottom_right.y() <= self.top_left.y() {
            return Err(format!(
                "Screenshot region must have positive height (bottom_right.y {} must be > top_left.y {})",
                self.bottom_right.y(),
                self.top_left.y()
            ));
        }
        Ok(())
    }
}

/// Parameters for taking a screenshot after actions.
/// If provided, a screenshot will be taken; if `None`, no screenshot is taken.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
pub struct ScreenshotParams {
    /// The maximum length of the long edge of the screenshot in pixels.
    pub max_long_edge_px: Option<usize>,
    /// The maximum total number of pixels in the screenshot.
    pub max_total_px: Option<usize>,
    /// Optional sub-region of `target` to capture, in target-relative physical pixels.
    /// If `None`, captures the full target.
    #[serde(default)]
    pub region: Option<ScreenshotRegion>,
    /// The surface to capture. `Screen` captures the main display (legacy); `Window` captures
    /// a specific window's image.
    #[serde(default)]
    pub target: Target,
}

pub struct Options {
    /// If set, a screenshot will be captured after the actions are executed.
    /// The parameters specify what constraints, if any, to apply to the screenshot.
    pub screenshot_params: Option<ScreenshotParams>,
    /// Whether background, per-window computer use is enabled. When false, actors must behave
    /// exactly like the legacy full-screen path: any window target is ignored, only the main
    /// display is captured, and no window list or captured-window metadata is returned.
    pub background_enabled: bool,
}

/// The buttons of a mouse.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Serialize, Deserialize)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
    /// Mouse button 3 (Back).
    Back,
    /// Mouse button 4 (Forward).
    Forward,
}

/// The result of performing an action.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ActionResult {
    pub screenshot: Option<Screenshot>,
    pub cursor_position: Option<Vector2I>,
    /// The on-screen windows, refreshed after the actions run, so the caller always has a fresh
    /// list to target next. Empty on platforms without window enumeration.
    pub windows: Vec<WindowInfo>,
    /// Metadata about the captured window, populated only when a window target was
    /// screenshotted, so window-local coordinates map onto the screenshot image.
    pub captured_window: Option<CapturedWindow>,
}

impl ActionResult {
    /// Builds a result that carries no window list or captured-window metadata (used by
    /// platforms and code paths that do not support per-window targeting).
    pub fn legacy(screenshot: Option<Screenshot>, cursor_position: Option<Vector2I>) -> Self {
        Self {
            screenshot,
            cursor_position,
            windows: Vec::new(),
            captured_window: None,
        }
    }
}

/// A simple representation of a screenshot.
#[derive(Clone, Eq, PartialEq)]
pub struct Screenshot {
    /// The width of the screenshot image data in pixels.
    pub width: usize,
    /// The height of the screenshot image data in pixels.
    pub height: usize,
    /// The original width of the screenshot before any downscaling was applied.
    pub original_width: usize,
    /// The original height of the screenshot before any downscaling was applied.
    pub original_height: usize,
    // TODO(AGENT-2283): consider making this a type that is cheap to clone
    // (e.g.: `Arc<[u8]>`)
    pub data: Vec<u8>,
    pub mime_type: Cow<'static, str>,
}

impl std::fmt::Debug for Screenshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Screenshot")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("original_width", &self.original_width)
            .field("original_height", &self.original_height)
            .field("num_data_bytes", &self.data.len())
            .finish()
    }
}

/// Remote derive helper for `Vector2I` from `pathfinder_geometry`.
#[derive(Serialize, Deserialize)]
#[serde(remote = "Vector2I")]
struct Vector2IDef {
    #[serde(getter = "get_vector2i_x")]
    x: i32,
    #[serde(getter = "get_vector2i_y")]
    y: i32,
}

fn get_vector2i_x(v: &Vector2I) -> i32 {
    v.x()
}

fn get_vector2i_y(v: &Vector2I) -> i32 {
    v.y()
}

impl From<Vector2IDef> for Vector2I {
    fn from(def: Vector2IDef) -> Self {
        Vector2I::new(def.x, def.y)
    }
}
