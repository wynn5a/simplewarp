#[path = "file_watchers/mod.rs"]
mod file_watchers;
use std::collections::{HashMap, HashSet};

use ai::skills::{ParsedSkill, SkillPathOrigin, SkillReference, SkillScope};
pub use file_watchers::{SkillWatcher, SkillWatcherEvent, extract_skill_parent_directory};
use warp_core::features::FeatureFlag;
use warp_util::local_or_remote_path::LocalOrRemotePath;
use warpui::{AppContext, Entity, ModelContext, ModelHandle, SingletonEntity};

use super::bundled::{BundledSkill, BundledSkills};
#[cfg(test)]
use super::bundled::{build_bundled_skill_context, read_bundled_skills};
use super::{ActiveSkillLookupError, SkillDescriptor, SkillManagerEvent, SkillPathQuery};
use crate::ai::skills::skill_utils::SkillDeduplicator;

pub struct SkillManager {
    /// Maps a directory path to the set of skill file paths defined in that directory.
    ///
    /// The key is the directory containing the `.agents/skills/` (or similar provider) folder,
    /// not the skills folder itself.
    ///
    /// Example: For a skill at `/repo/frontend/.agents/skills/deploy/SKILL.md`:
    /// - Key: `/repo/frontend`
    /// - Value (in the set): `/repo/frontend/.agents/skills/deploy/SKILL.md`
    ///
    /// NOT:
    /// - Key: `/repo/frontend/.agents/skills`
    directory_skills: HashMap<LocalOrRemotePath, HashSet<LocalOrRemotePath>>,
    skills_by_path: HashMap<LocalOrRemotePath, ParsedSkill>,
    /// Reverse lookup: skill name → set of paths with that name.
    /// This allows efficient lookup by skill name without scanning all paths.
    skills_by_name: HashMap<String, HashSet<LocalOrRemotePath>>,
    /// Skills bundled into Warp for the local host.
    bundled_skills: BundledSkills,
    #[allow(dead_code)]
    skill_watcher: ModelHandle<SkillWatcher>, // Can't remove this or it'll get cleaned up after new()
}

impl SkillManager {
    pub fn new(ctx: &mut ModelContext<Self>) -> Self {
        let (skill_watcher_tx, skill_watcher_rx) = async_channel::unbounded();

        ctx.spawn_stream_local(
            skill_watcher_rx,
            |me, message, ctx| {
                me.handle_skill_watcher_event(message, ctx);
            },
            |_, _| {}, // No cleanup needed when stream ends
        );

        // Create skill watcher
        let skill_watcher = ctx.add_model(|ctx| SkillWatcher::new(ctx, skill_watcher_tx));

        if FeatureFlag::BundledSkills.is_enabled() {
            ctx.spawn(BundledSkill::detect(), |me, result, _| {
                me.bundled_skills.set_local(result);
            });
        }

        Self {
            directory_skills: HashMap::new(),
            skills_by_path: HashMap::new(),
            skills_by_name: HashMap::new(),
            bundled_skills: BundledSkills::default(),
            skill_watcher,
        }
    }

    /// Returns skills available for the given working directory.
    pub fn get_skills_for_working_directory(
        &self,
        working_directory: Option<&LocalOrRemotePath>,
        ctx: &AppContext,
    ) -> Vec<SkillDescriptor> {
        self.get_skills_for_working_directory_with_origin(
            working_directory,
            &SkillPathOrigin::Local,
            ctx,
        )
    }

