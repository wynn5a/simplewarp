use std::cmp::Ordering;

use serde::{Deserialize, Serialize};

use super::workspace::BillingMetadata;
use crate::auth::UserUid;
use crate::server::ids::ServerId;

#[derive(Clone, Copy, Eq, PartialEq, Debug, Serialize, Deserialize)]
pub enum MembershipRole {
    Owner,
    Admin,
    User,
}

impl MembershipRole {
    pub fn is_owner(&self) -> bool {
        matches!(self, MembershipRole::Owner)
    }
}

#[derive(Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
pub struct TeamMember {
    pub uid: UserUid,
    pub email: String,
    pub role: MembershipRole,
}

impl PartialOrd for TeamMember {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for TeamMember {
    fn cmp(&self, other: &Self) -> Ordering {
        self.email.cmp(&other.email)
    }
}

#[derive(Clone, Debug)]
pub struct Team {
    pub uid: ServerId,
    pub name: String,
    /// The team's brand color as a hex string (e.g. "#7c3aed"), if set by the team admin.
    pub color: Option<String>,
    pub members: Vec<TeamMember>,
    pub billing_metadata: BillingMetadata,
}

impl Team {
    pub fn from_local_cache(
        uid: ServerId,
        name: String,
        billing_metadata: Option<BillingMetadata>,
        members: Option<Vec<TeamMember>>,
    ) -> Self {
        Self {
            uid,
            name,
            color: None,
            members: members.unwrap_or_default(),
            billing_metadata: billing_metadata.unwrap_or_default(),
        }
    }

    fn get_member_by_email(&self, email: &str) -> Option<&TeamMember> {
        self.members.iter().find(|member| member.email == email)
    }

    pub fn is_multi_admin_enabled(&self) -> bool {
        self.billing_metadata
            .tier
            .multi_admin_policy
            .is_some_and(|policy| policy.enabled)
    }

    pub fn has_admin_permissions(&self, user_email: &str) -> bool {
        self.get_member_by_email(user_email).is_some_and(|member| {
            member.role.is_owner()
                || (member.role == MembershipRole::Admin && self.is_multi_admin_enabled())
        })
    }
}
