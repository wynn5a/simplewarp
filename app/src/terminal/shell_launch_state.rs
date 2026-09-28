use super::available_shells::AvailableShell;
use super::shell::ShellName;
use crate::terminal::shell::ShellType;

/// The current state of launching a shell.
#[derive(Clone, Debug)]
pub enum ShellLaunchState {
    /// We are still determining the type of shell.
    DeterminingShell {
        /// `None` for mock terminals (e.g. transcript viewers) that have no real shell.
        available_shell: Option<AvailableShell>,
        display_name: ShellName,
    },
    /// We are spawning a shell of [`ShellType`].
    ShellSpawned {
        /// `None` for mock terminals (e.g. transcript viewers) that have no real shell.
        available_shell: Option<AvailableShell>,
        display_name: ShellName,
        shell_type: ShellType,
    },
}

impl ShellLaunchState {
    pub fn display_name(&self) -> &str {
        match self {
            Self::DeterminingShell { display_name, .. } => display_name,
            Self::ShellSpawned {
                display_name,
                shell_type,
                ..
            } => match display_name {
                ShellName::MoreDescriptive(name) => name,
                ShellName::LessDescriptive(_) => shell_type.name(),
            },
        }
    }

    pub fn available_shell(&self) -> Option<AvailableShell> {
        match self {
            Self::DeterminingShell {
                available_shell, ..
            }
            | Self::ShellSpawned {
                available_shell, ..
            } => available_shell.clone(),
        }
    }

    pub fn spawned_with_shell_type(self, shell_type: ShellType) -> Self {
        match self {
            Self::DeterminingShell {
                available_shell,
                display_name,
            }
            | Self::ShellSpawned {
                available_shell,
                display_name,
                ..
            } => Self::ShellSpawned {
                available_shell,
                display_name,
                shell_type,
            },
        }
    }
}
