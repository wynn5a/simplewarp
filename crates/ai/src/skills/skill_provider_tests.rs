use warp_util::local_or_remote_path::LocalOrRemotePath;

use super::{
    SkillProvider, SkillScope, get_provider_for_path, get_scope_for_path, home_skills_path,
};

#[test]
fn warp_home_skills_path_uses_warp_home_path() {
    assert_eq!(
        home_skills_path(SkillProvider::Warp),
        warp_core::paths::warp_home_skills_dir()
    );
}

#[test]
fn warp_home_skill_path_is_home_warp_skill() {
    let Some(warp_home_skills_dir) = warp_core::paths::warp_home_skills_dir() else {
        eprintln!("Skipping test: home directory not available");
        return;
    };
    let path = warp_home_skills_dir.join("my-skill").join("SKILL.md");

    assert_eq!(
        get_provider_for_path(&LocalOrRemotePath::Local(path.clone())),
        Some(SkillProvider::Warp)
    );
    assert_eq!(get_scope_for_path(&path), SkillScope::Home);
}

#[test]
fn local_project_provider_path_is_classified_by_structure() {
    let path = LocalOrRemotePath::Local(
        std::env::temp_dir()
            .join("repo")
            .join(".claude")
            .join("skills")
            .join("my-skill")
            .join("SKILL.md"),
    );

    assert_eq!(get_provider_for_path(&path), Some(SkillProvider::Claude));
}
