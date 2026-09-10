/// The server marshals its unset Go maps and slices as `null`, which a POST
/// target routinely carries for `headers`.
#[test]
fn upload_target_deserializes_null_headers_and_fields_as_empty() {
    use super::UploadTarget;

    let target: UploadTarget = serde_json::from_value(serde_json::json!({
        "url": "https://example.com/upload",
        "method": "PUT",
        "headers": null,
        "fields": null
    }))
    .unwrap();

    assert!(target.headers.is_empty());
    assert!(target.fields.is_empty());
}

/// Each `kind` here is a discriminator from the `UploadFieldValue` schema in
/// warp-server's `public_api/openapi.yaml`. A mismatch fails the whole target,
/// so every presigned POST upload would break.
#[test]
fn upload_field_value_deserializes_every_server_kind() {
    use super::{UploadFieldValue, UploadTarget};

    let target: UploadTarget = serde_json::from_value(serde_json::json!({
        "url": "https://example.com/upload",
        "method": "POST",
        "headers": {},
        "fields": [
            {"name": "key", "value": {"kind": "static", "value": "object/key"}},
            {"name": "x-amz-checksum-crc32c", "value": {"kind": "content_crc32c"}},
            {"name": "file", "value": {"kind": "content_data"}}
        ]
    }))
    .unwrap();

    assert!(matches!(
        &target.fields[0].value,
        UploadFieldValue::Static { value } if value == "object/key"
    ));
    assert!(matches!(
        target.fields[1].value,
        UploadFieldValue::ContentCrc32C
    ));
    assert!(matches!(
        target.fields[2].value,
        UploadFieldValue::ContentData
    ));
}
