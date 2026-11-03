//! The user's rules and the agent's memories as they go into a request, and the project that a
//! memory belongs to.

use std::path::PathBuf;

use repo_metadata::repositories::DetectedRepositories;
use warp_util::local_or_remote_path::LocalOrRemotePath;
use warpui::{AppContext, SingletonEntity};

use crate::ai::facts::{AIFact, AIMemory, AIMemorySource, CloudAIFactModel};
use crate::cloud_object::model::generic_string_model::GenericStringObjectId;
use crate::cloud_object::model::persistence::CloudModel;
use crate::terminal::model::session::SessionType;

/// A rule as the model sees it: its title, then its content.
pub type RuleText = (String, String);

/// The title of a memory the agent saved without a name.
pub const AGENT_MEMORY_TITLE: &str = "Agent memory";

/// The title of a rule the user saved without a name.
const UNTITLED_RULE_TITLE: &str = "Rule";

/// The rules that go into one request. Each list is sorted, so that an unchanged set of rules
/// produces an unchanged prompt.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RequestMemories {
    /// Rules for all of the user's work.
    pub global: Vec<RuleText>,
    /// Rules for the project the request is made in, when it has any.
    pub project: Option<ProjectMemories>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectMemories {
    pub root_path: String,
    pub rules: Vec<RuleText>,
}

/// Picks the rules for a request made in `project_root`: those for all work, and those saved for
/// that project. Rules saved for other projects and blank rules are left out.
pub fn select_memories<'a>(
    memories: impl IntoIterator<Item = &'a AIMemory>,
    project_root: Option<&str>,
) -> RequestMemories {
    let mut global = Vec::new();
    let mut project = Vec::new();
    for memory in memories {
        if memory.content.trim().is_empty() {
            continue;
        }
        match memory.root_path.as_deref() {
            None => global.push(rule_text(memory)),
            Some(root) if Some(root) == project_root => project.push(rule_text(memory)),
            Some(_) => {}
        }
    }
    global.sort();
    project.sort();

    RequestMemories {
        global,
        project: project_root
            .filter(|_| !project.is_empty())
            .map(|root_path| ProjectMemories {
                root_path: root_path.to_owned(),
                rules: project,
            }),
    }
}

fn rule_text(memory: &AIMemory) -> RuleText {
    let title = match memory.name.as_deref().filter(|name| !name.is_empty()) {
        Some(name) => name.to_owned(),
        None => match memory.source {
            AIMemorySource::Agent => AGENT_MEMORY_TITLE.to_owned(),
            AIMemorySource::User => UNTITLED_RULE_TITLE.to_owned(),
        },
    };
    (title, memory.content.clone())
}

/// Whether a memory with the same content (ignoring surrounding whitespace) is already saved for
/// the same scope.
pub fn contains_memory<'a>(
    memories: impl IntoIterator<Item = &'a AIMemory>,
    content: &str,
    root_path: Option<&str>,
) -> bool {
    let content = content.trim();
    memories
        .into_iter()
        .any(|memory| memory.content.trim() == content && memory.root_path.as_deref() == root_path)
}

/// Every rule that has not been deleted.
pub fn saved_memories(app: &AppContext) -> impl Iterator<Item = &AIMemory> {
    CloudModel::as_ref(app)
        .get_all_objects_of_type::<GenericStringObjectId, CloudAIFactModel>()
        .filter(|fact| fact.metadata.trashed_ts.is_none())
        .map(|fact| {
            let AIFact::Memory(memory) = &fact.model().string_model;
            memory
        })
}

/// The root of the repository that contains `cwd`, as the path that project memories are saved
/// under. A session on another host has none: its `cwd` names a directory over there, which must
/// not be matched against the repositories on this machine.
pub fn project_root_for(
    cwd: &str,
    session_type: Option<&SessionType>,
    app: &AppContext,
) -> Option<String> {
    match session_type {
        Some(SessionType::WarpifiedRemote) => None,
        None | Some(SessionType::Local) => DetectedRepositories::as_ref(app)
            .get_root_for_path(&LocalOrRemotePath::Local(PathBuf::from(cwd)))
            .map(|root| root.display_path()),
    }
}

#[cfg(test)]
#[path = "agent_memory_tests.rs"]
mod tests;
