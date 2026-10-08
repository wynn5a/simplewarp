use super::CloudObject;
use crate::server::cloud_objects::update_manager::{InitiatedBy, ObjectOperation};

pub struct CloudObjectToastMessage;

impl CloudObjectToastMessage {
    pub fn toast_message(object: &dyn CloudObject, operation: &ObjectOperation) -> Option<String> {
        let object_name = object.model_type_name().to_owned();

        match operation {
            ObjectOperation::Trash => Some(format!("{object_name} trashed")),
            ObjectOperation::Untrash => Some(format!("{object_name} restored")),
            _ => None,
        }
    }

    pub fn toast_deletion_confirm_message(
        num_objects: i32,
        operation: &ObjectOperation,
    ) -> Option<String> {
        let count_objects_message = match num_objects {
            1 => "1 object".to_string(),
            n => {
                format!("{n} objects")
            }
        };
        match operation {
            // Only user-initiated deletions are confirmed with a toast.
            ObjectOperation::Delete {
                initiated_by: InitiatedBy::User,
            } => Some(format!("{count_objects_message} deleted forever")),
            _ => None,
        }
    }
}
