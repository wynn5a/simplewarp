use super::CloudObject;
use crate::server::cloud_objects::update_manager::{
    InitiatedBy, ObjectOperation, OperationSuccessType,
};

pub struct CloudObjectToastMessage;

impl CloudObjectToastMessage {
    pub fn toast_message(
        object: &dyn CloudObject,
        operation: &ObjectOperation,
        success_type: &OperationSuccessType,
    ) -> Option<String> {
        let object_name = object.model_type_name().to_owned();

        match (operation, success_type) {
            (ObjectOperation::Trash, OperationSuccessType::Success) => {
                Some(format!("{object_name} trashed"))
            }
            (ObjectOperation::Untrash, OperationSuccessType::Success) => {
                Some(format!("{object_name} restored"))
            }
            _ => None,
        }
    }

    pub fn toast_deletion_confirm_message(
        num_objects: i32,
        operation: &ObjectOperation,
        success_type: &OperationSuccessType,
    ) -> Option<String> {
        let count_objects_message = match num_objects {
            1 => "1 object".to_string(),
            n => {
                format!("{n} objects")
            }
        };
        match (operation, success_type) {
            // We should only show deletion failure toasts for user-initiated deletions.
            (
                ObjectOperation::Delete {
                    initiated_by: InitiatedBy::User,
                },
                OperationSuccessType::Success,
            ) => Some(format!("{count_objects_message} deleted forever")),
            _ => None,
        }
    }
}
