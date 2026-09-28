use settings::SupportedPlatforms;
use settings::macros::define_settings_group;

define_settings_group!(CommandSearchSettings, settings: [
    show_global_workflows_in_universal_search: ShowGlobalWorkflowsInUniversalSearch {
        type: bool,
        default: false,
        supported_platforms: SupportedPlatforms::ALL,
        surface: settings::SettingSurfaces::GUI,
        private: false,
        toml_path: "workflows.show_global_workflows_in_universal_search",
        description: "Whether to show global workflows in universal search results.",
    },
]);
