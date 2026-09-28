use super::{CodeForge, SourceRepo};

#[test]
fn source_repo_checkout_ref_round_trips_and_is_optional() {
    // Present: the field serializes under the `checkout_ref` JSON tag and
    // survives a round trip.
    let pinned = SourceRepo::new(CodeForge::GitHub, "warpdotdev".into(), "warp".into())
        .with_checkout_ref(Some("abc123".into()));
    let json = serde_json::to_value(&pinned).unwrap();
    assert_eq!(json.get("checkout_ref").unwrap(), "abc123");
    assert_eq!(serde_json::from_value::<SourceRepo>(json).unwrap(), pinned);

    // Absent: the field is omitted from the wire form and defaults to None on
    // deserialize, so existing payloads without it keep working.
    let unpinned = SourceRepo::new(CodeForge::GitHub, "warpdotdev".into(), "warp".into());
    let json = serde_json::to_value(&unpinned).unwrap();
    assert!(!json.as_object().unwrap().contains_key("checkout_ref"));
    let parsed: SourceRepo =
        serde_json::from_value(serde_json::json!({"owner": "warpdotdev", "repo": "warp"})).unwrap();
    assert_eq!(parsed.checkout_ref, None);
}
