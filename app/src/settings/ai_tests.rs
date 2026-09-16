use settings::schema::SettingSchemaEntry;
use settings::{Setting, SettingSurfaces, SettingsMode};
use warpui::{App, SingletonEntity};

use super::*;
use crate::auth::AuthStateProvider;
use crate::test_util::settings::initialize_settings_for_tests;
use crate::workspaces::user_workspaces::UserWorkspaces;

#[test]
fn auto_approve_denylist_bypass_defaults_on_and_is_available_in_gui_settings() {
    let setting = AutoApproveBypassesCommandDenylist::new(None);
    assert!(*setting.value());
    assert_eq!(
        AutoApproveBypassesCommandDenylist::toml_path(),
        Some("agents.warp_agent.other.auto_approve_bypasses_command_denylist")
    );

    let entry = inventory::iter::<SettingSchemaEntry>
        .into_iter()
        .find(|entry| {
            entry.hierarchy == Some("agents.warp_agent.other")
                && entry.storage_key == "auto_approve_bypasses_command_denylist"
        })
        .expect("expected auto-approve denylist bypass schema entry");
    let surfaces: SettingSurfaces = (entry.surfaces_fn)();
    assert!(surfaces.includes(SettingsMode::Gui));
}

fn add_ai_enablement_dependencies_for_test(app: &mut App) {
    app.add_singleton_model(|_| AuthStateProvider::new_for_test());
    app.add_singleton_model(UserWorkspaces::default_mock);
}

// FocusedTerminalInfo Tests

#[test]
fn test_update_both_values_changed() {
    App::test((), |mut app| async move {
        // Create FocusedTerminalInfo with default values (false, false)
        let model_handle = app.add_model(|_| FocusedTerminalInfo::default());

        // Setup event tracking
        let (sender, receiver) = async_channel::unbounded();
        app.update(|ctx| {
            let sender = sender.clone();
            ctx.subscribe_to_model(
                &model_handle,
                move |_, event: &FocusedTerminalInfoEvent, _| match event {
                    FocusedTerminalInfoEvent::TerminalInfoUpdated => {
                        let _ = sender.try_send(());
                    }
                },
            );
        });

        // Update both values to (true, false)
        model_handle.update(&mut app, |model, ctx| {
            model.update(true, false, ctx);
        });

        // Verify model state
        model_handle.read(&app, |model, _| {
            assert!(model.contains_any_remote_blocks());
            assert!(!model.contains_any_restored_remote_blocks());
        });

        // Verify event was emitted exactly once
        let mut count = 0;
        while receiver.try_recv().is_ok() {
            count += 1;
        }
        assert_eq!(count, 1);
    });
}

#[test]
fn test_update_additional_value_changed() {
    App::test((), |mut app| async move {
        // Create FocusedTerminalInfo with default values (false, false)
        let model_handle = app.add_model(|_| FocusedTerminalInfo::default());

        // Setup event tracking
        let (sender, receiver) = async_channel::unbounded();
        app.update(|ctx| {
            let sender = sender.clone();
            ctx.subscribe_to_model(
                &model_handle,
                move |_, event: &FocusedTerminalInfoEvent, _| match event {
                    FocusedTerminalInfoEvent::TerminalInfoUpdated => {
                        let _ = sender.try_send(());
                    }
                },
            );
        });

        // First update to (true, false)
        model_handle.update(&mut app, |model, ctx| {
            model.update(true, false, ctx);
        });

        // Clear events by draining the channel
        while receiver.try_recv().is_ok() {}

        // Now update to (true, true) - only changing restored blocks
        model_handle.update(&mut app, |model, ctx| {
            model.update(true, true, ctx);
        });

        // Verify model state
        model_handle.read(&app, |model, _| {
            assert!(model.contains_any_remote_blocks());
            assert!(model.contains_any_restored_remote_blocks());
        });

        // Verify event was emitted exactly once
        let mut count = 0;
        while receiver.try_recv().is_ok() {
            count += 1;
        }
        assert_eq!(count, 1);
    });
}

