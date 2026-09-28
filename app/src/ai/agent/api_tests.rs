use super::ServerConversationToken;
use crate::ai::agent::ServerOutputId;

#[test]
fn debugging_payload_is_the_conversation_id() {
    let token = ServerConversationToken::new("conversation-token".to_owned());
    let request_id = ServerOutputId::new("request-id".to_owned());

    assert_eq!(
        token.debugging_payload(None),
        "{\"conversation_id\":\"conversation-token\"}"
    );
    assert_eq!(
        token.debugging_payload(Some(&request_id)),
        "{\"request_id\":\"request-id\",\"conversation_id\":\"conversation-token\"}"
    );
}
