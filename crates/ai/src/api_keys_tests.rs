use super::*;

fn make_manager(keys: ApiKeys) -> ApiKeyManager {
    ApiKeyManager {
        keys,
        secure_storage_write_version: 0,
    }
}

#[test]
fn persisted_provider_api_key_updates_request_state() {
    warpui_core::App::test((), |mut app| async move {
        app.update(|ctx| {
            warpui_extras::secure_storage::register_noop("test", ctx);
        });
        let manager = app.add_singleton_model(ApiKeyManager::new);

        manager
            .update(&mut app, |manager, ctx| {
                manager.persist_provider_key(
                    LLMProvider::Anthropic,
                    Some("sk-ant-test".to_owned()),
                    ctx,
                )
            })
            .expect("no-op secure storage should accept the provider key");

        manager.read(&app, |manager, _| {
            let request_keys = manager
                .api_keys_for_request()
                .expect("persisted provider key should be available to requests");
            assert_eq!(request_keys.anthropic, "sk-ant-test");
        });
    });
}

#[test]
fn persisted_provider_api_key_can_be_cleared() {
    warpui_core::App::test((), |mut app| async move {
        app.update(|ctx| {
            warpui_extras::secure_storage::register_noop("test", ctx);
        });
        let manager = app.add_singleton_model(ApiKeyManager::new);

        manager
            .update(&mut app, |manager, ctx| {
                manager.persist_provider_key(
                    LLMProvider::Anthropic,
                    Some("sk-ant-test".to_owned()),
                    ctx,
                )?;
                manager.persist_provider_key(LLMProvider::Anthropic, None, ctx)
            })
            .expect("no-op secure storage should clear the provider key");

        manager.read(&app, |manager, _| {
            assert_eq!(manager.keys().anthropic, None);
        });
    });
}
#[test]
fn custom_model_providers_preserves_configured_schema() {
    let mut endpoint = endpoint_with_keys(
        "Anthropic",
        "https://custom.io",
        "ep-key",
        &[("claude", None, "uuid-1")],
    );
    endpoint.schema = CustomEndpointSchema::AnthropicMessages;
    let mgr = make_manager(ApiKeys {
        custom_endpoints: vec![endpoint],
        ..Default::default()
    });

    let provider = &mgr
        .custom_model_providers_for_request()
        .expect("configured endpoint should be sent")
        .providers[0];
    assert_eq!(
        provider.schema,
        CustomEndpointSchema::AnthropicMessages as i32
    );
}

fn endpoint(
    name: &str,
    url: &str,
    api_key: &str,
    models: &[(&str, Option<&str>)],
) -> CustomEndpoint {
    endpoint_with_keys(
        name,
        url,
        api_key,
        &models
            .iter()
            .enumerate()
            .map(|(i, (n, a))| (*n, *a, format!("cfg-{i}")))
            .collect::<Vec<_>>()
            .iter()
            .map(|(n, a, k)| (*n, *a, k.as_str()))
            .collect::<Vec<_>>(),
    )
}

fn endpoint_with_keys(
    name: &str,
    url: &str,
    api_key: &str,
    models: &[(&str, Option<&str>, &str)],
) -> CustomEndpoint {
    CustomEndpoint {
        name: name.into(),
        url: url.into(),
        api_key: api_key.into(),
        schema: CustomEndpointSchema::default(),
        models: models
            .iter()
            .map(|(n, a, cfg)| CustomEndpointModel {
                name: (*n).into(),
                alias: a.map(|s| s.into()),
                config_key: (*cfg).into(),
            })
            .collect(),
    }
}

// ── serde round-trip ────────────────────────────────────────────

#[test]
fn serde_round_trip_empty() {
    let keys = ApiKeys::default();
    let json = serde_json::to_string(&keys).unwrap();
    let deser: ApiKeys = serde_json::from_str(&json).unwrap();
    assert_eq!(keys, deser);
}

#[test]
fn serde_round_trip_with_provider_keys() {
    let keys = ApiKeys {
        openai: Some("sk-openai".into()),
        anthropic: Some("sk-ant-abc".into()),
        google: Some("AIzaSy123".into()),
        open_router: Some("sk-or-xxx".into()),
        custom_endpoints: vec![],
    };
    let json = serde_json::to_string(&keys).unwrap();
    let deser: ApiKeys = serde_json::from_str(&json).unwrap();
    assert_eq!(keys, deser);
}

