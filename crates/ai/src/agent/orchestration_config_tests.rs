use super::*;

fn make_config(model: &str, harness: &str) -> OrchestrationConfig {
    OrchestrationConfig {
        model_id: model.to_string(),
        harness_type: harness.to_string(),
    }
}

#[test]
fn status_default_is_none() {
    assert_eq!(
        OrchestrationConfigStatus::default(),
        OrchestrationConfigStatus::None
    );
}

#[test]
fn status_predicates() {
    assert!(OrchestrationConfigStatus::Approved.is_approved());
    assert!(!OrchestrationConfigStatus::Approved.is_disapproved());
    assert!(OrchestrationConfigStatus::Disapproved.is_disapproved());
    assert!(!OrchestrationConfigStatus::None.is_approved());
}

#[test]
fn proto_round_trip_config_local() {
    let config = make_config("auto", "oz");
    let proto = config.to_proto();
    let round_tripped = OrchestrationConfig::from_proto(&proto);
    assert_eq!(config, round_tripped);
}

#[test]
fn persisted_remote_config_reads_as_local() {
    let proto = api::OrchestrationConfig {
        model_id: "auto".to_string(),
        harness: harness_type_to_proto("claude"),
        execution_mode: Some(api::orchestration_config::ExecutionMode::Remote(
            api::orchestration_config::Remote {
                environment_id: "env-1".to_string(),
                worker_host: "warp".to_string(),
                runner_id: String::new(),
            },
        )),
    };
    let config = OrchestrationConfig::from_proto(&proto);
    assert_eq!(config, make_config("auto", "claude"));
    assert!(matches!(
        config.to_proto().execution_mode,
        Some(api::orchestration_config::ExecutionMode::Local(_))
    ));
}

#[test]
fn proto_round_trip_status() {
    for status in [
        OrchestrationConfigStatus::None,
        OrchestrationConfigStatus::Approved,
        OrchestrationConfigStatus::Disapproved,
    ] {
        let proto = status.to_proto();
        let round_tripped = OrchestrationConfigStatus::from_proto(proto.as_ref());
        assert_eq!(status, round_tripped);
    }
}
