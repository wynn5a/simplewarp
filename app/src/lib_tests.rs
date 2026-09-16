use super::*;

#[test]
fn app_keeps_default_secure_storage_service_name() {
    let launch_mode = LaunchMode::App {
        args: Default::default(),
    };

    assert_eq!(
        launch_mode.secure_storage_service_name("dev.warp.Warp-Dev"),
        "dev.warp.Warp-Dev"
    );
}

#[test]
fn launch_modes_select_expected_logging_frontend() {
    let app = LaunchMode::App {
        args: Default::default(),
    };
    let test = LaunchMode::Test {
        driver: Box::new(None),
        is_integration_test: false,
    };

    assert_eq!(app.log_frontend(), LogFrontend::Gui);
    assert_eq!(test.log_frontend(), LogFrontend::Gui);
    assert_eq!(
        LaunchMode::RemoteServerProxy.log_frontend(),
        LogFrontend::Cli
    );
    assert_eq!(
        LaunchMode::RemoteServerDaemon {
            identity_key: "test".to_owned(),
        }
        .log_frontend(),
        LogFrontend::Cli
    );
}
