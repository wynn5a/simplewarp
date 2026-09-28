use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use anyhow::Result;
use futures::future::BoxFuture;
use repo_metadata::{
    RepoMetadataEvent, RepoMetadataModel, RepositoryIdentifier, StandingQueryContent,
};
use warp_util::local_or_remote_path::LocalOrRemotePath;
use warp_util::standardized_path::StandardizedPath;
use warpui_core::{Entity, ModelContext, SingletonEntity};

use super::GlobalRules;

pub type ProjectRuleContents = Vec<(LocalOrRemotePath, String)>;
/// App-provided reader for the exact rule paths discovered by repository metadata.
pub type ProjectRuleContentReader =
    fn(Vec<LocalOrRemotePath>) -> BoxFuture<'static, anyhow::Result<ProjectRuleContents>>;

fn standing_project_rule_paths<'a>(
    repo_id: &RepositoryIdentifier,
    contents: impl IntoIterator<Item = &'a StandingQueryContent>,
) -> Vec<LocalOrRemotePath> {
    contents
        .into_iter()
        .filter(|content| !content.is_directory)
        .filter_map(|content| match repo_id {
            RepositoryIdentifier(_) => content.path.to_local_path().map(LocalOrRemotePath::Local),
        })
        .collect()
}

#[derive(Debug, Clone)]
pub struct ProjectRule {
    pub path: LocalOrRemotePath,
    pub content: String,
}

#[derive(Debug, Clone)]
struct RuleAtPath {
    parent_path: LocalOrRemotePath,
    warp_md: Option<ProjectRule>,
    agents_md: Option<ProjectRule>,
}

impl RuleAtPath {
    fn respected_rule(&self) -> Option<&ProjectRule> {
        self.warp_md.as_ref().or(self.agents_md.as_ref())
    }
}

#[derive(Debug, Clone)]
pub struct ProjectRulesResult {
    pub root_path: LocalOrRemotePath,
    pub active_rules: Vec<ProjectRule>,
    pub additional_rule_paths: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectRulePath {
    pub path: PathBuf,
    pub project_root: PathBuf,
}

struct FindRulesResult {
    /// Rules that are active and should be eagerly applied.
    active_rules: Vec<ProjectRule>,
    /// Rule paths that are currently not active but available to be applied if
    /// a file under its directory is edited.
    available_rule_paths: Vec<String>,
}

#[derive(Debug, Default, Clone)]
struct ProjectRules {
    rules: Vec<RuleAtPath>,
}

impl ProjectRules {
    fn rule_paths(&self) -> impl Iterator<Item = &LocalOrRemotePath> {
        self.rules.iter().flat_map(|rule| {
            rule.warp_md
                .iter()
                .chain(rule.agents_md.iter())
                .map(|rule| &rule.path)
        })
    }
    fn local_rule_paths(&self) -> impl Iterator<Item = PathBuf> + '_ {
        self.rule_paths()
            .filter_map(|path| path.to_local_path().map(Path::to_path_buf))
    }
    fn retain_rule_paths(&mut self, retained_paths: &HashSet<LocalOrRemotePath>) {
        self.rules.retain_mut(|rule| {
            if rule
                .warp_md
                .as_ref()
                .is_some_and(|rule| !retained_paths.contains(&rule.path))
            {
                rule.warp_md = None;
            }
            if rule
                .agents_md
                .as_ref()
                .is_some_and(|rule| !retained_paths.contains(&rule.path))
            {
                rule.agents_md = None;
            }
            rule.warp_md.is_some() || rule.agents_md.is_some()
        });
    }
    /// Finds the set of rules that are active in the given path and the set that are available to be applied.
    fn find_active_or_applicable_rules(&self, path: &LocalOrRemotePath) -> FindRulesResult {
        let mut active_rules = Vec::new();
        let mut available_rule_paths = Vec::new();

        // Collect all applicable rules (rules in directories that are ancestors of the target path)
        for rule in &self.rules {
            if let Some(respected_rule) = rule.respected_rule() {
                // Check if the rule's directory is an ancestor of or equal to the target path
                if path.starts_with(&rule.parent_path) {
                    active_rules.push(respected_rule.clone());
                } else {
                    available_rule_paths.push(respected_rule.path.display_path());
                }
            }
        }

        FindRulesResult {
            active_rules,
            available_rule_paths,
        }
    }

