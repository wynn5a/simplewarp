use settings::Setting as _;
use warpui::App;

use super::PrivacySettings;
use crate::test_util::settings::initialize_settings_for_tests;

#[test]
fn default_regexes_are_added_only_once() {
    App::test((), |mut app| async move {
        initialize_settings_for_tests(&mut app);
        let privacy = app.add_singleton_model(PrivacySettings::mock);

        privacy.update(&mut app, |settings, ctx| {
            settings.initialize_default_regexes_once(ctx);
        });
        let initial_count = privacy.read(&app, |settings, _| settings.user_secret_regex_list.len());
        assert!(initial_count > 0, "recommended regexes should be added");
        assert!(privacy.read(&app, |settings, _| {
            *settings.has_initialized_default_secret_regexes.value()
        }));

        // A regex the user removed must not come back on the next launch.
        privacy.update(&mut app, |settings, ctx| {
            settings.remove_user_secret_regex(&0, ctx);
            settings.initialize_default_regexes_once(ctx);
        });
        let count_after_removal =
            privacy.read(&app, |settings, _| settings.user_secret_regex_list.len());
        assert_eq!(count_after_removal, initial_count - 1);
    });
}
