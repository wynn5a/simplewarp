pub mod app;
#[cfg(any(target_os = "linux", target_os = "freebsd"))]
pub mod linux;
#[cfg(target_os = "macos")]
pub mod mac;
#[cfg(target_os = "windows")]
pub mod windows;

pub mod headless;

pub mod current {
    cfg_if::cfg_if! {
        if #[cfg(any(target_os = "linux", target_os = "freebsd"))] {
            pub use super::linux::*;
        } else if #[cfg(target_os = "macos")] {
            pub use super::mac::*;
        } else if #[cfg(target_os = "windows")] {
            pub use super::windows::*;
        } else {
            pub use warpui_core::platform::test::*;
        }
    }
}

pub use app::AppBuilder;
pub use warpui_core::platform::*;

/// A trait for accessing internal per-platform concrete implementations
/// through a wrapper type.
trait AsInnerMut<Inner: ?Sized> {
    fn as_inner_mut(&mut self) -> &mut Inner;
}