    /// Upsert a rule to the set of project rules. This will create a new RuleAtPath entry if none exists and update the existing one
    /// otherwise.
    fn upsert_rule(&mut self, path: &LocalOrRemotePath, content: String) {
        let Some(parent) = path.parent() else {
            return;
        };
        let Some(file_name) = path.file_name() else {
            return;
        };

        let existing_rule = self
            .rules
            .iter_mut()
            .find(|rule| rule.parent_path == parent);

        let rule_file = Some(ProjectRule {
            path: path.clone(),
            content,
        });

        match existing_rule {
            Some(rule) => {
                if file_name.to_lowercase() == "warp.md" {
                    rule.warp_md = rule_file;
                } else if file_name.to_lowercase() == "agents.md" {
                    rule.agents_md = rule_file;
                }
            }
            None => {
                let mut rule = RuleAtPath {
                    parent_path: parent,
                    warp_md: None,
                    agents_md: None,
                };
                if file_name.to_lowercase() == "warp.md" {
                    rule.warp_md = rule_file;
                } else if file_name.to_lowercase() == "agents.md" {
                    rule.agents_md = rule_file;
                }
                self.rules.push(rule);
            }
        };
    }
}

/// Singleton model that keeps track of mapping between paths and rule files
/// Currently supports WARP.md files, but designed to be extensible
#[derive(Default)]
pub struct ProjectContextModel {
    /// Mapping from directory path to list of rule files found in that directory
    path_to_rules: HashMap<LocalOrRemotePath, ProjectRules>,
    /// Generation of the current (or most recently completed) refresh per repository. Guards
    /// against applying a result for a repository that was removed, or otherwise invalidated,
    /// while its refresh was in flight. The entry for a repository is cleared once its
    /// current-generation refresh completes.
    rule_refresh_generations: HashMap<RepositoryIdentifier, u64>,
    next_rule_refresh_generation: u64,
    /// Repositories with a rule-refresh read currently running in the background.
    ///
    /// The underlying file reads (`async_fs::read_to_string`, via a blocking thread pool) cannot
    /// be cancelled once started, so aborting the outer future doesn't stop them. Instead, this
    /// set is used to serialize refreshes per repository: while one is in flight, a superseding
    /// request is recorded in `rule_refresh_pending` rather than spawning an overlapping read.
    rule_refresh_in_flight: HashSet<RepositoryIdentifier>,
    /// Repositories that received a refresh request while a refresh was already in flight. The
    /// in-flight read's completion consults this to run exactly one coalesced follow-up refresh
    /// against the latest standing results, instead of one read per superseding event.
    rule_refresh_pending: HashSet<RepositoryIdentifier>,
    /// File-based global rules and their local watcher state. Kept separate
    /// from `path_to_rules`, which is project-scoped.
    pub(super) global_rules: GlobalRules,
}

#[derive(Default, Debug)]
pub struct RulesDelta {
    pub discovered_rules: Vec<ProjectRulePath>,
    pub deleted_rules: Vec<PathBuf>,
}

impl RulesDelta {
    /// Merge another delta into this one, preserving the ordering of operations.
    ///
    /// When the same path appears across sequential deltas the *last* operation
    /// wins. For example:
    ///   - (add A, delete A) → net effect is **delete**
    ///   - (delete A, add A) → net effect is **add**
    ///
    /// This is important because consumers (e.g. persistence) apply the delta
    /// incrementally; a symmetric "cancel both sides" approach would silently
    /// drop real state changes.
    #[cfg(test)]
    fn merge(&mut self, other: RulesDelta) {
        // Each newly-discovered path supersedes any prior deletion or earlier
        // discovery of the same path.
        for discovered in &other.discovered_rules {
            self.deleted_rules.retain(|p| *p != discovered.path);
            self.discovered_rules.retain(|r| r.path != discovered.path);
        }
        // Each newly-deleted path supersedes any prior discovery or earlier
        // deletion of the same path.
        for deleted in &other.deleted_rules {
            self.discovered_rules.retain(|r| r.path != *deleted);
            self.deleted_rules.retain(|p| *p != *deleted);
        }
        self.discovered_rules.extend(other.discovered_rules);
        self.deleted_rules.extend(other.deleted_rules);
    }
}

#[derive(Default, Debug)]
pub struct GlobalRulesDelta {
    pub discovered_rules: Vec<PathBuf>,
    pub deleted_rules: Vec<PathBuf>,
}

/// Events emitted by the ProjectContextModel
pub enum ProjectContextModelEvent {
    /// Emitted when a path has been indexed
    PathIndexed,
    /// Emitted when the known set of rule files changed
    KnownRulesChanged(RulesDelta),
    /// Emitted when the set of indexed global rule files changed
    GlobalRulesChanged(GlobalRulesDelta),
}

