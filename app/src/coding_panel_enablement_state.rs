#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CodingPanelEnablementState {
    Enabled,
    /// An SSH command has been detected at preexec time but the remote
    /// session has not finished bootstrapping yet. The file tree should
    /// show a loading state immediately to avoid flickering the stale
    /// local tree.
    PendingRemoteSession,
    /// The active session is on a remote host.
    RemoteSession,
    UnsupportedSession,
    Disabled,
}

impl CodingPanelEnablementState {
    pub(crate) fn from_session_env(
        is_enabled: bool,
        is_remote: bool,
        is_unsupported_session: bool,
    ) -> Self {
        if is_remote {
            Self::RemoteSession
        } else if is_unsupported_session {
            Self::UnsupportedSession
        } else if is_enabled {
            Self::Enabled
        } else {
            Self::Disabled
        }
    }
}
