use super::*;

#[test]
fn an_anthropic_prompt_that_is_too_long_overflows() {
    let body = r#"{"type":"error","error":{"type":"invalid_request_error","message":"prompt is too long: 215032 tokens > 200000 maximum"},"request_id":"req_011CUz"}"#;
    assert!(is_overflow_response(400, body));
}

#[test]
fn an_openai_chat_completions_overflow_is_matched_by_its_code() {
    let body = r#"{"error":{"message":"This model's maximum context length is 128000 tokens. However, your messages resulted in 131072 tokens. Please reduce the length of the messages.","type":"invalid_request_error","param":"messages","code":"context_length_exceeded"}}"#;
    assert!(is_overflow_response(400, body));
}

#[test]
fn an_openai_responses_overflow_is_matched() {
    let body = r#"{"error":{"message":"Your input exceeds the context window of this model. Please adjust your input and try again.","type":"invalid_request_error","param":"input","code":"context_length_exceeded"}}"#;
    assert!(is_overflow_response(400, body));
}

#[test]
fn a_compatible_server_overflow_without_a_code_is_matched_by_its_wording() {
    let body = r#"{"object":"error","message":"This model's maximum context length is 32768 tokens. However, you requested 40100 tokens (36004 in the messages, 4096 in the completion). Please reduce the length of the messages or completion.","type":"BadRequestError","param":null,"code":400}"#;
    assert!(is_overflow_response(400, body));
}

#[test]
fn a_gemini_overflow_is_matched_in_an_array_body() {
    let body = r#"[{"error":{"code":400,"message":"The input token count (1196265) exceeds the maximum number of tokens allowed (1048575).","status":"INVALID_ARGUMENT"}}]"#;
    assert!(is_overflow_response(400, body));
}

#[test]
fn a_gemini_overflow_is_matched_in_an_object_body() {
    let body = r#"{"error":{"code":400,"message":"The input token count (1196265) exceeds the maximum number of tokens allowed (1048575).","status":"INVALID_ARGUMENT"}}"#;
    assert!(is_overflow_response(400, body));
}

#[test]
fn other_bad_requests_do_not_overflow() {
    for body in [
        r#"{"type":"error","error":{"type":"invalid_request_error","message":"max_tokens: 200000 > 128000, which is the maximum allowed number of output tokens for claude-opus-5-5"}}"#,
        r#"{"type":"error","error":{"type":"invalid_request_error","message":"messages.1.content.0.tool_result: unexpected `tool_use_id` found in `tool_result` blocks"}}"#,
        r#"{"error":{"message":"Invalid value for 'tool_choice'.","type":"invalid_request_error","param":"tool_choice","code":"invalid_value"}}"#,
        r#"{"error":{"message":"The model `gpt-9` does not exist or you do not have access to it.","type":"invalid_request_error","param":null,"code":"model_not_found"}}"#,
        r#"[{"error":{"code":400,"message":"API key not valid. Please pass a valid API key.","status":"INVALID_ARGUMENT"}}]"#,
        "not json, but says prompt is too long",
        "",
    ] {
        assert!(!is_overflow_response(400, body), "{body}");
    }
}

#[test]
fn only_a_400_overflows() {
    let body = r#"{"type":"error","error":{"type":"invalid_request_error","message":"prompt is too long: 215032 tokens > 200000 maximum"}}"#;
    for status in [401, 413, 429, 500, 529] {
        assert!(!is_overflow_response(status, body), "{status}");
    }
}

#[test]
fn an_in_stream_error_overflows_by_code_or_wording() {
    assert!(is_overflow_error(
        "context_length_exceeded",
        "Your input exceeds the context window of this model."
    ));
    assert!(is_overflow_error(
        "",
        "This model's maximum context length is 65536 tokens."
    ));
    assert!(!is_overflow_error(
        "server_error",
        "The server had an error."
    ));
    assert!(!is_overflow_error(
        "rate_limit_exceeded",
        "Rate limit reached."
    ));
}

#[test]
fn an_older_anthropic_input_and_max_tokens_overflow_is_matched() {
    let body = r#"{"type":"error","error":{"type":"invalid_request_error","message":"input length and `max_tokens` exceed context limit: 188240 + 21333 > 200000, decrease input length or `max_tokens` and try again"}}"#;
    assert!(is_overflow_response(400, body));
}
