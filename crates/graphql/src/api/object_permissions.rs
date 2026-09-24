use std::fmt;

use crate::schema;

#[derive(cynic::Enum, Clone, Copy, Debug)]
pub enum OwnerType {
    #[cynic(rename = "Team")]
    Team,
    #[cynic(rename = "User")]
    User,
}

impl fmt::Display for OwnerType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let owner_type = match self {
            OwnerType::Team => "Team",
            OwnerType::User => "Personal",
        };
        write!(f, "{owner_type}")
    }
}
