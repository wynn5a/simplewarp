use settings::SupportedPlatforms;
use settings::macros::define_settings_group;
use warpui::accessibility::AccessibilityVerbosity;

define_settings_group!(AccessibilitySettings, settings: [
    a11y_verbosity: AccessibilityVerbosityState {
        type: AccessibilityVerbosity,
        default: AccessibilityVerbosity::default(),
        supported_platforms: SupportedPlatforms::ALL,
        surface: settings::SettingSurfaces::GUI,
        private: false,
        storage_key: "AccessibilityVerbosity",
        toml_path: "accessibility.accessibility_verbosity",
        description: "The verbosity level for screen reader announcements.",
    }
]);
