mod config;
mod state;

use std::fmt;

pub use config::*;
pub use state::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Channel {
    /// The open-source build of Warp.
    Oss,

    /// The integration test build.
    Integration,
}

impl fmt::Display for Channel {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(match self {
            Channel::Integration => "integration",
            Channel::Oss => "warp-oss",
        })
    }
}
