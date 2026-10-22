use dirs::home_dir;

use super::*;

#[test]
fn test_data_dir_path() {
    let home_dir = home_dir().expect("Should be able to compute home directory");
    // ChannelState, by default, is configured for Channel::SimpleWarp.
    cfg_if::cfg_if! {
        if #[cfg(target_os = "macos")] {
            assert_eq!(data_dir(), home_dir.join(".simplewarp"));
        } else if #[cfg(any(target_os = "linux", target_os = "freebsd"))] {
            assert_eq!(data_dir(), home_dir.join(".local/share/simplewarp"));
        } else if #[cfg(windows)] {
            assert_eq!(
                data_dir(),
                home_dir.join("AppData\\Roaming\\simplewarp\\SimpleWarp\\data")
            );
        } else {
            unimplemented!("Need to update tests for current platform!");
        }
    }
}

#[test]
fn test_config_local_dir_path() {
    let home_dir = home_dir().expect("Should be able to compute home directory");
    // ChannelState, by default, is configured for Channel::SimpleWarp.
    cfg_if::cfg_if! {
        if #[cfg(target_os = "macos")] {
            assert_eq!(config_local_dir(), home_dir.join(".simplewarp"));
        } else if #[cfg(any(target_os = "linux", target_os = "freebsd"))] {
            assert_eq!(config_local_dir(), home_dir.join(".config/simplewarp"));
        } else if #[cfg(windows)] {
            assert_eq!(
                config_local_dir(),
                home_dir.join("AppData\\Local\\simplewarp\\SimpleWarp\\config")
            );
        } else {
            unimplemented!("Need to update tests for current platform!");
        }
    }
}

#[cfg(target_os = "macos")]
#[test]
fn test_macos_config_dir_name_scopes_to_data_profile() {
    assert_eq!(
        macos_config_dir_name_for(Channel::SimpleWarp, None),
        ".simplewarp"
    );
    assert_eq!(
        macos_config_dir_name_for(Channel::Integration, None),
        ".simplewarp-integration"
    );

    // Each development profile must get its own directory so shared config
    // (notably settings.toml) cannot leak between profiles.
    assert_eq!(
        macos_config_dir_name_for(Channel::SimpleWarp, Some("myprofile")),
        ".simplewarp-myprofile"
    );
}

#[test]
fn test_app_mcp_config_file_path_matches_warp_home() {
    assert_eq!(app_mcp_config_file_path(), warp_home_mcp_config_file_path());
}

#[test]
fn test_warp_home_config_dir_path() {
    let home_dir = home_dir().expect("Should be able to compute home directory");
    let expected_dir_name = match ChannelState::data_profile() {
        Some(data_profile) => format!(".simplewarp-{data_profile}"),
        None => ".simplewarp".to_string(),
    };

    assert_eq!(
        warp_home_config_dir(),
        Some(home_dir.join(expected_dir_name))
    );
}

#[test]
fn test_warp_home_skills_and_mcp_paths() {
    let Some(config_dir) = warp_home_config_dir() else {
        panic!("Should be able to compute Warp home config directory");
    };

    assert_eq!(warp_home_skills_dir(), Some(config_dir.join("skills")));
    assert_eq!(
        warp_home_mcp_config_file_path(),
        Some(config_dir.join(".mcp.json"))
    );
}

#[test]
fn test_cache_dir_path() {
    let home_dir = home_dir().expect("Should be able to compute home directory");
    // ChannelState, by default, is configured for Channel::SimpleWarp.
    cfg_if::cfg_if! {
        if #[cfg(target_os = "macos")] {
            assert_eq!(
                cache_dir(),
                home_dir.join("Library/Application Support/dev.simplewarp.SimpleWarp")
            );
        } else if #[cfg(any(target_os = "linux", target_os = "freebsd"))] {
            assert_eq!(cache_dir(), home_dir.join(".cache/simplewarp"));
        } else if #[cfg(windows)] {
            assert_eq!(
                cache_dir(),
                home_dir.join("AppData\\Local\\simplewarp\\SimpleWarp\\cache")
            );
        } else {
            unimplemented!("Need to update tests for current platform!");
        }
    }
}

#[test]
fn test_state_dir_path() {
    let home_dir = home_dir().expect("Should be able to compute home directory");
    cfg_if::cfg_if! {
        // ChannelState, by default, is configured for Channel::SimpleWarp.
        if #[cfg(target_os = "macos")] {
            assert_eq!(
                state_dir(),
                home_dir.join("Library/Application Support/dev.simplewarp.SimpleWarp")
            );
        } else if #[cfg(any(target_os = "linux", target_os = "freebsd"))] {
            assert_eq!(state_dir(), home_dir.join(".local/state/simplewarp"));
        } else if #[cfg(windows)] {
            assert_eq!(
                state_dir(),
                home_dir.join("AppData\\Local\\simplewarp\\SimpleWarp\\data")
            );
        } else {
            unimplemented!("Need to update tests for current platform!");
        }
    }
}

