use std::path::{Path, PathBuf};

use ai::skills::SkillPathOrigin;
use warp_util::local_or_remote_path::LocalOrRemotePath;

mod telemetry;
pub use telemetry::SkillOpenOrigin;
mod bundled;
pub use ai::skills::SkillReference;
#[cfg(test)]
pub(crate) use bundled::BundledSkillActivation;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillManagerEvent {
    SkillsChanged { home_skills_changed: bool },
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ActiveSkillLookupError {
    #[error("Bundled skills are not available on this remote session")]
    BundledSkillsUnavailable,
    #[error("Skill not found: {reference}")]
    NotFound { reference: SkillReference },
}

impl ActiveSkillLookupError {
    pub(crate) fn for_reference(reference: &SkillReference, path_origin: &SkillPathOrigin) -> Self {
        if matches!(path_origin, SkillPathOrigin::Unavailable)
            && matches!(reference, SkillReference::BundledSkillId(_))
        {
            Self::BundledSkillsUnavailable
        } else {
            Self::NotFound {
                reference: reference.clone(),
            }
        }
    }
}

mod listed_skill;
pub use listed_skill::SkillDescriptor;

mod skill_utils;
pub use skill_utils::{
    icon_override_for_skill_name, list_skills_if_changed, render_skill_button,
    skill_path_from_location,
};
pub trait SkillPathQuery {
    fn to_skill_location(&self) -> LocalOrRemotePath;
}

impl SkillPathQuery for LocalOrRemotePath {
    fn to_skill_location(&self) -> LocalOrRemotePath {
        self.clone()
    }
}

impl SkillPathQuery for Path {
    fn to_skill_location(&self) -> LocalOrRemotePath {
        LocalOrRemotePath::Local(self.to_path_buf())
    }
}

impl SkillPathQuery for PathBuf {
    fn to_skill_location(&self) -> LocalOrRemotePath {
        LocalOrRemotePath::Local(self.clone())
    }
}

mod skill_manager;
pub use skill_manager::SkillManager;