#[test]
fn serde_round_trip_with_custom_endpoints() {
    let keys = ApiKeys {
        openai: None,
        anthropic: None,
        google: None,
        open_router: None,
        custom_endpoints: vec![
            endpoint("ep1", "https://a.io/v1", "key1", &[("gpt-4", Some("fast"))]),
            endpoint(
                "ep2",
                "https://b.io/v1",
                "key2",
                &[("llama-70b", None), ("mixtral", Some("mix"))],
            ),
        ],
    };
    let json = serde_json::to_string(&keys).unwrap();
    let deser: ApiKeys = serde_json::from_str(&json).unwrap();
    assert_eq!(keys, deser);
}

#[test]
fn serde_ignores_unknown_fields() {
    let json = r#"{"openai":"sk-x","unknown_field":"value","custom_endpoints":[]}"#;
    let keys: ApiKeys = serde_json::from_str(json).unwrap();
    assert_eq!(keys.openai, Some("sk-x".into()));
    assert!(keys.custom_endpoints.is_empty());
}
#[test]
fn serde_legacy_endpoint_defaults_to_chat_completions() {
    let endpoint: CustomEndpoint = serde_json::from_str(
        r#"{"name":"legacy","url":"https://example.com","api_key":"key","models":[]}"#,
    )
    .unwrap();
    assert_eq!(endpoint.schema, CustomEndpointSchema::OpenaiChatCompletions);
}

// ── has_any_key ─────────────────────────────────────────────────

#[test]
fn has_any_key_false_when_empty() {
    assert!(!ApiKeys::default().has_any_key());
}

#[test]
fn has_any_key_true_for_openai_only() {
    let keys = ApiKeys {
        openai: Some("sk-x".into()),
        ..Default::default()
    };
    assert!(keys.has_any_key());
}

#[test]
fn has_any_key_true_for_custom_endpoints_only() {
    let keys = ApiKeys {
        custom_endpoints: vec![endpoint("ep", "https://a.io", "key", &[("m", None)])],
        ..Default::default()
    };
    assert!(keys.has_any_key());
}

#[test]
fn has_any_key_false_for_endpoint_with_empty_api_key() {
    let keys = ApiKeys {
        custom_endpoints: vec![endpoint("ep", "https://a.io", "", &[("m", None)])],
        ..Default::default()
    };
    assert!(!keys.has_any_key());
}

// ── provider_key_count ─────────────────────────────────────────

#[test]
fn provider_key_count_zero_when_empty() {
    assert_eq!(ApiKeys::default().provider_key_count(), 0);
}

#[test]
fn provider_key_count_counts_each_provider_key() {
    let keys = ApiKeys {
        openai: Some("sk-o".into()),
        anthropic: Some("sk-a".into()),
        google: Some("AIza".into()),
        open_router: Some("sk-or".into()),
        custom_endpoints: vec![],
    };
    assert_eq!(keys.provider_key_count(), 4);
}

#[test]
fn provider_key_count_ignores_blank_keys_and_endpoints() {
    let keys = ApiKeys {
        openai: Some("sk-o".into()),
        anthropic: Some("   ".into()),
        google: None,
        open_router: None,
        custom_endpoints: vec![endpoint("ep", "https://a.io", "k", &[("m", None)])],
    };
    // Only the non-blank OpenAI key counts; the whitespace Anthropic key and the
    // custom endpoint are excluded.
    assert_eq!(keys.provider_key_count(), 1);
}

// ── custom_model_providers_for_request ──────────────────────────

#[test]
fn custom_model_providers_none_when_empty() {
    let mgr = make_manager(ApiKeys::default());
    assert!(mgr.custom_model_providers_for_request().is_none());
}

#[test]
fn custom_model_providers_populates_single_endpoint() {
    let mgr = make_manager(ApiKeys {
        custom_endpoints: vec![endpoint_with_keys(
            "My EP",
            "https://custom.io/v1",
            "ep-key",
            &[("big-model", Some("alias"), "uuid-1")],
        )],
        ..Default::default()
    });
    let result = mgr.custom_model_providers_for_request().unwrap();
    assert_eq!(result.providers.len(), 1);
    let p = &result.providers[0];
    assert_eq!(p.base_url, "https://custom.io/v1");
    assert_eq!(p.api_key, "ep-key");
    assert_eq!(p.models.len(), 1);
    assert_eq!(p.models[0].slug, "big-model");
    assert_eq!(p.models[0].config_key, "uuid-1");
    assert_eq!(p.schema, CustomEndpointSchema::OpenaiChatCompletions as i32);
}

