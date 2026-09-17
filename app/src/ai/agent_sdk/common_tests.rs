use super::classify_agent_mode_base_model_id;
use crate::ai::llms::LLMId;

#[test]
fn classify_returns_unknown_id_error_when_list_available_and_id_genuinely_invalid() {
    // A non-empty list that does not contain the id produces the
    // "Unknown model id" error (with suggestions).
    let valid_ids = vec![LLMId::from("auto"), LLMId::from("gpt-x")];
    let err = classify_agent_mode_base_model_id("claude-sonnet-4-5", &valid_ids)
        .expect_err("genuinely invalid id should error");
    let msg = format!("{err:#}");
    assert!(
        msg.contains("Unknown model id"),
        "should preserve the existing 'Unknown model id' error: {msg}"
    );
    assert!(
        msg.contains("auto") && msg.contains("gpt-x"),
        "should list the available model suggestions: {msg}"
    );
}

#[test]
fn classify_accepts_custom_endpoint_id_in_choices() {
    // A custom-endpoint (local) id that is among the choices validates, because
    // custom endpoints are independent of server health (the validator chains
    // custom choices alongside the server list).
    let valid_ids = vec![LLMId::from("custom-config-key")];
    let id = classify_agent_mode_base_model_id("custom-config-key", &valid_ids)
        .expect("an id present in the choices should validate");
    assert_eq!(id.as_str(), "custom-config-key");
}
