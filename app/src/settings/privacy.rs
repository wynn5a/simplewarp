use std::fmt::Display;

use regex::Regex;
use serde::{Deserialize, Serialize};
use settings::macros::{define_settings_group, maybe_define_setting, register_settings_events};
use settings::{RespectUserSyncSetting, Setting, SupportedPlatforms, SyncToCloud};
use warp_errors::{report_error, report_if_error};
use warpui::{AppContext, Entity, ModelContext, SingletonEntity, UpdateModel};

use super::cloud_preferences_syncer::CloudPreferencesSyncer;
use crate::cloud_object::model::persistence::CloudModel;
use crate::terminal::safe_mode_settings::SafeModeSettings;
use crate::workspaces::workspace::EnterpriseSecretRegex;

pub trait RegexDisplayInfo {
    fn pattern(&self) -> &str;
    fn name(&self) -> Option<&str>;
}

pub const TELEMETRY_ENABLED_DEFAULTS_KEY: &str = "TelemetryEnabled";
pub const CRASH_REPORTING_ENABLED_DEFAULTS_KEY: &str = "CrashReportingEnabled";
pub const CLOUD_CONVERSATION_STORAGE_ENABLED_DEFAULTS_KEY: &str = "CloudConversationStorageEnabled";

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[schemars(description = "A custom regex pattern for detecting and redacting secrets.")]
pub struct CustomSecretRegex {
    #[serde(with = "serde_regex")]
    #[schemars(with = "String", description = "The regex pattern to match secrets.")]
    pub pattern: Regex,
    #[serde(default)]
    #[schemars(description = "Optional display name for this secret pattern.")]
    pub name: Option<String>,
}

impl CustomSecretRegex {
    pub fn pattern(&self) -> &Regex {
        &self.pattern
    }
}

impl RegexDisplayInfo for CustomSecretRegex {
    fn pattern(&self) -> &str {
        self.pattern.as_str()
    }

    fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }
}

impl RegexDisplayInfo for EnterpriseSecretRegex {
    fn pattern(&self) -> &str {
        &self.pattern
    }

    fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }
}

impl Display for CustomSecretRegex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.pattern.as_str())
    }
}

impl PartialEq for CustomSecretRegex {
    /// We do not factor in the name to equality checks --
    /// if the regex is the same, then the regex is the same.
    /// This allows us to avoid adding duplicate regexes.
    fn eq(&self, other: &Self) -> bool {
        self.pattern.as_str() == other.pattern.as_str()
    }
}

impl settings_value::SettingsValue for CustomSecretRegex {}

define_settings_group!(WarpDrivePrivacySettings, settings: [
    is_telemetry_enabled: IsTelemetryEnabled {
        type: bool,
        default: true,
        supported_platforms: SupportedPlatforms::ALL,
        sync_to_cloud: SyncToCloud::Globally(RespectUserSyncSetting::No),
        surface: settings::SettingSurfaces::ALL,
        private: false,
        storage_key: "TelemetryEnabled",
        toml_path: "privacy.telemetry_enabled",
        description: "Whether anonymous usage telemetry is collected.",
    },
    is_crash_reporting_enabled: IsCrashReportingEnabled {
        type: bool,
        default: true,
        supported_platforms: SupportedPlatforms::ALL,
        sync_to_cloud: SyncToCloud::Globally(RespectUserSyncSetting::No),
        surface: settings::SettingSurfaces::ALL,
        private: false,
        storage_key: "CrashReportingEnabled",
        toml_path: "privacy.crash_reporting_enabled",
        description: "Whether crash reports are sent.",
    },
    is_cloud_conversation_storage_enabled: IsCloudConversationStorageEnabled {
        type: bool,
        default: true,
        supported_platforms: SupportedPlatforms::ALL,
        sync_to_cloud: SyncToCloud::Globally(RespectUserSyncSetting::No),
        surface: settings::SettingSurfaces::ALL,
        private: false,
        storage_key: "CloudConversationStorageEnabled",
        toml_path: "agents.cloud_conversation_storage_enabled",
        description: "Whether conversations are stored in the cloud.",
    },
]);

