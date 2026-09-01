/// Identifies who a secret belongs to. Local-only now that the managed-secrets server
/// client is gone, but the shape is still used to label auth-secret entries in the UI.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum SecretOwner {
    CurrentUser,
    Team { team_uid: String },
}
