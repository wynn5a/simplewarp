use std::collections::HashMap;

use anyhow::{Result, anyhow};
use warpui_core::{AppContext, Entity, SingletonEntity};

type UpdateFn = Box<dyn FnMut(String, &mut AppContext) -> Result<()>>;

/// Loads a value into memory without persisting. Parameters: (serialized_value, explicitly_set, ctx).
type LoadFn = Box<dyn FnMut(String, bool, &mut AppContext) -> Result<()>>;

/// Checks that a serialized value parses as the setting's value type.
type ValidateFn = Box<dyn Fn(&str) -> Result<()>>;

/// Intermediate data collected for each setting during reload, before
/// calling the mutable `load_fns`.
struct SettingReloadEntry {
    storage_key: String,
    read_value: Option<String>,
    serialized_default: String,
    toml_key: &'static str,
    hierarchy: Option<&'static str>,
}

#[derive(Debug)]
struct SettingsInfo {
    serialized_default_value: String,
    /// The default value serialized using the `SettingsValue` trait
    /// for the settings file.
    file_serialized_default_value: String,
    hierarchy: Option<&'static str>,
    /// The key used in the TOML settings file (last segment of `toml_path`).
    /// For settings without a `toml_path`, this equals the storage key.
    toml_key: &'static str,
    /// The maximum number of TOML section-table levels to use when rendering
    /// this setting's value in the settings file. `None` means unlimited.
    max_table_depth: Option<u32>,
    /// Whether this setting is private (not shown in the user-visible settings file).
    is_private: bool,
}

/// Provides access to every registered setting by storage key, independent of
/// the settings group that defines it.
#[derive(Default)]
pub struct SettingsManager {
    /// Settings info by storage key
    settings: HashMap<String, SettingsInfo>,

    /// Functions for updating settings by storage key
    update_fns: HashMap<String, UpdateFn>,

    /// Functions for loading a value into memory without persisting to storage.
    /// Used during hot-reload to avoid write-back loops with the file watcher.
    load_fns: HashMap<String, LoadFn>,

    /// Functions for checking that a serialized value is valid for the setting.
    validate_fns: HashMap<String, ValidateFn>,
}

impl SettingsManager {
    /// Registers the functions that update, load, and validate the setting with
    /// the given storage key, along with its storage metadata.
    #[allow(clippy::too_many_arguments)]
    pub fn register_setting(
        &mut self,
        storage_key: &str,
        serialized_default_value: String,
        file_serialized_default_value: String,
        hierarchy: Option<&'static str>,
        toml_key: &'static str,
        max_table_depth: Option<u32>,
        is_private: bool,
        update_fn: impl FnMut(String, &mut AppContext) -> Result<()> + 'static,
        load_fn: impl FnMut(String, bool, &mut AppContext) -> Result<()> + 'static,
        validate_fn: impl Fn(&str) -> Result<()> + 'static,
    ) {
        self.update_fns
            .insert(storage_key.to_owned(), Box::new(update_fn));
        self.load_fns
            .insert(storage_key.to_owned(), Box::new(load_fn));
        self.validate_fns
            .insert(storage_key.to_owned(), Box::new(validate_fn));
        self.settings.insert(
            storage_key.to_owned(),
            SettingsInfo {
                serialized_default_value,
                file_serialized_default_value,
                hierarchy,
                toml_key,
                max_table_depth,
                is_private,
            },
        );
    }

