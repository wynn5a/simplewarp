pub mod persistence;

use ai::document::AIDocumentId;
use cloud_objects::cloud_object::GenericCloudObject;
use cloud_objects::ids::ServerId;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct CloudNotebookModel {
    pub title: String,
    pub data: String,
    pub ai_document_id: Option<AIDocumentId>,
}

/// This is the notebook_id in the database associated with this notebook.
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub struct NotebookId(ServerId);
cloud_objects::server_id_traits! { NotebookId, "Notebook" }

/// `CloudNotebook` is a notebook retrieved from the server.
pub type CloudNotebook = GenericCloudObject<NotebookId, CloudNotebookModel>;