maybe_define_setting!(CustomSecretRegexList, group: PrivacySettings, {
    type: Vec<CustomSecretRegex>,
    default: Vec::new(),
    supported_platforms: SupportedPlatforms::ALL,
    sync_to_cloud: SyncToCloud::Globally(RespectUserSyncSetting::No),
    surface: settings::SettingSurfaces::GUI,
    private: false,
    toml_path: "privacy.custom_secret_regex_list",
    description: "Custom regex patterns for detecting and redacting secrets.",
});

maybe_define_setting!(HasInitializedDefaultSecretRegexes, group: PrivacySettings, {
    type: bool,
    default: false,
    supported_platforms: SupportedPlatforms::ALL,
    sync_to_cloud: SyncToCloud::Globally(RespectUserSyncSetting::No),
    surface: settings::SettingSurfaces::GUI,
    private: true,
});

/// Singleton model for managing the user's privacy settings (whether the user has enabled crash
/// reporting and/or telemetry).
pub struct PrivacySettings {
    pub is_telemetry_enabled: bool,
    pub is_crash_reporting_enabled: bool,
    pub is_cloud_conversation_storage_enabled: bool,
    pub has_initialized_default_secret_regexes: HasInitializedDefaultSecretRegexes,
    /// List of user defined secret regexes.
    /// Enterprise-level secret regexes will always take precedence over user-level secrets,
    /// but they both used to support additive behavior.
    /// It's a [Vec<CustomSecretRegex>], but also a user setting.
    pub user_secret_regex_list: CustomSecretRegexList,
    /// List of enterprise-level secret regexes provided by the organization.
    /// These are kept separate from user-level secrets to support additive behavior.
    pub enterprise_secret_regex_list: Vec<CustomSecretRegex>,
    /// Whether or not the user's organization has forced telemetry on, in which case we ignore any
    /// user local/cloud settings. If false, we fall back to the user's settings.
    /// This is populated by the server when teams data is fetched.
    pub is_telemetry_force_enabled: bool,
    /// Whether or not the user's organization has enabled enterprise secret redaction.
    /// This is populated by the server when teams data is fetched.
    pub is_enterprise_secret_redaction_enabled: bool,
}

impl PrivacySettings {
    /// Registers a singleton PrivacySettings model on `app`.
    ///
    /// We expose this function publicly (while keeping the constructor private) to prevent
    /// instantiation another PrivacySettings struct, in the case where a developer might be
    /// unaware that it is registered as a singleton model.
    pub fn register_singleton(ctx: &mut AppContext) {
        let handle = ctx.add_singleton_model(PrivacySettings::new);

        register_settings_events!(
            PrivacySettings,
            user_secret_regex_list,
            CustomSecretRegexList,
            handle,
            ctx
        );
    }