#[test]
fn test_update_no_change() {
    App::test((), |mut app| async move {
        // Create FocusedTerminalInfo with default values (false, false)
        let model_handle = app.add_model(|_| FocusedTerminalInfo::default());

        // Setup event tracking
        let (sender, receiver) = async_channel::unbounded();
        app.update(|ctx| {
            let sender = sender.clone();
            ctx.subscribe_to_model(
                &model_handle,
                move |_, event: &FocusedTerminalInfoEvent, _| match event {
                    FocusedTerminalInfoEvent::TerminalInfoUpdated => {
                        let _ = sender.try_send(());
                    }
                },
            );
        });

        // First update to (true, true)
        model_handle.update(&mut app, |model, ctx| {
            model.update(true, true, ctx);
        });

        // Clear events by draining the channel
        while receiver.try_recv().is_ok() {}

        // Update with same values (true, true)
        model_handle.update(&mut app, |model, ctx| {
            model.update(true, true, ctx);
        });

        // Verify model state remains the same
        model_handle.read(&app, |model, _| {
            assert!(model.contains_any_remote_blocks());
            assert!(model.contains_any_restored_remote_blocks());
        });

        // Verify no event was emitted
        let mut count = 0;
        while receiver.try_recv().is_ok() {
            count += 1;
        }
        assert_eq!(count, 0);
    });
}

#[test]
fn test_update_only_remote_toggles() {
    App::test((), |mut app| async move {
        // Create FocusedTerminalInfo with default values (false, false)
        let model_handle = app.add_model(|_| FocusedTerminalInfo::default());

        // Setup event tracking
        let (sender, receiver) = async_channel::unbounded();
        app.update(|ctx| {
            let sender = sender.clone();
            ctx.subscribe_to_model(
                &model_handle,
                move |_, event: &FocusedTerminalInfoEvent, _| match event {
                    FocusedTerminalInfoEvent::TerminalInfoUpdated => {
                        let _ = sender.try_send(());
                    }
                },
            );
        });

        // First update to (true, true)
        model_handle.update(&mut app, |model, ctx| {
            model.update(true, true, ctx);
        });

        // Clear events by draining the channel
        while receiver.try_recv().is_ok() {}

        // Update with (false, true) - only remote blocks changes
        model_handle.update(&mut app, |model, ctx| {
            model.update(false, true, ctx);
        });

        // Verify model state
        model_handle.read(&app, |model, _| {
            assert!(!model.contains_any_remote_blocks());
            assert!(model.contains_any_restored_remote_blocks());
        });

        // Verify event was emitted exactly once
        let mut count = 0;
        while receiver.try_recv().is_ok() {
            count += 1;
        }
        assert_eq!(count, 1);
    });
}

#[test]
fn test_update_only_restored_toggles() {
    App::test((), |mut app| async move {
        // Create FocusedTerminalInfo with default values (false, false)
        let model_handle = app.add_model(|_| FocusedTerminalInfo::default());

        // Setup event tracking
        let (sender, receiver) = async_channel::unbounded();
        app.update(|ctx| {
            let sender = sender.clone();
            ctx.subscribe_to_model(
                &model_handle,
                move |_, event: &FocusedTerminalInfoEvent, _| match event {
                    FocusedTerminalInfoEvent::TerminalInfoUpdated => {
                        let _ = sender.try_send(());
                    }
                },
            );
        });

        // First update to (true, true)
        model_handle.update(&mut app, |model, ctx| {
            model.update(true, true, ctx);
        });

        // Clear events by draining the channel
        while receiver.try_recv().is_ok() {}

        // Update with (true, false) - only restored blocks changes
        model_handle.update(&mut app, |model, ctx| {
            model.update(true, false, ctx);
        });

        // Verify model state
        model_handle.read(&app, |model, _| {
            assert!(model.contains_any_remote_blocks());
            assert!(!model.contains_any_restored_remote_blocks());
        });

        // Verify event was emitted exactly once
        let mut count = 0;
        while receiver.try_recv().is_ok() {
            count += 1;
        }
        assert_eq!(count, 1);
    });
}

// ToolbarCommandMap Tests

#[test]
fn test_toolbar_command_map_deserialize_from_map() {
    let json = serde_json::json!({
        "^claude": "Claude",
        "^gemini": "Gemini",
        "^codex": ""
    });
    let map: ToolbarCommandMap = serde_json::from_value(json).unwrap();
    assert_eq!(map.0.len(), 3);
    assert_eq!(map.0["^claude"], "Claude");
    assert_eq!(map.0["^gemini"], "Gemini");
    assert_eq!(map.0["^codex"], "");
}

