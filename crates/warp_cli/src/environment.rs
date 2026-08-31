//! Shared `--environment` arguments.
//!
//! The `warp environment` subcommand and its handler are gone with cloud environments; these
//! two argument groups stay because `warp agent` and `warp schedule` still accept an
//! environment id on the command line.

use clap::Args;

/// Common arguments for selecting an environment when creating an object.
#[derive(Args, Clone, Debug)]
#[group(required = false, multiple = false)]
pub struct EnvironmentCreateArgs {
    /// Cloud environment to run the agent in.
    #[arg(long = "environment", value_name = "ENVIRONMENT_ID", short = 'e')]
    pub environment: Option<String>,

    /// Do not run the agent in an environment (not recommended).
    #[arg(long = "no-environment")]
    pub no_environment: bool,
}

/// Common arguments for selecting an environment when updating an object.
#[derive(Args, Clone, Debug)]
#[group(required = false, multiple = false)]
pub struct EnvironmentUpdateArgs {
    /// Cloud environment to run the agent in.
    #[arg(long = "environment", value_name = "ENVIRONMENT_ID", short = 'e')]
    pub environment: Option<String>,

    /// Do not run the agent in an environment (not recommended).
    #[arg(long = "remove-environment")]
    pub remove_environment: bool,
}
