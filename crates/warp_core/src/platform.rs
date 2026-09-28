use line_ending::LineEnding;

#[derive(Debug, Clone)]
pub enum SessionPlatform {
    MSYS2,
    WSL,
    Native,
    /// A shell running inside a Linux Docker sandbox container.
    DockerSandbox,
}

impl SessionPlatform {
    #[allow(clippy::disallowed_methods)]
    pub fn default_line_ending(&self) -> LineEnding {
        match self {
            SessionPlatform::MSYS2 | SessionPlatform::WSL | SessionPlatform::DockerSandbox => {
                LineEnding::LF
            }
            SessionPlatform::Native => LineEnding::from_current_platform(),
        }
    }
}

/// An operating system a shell can run on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetOS {
    MacOS,
    Linux,
    Windows,
}

impl TargetOS {
    /// Returns the operating system this binary was built for, or `None` if it is not supported.
    pub fn current() -> Option<Self> {
        if cfg!(target_os = "macos") {
            Some(TargetOS::MacOS)
        } else if cfg!(any(target_os = "linux", target_os = "freebsd")) {
            Some(TargetOS::Linux)
        } else if cfg!(target_os = "windows") {
            Some(TargetOS::Windows)
        } else {
            None
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            TargetOS::MacOS => "MacOS",
            TargetOS::Linux => "Linux",
            TargetOS::Windows => "Windows",
        }
    }
}
