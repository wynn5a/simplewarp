use warp_multi_agent_api as api;

use super::*;

fn settings_with(model: &str, keys: api::request::settings::ApiKeys) -> api::request::Settings {
    api::request::Settings {
        model_config: Some(api::request::settings::ModelConfig {
            base: model.to_string(),
            ..Default::default()
        }),
        api_keys: Some(keys),
        ..Default::default()
    }
}

fn request_with(settings: api::request::Settings) -> api::Request {
    api::Request {
        settings: Some(settings),
        ..Default::default()
    }
}

#[test]
fn claude_slug_resolves_to_anthropic() {
    let request = request_with(settings_with(
        "claude-sonnet-4-20250514",
        api::request::settings::ApiKeys {
            anthropic: "sk-ant-test".to_string(),
            ..Default::default()
        },
    ));

    let target = resolve_target(&request).expect("expected a target");
    assert_eq!(target.schema, Schema::AnthropicMessages);
    assert_eq!(target.base_url, "https://api.anthropic.com/v1");
    assert_eq!(target.api_key, "sk-ant-test");
    assert_eq!(
        target.endpoint_url(),
        "https://api.anthropic.com/v1/messages"
    );
}

#[test]
fn gpt_slug_resolves_to_openai() {
    let request = request_with(settings_with(
        "gpt-5",
        api::request::settings::ApiKeys {
            openai: "sk-openai-test".to_string(),
            ..Default::default()
        },
    ));

    let target = resolve_target(&request).expect("expected a target");
    assert_eq!(target.schema, Schema::OpenaiChatCompletions);
    assert_eq!(
        target.endpoint_url(),
        "https://api.openai.com/v1/chat/completions"
    );
}

#[test]
fn a_slug_with_a_slash_points_the_user_at_a_custom_endpoint() {
    // `anthropic/claude-sonnet-4` is an OpenRouter-style slug. This crate has no base URL for
    // an aggregator, and picking one would send the key to a host the user never named.
    let request = request_with(settings_with(
        "anthropic/claude-sonnet-4",
        api::request::settings::ApiKeys {
            // An Anthropic key is set to prove the slash is not read as an Anthropic model.
            anthropic: "sk-ant-test".to_string(),
            ..Default::default()
        },
    ));

    let error = resolve_target(&request).expect_err("expected an error");
    assert!(
        matches!(error, Error::AggregatorModel { .. }),
        "got {error:?}"
    );
}

#[test]
fn an_aggregator_slug_still_works_as_a_custom_endpoint() {
    use api::request::settings::custom_model_providers::{
        CustomEndpointSchema, CustomModel, CustomModelProvider,
    };

    let mut settings = settings_with(
        "openrouter-base",
        api::request::settings::ApiKeys::default(),
    );
    settings.custom_model_providers = Some(api::request::settings::CustomModelProviders {
        providers: vec![CustomModelProvider {
            base_url: "https://openrouter.ai/api/v1".to_string(),
            api_key: "sk-or-test".to_string(),
            models: vec![CustomModel {
                slug: "anthropic/claude-sonnet-4".to_string(),
                config_key: "openrouter-base".to_string(),
            }],
            schema: CustomEndpointSchema::OpenaiChatCompletions as i32,
        }],
    });

    let target = resolve_target(&request_with(settings)).expect("expected a target");
    assert_eq!(target.model, "anthropic/claude-sonnet-4");
    assert_eq!(
        target.endpoint_url(),
        "https://openrouter.ai/api/v1/chat/completions"
    );
}

#[test]
fn a_missing_key_is_an_error() {
    let request = request_with(settings_with(
        "claude-sonnet-4-20250514",
        api::request::settings::ApiKeys::default(),
    ));

    let error = resolve_target(&request).expect_err("expected a missing-key error");
    assert!(matches!(error, Error::NoApiKey { .. }), "got {error:?}");
}

