use dirs::home_dir;

use super::*;

#[test]
fn test_data_dir_path() {
    let home_dir = home_dir().expect("Should be able to compute home directory");
    // ChannelState, by default, is configured for Channel::Oss.
    cfg_if::cfg_if! {
        if #[cfg(target_os = "macos")] {
            assert_eq!(data_dir(), home_dir.join(".warp-oss"));
        } else if #[cfg(any(target_os = "linux", target_os = "freebsd"))] {
            assert_eq!(data_dir(), home_dir.join(".local/share/warp-oss"));
        } else if #[cfg(windows)] {
            assert_eq!(data_dir(), home_dir.join("AppData\\Roaming\\warp\\WarpOss\\data"));
        } else {
            unimplemented!("Need to update tests for current platform!");
        }
    }
}

#[test]
fn test_config_local_dir_path() {
    let home_dir = home_dir().expect("Should be able to compute home directory");
    // ChannelState, by default, is configured for Channel::Oss.
    cfg_if::cfg_if! {
        if #[cfg(target_os = "macos")] {
            assert_eq!(config_local_dir(), home_dir.join(".warp-oss"));
        } else if #[cfg(any(target_os = "linux", target_os = "freebsd"))] {
            assert_eq!(config_local_dir(), home_dir.join(".config/warp-oss"));
        } else if #[cfg(windows)] {
            assert_eq!(config_local_dir(), home_dir.join("AppData\\Local\\warp\\WarpOss\\config"));
        } else {
            unimplemented!("Need to update tests for current platform!");
        }
    }
}

#[cfg(target_os = "macos")]
#[test]
fn test_macos_config_dir_name_scopes_to_data_profile() {
    assert_eq!(macos_config_dir_name_for(Channel::Stable, None), ".warp");
    assert_eq!(
        macos_config_dir_name_for(Channel::Local, None),
        ".warp-local"
    );

    // Each development profile must get its own directory so shared config
    // (notably settings.toml) cannot leak between profiles.
    assert_eq!(
        macos_config_dir_name_for(Channel::Local, Some("myprofile")),
        ".warp-local-myprofile"
    );
    assert_eq!(
        macos_config_dir_name_for(Channel::Stable, Some("myprofile")),
        ".warp-myprofile"
    );
}

#[test]
fn test_gui_app_id_maps_oss_channel_to_oss_gui() {
    let gui_app_id = gui_app_id_for_channel(Channel::Oss, AppId::new("dev", "warp", "WarpOther"));

    assert_eq!(gui_app_id.to_string(), "dev.warp.WarpOss");
}

#[test]
fn test_gui_config_and_mcp_paths_resolve_explicit_sources() {
    let home_dir = home_dir().expect("Should be able to compute home directory");
    let gui_config_dir = gui_config_local_dir().expect("GUI config path should resolve");

    cfg_if::cfg_if! {
        if #[cfg(target_os = "macos")] {
            assert_eq!(gui_config_dir, home_dir.join(".warp-oss"));
        } else if #[cfg(any(target_os = "linux", target_os = "freebsd"))] {
            assert_eq!(gui_config_dir, home_dir.join(".config/warp-oss"));
        } else if #[cfg(windows)] {
            assert_eq!(
                gui_config_dir,
                home_dir.join("AppData\\Local\\warp\\WarpOss\\config")
            );
        } else {
            unimplemented!("Need to update tests for current platform!");
        }
    }

    assert_eq!(gui_mcp_config_file_path(), warp_home_mcp_config_file_path());
}
#[test]
fn test_warp_home_config_dir_path() {
    let home_dir = home_dir().expect("Should be able to compute home directory");
    let expected_dir_name = match ChannelState::data_profile() {
        Some(data_profile) => format!(".warp-oss-{data_profile}"),
        None => ".warp-oss".to_string(),
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
    // ChannelState, by default, is configured for Channel::Oss.
    cfg_if::cfg_if! {
        if #[cfg(target_os = "macos")] {
            assert_eq!(cache_dir(), home_dir.join("Library/Application Support/dev.warp.WarpOss"));
        } else if #[cfg(any(target_os = "linux", target_os = "freebsd"))] {
            assert_eq!(cache_dir(), home_dir.join(".cache/warp-oss"));
        } else if #[cfg(windows)] {
            assert_eq!(cache_dir(), home_dir.join("AppData\\Local\\warp\\WarpOss\\cache"));
        } else {
            unimplemented!("Need to update tests for current platform!");
        }
    }
}

#[test]
fn test_state_dir_path() {
    let home_dir = home_dir().expect("Should be able to compute home directory");
    cfg_if::cfg_if! {
        // ChannelState, by default, is configured for Channel::Oss.
        if #[cfg(target_os = "macos")] {
            assert_eq!(state_dir(), home_dir.join("Library/Application Support/dev.warp.WarpOss"));
        } else if #[cfg(any(target_os = "linux", target_os = "freebsd"))] {
            assert_eq!(state_dir(), home_dir.join(".local/state/warp-oss"));
        } else if #[cfg(windows)] {
            assert_eq!(state_dir(), home_dir.join("AppData\\Local\\warp\\WarpOss\\data"));
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
