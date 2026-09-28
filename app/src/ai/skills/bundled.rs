use std::collections::HashMap;
use std::path::{Path, PathBuf};

use ai::skills::{ParsedSkill, SkillPathOrigin, SkillReference, parse_bundled_skill};
use futures::TryStreamExt;
use warp_core::channel::ChannelState;
use warp_core::ui::icons::Icon;
use warp_errors::report_error;
use warp_util::local_or_remote_path::LocalOrRemotePath;
use warpui::{AppContext, SingletonEntity};

use super::SkillDescriptor;
use crate::ai::mcp::{McpIntegration, TemplatableMCPServerManager};
use crate::keyboard::keybinding_file_path;
use crate::settings::user_preferences_toml_file_path;

/// Activation condition for a bundled skill.
#[derive(Debug, Clone)]
pub enum BundledSkillActivation {
    /// Always active.
    Always,
    /// Active only when a specific MCP server is running.
    RequiresMcp(McpIntegration),
    /// Active only when a specific file exists on disk.
    RequiresFile(PathBuf),
}

impl BundledSkillActivation {
    pub fn is_enabled(&self, ctx: &AppContext) -> bool {
        match self {
            Self::Always => true,
            Self::RequiresMcp(integration) => {
                TemplatableMCPServerManager::as_ref(ctx).is_mcp_server_running(*integration)
            }
            Self::RequiresFile(path) => path.exists(),
        }
    }
}

/// Catalog of bundled skills for the local host.
#[derive(Debug, Default)]
pub struct BundledSkills {
    local: BundledSkill,
}

impl BundledSkills {
    pub fn set_local(&mut self, bundled_skill: BundledSkill) {
        self.local = bundled_skill;
    }

    pub fn active_descriptors(
        &self,
        path_origin: &SkillPathOrigin,
        ctx: &AppContext,
    ) -> Vec<SkillDescriptor> {
        match path_origin {
            SkillPathOrigin::Local | SkillPathOrigin::RestoredDisplayOnly => {
                self.local.active_descriptors(ctx)
            }
            SkillPathOrigin::Unavailable => Vec::new(),
        }
    }

    pub fn reference_for_path(&self, path: &LocalOrRemotePath) -> Option<SkillReference> {
        self.local.reference_for_path(path)
    }

    pub fn local_skill(&self, id: &str) -> Option<&ParsedSkill> {
        self.local.skill(id)
    }

    pub fn active_skill(
        &self,
        id: &str,
        path_origin: &SkillPathOrigin,
        ctx: &AppContext,
    ) -> Option<&ParsedSkill> {
        self.for_path_origin(path_origin)?.active_skill(id, ctx)
    }

    /// Returns the bundled catalog selected by the execution path origin.
    fn for_path_origin(&self, path_origin: &SkillPathOrigin) -> Option<&BundledSkill> {
        match path_origin {
            SkillPathOrigin::Local | SkillPathOrigin::RestoredDisplayOnly => Some(&self.local),
            SkillPathOrigin::Unavailable => None,
        }
    }

    #[cfg(test)]
    pub fn insert_local_for_testing(
        &mut self,
        id: impl Into<String>,
        skill: ParsedSkill,
        activation: BundledSkillActivation,
    ) {
        self.local.insert_for_testing(id, skill, activation);
    }
}

/// One bundled skill definition with its activation condition and icon.
#[derive(Debug, Clone)]
struct BundledSkillDefinition {
    skill: ParsedSkill,
    activation: BundledSkillActivation,
    icon: Icon,
}

/// Skills bundled with Warp for a single host.
#[derive(Debug, Default)]
pub struct BundledSkill {
    definitions: HashMap<String, BundledSkillDefinition>,
}

impl BundledSkill {
    /// Detect all skill definitions bundled with Warp for the local host.
    pub async fn detect() -> Self {
        let Some(resources_dir) = warp_core::paths::bundled_resources_dir() else {
            return Self::default();
        };
        Self::detect_in_resources_dir(resources_dir).await
    }