    /// Returns a new PrivacySettings object initialized from locally cached values.
    fn new(ctx: &mut ModelContext<Self>) -> Self {
        // Initialize from `WarpDrivePrivacySettings`, which is the source of truth for these
        // booleans.
        let warp_drive_privacy = WarpDrivePrivacySettings::as_ref(ctx);
        let is_telemetry_enabled = *warp_drive_privacy.is_telemetry_enabled.value();
        let is_crash_reporting_enabled = *warp_drive_privacy.is_crash_reporting_enabled.value();
        let is_cloud_conversation_storage_enabled = *warp_drive_privacy
            .is_cloud_conversation_storage_enabled
            .value();

        // Listen for changes to the cloud model and update ourselves when they happen.
        ctx.subscribe_to_model(
            &WarpDrivePrivacySettings::handle(ctx),
            |me, _, event, ctx| {
                let privacy_settings = WarpDrivePrivacySettings::as_ref(ctx);
                match event {
                    WarpDrivePrivacySettingsChangedEvent::IsTelemetryEnabled { .. } => {
                        me.set_is_telemetry_enabled(
                            *privacy_settings.is_telemetry_enabled.value(),
                            ctx,
                        );
                    }
                    WarpDrivePrivacySettingsChangedEvent::IsCrashReportingEnabled { .. } => {
                        me.set_is_crash_reporting_enabled(
                            *privacy_settings.is_crash_reporting_enabled.value(),
                            ctx,
                        );
                    }
                    WarpDrivePrivacySettingsChangedEvent::IsCloudConversationStorageEnabled {
                        ..
                    } => {
                        me.set_is_cloud_conversation_storage_enabled(
                            *privacy_settings
                                .is_cloud_conversation_storage_enabled
                                .value(),
                            ctx,
                        );
                    }
                }
            },
        );

        let user_secret_regex_list: CustomSecretRegexList =
            CustomSecretRegexList::new_from_storage(ctx);
        let has_initialized_default_secret_regexes: HasInitializedDefaultSecretRegexes =
            HasInitializedDefaultSecretRegexes::new_from_storage(ctx);

        Self {
            is_crash_reporting_enabled,
            is_telemetry_enabled,
            is_cloud_conversation_storage_enabled,
            user_secret_regex_list,
            has_initialized_default_secret_regexes,
            is_telemetry_force_enabled: false,
            is_enterprise_secret_redaction_enabled: false,
            enterprise_secret_regex_list: Vec::new(),
        }
    }

    pub fn is_telemetry_force_enabled(&self) -> bool {
        self.is_telemetry_force_enabled
    }

    pub fn set_is_telemetry_force_enabled(&mut self, is_telemetry_force_enabled: bool) {
        self.is_telemetry_force_enabled = is_telemetry_force_enabled;
    }

    pub fn is_enterprise_secret_redaction_enabled(&self) -> bool {
        self.is_enterprise_secret_redaction_enabled
    }

    pub fn set_enterprise_secret_redaction_settings(
        &mut self,
        enabled: bool,
        enterprise_regexes: Vec<EnterpriseSecretRegex>,
        change_event_reason: ChangeEventReason,
        ctx: &mut ModelContext<Self>,
    ) {
        if enabled {
            // First time: Force enable secret redaction setting (safe mode).
            if !self.is_enterprise_secret_redaction_enabled {
                let safe_mode_settings = SafeModeSettings::handle(ctx);
                ctx.update_model(&safe_mode_settings, |safe_mode_settings, ctx| {
                    let _ = safe_mode_settings.safe_mode_enabled.set_value(true, ctx);
                });
            }

            // Convert EnterpriseSecretRegex to CustomSecretRegex for internal use
            let mut enterprise_secrets = Vec::new();
            for enterprise_regex in enterprise_regexes {
                match Regex::new(&enterprise_regex.pattern) {
                    Ok(regex) => {
                        enterprise_secrets.push(CustomSecretRegex {
                            pattern: regex,
                            name: enterprise_regex.name,
                        });
                    }
                    _ => {
                        report_error!(
                            "Invalid enterprise secret regex pattern",
                            extra: { "pattern" => %enterprise_regex.pattern }
                        );
                    }
                }
            }
            self.enterprise_secret_regex_list = enterprise_secrets;
        } else {
            // Clear enterprise secrets when disabled
            self.enterprise_secret_regex_list.clear();
        }

        self.is_enterprise_secret_redaction_enabled = enabled;

        ctx.emit(PrivacySettingsChangedEvent::CustomSecretRegexList {
            change_event_reason,
        });
        ctx.notify();
    }

    pub fn refresh_to_default(&mut self) {
        // TODO(zach): this seems incorrect - should we also update the values on disk?
        self.is_telemetry_enabled = true;
        self.is_crash_reporting_enabled = true;
        self.is_cloud_conversation_storage_enabled = true;
        self.is_telemetry_force_enabled = false;
        self.is_enterprise_secret_redaction_enabled = false;
    }

