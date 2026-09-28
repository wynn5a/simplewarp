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

/// Creates the native system clipboard implementation used by the GUI
/// platform delegate without requiring a graphical event loop.
pub fn create_system_clipboard() -> anyhow::Result<Box<dyn crate::Clipboard + Send>> {
    cfg_if::cfg_if! {
        if #[cfg(target_os = "macos")] {
            Ok(Box::new(mac::clipboard::Clipboard::new()?))
        } else if #[cfg(any(target_os = "linux", target_os = "freebsd"))] {
            Ok(Box::new(crate::windowing::winit::linux::LinuxClipboard::new()?))
        } else if #[cfg(target_os = "windows")] {
            Ok(Box::new(crate::windowing::winit::windows::WindowsClipboard::new()?))
        } else {
            anyhow::bail!("System clipboard is unavailable on this platform")
        }
    }
}

/// A trait for accessing internal per-platform concrete implementations
/// through a wrapper type.
#[allow(dead_code)]
trait AsInnerMut<Inner: ?Sized> {
    fn as_inner_mut(&mut self) -> &mut Inner;
}
