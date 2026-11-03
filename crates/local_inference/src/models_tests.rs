use super::*;

#[test]
fn an_anthropic_list_keeps_the_display_name() {
    let body = r#"{"data":[
        {"id":"claude-sonnet-4-5-20250929","display_name":"Claude Sonnet 4.5","type":"model"},
        {"id":"claude-opus-4-1-20250805","display_name":"Claude Opus 4.1","type":"model"}
    ]}"#;

    let models = parse_models(Provider::Anthropic, body).expect("expected a list");
    assert_eq!(models.len(), 2);
    assert_eq!(models[0].id, "claude-sonnet-4-5-20250929");
    assert_eq!(models[0].display_name, "Claude Sonnet 4.5");
    assert_eq!(models[0].provider, Provider::Anthropic);
}

#[test]
fn an_openai_list_falls_back_to_the_slug_for_a_label() {
    // OpenAI returns no human-readable name.
    let body = r#"{"data":[{"id":"gpt-5","object":"model","owned_by":"openai"}]}"#;

    let models = parse_models(Provider::OpenAI, body).expect("expected a list");
    assert_eq!(models[0].id, "gpt-5");
    assert_eq!(models[0].display_name, "gpt-5");
}

#[test]
fn google_ids_lose_their_models_prefix() {
    // The slug sent to the inference endpoint must not carry the `models/` prefix that the
    // listing returns, or every request 404s.
    let body = r#"{"data":[{"id":"models/gemini-2.5-pro","object":"model"}]}"#;

    let models = parse_models(Provider::Google, body).expect("expected a list");
    assert_eq!(models[0].id, "gemini-2.5-pro");
}

#[test]
fn an_entry_with_no_id_is_skipped_rather_than_breaking_the_list() {
    let body = r#"{"data":[{"object":"model"},{"id":"gpt-5"}]}"#;

    let models = parse_models(Provider::OpenAI, body).expect("expected a list");
    assert_eq!(models.len(), 1);
    assert_eq!(models[0].id, "gpt-5");
}

#[test]
fn a_body_with_no_data_array_is_an_empty_list_not_an_error() {
    let models = parse_models(Provider::OpenAI, r#"{"error":"nope"}"#).expect("expected a list");
    assert!(models.is_empty());
}

#[test]
fn a_broken_body_is_an_error() {
    assert!(parse_models(Provider::OpenAI, "not json").is_err());
}

#[test]
fn each_provider_speaks_the_schema_its_endpoint_expects() {
    assert_eq!(Provider::Anthropic.schema(), Schema::AnthropicMessages);
    for provider in [Provider::OpenAI, Provider::Google] {
        assert_eq!(provider.schema(), Schema::OpenaiChatCompletions);
    }
}

#[tokio::test]
async fn an_empty_key_lists_nothing_without_a_request() {
    // No key means no provider to ask, so this must not reach the network.
    let models = list_models(Provider::Anthropic, "")
        .await
        .expect("expected an empty list");
    assert!(models.is_empty());
}

#[test]
fn an_anthropic_list_reports_each_model_window() {
    let body = r#"{"data":[
        {"id":"claude-opus-5-5","display_name":"Claude Opus 5.5","max_input_tokens":1000000},
        {"id":"claude-sonnet-4-5-20250929","display_name":"Claude Sonnet 4.5"}
    ]}"#;

    let models = parse_models(Provider::Anthropic, body).expect("expected a list");
    assert_eq!(models[0].context_window, Some(1_000_000));
    // No window in the listing falls back to the table.
    assert_eq!(models[1].context_window, Some(200_000));
}

#[test]
fn a_listed_window_beyond_the_plain_request_window_is_capped() {
    // Sonnet 4.5 lists 1M, but only a beta header this crate does not send unlocks it.
    let body = r#"{"data":[
        {"id":"claude-sonnet-4-5-20250929","max_input_tokens":1000000},
        {"id":"claude-opus-5-5","max_input_tokens":800000},
        {"id":"claude-new-family","max_input_tokens":300000},
        {"id":"unknown-model","max_input_tokens":64000}
    ]}"#;

    let models = parse_models(Provider::Anthropic, body).expect("expected a list");
    assert_eq!(models[0].context_window, Some(200_000));
    assert_eq!(models[1].context_window, Some(800_000));
    assert_eq!(models[2].context_window, Some(200_000));
    assert_eq!(models[3].context_window, Some(64_000));
}

#[test]
fn gpt_5_chat_snapshots_take_128k() {
    for id in ["gpt-5-chat-latest", "gpt-5.1-chat-latest", "gpt-5.2-chat"] {
        assert_eq!(context_window(id), Some(128_000), "{id}");
    }
    assert_eq!(context_window("gpt-5.1"), Some(272_000));
}

#[test]
fn gpt_4_1_takes_its_exact_window() {
    assert_eq!(context_window("gpt-4.1-mini"), Some(1_047_576));
}

#[test]
fn a_listing_without_windows_uses_the_table() {
    let body = r#"{"data":[{"id":"gpt-5.4"},{"id":"some-new-model"}]}"#;

    let models = parse_models(Provider::OpenAI, body).expect("expected a list");
    assert_eq!(models[0].context_window, Some(272_000));
    assert_eq!(models[1].context_window, None);
}

#[test]
fn the_window_table_covers_the_built_in_models() {
    assert_eq!(context_window("claude-sonnet-4-5"), Some(200_000));
    assert_eq!(context_window("gpt-5.4"), Some(272_000));
    assert_eq!(context_window("gemini-2.5-pro"), Some(1_048_576));
}

#[test]
fn dated_snapshots_take_their_family_window() {
    assert_eq!(context_window("claude-haiku-4-5-20251001"), Some(200_000));
    assert_eq!(context_window("claude-opus-4-1-20250805"), Some(200_000));
    assert_eq!(context_window("claude-3-5-haiku-20241022"), Some(200_000));
    assert_eq!(context_window("gpt-4o-2024-08-06"), Some(128_000));
}

#[test]
fn newer_claude_generations_have_a_million_tokens() {
    for id in [
        "claude-opus-4-6",
        "claude-sonnet-4-6",
        "claude-opus-5-5",
        "claude-sonnet-5-5",
        "claude-haiku-5-5",
        "claude-fable-5-1",
    ] {
        assert_eq!(context_window(id), Some(1_000_000), "{id}");
    }
}

#[test]
fn an_unknown_slug_has_no_window() {
    assert_eq!(context_window("llama-3.1-70b"), None);
    assert_eq!(context_window(""), None);
}