    /// Detect all skill definitions under the given resources root on the
    /// local filesystem, rendering skill content against this host.
    async fn detect_in_resources_dir(resources_dir: PathBuf) -> Self {
        let (mut definitions, figma_definitions) = futures::join!(
            load_bundled_skill_definitions(&resources_dir),
            load_figma_skill_definitions(&resources_dir)
        );
        definitions.extend(figma_definitions);
        Self { definitions }
    }

    /// Returns descriptors for bundled skills whose activation conditions are met.
    pub fn active_descriptors(&self, ctx: &AppContext) -> Vec<SkillDescriptor> {
        self.definitions
            .iter()
            .filter(|(_, definition)| definition.activation.is_enabled(ctx))
            .map(|(id, definition)| {
                SkillDescriptor::new_bundled(id.clone(), definition.skill.clone(), definition.icon)
            })
            .collect()
    }

    /// Returns a bundled skill reference when the path belongs to a bundled skill.
    pub fn reference_for_path(&self, path: &LocalOrRemotePath) -> Option<SkillReference> {
        self.definitions
            .iter()
            .find(|(_, definition)| definition.skill.path == *path)
            .map(|(id, _)| SkillReference::BundledSkillId(id.clone()))
    }

    /// Returns a bundled skill definition by ID.
    pub fn skill(&self, id: &str) -> Option<&ParsedSkill> {
        self.definitions.get(id).map(|definition| &definition.skill)
    }

    /// Returns a bundled skill by ID only if its activation condition is met.
    pub fn active_skill(&self, id: &str, ctx: &AppContext) -> Option<&ParsedSkill> {
        let definition = self.definitions.get(id)?;
        definition
            .activation
            .is_enabled(ctx)
            .then_some(&definition.skill)
    }

    #[cfg(test)]
    pub fn insert_for_testing(
        &mut self,
        id: impl Into<String>,
        skill: ParsedSkill,
        activation: BundledSkillActivation,
    ) {
        let id = id.into();
        self.definitions.insert(
            id.clone(),
            BundledSkillDefinition {
                skill,
                activation,
                icon: icon_for_bundled_skill(&id),
            },
        );
    }
}

/// Load skill definitions bundled with Warp.
async fn load_bundled_skill_definitions(
    resources_dir: &Path,
) -> HashMap<String, BundledSkillDefinition> {
    let skills_dir = resources_dir.join("bundled").join("skills");
    read_bundled_skills(&skills_dir, resources_dir)
        .await
        .into_iter()
        .map(|(id, skill)| {
            let icon = icon_for_bundled_skill(&id);
            let activation = activation_for_bundled_skill(&id, resources_dir);
            let bundled = BundledSkillDefinition {
                skill,
                activation,
                icon,
            };
            (id, bundled)
        })
        .collect()
}

/// Load Figma-specific bundled skills from the `figma/` subdirectory.
async fn load_figma_skill_definitions(
    resources_dir: &Path,
) -> HashMap<String, BundledSkillDefinition> {
    let figma_skills_dir = resources_dir
        .join("bundled")
        .join("mcp_skills")
        .join("figma");
    read_bundled_skills(&figma_skills_dir, resources_dir)
        .await
        .into_iter()
        .map(|(id, skill)| {
            let bundled = BundledSkillDefinition {
                skill,
                activation: BundledSkillActivation::RequiresMcp(McpIntegration::Figma),
                icon: Icon::Figma,
            };
            (id, bundled)
        })
        .collect()
}

