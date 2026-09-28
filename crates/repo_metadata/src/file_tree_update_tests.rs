use std::path::{Path, PathBuf};

use warp_util::standardized_path::StandardizedPath;

use crate::entry::{DirectoryEntry, Entry, FileId, FileMetadata};
use crate::file_tree_store::FileTreeEntry;
use crate::file_tree_update::*;
use crate::local_model::LocalRepoMetadataModel;

// ── Helpers ──────────────────────────────────────────────────────────

/// Platform-appropriate absolute root for test paths.
/// On Windows `/repo` is not a valid local absolute path, so we use
/// a drive-letter prefix instead.
#[cfg(windows)]
const TEST_REPO_ROOT: &str = "C:\\repo";
#[cfg(not(windows))]
const TEST_REPO_ROOT: &str = "/repo";

/// Creates a `StandardizedPath` from a Unix-style test path like
/// `"/repo/src/main.rs"`, replacing the `/repo` prefix with the
/// platform-appropriate [`TEST_REPO_ROOT`].
fn std_path(unix_path: &str) -> StandardizedPath {
    let local = unix_path.replacen("/repo", TEST_REPO_ROOT, 1);
    StandardizedPath::try_from_local(Path::new(&local)).unwrap()
}

/// Like [`std_path`] but returns a `PathBuf` suitable for use inside
/// `FileTreeMutation` (which carries local filesystem paths).
fn mutation_path(unix_path: &str) -> PathBuf {
    PathBuf::from(unix_path.replacen("/repo", TEST_REPO_ROOT, 1))
}

fn file(path: &str) -> Entry {
    Entry::File(FileMetadata {
        path: std_path(path),
        file_id: FileId::new(),
        extension: std::path::Path::new(path)
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_string),
        ignored: false,
    })
}

fn dir(path: &str, children: Vec<Entry>) -> Entry {
    Entry::Directory(DirectoryEntry {
        path: std_path(path),
        children,
        ignored: false,
        loaded: true,
    })
}

fn ignored_file(path: &str) -> Entry {
    Entry::File(FileMetadata {
        path: std_path(path),
        file_id: FileId::new(),
        extension: None,
        ignored: true,
    })
}

fn build_tree_from_entry(entry: Entry) -> FileTreeEntry {
    FileTreeEntry::from(entry)
}

// ── flatten_entry_metadata tests ─────────────────────────────────────

#[test]
fn flatten_single_file() {
    let entry = file("/repo/src/main.rs");
    let metadata = flatten_entry_metadata(&entry);

    assert_eq!(metadata.len(), 1);
    assert!(matches!(
        &metadata[0],
        RepoNodeMetadata::File(f) if f.path == std_path("/repo/src/main.rs")
    ));
}

#[test]
fn flatten_directory_with_children_is_depth_first_preorder() {
    let entry = dir(
        "/repo/src",
        vec![
            dir(
                "/repo/src/components",
                vec![
                    file("/repo/src/components/button.rs"),
                    file("/repo/src/components/modal.rs"),
                ],
            ),
            file("/repo/src/main.rs"),
        ],
    );

    let metadata = flatten_entry_metadata(&entry);

    assert_eq!(metadata.len(), 5);
    assert!(
        matches!(&metadata[0], RepoNodeMetadata::Directory(d) if d.path == std_path("/repo/src"))
    );
    assert!(
        matches!(&metadata[1], RepoNodeMetadata::Directory(d) if d.path == std_path("/repo/src/components"))
    );
    assert!(
        matches!(&metadata[2], RepoNodeMetadata::File(f) if f.path == std_path("/repo/src/components/button.rs"))
    );
    assert!(
        matches!(&metadata[3], RepoNodeMetadata::File(f) if f.path == std_path("/repo/src/components/modal.rs"))
    );
    assert!(
        matches!(&metadata[4], RepoNodeMetadata::File(f) if f.path == std_path("/repo/src/main.rs"))
    );
}