    /// Constructor for tests only.
    #[cfg(any(test, feature = "test-util"))]
    pub fn mock(_ctx: &mut ModelContext<Self>) -> Self {
        Self {
            is_crash_reporting_enabled: true,
            is_telemetry_enabled: true,
            is_cloud_conversation_storage_enabled: true,
            user_secret_regex_list: CustomSecretRegexList::new(None),
            has_initialized_default_secret_regexes: HasInitializedDefaultSecretRegexes::new(None),
            is_telemetry_force_enabled: false,
            is_enterprise_secret_redaction_enabled: false,
            enterprise_secret_regex_list: Vec::new(),
        }
    }

    /// Sets `is_crash_reporting_enabled` to the given value.
    ///
    /// Additionally, this writes the given value to the user's local defaults, and emits a
    /// `PrivacySettingsEvent::UpdateIsCrashReportingEnabled` event.
    pub fn set_is_crash_reporting_enabled(
        &mut self,
        new_value: bool,
        ctx: &mut ModelContext<PrivacySettings>,
    ) {
        let old_value = self.is_crash_reporting_enabled;
        if new_value != old_value {
            self.is_crash_reporting_enabled = new_value;

            WarpDrivePrivacySettings::handle(ctx).update(ctx, |settings, ctx| {
                log::info!("Setting is_crash_reporting_enabled to {new_value}");
                let _ = settings
                    .is_crash_reporting_enabled
                    .set_value(new_value, ctx);
            });

            ctx.emit(PrivacySettingsChangedEvent::UpdateIsCrashReportingEnabled {
                old_value,
                new_value,
            });
            ctx.notify();
        }
    }

    /// Sets `is_telemetry_enabled` to the given value.
    ///
    /// Additionally, this writes the given value to the user's local defaults, and emits a
    /// `PrivacySettingsEvent::UpdateIsTelemetryEnabled` event.
    pub fn set_is_telemetry_enabled(
        &mut self,
        new_value: bool,
        ctx: &mut ModelContext<PrivacySettings>,
    ) {
        let old_value = self.is_telemetry_enabled;
        if new_value != old_value {
            self.is_telemetry_enabled = new_value;

            WarpDrivePrivacySettings::handle(ctx).update(ctx, |settings, ctx| {
                log::info!("Setting is_telemetry_enabled to {new_value}");
                let _ = settings.is_telemetry_enabled.set_value(new_value, ctx);
            });

            ctx.emit(PrivacySettingsChangedEvent::UpdateIsTelemetryEnabled {
                old_value,
                new_value,
            });
            ctx.notify();
        }
    }

    pub fn set_is_cloud_conversation_storage_enabled(
        &mut self,
        new_value: bool,
        ctx: &mut ModelContext<PrivacySettings>,
    ) {
        let old_value = self.is_cloud_conversation_storage_enabled;
        if new_value == old_value {
            return;
        }

        self.is_cloud_conversation_storage_enabled = new_value;

        WarpDrivePrivacySettings::handle(ctx).update(ctx, |settings, ctx| {
            log::info!("Setting is_cloud_conversation_storage_enabled to {new_value}");
            let _ = settings
                .is_cloud_conversation_storage_enabled
                .set_value(new_value, ctx);
        });

        ctx.emit(
            PrivacySettingsChangedEvent::UpdateIsCloudConversationStorageEnabled {
                old_value,
                new_value,
            },
        );
        ctx.notify();
    }

    pub fn remove_user_secret_regex(&mut self, idx: &usize, ctx: &mut ModelContext<Self>) {
        let mut new_user_secret_regex_list = self.user_secret_regex_list.to_vec();
        new_user_secret_regex_list.remove(*idx);
        if self
            .user_secret_regex_list
            .set_value(new_user_secret_regex_list, ctx)
            .is_err()
        {
            report_error!("Custom Secret Regex List failed to serialize")
        }
    }