#[test]
fn test_toolbar_command_map_deserialize_from_legacy_vec() {
    let json = serde_json::json!(["^claude", "^gemini", "^custom"]);
    let map: ToolbarCommandMap = serde_json::from_value(json).unwrap();
    assert_eq!(map.0.len(), 3);
    // Legacy vec format should assign empty agent values.
    for (_, agent) in map.0.iter() {
        assert_eq!(agent, "");
    }
    let keys: Vec<_> = map.0.keys().collect();
    assert_eq!(keys, vec!["^claude", "^gemini", "^custom"]);
}

#[test]
fn test_toolbar_command_map_from_file_value_map_format() {
    use settings_value::SettingsValue;

    let value = serde_json::json!({
        "^claude": "Claude",
        "^amp": "Amp"
    });
    let map = ToolbarCommandMap::from_file_value(&value).unwrap();
    assert_eq!(map.0.len(), 2);
    assert_eq!(map.0["^claude"], "Claude");
    assert_eq!(map.0["^amp"], "Amp");
}

#[test]
fn test_toolbar_command_map_from_file_value_legacy_array() {
    use settings_value::SettingsValue;

    // Patterns are intentionally non-alphabetical to verify insertion order is preserved.
    let value = serde_json::json!(["^zebra", "^alpha", "^middle"]);
    let map = ToolbarCommandMap::from_file_value(&value).unwrap();
    assert_eq!(map.0.len(), 3);
    assert_eq!(map.0["^zebra"], "");
    assert_eq!(map.0["^alpha"], "");
    assert_eq!(map.0["^middle"], "");
    let keys: Vec<_> = map.0.keys().collect();
    assert_eq!(keys, vec!["^zebra", "^alpha", "^middle"]);
}

#[test]
fn test_toolbar_command_map_from_file_value_invalid() {
    use settings_value::SettingsValue;

    let value = serde_json::json!(42);
    assert!(ToolbarCommandMap::from_file_value(&value).is_none());
}

#[test]
fn test_toolbar_command_map_roundtrip() {
    use settings_value::SettingsValue;

    let mut inner = IndexMap::new();
    inner.insert("^claude".to_string(), "Claude".to_string());
    inner.insert("^custom".to_string(), String::new());
    let original = ToolbarCommandMap::new(inner);

    let file_value = original.to_file_value();
    let restored = ToolbarCommandMap::from_file_value(&file_value).unwrap();
    assert_eq!(original, restored);
}

#[test]
fn test_toolbar_command_map_matched_agent() {
    App::test((), |mut app| async move {
        initialize_settings_for_tests(&mut app);

        let mut map = IndexMap::new();
        map.insert("^claude".to_string(), "Claude".to_string());
        map.insert("^gemini".to_string(), "Gemini".to_string());
        map.insert("^custom-tool".to_string(), String::new());

        AISettings::handle(&app).update(&mut app, |settings, ctx| {
            report_if_error!(
                settings
                    .cli_agent_footer_enabled_commands
                    .set_value(ToolbarCommandMap::new(map), ctx)
            );
        });

        app.read(|ctx| {
            let agent = CompiledCommandsForCodingAgentToolbar::matched_agent(ctx, "claude chat");
            assert_eq!(agent, Some(CLIAgent::Claude));

            let agent = CompiledCommandsForCodingAgentToolbar::matched_agent(ctx, "gemini ask");
            assert_eq!(agent, Some(CLIAgent::Gemini));

            let agent =
                CompiledCommandsForCodingAgentToolbar::matched_agent(ctx, "custom-tool --flag");
            assert_eq!(agent, Some(CLIAgent::Unknown));

            let agent =
                CompiledCommandsForCodingAgentToolbar::matched_agent(ctx, "unmatched-command");
            assert_eq!(agent, None);
        });
    });
}

#[test]
fn orchestration_is_enabled_when_ai_is_enabled() {
    App::test((), |mut app| async move {
        initialize_settings_for_tests(&mut app);
        add_ai_enablement_dependencies_for_test(&mut app);

        AISettings::handle(&app).read(&app, |settings, ctx| {
            assert!(settings.is_orchestration_enabled(ctx));
        });
    });
}