#[test]
fn flatten_preserves_ignored_flag() {
    let entry = dir("/repo", vec![ignored_file("/repo/secret.env")]);

    let metadata = flatten_entry_metadata(&entry);
    assert_eq!(metadata.len(), 2);
    assert!(matches!(
        &metadata[1],
        RepoNodeMetadata::File(f) if f.ignored
    ));
}

// ── apply_file_tree_mutations update-generation tests ─────────────────

#[test]
fn apply_mutations_generates_update_for_remove() {
    use crate::local_model::FileTreeMutation;

    let initial = dir("/repo", vec![file("/repo/old.rs")]);
    let mut tree = build_tree_from_entry(initial);
    let mutations = vec![FileTreeMutation::Remove(mutation_path("/repo/old.rs"))];

    let update =
        LocalRepoMetadataModel::apply_file_tree_mutations(&mut tree, mutations, false, true)
            .expect("update should be produced");

    assert_eq!(update.remove_entries.len(), 1);
    assert_eq!(update.remove_entries[0], std_path("/repo/old.rs"));
    assert!(update.update_entries.is_empty());
}

#[test]
fn apply_mutations_generates_update_for_add_file() {
    use crate::local_model::FileTreeMutation;

    let initial = dir("/repo", vec![dir("/repo/src", vec![])]);
    let mut tree = build_tree_from_entry(initial);
    let mutations = vec![FileTreeMutation::AddFile {
        path: mutation_path("/repo/src/new.rs"),
        is_ignored: false,
        extension: Some("rs".to_string()),
    }];

    let update =
        LocalRepoMetadataModel::apply_file_tree_mutations(&mut tree, mutations, false, true)
            .expect("update should be produced");

    assert!(update.remove_entries.is_empty());
    assert_eq!(update.update_entries.len(), 1);
    assert_eq!(
        update.update_entries[0].parent_path_to_replace,
        std_path("/repo/src")
    );
    assert_eq!(update.update_entries[0].subtree_metadata.len(), 1);
    assert!(matches!(
        &update.update_entries[0].subtree_metadata[0],
        RepoNodeMetadata::File(f) if f.path == std_path("/repo/src/new.rs")
            && f.extension == Some("rs".to_string())
            && !f.ignored
    ));
}

#[test]
fn apply_mutations_generates_update_for_add_directory_subtree() {
    use crate::local_model::FileTreeMutation;

    let subtree = dir(
        "/repo/src/components",
        vec![
            file("/repo/src/components/button.rs"),
            file("/repo/src/components/modal.rs"),
        ],
    );

    let initial = dir("/repo", vec![dir("/repo/src", vec![])]);
    let mut tree = build_tree_from_entry(initial);
    let mutations = vec![FileTreeMutation::AddDirectorySubtree {
        dir_path: mutation_path("/repo/src/components"),
        subtree,
    }];

    let update =
        LocalRepoMetadataModel::apply_file_tree_mutations(&mut tree, mutations, false, true)
            .expect("update should be produced");

    assert!(update.remove_entries.is_empty());
    assert_eq!(update.update_entries.len(), 1);
    let entry_update = &update.update_entries[0];
    assert_eq!(entry_update.parent_path_to_replace, std_path("/repo/src"));
    // 1 dir + 2 files
    assert_eq!(entry_update.subtree_metadata.len(), 3);
    assert!(matches!(
        &entry_update.subtree_metadata[0],
        RepoNodeMetadata::Directory(d) if d.path == std_path("/repo/src/components")
    ));
}

#[test]
fn apply_mutations_generates_update_for_add_empty_directory() {
    use crate::local_model::FileTreeMutation;

    let initial = dir("/repo", vec![dir("/repo/src", vec![])]);
    let mut tree = build_tree_from_entry(initial);
    let mutations = vec![FileTreeMutation::AddUnloadedDirectory {
        path: mutation_path("/repo/src/empty"),
        is_ignored: true,
    }];

    let update =
        LocalRepoMetadataModel::apply_file_tree_mutations(&mut tree, mutations, false, true)
            .expect("update should be produced");

    assert!(update.remove_entries.is_empty());
    assert_eq!(update.update_entries.len(), 1);
    assert!(matches!(
        &update.update_entries[0].subtree_metadata[0],
        RepoNodeMetadata::Directory(d) if d.path == std_path("/repo/src/empty")
            && d.ignored
            && !d.loaded
    ));
}

