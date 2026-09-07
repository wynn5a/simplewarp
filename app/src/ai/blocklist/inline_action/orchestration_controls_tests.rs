use warpui::App;

use super::runner_controls_enabled;
use crate::{GlobalResourceHandles, GlobalResourceHandlesProvider};

fn initialize_app(app: &mut App) {
    app.update(crate::settings::init_and_register_user_preferences);

    let global_resources = GlobalResourceHandles::mock(app);
    app.add_singleton_model(|_| GlobalResourceHandlesProvider::new(global_resources));
}

#[test]
fn runner_controls_stay_disabled() {
    // The controls needed a server-assigned experiment arm on top of the flag,
    // and this build has no server to assign one. The flag itself is gone now,
    // so there is only the one surviving behavior to pin.
    App::test((), |mut app| async move {
        initialize_app(&mut app);

        app.read(|ctx| assert!(!runner_controls_enabled(ctx)));
    });
}