/// Read bundled skill definitions from the specified directory, rendering
/// handlebars variables against this host's filesystem (`resources_dir` is
/// the resources root the skills belong to).
///
/// Only ever runs against the local app's own bundled resources, so local `Path` semantics (this
/// OS's encoding) are correct here.
pub(crate) async fn read_bundled_skills(
    skills_dir: &Path,
    resources_dir: &Path,
) -> HashMap<String, ParsedSkill> {
    let mut skills = HashMap::new();

    let Ok(mut entries) = async_fs::read_dir(skills_dir).await else {
        return skills;
    };

    while let Ok(Some(entry)) = entries.try_next().await {
        let entry_path = entry.path();
        if !entry_path.is_dir() {
            continue;
        }

        let skill_file_path = entry_path.join("SKILL.md");
        let mut skill = match parse_bundled_skill(&skill_file_path) {
            Ok(skill) => skill,
            Err(err) => {
                report_error!(err.context(format!(
                    "Failed to parse bundled skill at {}",
                    skill_file_path.display()
                )));
                continue;
            }
        };

        // We use the directory name as the skill ID (guaranteed unique within bundled skills).
        let Some(skill_id) = entry_path.file_name().and_then(|s| s.to_str()) else {
            log::warn!("Could not resolve bundled skill ID, skipping skill");
            continue;
        };
        let context = build_bundled_skill_context(resources_dir, &entry_path);

        // Apply variable substitution to the skill content.
        skill.content = handlebars::render_template(&skill.content, &context);
        skills.insert(skill_id.to_owned(), skill);
    }

    log::info!("Read {} bundled skills", skills.len());

    skills
}

fn display_optional_path(path: Option<PathBuf>) -> String {
    path.unwrap_or_default().display().to_string()
}

/// Builds the context map for bundled skill variable substitution.
///
/// Supported variables:
/// - `{{warp_cli_binary_name}}` - The CLI binary name (e.g., `warp` or `warp-cli`)
/// - `{{warp_url_scheme}}` - The URL scheme (e.g., `simplewarp`, `warposs`)
/// - `{{settings_schema_path}}` - Path to the bundled JSON settings schema
/// - `{{skill_dir}}` - Path to the bundled skill's directory
/// - `{{settings_file_path}}` - Path to the user's settings TOML file
/// - `{{keybindings_file_path}}` - Path to the user's keybindings YAML file
/// - `{{gui_settings_file_path}}` - Path to the GUI settings TOML file
/// - `{{gui_mcp_config_file_path}}` - Path to the GUI global MCP config
pub(crate) fn build_bundled_skill_context(
    resources_dir: &Path,
    skill_dir: &Path,
) -> HashMap<String, String> {
    [
        (
            "warp_cli_binary_name".to_owned(),
            ChannelState::cli_command_name().to_owned(),
        ),
        (
            "warp_url_scheme".to_owned(),
            ChannelState::url_scheme().to_owned(),
        ),
        (
            "settings_file_path".to_owned(),
            user_preferences_toml_file_path().display().to_string(),
        ),
        (
            "keybindings_file_path".to_owned(),
            keybinding_file_path().display().to_string(),
        ),
        (
            "gui_settings_file_path".to_owned(),
            display_optional_path(
                warp_core::paths::gui_config_local_dir()
                    .map(|config_dir| config_dir.join("settings.toml")),
            ),
        ),
        (
            "gui_mcp_config_file_path".to_owned(),
            display_optional_path(warp_core::paths::gui_mcp_config_file_path()),
        ),
        (
            "settings_schema_path".to_owned(),
            resources_dir
                .join("settings_schema.json")
                .display()
                .to_string(),
        ),
        ("skill_dir".to_owned(), skill_dir.display().to_string()),
    ]
    .into_iter()
    .collect()
}

/// Returns the icon for a bundled skill, given its directory-based ID.
/// Skills with a known brand (e.g. `pr-comments` → GitHub) get a
/// branded icon; everything else falls back to the Warp logo.
pub(crate) fn icon_for_bundled_skill(skill_id: &str) -> Icon {
    match skill_id {
        "pr-comments" => Icon::Github,
        _ => Icon::WarpLogoLight,
    }
}

/// Returns the activation condition for a bundled skill.
///
/// Most skills are always active. Other skills appear only when their required
/// integration or bundled resource is available.
pub(crate) fn activation_for_bundled_skill(
    skill_id: &str,
    resources_dir: &Path,
) -> BundledSkillActivation {
    match skill_id {
        "modify-settings" => {
            BundledSkillActivation::RequiresFile(resources_dir.join("settings_schema.json"))
        }
        _ => BundledSkillActivation::Always,
    }
}

#[cfg(test)]
#[path = "bundled_tests.rs"]
mod tests;