#[test]
fn a_custom_endpoint_wins_over_a_builtin_provider() {
    use api::request::settings::custom_model_providers::{
        CustomEndpointSchema, CustomModel, CustomModelProvider,
    };

    let mut settings = settings_with(
        "local-base",
        api::request::settings::ApiKeys {
            anthropic: "sk-ant-test".to_string(),
            ..Default::default()
        },
    );
    settings.custom_model_providers = Some(api::request::settings::CustomModelProviders {
        providers: vec![CustomModelProvider {
            // The trailing slash must not survive into the endpoint URL.
            base_url: "http://localhost:11434/v1/".to_string(),
            api_key: "not-needed".to_string(),
            models: vec![CustomModel {
                slug: "qwen3-coder".to_string(),
                config_key: "local-base".to_string(),
            }],
            schema: CustomEndpointSchema::OpenaiChatCompletions as i32,
        }],
    });

    let target = resolve_target(&request_with(settings)).expect("expected a target");
    assert_eq!(target.schema, Schema::OpenaiChatCompletions);
    assert_eq!(target.model, "qwen3-coder");
    assert_eq!(
        target.endpoint_url(),
        "http://localhost:11434/v1/chat/completions"
    );
}

#[test]
fn a_custom_endpoint_can_speak_anthropic() {
    use api::request::settings::custom_model_providers::{
        CustomEndpointSchema, CustomModel, CustomModelProvider,
    };

    let mut settings = settings_with("proxy-base", api::request::settings::ApiKeys::default());
    settings.custom_model_providers = Some(api::request::settings::CustomModelProviders {
        providers: vec![CustomModelProvider {
            base_url: "https://proxy.example.com/v1".to_string(),
            api_key: "sk-proxy".to_string(),
            models: vec![CustomModel {
                slug: "claude-sonnet-4".to_string(),
                config_key: "proxy-base".to_string(),
            }],
            schema: CustomEndpointSchema::AnthropicMessages as i32,
        }],
    });

    let target = resolve_target(&request_with(settings)).expect("expected a target");
    assert_eq!(target.schema, Schema::AnthropicMessages);
    assert_eq!(target.api_key, "sk-proxy");
}

#[test]
fn the_second_custom_provider_is_still_searched() {
    use api::request::settings::custom_model_providers::{
        CustomEndpointSchema, CustomModel, CustomModelProvider,
    };

    let provider = |base_url: &str, config_key: &str| CustomModelProvider {
        base_url: base_url.to_string(),
        api_key: "key".to_string(),
        models: vec![CustomModel {
            slug: "slug".to_string(),
            config_key: config_key.to_string(),
        }],
        schema: CustomEndpointSchema::OpenaiChatCompletions as i32,
    };

    let mut settings = settings_with("second", api::request::settings::ApiKeys::default());
    settings.custom_model_providers = Some(api::request::settings::CustomModelProviders {
        providers: vec![
            provider("https://first.example.com", "first"),
            provider("https://second.example.com", "second"),
        ],
    });

    let target = resolve_target(&request_with(settings)).expect("expected a target");
    assert_eq!(target.base_url, "https://second.example.com");
}

#[test]
fn a_warp_router_id_reports_that_nothing_is_configured() {
    // `auto` was Warp's server-side router. It names no real model, so a missing-key error
    // would point the user at a model they never chose.
    for router in ["auto", "cli-agent-auto", "computer-use-agent-auto"] {
        let request = request_with(settings_with(
            router,
            api::request::settings::ApiKeys {
                anthropic: "sk-ant-test".to_string(),
                ..Default::default()
            },
        ));

        let error = resolve_target(&request).expect_err("expected an error");
        assert!(
            matches!(error, Error::NoModelConfigured),
            "{router} gave {error:?}"
        );
    }
}

#[test]
fn a_custom_endpoint_named_auto_still_wins() {
    use api::request::settings::custom_model_providers::{
        CustomEndpointSchema, CustomModel, CustomModelProvider,
    };

    // The router check must not shadow a real endpoint the user configured under that key.
    let mut settings = settings_with("auto", api::request::settings::ApiKeys::default());
    settings.custom_model_providers = Some(api::request::settings::CustomModelProviders {
        providers: vec![CustomModelProvider {
            base_url: "http://localhost:11434/v1".to_string(),
            api_key: "key".to_string(),
            models: vec![CustomModel {
                slug: "qwen3-coder".to_string(),
                config_key: "auto".to_string(),
            }],
            schema: CustomEndpointSchema::OpenaiChatCompletions as i32,
        }],
    });

    let target = resolve_target(&request_with(settings)).expect("expected a target");
    assert_eq!(target.model, "qwen3-coder");
}