// VOICE_INPUT_LANGUAGES catalog tests

#[test]
fn test_voice_input_languages_auto_detect_is_first_with_empty_code() {
    // The picker relies on the first entry being the Auto-detect sentinel with an
    // empty code, since an empty stored value means "don't force a language".
    let (code, name) = VOICE_INPUT_LANGUAGES[0];
    assert_eq!(code, "");
    assert_eq!(name, "Auto-detect");
}

#[test]
fn test_voice_input_languages_has_full_catalog() {
    // Sanity check that we ship the full list rather than a small curated subset:
    // Auto-detect plus well over 100 ISO-639-1 languages.
    assert!(
        VOICE_INPUT_LANGUAGES.len() > 150,
        "expected the full ISO-639-1 catalog, got {} entries",
        VOICE_INPUT_LANGUAGES.len()
    );
}

#[test]
fn test_voice_input_languages_codes_and_names_are_valid_and_unique() {
    use std::collections::HashSet;

    let mut seen_codes = HashSet::new();
    let mut seen_names = HashSet::new();
    for (index, (code, name)) in VOICE_INPUT_LANGUAGES.iter().enumerate() {
        assert!(
            !name.is_empty(),
            "language name must not be empty: {code:?}"
        );
        assert!(
            seen_names.insert(*name),
            "duplicate language name: {name:?}"
        );
        assert!(
            seen_codes.insert(*code),
            "duplicate language code: {code:?}"
        );

        if index == 0 {
            // Auto-detect sentinel: empty code, validated separately.
            continue;
        }
        // Every real language uses a two-letter lowercase ISO-639-1 code.
        assert_eq!(
            code.len(),
            2,
            "expected a 2-letter ISO-639-1 code: {code:?}"
        );
        assert!(
            code.chars().all(|c| c.is_ascii_lowercase()),
            "ISO-639-1 code must be lowercase ascii: {code:?}"
        );
    }
}

#[test]
fn test_voice_input_languages_includes_common_languages() {
    // A representative spot check, including Marathi (mr) which was explicitly
    // requested in the review that motivated the full list.
    for expected in [("en", "English"), ("es", "Spanish"), ("mr", "Marathi")] {
        assert!(
            VOICE_INPUT_LANGUAGES.contains(&expected),
            "catalog is missing {expected:?}"
        );
    }
}
#[test]
fn ai_autodetection_defaults_to_opt_in() {
    App::test((), |mut app| async move {
        initialize_settings_for_tests(&mut app);
        add_ai_enablement_dependencies_for_test(&mut app);

        AISettings::handle(&app).read(&app, |settings, ctx| {
            // NLD is opt-in: a fresh user who never touched the setting has it off.
            // This fails before the default flip (default was `true`) and passes after.
            assert!(!*settings.ai_autodetection_enabled_internal.value());
            // AI is enabled by default, so the getter reflects the opt-in setting
            // rather than a disabled-AI state.
            assert!(settings.is_any_ai_enabled(ctx));
            assert!(!settings.is_ai_autodetection_enabled(ctx));
        });
    });
}

#[test]
fn ai_autodetection_setting_can_be_toggled_on_and_off() {
    App::test((), |mut app| async move {
        initialize_settings_for_tests(&mut app);
        add_ai_enablement_dependencies_for_test(&mut app);

        // Mirrors what `/enable-natural-language-detection` does in the TUI.
        AISettings::handle(&app).update(&mut app, |settings, ctx| {
            settings
                .ai_autodetection_enabled_internal
                .set_value(true, ctx)
                .unwrap();
        });
        AISettings::handle(&app).read(&app, |settings, ctx| {
            assert!(*settings.ai_autodetection_enabled_internal.value());
            assert!(settings.is_ai_autodetection_enabled(ctx));
        });

        // Mirrors what `/disable-natural-language-detection` does in the TUI.
        AISettings::handle(&app).update(&mut app, |settings, ctx| {
            settings
                .ai_autodetection_enabled_internal
                .set_value(false, ctx)
                .unwrap();
        });
        AISettings::handle(&app).read(&app, |settings, ctx| {
            assert!(!*settings.ai_autodetection_enabled_internal.value());
            assert!(!settings.is_ai_autodetection_enabled(ctx));
        });
    });
}