#[test]
fn test_project_path_for_warp_app_id() {
    let project_dirs = project_dirs_for_app_id(AppId::new("dev", "warp", "Warp"), None)
        .expect("should be able to compute project dirs");
    cfg_if::cfg_if! {
        if #[cfg(target_os = "macos")] {
            assert_eq!(project_dirs.project_path(), "dev.warp.Warp");
        } else if #[cfg(any(target_os = "linux", target_os = "freebsd"))] {
            assert_eq!(project_dirs.project_path(), "warp-terminal");
        } else if #[cfg(windows)] {
            assert_eq!(project_dirs.project_path(), "warp\\Warp");
        } else {
            unimplemented!("Need to update tests for current platform!");
        }
    }
}

#[test]
fn test_project_path_for_warp_dev_app_id() {
    let project_dirs = project_dirs_for_app_id(AppId::new("dev", "warp", "WarpDev"), None)
        .expect("should be able to compute project dirs");
    cfg_if::cfg_if! {
        if #[cfg(target_os = "macos")] {
            assert_eq!(project_dirs.project_path(), "dev.warp.WarpDev");
        } else if #[cfg(any(target_os = "linux", target_os = "freebsd"))] {
            assert_eq!(project_dirs.project_path(), "warp-terminal-dev");
        } else if #[cfg(windows)] {
            assert_eq!(project_dirs.project_path(), "warp\\WarpDev");
        } else {
            unimplemented!("Need to update tests for current platform!");
        }
    }
}

#[test]
fn test_project_path_for_oss_app_id() {
    let project_dirs = project_dirs_for_app_id(AppId::new("dev", "warp", "WarpOss"), None)
        .expect("should be able to compute project dirs");
    cfg_if::cfg_if! {
        if #[cfg(target_os = "macos")] {
            assert_eq!(project_dirs.project_path(), "dev.warp.WarpOss");
        } else if #[cfg(any(target_os = "linux", target_os = "freebsd"))] {
            assert_eq!(project_dirs.project_path(), "warp-oss");
        } else if #[cfg(windows)] {
            assert_eq!(project_dirs.project_path(), "warp\\WarpOss");
        } else {
            unimplemented!("Need to update tests for current platform!");
        }
    }
}

#[test]
fn test_migrate_config_dir_moves_legacy_contents() {
    let temp_dir = tempfile::tempdir().expect("Should be able to create temp dir");
    let legacy_dir = temp_dir.path().join(".warp-oss");
    let current_dir = temp_dir.path().join(".simplewarp");
    std::fs::create_dir_all(&legacy_dir).expect("Should be able to create legacy dir");
    std::fs::write(legacy_dir.join("settings.toml"), "theme = \"dark\"")
        .expect("Should be able to write settings");

    migrate_config_dir(&legacy_dir, &current_dir);

    assert!(!legacy_dir.exists());
    assert_eq!(
        std::fs::read_to_string(current_dir.join("settings.toml"))
            .expect("Should be able to read migrated settings"),
        "theme = \"dark\""
    );
}

#[test]
fn test_migrate_config_dir_keeps_existing_destination() {
    let temp_dir = tempfile::tempdir().expect("Should be able to create temp dir");
    let legacy_dir = temp_dir.path().join(".warp-oss");
    let current_dir = temp_dir.path().join(".simplewarp");
    std::fs::create_dir_all(&legacy_dir).expect("Should be able to create legacy dir");
    std::fs::create_dir_all(&current_dir).expect("Should be able to create current dir");
    std::fs::write(legacy_dir.join("settings.toml"), "legacy = true")
        .expect("Should be able to write legacy settings");
    std::fs::write(current_dir.join("settings.toml"), "current = true")
        .expect("Should be able to write current settings");

    migrate_config_dir(&legacy_dir, &current_dir);

    assert!(legacy_dir.exists());
    assert_eq!(
        std::fs::read_to_string(current_dir.join("settings.toml"))
            .expect("Should be able to read current settings"),
        "current = true"
    );
}

#[test]
fn test_migrate_config_dir_ignores_missing_legacy() {
    let temp_dir = tempfile::tempdir().expect("Should be able to create temp dir");
    let legacy_dir = temp_dir.path().join(".warp-oss");
    let current_dir = temp_dir.path().join(".simplewarp");

    migrate_config_dir(&legacy_dir, &current_dir);

    assert!(!current_dir.exists());
}
