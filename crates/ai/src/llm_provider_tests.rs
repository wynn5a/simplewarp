use super::LLMProvider;

#[test]
fn every_api_key_provider_has_a_key_slot() {
    let mut keys = crate::api_keys::ApiKeys::default();
    for provider in LLMProvider::API_KEY_PROVIDERS {
        assert!(provider.set_api_key(&mut keys, Some("k".into())));
        assert_eq!(provider.api_key(&keys), Some("k"));
    }
}
