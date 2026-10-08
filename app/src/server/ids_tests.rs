use super::{ClientId, SyncId};

#[test]
pub fn test_client_sync_id_serialization() {
    let id: SyncId = SyncId::from(ClientId::new());
    let serialized = serde_json::to_string(&id).expect("failed to serialize");
    assert_eq!(serialized, format!("\"{}\"", id.uid()));
    let deserialized: SyncId =
        serde_json::from_str(serialized.as_str()).expect("failed to deserialize");
    assert_eq!(id, deserialized);
}

/// Ids minted by the server fork this one descends from are not client ids, so they are refused
/// rather than guessed at.
#[test]
pub fn test_server_style_sync_id_is_refused() {
    assert!(serde_json::from_str::<SyncId>("\"Ymgrzu0nh2HwDNeYEtXF1x\"").is_err());
}
