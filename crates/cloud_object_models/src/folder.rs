pub mod persistence;

use cloud_objects::cloud_object::GenericCloudObject;
use cloud_objects::ids::FolderId;

/// The model for a `CloudFolder`.
#[derive(Clone, Debug, PartialEq)]
pub struct CloudFolderModel {
    pub name: String,
    pub is_open: bool,
    pub is_warp_pack: bool,
}

impl CloudFolderModel {
    pub fn new(name: &str, is_warp_pack: bool) -> Self {
        Self {
            name: name.to_owned(),
            is_open: false,
            is_warp_pack,
        }
    }
}

/// `CloudFolder` is a folder retrieved from the server.
pub type CloudFolder = GenericCloudObject<FolderId, CloudFolderModel>;
