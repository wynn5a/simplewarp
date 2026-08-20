use warpui::App;

use super::runner_controls_enabled;
use crate::features::FeatureFlag;
use crate::{GlobalResourceHandles, GlobalResourceHandlesProvider};

fn initialize_app(app: &mut App) {
    app.update(crate::settings::init_and_register_user_preferences);

    let global_resources = GlobalResourceHandles::mock(app);
    app.add_singleton_model(|_| GlobalResourceHandlesProvider::new(global_resources));
}

#[test]
fn runner_controls_stay_disabled_whatever_the_feature_flag_says() {
    // The controls needed a server-assigned experiment arm on top of the flag,
    // and this build has no server to assign one. Pinning both flag states
    // guards against a later change that drops only the experiment half and
    // silently turns the cloud runner controls on.
    App::test((), |mut app| async move {
        initialize_app(&mut app);

        for enabled in [false, true] {
            let _cloud_agent_runners = FeatureFlag::CloudAgentRunners.override_enabled(enabled);
            app.read(|ctx| assert!(!runner_controls_enabled(ctx)));
        }
    });
}