impl ProjectContextModel {
    pub fn new_from_persisted(
        persisted_rules: Vec<ProjectRulePath>,
        project_rule_content_reader: ProjectRuleContentReader,
        ctx: &mut ModelContext<Self>,
    ) -> Self {
        let model = Self::default();
        {
            ctx.subscribe_to_model(&RepoMetadataModel::handle(ctx), move |me, _, event, ctx| {
                match event {
                    RepoMetadataEvent::RepositoryUpdated { id } => {
                        me.refresh_project_rules_for_repo(
                            id.clone(),
                            project_rule_content_reader,
                            ctx,
                        );
                    }
                    RepoMetadataEvent::StandingQueryResultsUpdated { id, delta } => {
                        if delta.project_rules_changed() {
                            me.refresh_project_rules_for_repo(
                                id.clone(),
                                project_rule_content_reader,
                                ctx,
                            );
                        }
                    }
                    RepoMetadataEvent::RepositoryRemoved { id } => {
                        me.remove_project_rules_for_repo(id, ctx);
                    }
                    RepoMetadataEvent::FileTreeUpdated { .. }
                    | RepoMetadataEvent::FileTreeEntryUpdated { .. }
                    | RepoMetadataEvent::UpdatingRepositoryFailed { .. } => {}
                }
            });

            ctx.spawn(
                async move { Self::read_persisted_rules(persisted_rules).await },
                |me, mut res, ctx| {
                    // Metadata refreshes may have completed before persistence loads; retain
                    // the fresher metadata-backed state for overlapping roots.
                    res.extend(me.path_to_rules.drain());
                    me.path_to_rules = res;
                    ctx.emit(ProjectContextModelEvent::PathIndexed);
                },
            );
        }

        model
    }

    /// Reconciles project rule contents from the repository metadata standing result set.
    pub fn index_and_store_rules(
        &mut self,
        root_path: PathBuf,
        project_rule_content_reader: ProjectRuleContentReader,
        ctx: &mut ModelContext<Self>,
    ) -> Result<()> {
        {
            let repo_path = StandardizedPath::from_local_canonicalized(&root_path)?;
            let repo_id = RepositoryIdentifier::local(repo_path.clone());
            if RepoMetadataModel::as_ref(ctx)
                .standing_query_results(&repo_id, ctx)
                .is_none()
            {
                RepoMetadataModel::handle(ctx).update(ctx, |metadata, ctx| {
                    metadata.index_lazy_loaded_path(&repo_path, ctx)
                })?;
            }
            self.refresh_project_rules_for_repo(repo_id, project_rule_content_reader, ctx);
        }
        Ok(())
    }

    /// Requests a rule refresh for `repo_id`, reading the currently standing rule paths.
    ///
    /// If a refresh is already reading files for this repo, the request is coalesced: it is
    /// recorded in `rule_refresh_pending` and a single follow-up refresh runs once the in-flight
    /// read completes, instead of spawning a second, overlapping read.
    fn refresh_project_rules_for_repo(
        &mut self,
        repo_id: RepositoryIdentifier,
        project_rule_content_reader: ProjectRuleContentReader,
        ctx: &mut ModelContext<Self>,
    ) {
        if repo_id.to_local_or_remote_path().is_none() {
            return;
        };
        if self.rule_refresh_in_flight.contains(&repo_id) {
            self.rule_refresh_pending.insert(repo_id);
            return;
        }
        self.start_rule_refresh(repo_id, project_rule_content_reader, ctx);
    }

    /// Spawns the background read for a rule refresh. Callers must first confirm via
    /// `rule_refresh_in_flight` that no refresh is already running for `repo_id`.
    fn start_rule_refresh(
        &mut self,
        repo_id: RepositoryIdentifier,
        project_rule_content_reader: ProjectRuleContentReader,
        ctx: &mut ModelContext<Self>,
    ) {
        self.rule_refresh_in_flight.insert(repo_id.clone());
        self.rule_refresh_pending.remove(&repo_id);

        let rule_paths = standing_project_rule_paths(
            &repo_id,
            RepoMetadataModel::as_ref(ctx)
                .standing_query_results(&repo_id, ctx)
                .into_iter()
                .flat_map(|results| results.project_rules()),
        );
        let read_rule_contents = project_rule_content_reader(rule_paths.clone());

        self.next_rule_refresh_generation += 1;
        let refresh_generation = self.next_rule_refresh_generation;
        self.rule_refresh_generations
            .insert(repo_id.clone(), refresh_generation);
        let repo_id_for_result = repo_id.clone();
        ctx.spawn(read_rule_contents, move |me, result, ctx| {
            me.rule_refresh_in_flight.remove(&repo_id_for_result);
            // Guards against applying a result for a repository that was removed (or
            // otherwise invalidated) while this refresh was reading files.
            if me.rule_refresh_generations.get(&repo_id_for_result) == Some(&refresh_generation) {
                me.rule_refresh_generations.remove(&repo_id_for_result);
                match result {
                    Ok(contents) => {
                        if let Some(project_root) = repo_id_for_result.to_local_or_remote_path() {
                            let existing_rules = me
                                .path_to_rules
                                .get(&project_root)
                                .cloned()
                                .unwrap_or_default();
                            let rules =
                                Self::reconcile_project_rules(rule_paths, contents, existing_rules);
                            me.apply_project_rules(repo_id_for_result.clone(), rules, ctx);
                        }
                    }
                    Err(error) => log::warn!("Failed to read project rules: {error}"),
                }
            }
            // A refresh requested while this read was in flight couldn't be served by it (the
            // standing results may have changed since this read started), so run exactly one
            // coalesced follow-up against the latest standing results.
            if me.rule_refresh_pending.remove(&repo_id_for_result) {
                me.start_rule_refresh(repo_id_for_result, project_rule_content_reader, ctx);
            }
        });
    }