    /// Returns the storage keys for all public (non-private) settings.
    pub fn public_storage_keys(&self) -> impl Iterator<Item = &str> + '_ {
        self.settings
            .iter()
            .filter(|(_, info)| !info.is_private)
            .map(|(key, _)| key.as_str())
    }

    /// Updates the setting with the given storage key to a new value, returning
    /// a result indicating whether the update was successful.
    pub fn update_setting_with_storage_key(
        &mut self,
        storage_key: &str,
        new_value: String,
        ctx: &mut AppContext,
    ) -> Result<()> {
        self.update_fns
            .get_mut(storage_key)
            .map(|update_fn| update_fn(new_value, ctx))
            .unwrap_or_else(|| {
                Err(anyhow!(
                    "no update fn registered for storage key {}",
                    storage_key
                ))
            })
    }

    pub fn default_values(&self) -> impl Iterator<Item = (String, String)> + '_ {
        self.settings
            .iter()
            .map(|(key, info)| (key.clone(), info.serialized_default_value.clone()))
    }

    /// Loads a setting value into memory without persisting to storage.
    ///
    /// `explicitly_set` indicates whether the value came from the file (`true`)
    /// or is a default for an absent key (`false`).
    pub fn load_setting(
        &mut self,
        storage_key: &str,
        value: String,
        explicitly_set: bool,
        ctx: &mut AppContext,
    ) -> Result<()> {
        self.load_fns
            .get_mut(storage_key)
            .map(|load_fn| load_fn(value, explicitly_set, ctx))
            .unwrap_or_else(|| {
                Err(anyhow!(
                    "no load fn registered for storage key {}",
                    storage_key
                ))
            })
    }

    /// Reloads all public (non-private) settings from the preferences backend.
    ///
    /// Call this after the backing store has been refreshed from disk (e.g.
    /// via [`UserPreferences::reload_from_disk`]) so that every in-memory
    /// setting picks up the new values.
    ///
    /// Uses [`load_setting`](Self::load_setting) to update in-memory values
    /// without writing back to the preferences backend, avoiding write-back
    /// loops with the file watcher. Keys present in the file are loaded with
    /// `explicitly_set = true`; absent keys are reset to their default with
    /// `explicitly_set = false`.
    /// Returns the storage keys of any settings that failed to load.
    pub fn reload_all_public_settings(&mut self, ctx: &mut AppContext) -> Vec<String> {
        let prefs = <super::PublicPreferences as SingletonEntity>::as_ref(ctx).as_preferences();

        // Read every non-private setting from the (now-reloaded) preferences,
        // collecting them up-front to release the immutable borrow on
        // `self.settings` before calling the mutable `load_fns`.
        let updates: Vec<SettingReloadEntry> = self
            .settings
            .iter()
            .filter(|(_, info)| !info.is_private)
            .map(|(key, info)| {
                let read_value =
                    match prefs.read_value_with_hierarchy(info.toml_key, info.hierarchy) {
                        Ok(v) => v,
                        Err(err) => {
                            log::warn!("Failed to read setting {key} during reload: {err}");
                            None
                        }
                    };
                SettingReloadEntry {
                    storage_key: key.clone(),
                    read_value,
                    serialized_default: info.serialized_default_value.clone(),
                    toml_key: info.toml_key,
                    hierarchy: info.hierarchy,
                }
            })
            .collect();

        let mut failed_keys = Vec::new();
        for entry in updates {
            let (effective_value, explicitly_set) = match entry.read_value {
                Some(v) => (v, true),
                None => (entry.serialized_default, false),
            };
            if let Err(err) =
                self.load_setting(&entry.storage_key, effective_value, explicitly_set, ctx)
            {
                log::warn!("Failed to reload setting {}: {err}", entry.storage_key);
                // Re-inhibit this key so writes don't overwrite the
                // user's broken-but-fixable value in the file.
                let prefs =
                    <super::PublicPreferences as SingletonEntity>::as_ref(ctx).as_preferences();
                prefs.inhibit_writes_for_key(entry.toml_key, entry.hierarchy);
                failed_keys.push(entry.toml_key.to_string());
            }
        }
        failed_keys
    }

    /// Validates all public settings by reading each from the preferences
    /// backend and attempting deserialization. Returns the storage keys of
    /// any settings whose stored value cannot be deserialized.
    ///
    /// This is a read-only check — it does not modify in-memory state.
    /// Call after [`register_all_settings`] on startup to detect invalid
    /// values in the settings file.
    pub fn validate_all_public_settings(&self, ctx: &AppContext) -> Vec<String> {
        let prefs = <super::PublicPreferences as SingletonEntity>::as_ref(ctx).as_preferences();

        self.settings
            .iter()
            .filter(|(_, info)| !info.is_private)
            .filter_map(|(key, info)| {
                let value = prefs
                    .read_value_with_hierarchy(info.toml_key, info.hierarchy)
                    .ok()
                    .flatten()?;

                if let Some(validate_fn) = self.validate_fns.get(key)
                    && validate_fn(&value).is_err()
                {
                    return Some(info.toml_key.to_string());
                }
                None
            })
            .collect()
    }

    /// Returns all registered settings with their toml key, serialized
    /// default value (in file format), hierarchy path, and max table depth,
    /// for use when writing the user-visible settings file.
    pub fn default_values_for_settings_file(
        &self,
    ) -> impl Iterator<Item = (&str, &str, Option<&'static str>, Option<u32>)> + '_ {
        self.settings
            .iter()
            .filter(|(_, info)| !info.is_private)
            .map(|(_, info)| {
                (
                    info.toml_key,
                    info.file_serialized_default_value.as_str(),
                    info.hierarchy,
                    info.max_table_depth,
                )
            })
    }
}

impl Entity for SettingsManager {
    type Event = ();
}

/// Mark SettingsManager as global application state.
impl SingletonEntity for SettingsManager {}