    /// Initializes the custom secret regex list with the default regexes, provided
    /// non matches can be found.
    /// This can be called when a user first enables secret redaction.
    pub fn add_all_recommended_regex(&mut self, ctx: &mut ModelContext<Self>) {
        let mut new_user_secret_regex_list = self.user_secret_regex_list.to_vec();
        let num_existing_regexes = new_user_secret_regex_list.len();

        // Add all the default regexes if they don't already exist
        for default_regex in crate::terminal::model::secrets::regexes::DEFAULT_REGEXES_WITH_NAMES {
            match Regex::new(default_regex.pattern) {
                Ok(regex) => {
                    let custom_regex = CustomSecretRegex {
                        pattern: regex,
                        name: Some(default_regex.name.to_string()),
                    };
                    if !new_user_secret_regex_list.contains(&custom_regex) {
                        new_user_secret_regex_list.push(custom_regex);
                    }
                }
                _ => {
                    report_error!(
                        "Failed to compile default regex",
                        extra: { "pattern" => %default_regex.pattern }
                    );
                }
            }
        }

        if num_existing_regexes == new_user_secret_regex_list.len() {
            return;
        }

        if self
            .user_secret_regex_list
            .set_value(new_user_secret_regex_list, ctx)
            .is_err()
        {
            report_error!("Failed to serialize default regexes to custom secret regex list")
        }

        ctx.notify();
    }

    /// Disables the default regex trigger, so that it will not be executed.
    pub fn disable_default_regex_trigger(&mut self, ctx: &mut ModelContext<Self>) {
        if self
            .has_initialized_default_secret_regexes
            .set_value(true, ctx)
            .is_err()
        {
            report_error!("Failed to disable default regex trigger");
        }
    }

    /// Initializes the custom secret regex list with the default regexes.
    /// This will only be executed once per user, and only if they haven't already initialized.
    pub fn initialize_default_regexes_once(&mut self, ctx: &mut ModelContext<Self>) {
        // Only initialize if we haven't done so before
        if !*self.has_initialized_default_secret_regexes.value() {
            self.add_all_recommended_regex(ctx);

            // Mark as initialized
            if self
                .has_initialized_default_secret_regexes
                .set_value(true, ctx)
                .is_err()
            {
                report_error!("Failed to set has_initialized_default_secret_regexes flag");
            }
        }
    }

    /// We wait until warp drive prefs have loaded and then either
    /// 1) use them as the data store for is_telemetry_enabled and is_crash_reporting_enabled, if those
    ///    values are set in warp drive, or
    /// 2) update the warp drive prefs to match the values from the legacy user_settings endpoint so
    ///    that we can use warp drive prefs going forward.
    pub fn maybe_sync_with_warp_drive_prefs(&mut self, ctx: &mut ModelContext<Self>) {
        self.handle_warp_drive_objects_loaded(ctx);
    }

