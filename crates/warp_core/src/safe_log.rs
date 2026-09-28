//! Loggers for messages that may contain sensitive info.
//!
//! Each macro takes two messages, labeled `safe:` and `full:`. Only the safe one is emitted; the
//! full one was for Warp's internal dogfood channels, which this build does not have.

/// Safe Logger for sensitive info messages.
#[macro_export]
macro_rules! safe_info {
    (safe: ($($safe_arg:tt)+), full: ($($full_arg:tt)+)) => {{
        $crate::__discard_full_log_args!($($full_arg)+);
        log::info!($($safe_arg)+)
    }};
}

/// Safe Logger for sensitive warning messages.
#[macro_export]
macro_rules! safe_warn {
    (safe: ($($safe_arg:tt)+), full: ($($full_arg:tt)+)) => {{
        $crate::__discard_full_log_args!($($full_arg)+);
        log::warn!($($safe_arg)+)
    }};
}

/// Safe Logger for sensitive error messages.
#[macro_export]
macro_rules! safe_error {
    (safe: ($($safe_arg:tt)+), full: ($($full_arg:tt)+)) => {{
        $crate::__discard_full_log_args!($($full_arg)+);
        log::error!($($safe_arg)+)
    }};
}

/// Safe Logger for sensitive debug messages. Debug messages are generally not
/// logged at all in release channels, but could be enabled if a user sets
/// the `RUST_LOG` environment variable.
#[macro_export]
macro_rules! safe_debug {
    (safe: ($($safe_arg:tt)+), full: ($($full_arg:tt)+)) => {{
        $crate::__discard_full_log_args!($($full_arg)+);
        log::debug!($($safe_arg)+)
    }};
}

/// Safe `anyhow::Error` builder for sensitive error messages.
#[macro_export]
macro_rules! safe_anyhow {
    (safe: ($($safe_arg:tt)+), full: ($($full_arg:tt)+)) => {{
        $crate::__discard_full_log_args!($($full_arg)+);
        anyhow::anyhow!($($safe_arg)+)
    }};
}

/// Safe `eprint!` for sensitive error messages.
///
/// The safe message will only be printed if it is not empty.
/// This macro is mostly useful for the SDK, where access to the debug log is limited.
#[macro_export]
macro_rules! safe_eprintln {
    (safe: ($($safe_arg:tt)*), full: ($($full_arg:tt)+)) => {{
        $crate::__discard_full_log_args!($($full_arg)+);
        if !stringify!($($safe_arg)*).trim().is_empty() {
            eprintln!($($safe_arg)*)
        }
    }};
}

/// Type-checks and borrows the `full:` arguments without formatting them, so values referenced
/// only there don't trip `unused_variables`.
#[doc(hidden)]
#[macro_export]
macro_rules! __discard_full_log_args {
    ($($full_arg:tt)+) => {
        let _ = || format!($($full_arg)+);
    };
}
