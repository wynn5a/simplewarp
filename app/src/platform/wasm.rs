pub use warp_web_event_bus::{WarpEvent, emit_event};

/// This function should be called early in application initialization to ensure that
/// static variables are initialized.
pub(super) fn init() {
    unsafe {
        unsafe extern "C" {
            /// __wasm_call_ctors is a function defined by the `wasm-ld` linker, and is used to
            /// initialize static variables.
            ///
            /// It should be called once at runtime before other code is executed.
            fn __wasm_call_ctors();
        }

        __wasm_call_ctors();
    }
}