    /// Returns skills available for the given working directory and execution host.
    pub fn get_skills_for_working_directory_with_origin(
        &self,
        working_directory: Option<&LocalOrRemotePath>,
        path_origin: &SkillPathOrigin,
        ctx: &AppContext,
    ) -> Vec<SkillDescriptor> {
        // Collect file-backed skills for one shared deduplication pass. Home skills use
        // the home directory as their dir_path; project skills use their owning directory.
        let mut skill_paths = Vec::new();
        let mut deduplicator = SkillDeduplicator::default();

        if let Some(home_dir) = self.home_directory_for_origin(path_origin)
            && let Some(home_skill_paths) = self.directory_skills.get(&home_dir)
        {
            skill_paths.extend(
                home_skill_paths
                    .iter()
                    .cloned()
                    .map(|path| (home_dir.clone(), path)),
            );
        }

        if let Some(working_directory) = working_directory {
            let repo_root = repo_metadata::repositories::DetectedRepositories::as_ref(ctx)
                .get_root_for_path(working_directory);

            for (dir, dir_skill_paths) in &self.directory_skills {
                if self.is_home_directory(dir) {
                    continue;
                }
                // Only include skills from directories that are ancestors of the working directory
                // (or the working directory itself)
                if working_directory.starts_with(dir) {
                    // Also verify this directory is within the detected repo (if any)
                    if repo_root.as_ref().is_none_or(|root| dir.starts_with(root)) {
                        for path in dir_skill_paths {
                            skill_paths.push((dir.clone(), path.clone()));
                        }
                    }
                }
            }
        }

        // Deduplicate skills with identical content installed under the same directory across
        // multiple providers, keeping the skill from the highest-priority provider per
        // [`SKILL_PROVIDER_DEFINITIONS`].
        deduplicator.extend_paths(&skill_paths, &self.skills_by_path);
        let mut skills = deduplicator.into_descriptors();

        // Apply icon overrides for well-known skill names (e.g. partner integrations).
        for skill in &mut skills {
            if skill.icon_override.is_none() {
                skill.icon_override =
                    crate::ai::skills::skill_utils::icon_override_for_skill_name(&skill.name);
            }
        }

        // Append bundled skills whose activation condition is met. Only local execution hosts
        // see the local catalog; SSH sessions never see the local client's bundled skills.
        if FeatureFlag::BundledSkills.is_enabled() {
            skills.extend(self.bundled_skills.active_descriptors(path_origin, ctx));
        }

        skills
    }

    /// Returns a reference to a parsed skill for a specific SKILL.md file path, if it is cached.
    pub fn skill_by_path<P: SkillPathQuery + ?Sized>(
        &self,
        skill_path: &P,
    ) -> Option<&ParsedSkill> {
        let location = skill_path.to_skill_location();
        self.skill_by_location(&location)
    }

    /// Returns the appropriate `SkillReference` for a skill at the given path.
    /// For bundled skills, returns `BundledSkillId`; otherwise returns `Path`.
    pub fn reference_for_skill_path<P: SkillPathQuery + ?Sized>(
        &self,
        skill_path: &P,
    ) -> SkillReference {
        let skill_path = skill_path.to_skill_location();
        // Check if this path belongs to a bundled skill.
        if let Some(reference) = self.bundled_skills.reference_for_path(&skill_path) {
            return reference;
        }
        // Default to path-based reference.
        SkillReference::Path(skill_path)
    }

    /// Get the definition of a skill for the selected execution host only if it is active.
    ///
    /// Path-based user skills are always controlled by normal path scoping. Bundled skills
    /// additionally respect their runtime activation state so stale references cannot invoke
    /// disabled bundled skills.
    pub fn active_skill_by_reference_with_origin(
        &self,
        reference: &SkillReference,
        path_origin: &SkillPathOrigin,
        ctx: &AppContext,
    ) -> Result<&ParsedSkill, ActiveSkillLookupError> {
        let skill = match reference {
            SkillReference::Path(path) => self.skills_by_path.get(path),
            SkillReference::BundledSkillId(id) => {
                self.bundled_skills.active_skill(id, path_origin, ctx)
            }
        };
        skill.ok_or_else(|| ActiveSkillLookupError::for_reference(reference, path_origin))
    }

    /// Returns a local bundled skill by ID only if its activation condition is met.
    pub fn active_local_bundled_skill(&self, id: &str, ctx: &AppContext) -> Option<&ParsedSkill> {
        self.bundled_skills
            .active_skill(id, &SkillPathOrigin::Local, ctx)
    }

    fn home_directory_for_origin(
        &self,
        path_origin: &SkillPathOrigin,
    ) -> Option<LocalOrRemotePath> {
        match path_origin {
            SkillPathOrigin::Local => dirs::home_dir().map(LocalOrRemotePath::Local),
            SkillPathOrigin::RestoredDisplayOnly | SkillPathOrigin::Unavailable => None,
        }
    }

    fn is_home_directory(&self, path: &LocalOrRemotePath) -> bool {
        match path {
            LocalOrRemotePath::Local(path) => dirs::home_dir().as_ref() == Some(path),
        }
    }

    fn skill_by_location(&self, location: &LocalOrRemotePath) -> Option<&ParsedSkill> {
        self.skills_by_path.get(location)
    }

