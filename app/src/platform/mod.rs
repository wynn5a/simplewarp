#[cfg(target_os = "macos")]
pub mod mac;
#[cfg(windows)]
pub mod windows;

pub fn init() {}
