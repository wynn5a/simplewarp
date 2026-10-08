use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::Arc;

use base64::Engine as _;
use base64::prelude::BASE64_STANDARD;
use bytes::Bytes;
use warpui_core::assets::asset_cache::{AssetSource, AsyncAssetId, AsyncAssetType};

// This crate never fetches over the network: a remote image in a document is not loaded, so
// opening a markdown file or notebook cannot reach out to a host named inside it.

/// Namespace marker for inline base64 `data:` URI async asset sources.
pub struct DataUriAsset;
impl AsyncAssetType for DataUriAsset {}

pub const MAX_DATA_URI_PAYLOAD_BYTES: usize = 16 * 1024 * 1024;

/// Returns `true` if `source` is a base64 `data:` URI whose encoded payload
/// exceeds `MAX_DATA_URI_PAYLOAD_BYTES`. Non-`data:` URIs and `data:` URIs
/// without a `;base64` marker return `false`.
pub fn data_uri_exceeds_limit(source: &str) -> bool {
    let Some((header, payload)) = source
        .strip_prefix("data:")
        .and_then(|rest| rest.split_once(','))
    else {
        return false;
    };
    header
        .split(';')
        .any(|segment| segment.eq_ignore_ascii_case("base64"))
        && payload.len() > MAX_DATA_URI_PAYLOAD_BYTES
}

/// Creates an [`AssetSource::Async`] that decodes an inline base64 `data:` URI
/// (e.g. `data:image/png;base64,<payload>`) into its raw bytes.
pub fn data_uri_source(source: &str) -> Option<AssetSource> {
    // data:[<mediatype>][;base64],<payload>
    let (header, payload) = source.strip_prefix("data:")?.split_once(',')?;
    if !header
        .split(';')
        .any(|segment| segment.eq_ignore_ascii_case("base64"))
    {
        return None;
    }

    // `source` is untrusted; reject oversized payloads before cloning/decoding
    if data_uri_exceeds_limit(source) {
        return None;
    }

    // Derive a compact, stable cache key from the full URI so identical payloads
    // dedupe and we don't retain the (potentially large) data URI as the key.
    let mut hasher = DefaultHasher::new();
    source.hash(&mut hasher);
    let id = format!("{:x}", hasher.finish());

    // base64 payloads may contain embedded whitespace/newlines; strip it before
    // decoding.
    let payload: String = payload.chars().filter(|c| !c.is_whitespace()).collect();

    Some(AssetSource::Async {
        id: AsyncAssetId::new::<DataUriAsset>(id),
        fetch: Arc::new(move || {
            let payload = payload.clone();
            Box::pin(async move {
                BASE64_STANDARD
                    .decode(payload.as_bytes())
                    .map(Bytes::from)
                    .map_err(Into::into)
            })
        }),
    })
}

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