    fn remove_skill_by_path(&mut self, skill_path: &LocalOrRemotePath) {
        let Some(skill) = self.skills_by_path.remove(skill_path) else {
            return;
        };
        let remove_name = self
            .skills_by_name
            .get_mut(&skill.name)
            .is_some_and(|paths| {
                paths.remove(skill_path);
                paths.is_empty()
            });
        if remove_name {
            self.skills_by_name.remove(&skill.name);
        }
    }

    fn handle_skill_watcher_event(
        &mut self,
        event: SkillWatcherEvent,
        ctx: &mut ModelContext<Self>,
    ) {
        let home_skills_changed = match &event {
            SkillWatcherEvent::SkillsAdded { skills } => {
                skills.iter().any(|skill| skill.scope == SkillScope::Home)
            }
            SkillWatcherEvent::SkillsDeleted { paths } => paths.iter().any(|path| {
                self.skills_by_path.values().any(|skill| {
                    skill.scope == SkillScope::Home
                        && (skill.path.starts_with(path) || path.starts_with(&skill.path))
                })
            }),
        };
        match event {
            SkillWatcherEvent::SkillsAdded { skills } => {
                self.handle_skills_added(skills);
            }
            SkillWatcherEvent::SkillsDeleted { paths } => {
                self.handle_skills_deleted(paths);
            }
        }
        ctx.emit(SkillManagerEvent::SkillsChanged {
            home_skills_changed,
        });
    }

    pub fn handle_skills_added(&mut self, skills: Vec<ParsedSkill>) {
        for skill in skills {
            match extract_skill_parent_directory(&skill.path) {
                Ok(parent_dir) => {
                    self.directory_skills
                        .entry(parent_dir)
                        .or_default()
                        .insert(skill.path.clone());

                    self.skills_by_name
                        .entry(skill.name.clone())
                        .or_default()
                        .insert(skill.path.clone());
                    self.skills_by_path.insert(skill.path.clone(), skill);
                }
                _ => {
                    log::warn!(
                        "Could not extract parent directory for skill: {:?}",
                        skill.path
                    );
                }
            }
        }
    }

    /// Registers skills loaded from `WARP_SKILL_DIRS` environment variable directories
    /// as personal (home) tier skills.
    ///
    /// Unlike [`handle_skills_added`], this method does not require each skill's path
    /// to follow a known provider directory structure. Skills are stored directly
    /// under the local home directory bucket so they are always in scope—the same
    /// precedence as `~/.agents/skills` and other personal skills.
    ///
    /// Call this after reading skills with [`ai::skills::read_skills_for_skills_dirs`].
    pub fn add_skills_dirs_skills(&mut self, skills: Vec<ParsedSkill>) {
        let Some(home_dir) = dirs::home_dir() else {
            log::warn!("WARP_SKILL_DIRS: home directory unavailable; cannot register env skills");
            return;
        };
        let home_dir = LocalOrRemotePath::Local(home_dir);
        for skill in skills {
            self.directory_skills
                .entry(home_dir.clone())
                .or_default()
                .insert(skill.path.clone());
            self.skills_by_name
                .entry(skill.name.clone())
                .or_default()
                .insert(skill.path.clone());
            self.skills_by_path.insert(skill.path.clone(), skill);
        }
    }

    fn handle_skills_deleted(&mut self, paths: Vec<LocalOrRemotePath>) {
        for path in paths {
            self.handle_path_deleted(&path);
        }
    }

    fn handle_path_deleted(&mut self, path: &LocalOrRemotePath) {
        // Delete all skills that are affected by this deleted path
        for (dir, skill_paths) in &self.directory_skills.clone() {
            if dir.starts_with(path) {
                // Delete this entire entry and remove all skill_paths under this directory from cache
                for skill_path in skill_paths {
                    self.remove_skill_by_path(skill_path);
                }
                self.directory_skills.remove(dir);
            } else if path.starts_with(dir) {
                // Remove all skills under this directory that is a child of the deleted path
                for skill_path in skill_paths {
                    if skill_path.starts_with(path) {
                        self.remove_skill_by_path(skill_path);
                        self.directory_skills
                            .entry(dir.clone())
                            .or_default()
                            .remove(skill_path);
                    }
                }
            }
        }
    }
}
impl Entity for SkillManager {
    type Event = SkillManagerEvent;
}

impl SingletonEntity for SkillManager {}

#[cfg(test)]
#[path = "skill_manager_tests.rs"]
mod tests;