    fn handle_warp_drive_objects_loaded(&mut self, ctx: &mut ModelContext<Self>) {
        self.initialize_default_regexes_once(ctx);
        // Check if the warp drive preferences are set. If they are, and telemetry and crash reporting
        // are set as warp drive prefs, then use those.  Otherwise, update the warp drive prefs to match
        // the values from the legacy user_settings endpoint so that we can use warp drive prefs going forward.
        let cloud_model = CloudModel::as_ref(ctx);
        let cloud_prefs = cloud_model.get_all_cloud_preferences_by_storage_key();
        let cloud_telemetry_value =
            cloud_prefs
                .get(IsTelemetryEnabled::storage_key())
                .map(|pref| {
                    pref.model()
                        .string_model
                        .value
                        .as_bool()
                        .unwrap_or_default()
                });
        let cloud_crash_reporting_value = cloud_prefs
            .get(IsCrashReportingEnabled::storage_key())
            .map(|pref| {
                pref.model()
                    .string_model
                    .value
                    .as_bool()
                    .unwrap_or_default()
            });
        let cloud_conversation_storage_value = cloud_prefs
            .get(IsCloudConversationStorageEnabled::storage_key())
            .map(|pref| {
                pref.model()
                    .string_model
                    .value
                    .as_bool()
                    .unwrap_or_default()
            });

        match (
            cloud_telemetry_value,
            cloud_crash_reporting_value,
            cloud_conversation_storage_value,
        ) {
            (
                Some(is_telemetry_enabled),
                Some(is_crash_reporting_enabled),
                Some(is_cloud_conversation_storage_enabled),
            ) => {
                log::info!(
                    "Warp Drive privacy preferences are set, using those for telemetry={is_telemetry_enabled}, \
                    crash_reporting={is_crash_reporting_enabled}, cloud_conversation_storage={is_cloud_conversation_storage_enabled}"
                );
                self.set_is_telemetry_enabled(is_telemetry_enabled, ctx);
                self.set_is_crash_reporting_enabled(is_crash_reporting_enabled, ctx);
                self.set_is_cloud_conversation_storage_enabled(
                    is_cloud_conversation_storage_enabled,
                    ctx,
                );
            }
            _ => {
                log::info!(
                    "Warp Drive privacy preferences are not set, syncing local PrivacySettings values to \
                    WarpDrivePrivacySettings and cloud. telemetry={}, crash_reporting={}, \
                    cloud_conversation_storage={}",
                    self.is_telemetry_enabled,
                    self.is_crash_reporting_enabled,
                    self.is_cloud_conversation_storage_enabled
                );
                // First, ensure WarpDrivePrivacySettings (the define_settings_group model)
                // reflects the actual PrivacySettings in-memory values. These may differ
                // because WarpDrivePrivacySettings defaults to `true` for all three settings,
                // while the user may have changed them to `false` via PrivacySettings before
                // signing up. Without this step, maybe_sync_local_prefs_to_cloud would read
                // the stale WarpDrivePrivacySettings defaults and push those to the cloud.
                WarpDrivePrivacySettings::handle(ctx).update(ctx, |settings, ctx| {
                    report_if_error!(
                        settings
                            .is_telemetry_enabled
                            .set_value(self.is_telemetry_enabled, ctx)
                    );
                    report_if_error!(
                        settings
                            .is_crash_reporting_enabled
                            .set_value(self.is_crash_reporting_enabled, ctx)
                    );
                    report_if_error!(
                        settings
                            .is_cloud_conversation_storage_enabled
                            .set_value(self.is_cloud_conversation_storage_enabled, ctx)
                    );
                });
                CloudPreferencesSyncer::handle(ctx).update(ctx, |syncer, ctx| {
                    syncer.maybe_sync_local_prefs_to_cloud(
                        vec![
                            IsTelemetryEnabled::storage_key().to_string(),
                            IsCrashReportingEnabled::storage_key().to_string(),
                            IsCloudConversationStorageEnabled::storage_key().to_string(),
                        ],
                        ctx,
                    );
                });
            }
        }
    }
}

/// Events emitted when PrivacySettings is updated.
#[derive(Clone, Copy)]
pub enum PrivacySettingsChangedEvent {
    UpdateIsTelemetryEnabled {
        old_value: bool,
        new_value: bool,
    },
    UpdateIsCrashReportingEnabled {
        old_value: bool,
        new_value: bool,
    },
    UpdateIsCloudConversationStorageEnabled {
        old_value: bool,
        new_value: bool,
    },
    CustomSecretRegexList {
        change_event_reason: ChangeEventReason,
    },
    HasInitializedDefaultSecretRegexes {
        change_event_reason: ChangeEventReason,
    },
}

impl Entity for PrivacySettings {
    type Event = PrivacySettingsChangedEvent;
}

impl SingletonEntity for PrivacySettings {}

#[cfg(test)]
#[path = "privacy_tests.rs"]
mod tests;
