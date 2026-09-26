use settings::macros::define_settings_group;
use settings::{RespectUserSyncSetting, SupportedPlatforms, SyncToCloud};

use crate::cloud_object::DriveSortOrder;

// The group is reduced to `sorting_choice`: the drive index browse surface that
// reads it survives until the drive index slice removes both sides. The
// enable_warp_drive toggle and the panel-only sharing_onboarding state fell
// with the rest of the drive surface.
define_settings_group!(WarpDriveSettings, settings: [
    sorting_choice: WarpDriveSortingChoice {
        type: DriveSortOrder,
        default: DriveSortOrder::ByObjectType,
        supported_platforms: SupportedPlatforms::ALL,
        sync_to_cloud: SyncToCloud::Globally(RespectUserSyncSetting::Yes),
        surface: settings::SettingSurfaces::GUI,
        private: false,
        toml_path: "warp_drive.sorting_choice",
        description: "The sort order for items in Warp Drive.",
    },
]);