#[test]
fn apply_mutations_generates_update_for_mixed_mutations() {
    use crate::local_model::FileTreeMutation;

    let initial = dir("/repo", vec![file("/repo/old.rs")]);
    let mut tree = build_tree_from_entry(initial);
    let mutations = vec![
        FileTreeMutation::Remove(mutation_path("/repo/old.rs")),
        FileTreeMutation::AddFile {
            path: mutation_path("/repo/new.rs"),
            is_ignored: false,
            extension: Some("rs".to_string()),
        },
        FileTreeMutation::AddUnloadedDirectory {
            path: mutation_path("/repo/new_dir"),
            is_ignored: false,
        },
    ];

    let update =
        LocalRepoMetadataModel::apply_file_tree_mutations(&mut tree, mutations, false, true)
            .expect("update should be produced");

    assert_eq!(update.remove_entries.len(), 1);
    assert_eq!(update.update_entries.len(), 2);
}

#[test]
fn apply_mutations_returns_none_when_emit_updates_is_false() {
    use crate::local_model::FileTreeMutation;

    let initial = dir("/repo", vec![file("/repo/old.rs")]);
    let mut tree = build_tree_from_entry(initial);
    let mutations = vec![FileTreeMutation::Remove(mutation_path("/repo/old.rs"))];

    let update =
        LocalRepoMetadataModel::apply_file_tree_mutations(&mut tree, mutations, false, false);

    assert!(
        update.is_none(),
        "should return None when emit_updates is false"
    );
    assert!(
        tree.get(&std_path("/repo/old.rs")).is_none(),
        "old.rs should still be removed from the tree"
    );
}

// ── Lazy-load filtering test ─────────────────────────────────────────

#[test]
fn lazy_load_filters_mutations_for_unloaded_parents() {
    use crate::local_model::FileTreeMutation;

    let initial = Entry::Directory(DirectoryEntry {
        path: std_path("/repo"),
        children: vec![
            Entry::Directory(DirectoryEntry {
                path: std_path("/repo/src"),
                children: vec![],
                ignored: false,
                loaded: true,
            }),
            Entry::Directory(DirectoryEntry {
                path: std_path("/repo/vendor"),
                children: vec![],
                ignored: false,
                loaded: false,
            }),
        ],
        ignored: false,
        loaded: true,
    });
    let mut tree = build_tree_from_entry(initial);

    let mutations = vec![
        FileTreeMutation::AddFile {
            path: mutation_path("/repo/src/main.rs"),
            is_ignored: false,
            extension: Some("rs".to_string()),
        },
        FileTreeMutation::AddFile {
            path: mutation_path("/repo/vendor/lib.rs"),
            is_ignored: false,
            extension: Some("rs".to_string()),
        },
    ];

    let update =
        LocalRepoMetadataModel::apply_file_tree_mutations(&mut tree, mutations, true, true)
            .expect("update should be produced when emit_updates is true");

    assert!(
        tree.get(&std_path("/repo/src/main.rs")).is_some(),
        "main.rs under loaded src/ should exist"
    );
    assert!(
        tree.get(&std_path("/repo/vendor/lib.rs")).is_none(),
        "lib.rs under unloaded vendor/ should NOT exist"
    );

    assert!(update.remove_entries.is_empty());
    assert_eq!(
        update.update_entries.len(),
        1,
        "update should only contain the applied mutation"
    );
    assert!(matches!(
        &update.update_entries[0].subtree_metadata[0],
        RepoNodeMetadata::File(f) if f.path == std_path("/repo/src/main.rs")
    ));
}