    fn reconcile_project_rules(
        rule_paths: Vec<LocalOrRemotePath>,
        rule_contents: ProjectRuleContents,
        mut existing_rules: ProjectRules,
    ) -> ProjectRules {
        let retained_paths = rule_paths.iter().cloned().collect::<HashSet<_>>();
        existing_rules.retain_rule_paths(&retained_paths);
        for (path, content) in rule_contents {
            existing_rules.upsert_rule(&path, content);
        }
        existing_rules
    }

    fn remove_project_rules_for_repo(
        &mut self,
        repo_id: &RepositoryIdentifier,
        ctx: &mut ModelContext<Self>,
    ) {
        self.rule_refresh_generations.remove(repo_id);
        self.rule_refresh_pending.remove(repo_id);
        let Some(project_root) = repo_id.to_local_or_remote_path() else {
            return;
        };
        if let Some(rules) = self.path_to_rules.remove(&project_root) {
            let deleted_rules = rules.local_rule_paths().collect();
            ctx.emit(ProjectContextModelEvent::KnownRulesChanged(RulesDelta {
                discovered_rules: Vec::new(),
                deleted_rules,
            }));
            ctx.emit(ProjectContextModelEvent::PathIndexed);
        }
    }

    fn apply_project_rules(
        &mut self,
        repo_id: RepositoryIdentifier,
        rules: ProjectRules,
        ctx: &mut ModelContext<Self>,
    ) {
        let Some(project_root) = repo_id.to_local_or_remote_path() else {
            return;
        };
        let RepositoryIdentifier(local_root) = &repo_id;
        let Some(local_root) = local_root.to_local_path() else {
            return;
        };
        let new_paths = rules.local_rule_paths().collect::<Vec<_>>();
        let previous = self
            .path_to_rules
            .insert(project_root, rules)
            .unwrap_or_default();
        let deleted_rules = previous
            .local_rule_paths()
            .filter(|path| !new_paths.contains(path))
            .collect();
        let discovered_rules = new_paths
            .into_iter()
            .map(|path| ProjectRulePath {
                path,
                project_root: local_root.clone(),
            })
            .collect();
        ctx.emit(ProjectContextModelEvent::KnownRulesChanged(RulesDelta {
            discovered_rules,
            deleted_rules,
        }));
        ctx.emit(ProjectContextModelEvent::PathIndexed);
    }

    /// Index all configured global rule sources.
    ///
    /// `ProjectContextModel` remains the public rule-context facade; the
    /// global source registry, cache, and watcher plumbing live in
    /// `global_rules`.
    pub fn index_global_rules(&mut self, ctx: &mut ModelContext<Self>) {
        self.global_rules.index(ctx);
    }

    /// Project-only rule lookup. Returns `Some` only when an indexed project
    /// root above `path` actually contributes a rule — globals are
    /// deliberately ignored.
    ///
    /// Use this for callers that need a project-initialization signal rather
    /// than the full rule context sent to agents.
    pub fn find_applicable_project_rules(
        &self,
        path: &LocalOrRemotePath,
    ) -> Option<ProjectRulesResult> {
        let mut current_path = path.clone();

        // Walk upwards from `path` toward the filesystem root, stopping at the
        // first directory we have indexed project rules for. `path_to_rules`
        // is keyed by indexed project root, so popping the path produces
        // every ancestor directory until we hit a known root or `pop()`
        // returns false (we've reached the top of the path).
        loop {
            if let Some(rules) = self.path_to_rules.get(&current_path) {
                let result = rules.find_active_or_applicable_rules(path);
                if result.active_rules.is_empty() && result.available_rule_paths.is_empty() {
                    return None;
                }
                return Some(ProjectRulesResult {
                    root_path: current_path,
                    active_rules: result.active_rules,
                    additional_rule_paths: result.available_rule_paths,
                });
            }

            current_path = current_path.parent()?;
        }
    }

