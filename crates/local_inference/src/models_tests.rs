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
