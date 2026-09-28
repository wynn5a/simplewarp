use std::{env, fs};

/// Detect whether or not we are running in a Namespace instance.
pub fn is_in_namespace_instance() -> bool {
    // For Namespace, match their CLI's logic for detecting a token:
    // https://github.com/namespacelabs/integrations/blob/08d0acd17ce05f8486ec8da329066dd6a12572a0/auth/token.go#L116-L131
    env::var("NSC_TOKEN_FILE").is_ok() || fs::exists("/var/run/nsc/token.json").is_ok_and(|v| v)
}
