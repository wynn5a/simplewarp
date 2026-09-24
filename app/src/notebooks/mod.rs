pub mod active_notebook_data;
mod context_menu;
pub mod editor;
pub mod file;
pub mod link;
pub mod manager;
pub mod notebook;
mod styles;
pub mod telemetry;

use async_trait::async_trait;
pub use cloud_object_models::{CloudNotebook, CloudNotebookModel, NotebookId, SerializedNotebook};
use cloud_objects::cloud_object::SerializedModel;
use serde::{Deserialize, Serialize};
use warpui::AppContext;

use crate::appearance::Appearance;
use crate::cloud_object::{
    CloudModelType, CloudObjectUpsertParams, ObjectType, Owner, WarpDriveItem,
};
use crate::drive::CloudObjectTypeAndId;
use crate::drive::items::notebook::WarpDriveNotebook;
use crate::persistence::ModelEvent;
use crate::server::ids::SyncId;

#[cfg_attr(not(target_family = "wasm"), async_trait)]
#[cfg_attr(target_family = "wasm", async_trait(?Send))]
impl CloudModelType for CloudNotebookModel {
    type CloudObjectType = CloudNotebook;
    type IdType = NotebookId;

    fn serialized(&self) -> SerializedModel {
        let serialized = SerializedNotebook {
            data: self.data.clone(),
            ai_document_id: self.ai_document_id.as_ref().map(|id| id.to_string()),
            conversation_id: self.conversation_id.clone(),
        };
        let json = serde_json::to_string(&serialized).expect("Failed to serialize notebook");
        SerializedModel::new(json)
    }

    fn model_type_name(&self) -> &'static str {
        if self.ai_document_id.is_some() {
            "Plan"
        } else {
            "Notebook"
        }
    }

    fn object_type(&self) -> ObjectType {
        ObjectType::Notebook
    }

    fn cloud_object_type_and_id(&self, id: SyncId) -> CloudObjectTypeAndId {
        CloudObjectTypeAndId::Notebook(id)
    }

    fn display_name(&self) -> String {
        self.title.clone()
    }

    fn set_display_name(&mut self, name: &str) {
        name.clone_into(&mut self.title);
    }

    fn upsert_event(params: CloudObjectUpsertParams<Self>) -> ModelEvent {
        ModelEvent::UpsertNotebook {
            notebook: CloudNotebook::from(params),
        }
    }

    fn should_update_after_server_conflict(&self) -> bool {
        true
    }
    fn renders_in_warp_drive(&self) -> bool {
        true
    }

    fn can_export(&self) -> bool {
        true
    }

    fn to_warp_drive_item(
        &self,
        id: SyncId,
        _appearance: &Appearance,
        notebook: &CloudNotebook,
    ) -> Option<Box<dyn WarpDriveItem>> {
        Some(Box::new(WarpDriveNotebook::new(
            self.cloud_object_type_and_id(id),
            notebook.clone(),
            notebook.model().ai_document_id.is_some(),
        )))
    }
}

/// A notebook location. Mainly, this lets us distinguish between cloud and file-based notebooks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub enum NotebookLocation {
    /// A cloud notebook in the user's personal space.
    PersonalCloud,
    /// A cloud notebook in a team space.
    Team,
    /// A notebook backed by a local file.
    LocalFile,
    /// A notebook backed by a remote file.
    RemoteFile,
}

impl From<Owner> for NotebookLocation {
    fn from(owner: Owner) -> Self {
        // TODO(ben): Account for shared objects in notebook telemetry.
        match owner {
            Owner::User { .. } => NotebookLocation::PersonalCloud,
            Owner::Team { .. } => NotebookLocation::Team,
        }
    }
}

/// Initialize notebooks-related keybindings.
pub fn init(app: &mut AppContext) {
    self::notebook::init(app);
    self::file::init(app);
    self::editor::view::init(app);
}

/// Translate a notebook's Markdown content into an external Markdown format.
///
/// This:
/// * Normalizes code block languages
/// * Includes extra context for embedded objects.
#[cfg_attr(not(feature = "local_fs"), allow(dead_code))]
pub fn export_notebook(data: &str, ctx: &AppContext) -> anyhow::Result<String> {
    use warp_editor::content::buffer::Buffer;
    use warp_editor::content::markdown::MarkdownStyle;

    // Parse the Markdown directly rather than using [`Buffer::from_markdown`] so that we can
    // report errors to the exporter.
    let parsed = markdown_parser::parse_markdown(data)?;
    Ok(Buffer::export_to_markdown(
        parsed,
        Some(editor::notebook_embedded_item_conversion),
        MarkdownStyle::Export {
            app_context: Some(ctx),
            should_not_escape_markdown_punctuation: false,
        },
    ))
}