#[test]
fn multiple_endpoints_all_serialize() {
    let mgr = make_manager(ApiKeys {
        custom_endpoints: vec![
            endpoint_with_keys(
                "ep1",
                "https://a.io",
                "k1",
                &[("gpt-4", Some("fast"), "uuid-a")],
            ),
            endpoint_with_keys(
                "ep2",
                "https://b.io",
                "k2",
                &[
                    ("llama-70b", None, "uuid-b"),
                    ("mixtral", Some("mix"), "uuid-c"),
                ],
            ),
        ],
        ..Default::default()
    });
    let result = mgr.custom_model_providers_for_request().unwrap();
    assert_eq!(result.providers.len(), 2);
    assert_eq!(result.providers[0].base_url, "https://a.io");
    assert_eq!(result.providers[0].models[0].config_key, "uuid-a");
    assert_eq!(result.providers[1].base_url, "https://b.io");
    assert_eq!(result.providers[1].models.len(), 2);
    assert_eq!(result.providers[1].models[0].slug, "llama-70b");
    assert_eq!(result.providers[1].models[0].config_key, "uuid-b");
    assert_eq!(result.providers[1].models[1].config_key, "uuid-c");
}

#[test]
fn empty_api_key_endpoints_are_skipped() {
    let mgr = make_manager(ApiKeys {
        custom_endpoints: vec![
            endpoint_with_keys("empty", "https://a.io", "", &[("m", None, "uuid-x")]),
            endpoint_with_keys("ok", "https://b.io", "k", &[("m", None, "uuid-y")]),
        ],
        ..Default::default()
    });
    let result = mgr.custom_model_providers_for_request().unwrap();
    assert_eq!(result.providers.len(), 1);
    assert_eq!(result.providers[0].base_url, "https://b.io");
}

#[test]
fn endpoints_with_only_empty_models_are_skipped() {
    let mgr = make_manager(ApiKeys {
        custom_endpoints: vec![endpoint_with_keys(
            "ep",
            "https://a.io",
            "k",
            &[("", None, "uuid-z")],
        )],
        ..Default::default()
    });
    assert!(mgr.custom_model_providers_for_request().is_none());
}

// ── display_label fallback ─────────────────────────────────────

#[test]
fn display_label_uses_alias_when_present() {
    let m = CustomEndpointModel {
        name: "raw-name".into(),
        alias: Some("My Alias".into()),
        config_key: "k".into(),
    };
    assert_eq!(m.display_label(), "My Alias");
}

#[test]
fn display_label_falls_back_to_name_when_alias_missing() {
    let m = CustomEndpointModel {
        name: "raw-name".into(),
        alias: None,
        config_key: "k".into(),
    };
    assert_eq!(m.display_label(), "raw-name");
}

#[test]
fn display_label_falls_back_to_name_when_alias_is_whitespace() {
    let m = CustomEndpointModel {
        name: "raw-name".into(),
        alias: Some("   ".into()),
        config_key: "k".into(),
    };
    assert_eq!(m.display_label(), "raw-name");
}

// ── api_keys_for_request ────────────────────────────────────────

#[test]
fn api_keys_for_request_none_when_empty() {
    let mgr = make_manager(ApiKeys::default());
    assert!(mgr.api_keys_for_request().is_none());
}

#[test]
fn api_keys_for_request_populates_provider_keys() {
    let mgr = make_manager(ApiKeys {
        openai: Some("sk-o".into()),
        anthropic: Some("sk-a".into()),
        ..Default::default()
    });
    let result = mgr.api_keys_for_request().unwrap();
    assert_eq!(result.openai, "sk-o");
    assert_eq!(result.anthropic, "sk-a");
    assert!(result.google.is_empty());
}

#[test]
fn api_keys_for_request_none_for_custom_endpoints_only() {
    let mgr = make_manager(ApiKeys {
        custom_endpoints: vec![endpoint("ep", "https://a.io", "k", &[("m", None)])],
        ..Default::default()
    });
    assert!(mgr.api_keys_for_request().is_none());
}

// ── ApiKeyManager::has_any_key ──────────────────

#[test]
fn manager_has_any_key_false_when_no_keys() {
    let mgr = make_manager(ApiKeys::default());
    assert!(!mgr.has_any_key());
}

#[test]
fn manager_has_any_key_true_for_pasted_key() {
    let mgr = make_manager(ApiKeys {
        openai: Some("sk-x".into()),
        ..Default::default()
    });
    assert!(mgr.has_any_key());
}
