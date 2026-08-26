pub mod cloud_action_confirmation_dialog;
pub mod cloud_object_naming_dialog;
pub mod cloud_object_styling;
pub mod drive_helpers;
pub mod empty_trash_confirmation_dialog;
pub mod export;
pub mod import;
pub(crate) mod index;
pub mod items;
pub mod panel;
pub mod settings;
pub mod workflows;

pub use cloud_objects::drive::CloudObjectTypeAndId;
pub use index::DriveIndexVariant;
pub use panel::{DrivePanel, DrivePanelEvent};
use warp_core::user_preferences::GetUserPreferences as _;
use warpui::AppContext;

pub fn should_auto_open_welcome_folder(app: &mut AppContext) -> bool {
    app.private_user_preferences()
        .read_value(settings::HAS_AUTO_OPENED_WELCOME_FOLDER)
        .unwrap_or_default()
        .and_then(|s| serde_json::from_str(&s).ok())
        .map(|has_opened: bool| !has_opened)
        .unwrap_or(true)
}

pub fn write_has_auto_opened_welcome_folder_to_user_defaults(app: &mut AppContext) {
    let _ = app
        .private_user_preferences()
        .write_value(settings::HAS_AUTO_OPENED_WELCOME_FOLDER, true.to_string());
}
