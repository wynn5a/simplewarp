// Only the cloud-preferences syncer tests use this, and those are gated off in a build with
// no Warp account to sync with.
#[cfg(all(test, not(feature = "local_only")))]
pub mod fake_object_client;
pub mod listener;
#[cfg(test)]
pub mod test_utils;
pub mod update_manager;