    /// Returns the rules applicable to `path`, layering global rules on top of
    /// any project rules discovered up the directory tree.
    ///
    /// Precedence is `global > project WARP.md > project AGENTS.md`. Globals
    /// are always included (when present) regardless of project state; the
    /// existing in-directory `WARP.md > AGENTS.md` shadow inside
    /// [`RuleAtPath::respected_rule`] still applies to project rules.
    ///
    /// This is the entry point used by `BlocklistAIContextModel` when packing
    /// `AIAgentContext::ProjectRules` for an agent query. Callers that need
    /// a project-only signal should use
    /// [`Self::find_applicable_project_rules`] instead.
    pub fn find_applicable_rules(&self, path: &LocalOrRemotePath) -> Option<ProjectRulesResult> {
        let project_result = self.find_applicable_project_rules(path);

        // Layered precedence: global rules are always included alongside
        // project rules. `global_rules` is a `BTreeMap`, so iteration is
        // sorted by path — deterministic without needing a separate
        // ordering pass.
        let mut active_rules: Vec<ProjectRule> = self.global_rules.active_rules().collect();
        let (project_root, additional_rule_paths) = match project_result {
            Some(project) => {
                active_rules.extend(project.active_rules);
                (Some(project.root_path), project.additional_rule_paths)
            }
            None => (None, Vec::new()),
        };

        if active_rules.is_empty() && additional_rule_paths.is_empty() {
            return None;
        }

        // Use the indexed project root when available; otherwise fall back to
        // the parent of the first global rule.
        let root_path =
            project_root.or_else(|| active_rules.first().and_then(|rule| rule.path.parent()))?;

        Some(ProjectRulesResult {
            root_path,
            active_rules,
            additional_rule_paths,
        })
    }

    async fn read_persisted_rules(
        rule_paths: Vec<ProjectRulePath>,
    ) -> HashMap<LocalOrRemotePath, ProjectRules> {
        let mut rules: HashMap<LocalOrRemotePath, ProjectRules> = HashMap::new();

        for rule in rule_paths {
            match async_fs::read_to_string(&rule.path).await {
                Ok(content) => {
                    let existing_rules = rules
                        .entry(LocalOrRemotePath::Local(rule.project_root))
                        .or_default();
                    existing_rules.upsert_rule(&LocalOrRemotePath::Local(rule.path), content);
                }
                Err(e) => {
                    log::debug!(
                        "Failed to read rule file from persistence {}: {}",
                        rule.path.display(),
                        e
                    );
                    // Continue processing other files even if one fails
                }
            }
        }

        rules
    }

    pub fn indexed_rules(&self) -> impl Iterator<Item = LocalOrRemotePath> + '_ {
        self.path_to_rules.values().flat_map(|rules| {
            rules.rules.iter().filter_map(|rules| {
                rules
                    .respected_rule()
                    .map(|project_rule| project_rule.path.clone())
            })
        })
    }

    /// Absolute locations of every indexed global rule file (e.g. `~/.agents/AGENTS.md`).
    /// Iteration order is sorted by path because global rules are backed by a `BTreeMap`.
    pub fn global_rule_paths(&self) -> impl Iterator<Item = LocalOrRemotePath> + '_ {
        self.global_rules.paths()
    }

    /// Returns every indexed global rule with its cached content, sorted by path.
    pub fn global_rules(&self) -> impl Iterator<Item = ProjectRule> + '_ {
        self.global_rules.active_rules()
    }
    /// Returns the rule file paths associated with a specific workspace root path.
    pub fn rules_for_workspace(&self, workspace_path: &Path) -> Vec<PathBuf> {
        self.path_to_rules
            .get(&LocalOrRemotePath::Local(workspace_path.to_path_buf()))
            .into_iter()
            .flat_map(|rules| {
                rules.rules.iter().filter_map(|rule| {
                    rule.respected_rule().and_then(|project_rule| {
                        project_rule.path.to_local_path().map(Path::to_path_buf)
                    })
                })
            })
            .collect()
    }
}

impl Entity for ProjectContextModel {
    type Event = ProjectContextModelEvent;
}

impl SingletonEntity for ProjectContextModel {}

#[cfg(test)]
#[path = "model_tests.rs"]
mod tests;
