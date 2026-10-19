use registry::register_uri_handler;
use warpui::AppContext;

mod registry;

pub(super) fn init(_ctx: &mut AppContext) {
    register_uri_handler();
}
