//! Entity submodel that encapsulates all filesystem access for diff application.
//!
//! The executor holds a [`ModelHandle<ApplyDiffModel>`] and calls
//! [`ApplyDiffModel::apply_diffs`], which resolves the session context and calls [`apply_edits`]
//! with a `std::fs`-backed closure. Remote sessions are rejected.

use ai::diff_validation::AIRequestedCodeDiff;
use futures::FutureExt;
use vec1::Vec1;
use warpui::r#async::BoxFuture;
use warpui::{Entity, ModelContext, ModelHandle, SingletonEntity as _};

use super::diff_application::{DiffApplicationError, FileReadResult, apply_edits};
use crate::ai::agent::{AIIdentifiers, FileEdit};
use crate::ai::blocklist::SessionContext;
use crate::auth::AuthStateProvider;
use crate::terminal::model::session::active_session::ActiveSession;

/// Entity submodel that encapsulates filesystem access for diff application.
///
/// Held as a [`ModelHandle`] by the [`super::RequestFileEditsExecutor`].
pub(crate) struct ApplyDiffModel {
    active_session: ModelHandle<ActiveSession>,
}

impl Entity for ApplyDiffModel {
    type Event = ();
}

impl ApplyDiffModel {
    pub fn new(active_session: ModelHandle<ActiveSession>) -> Self {
        Self { active_session }
    }

    /// Resolves session context from the model context, then returns a future that applies the
    /// edits locally.
    pub fn apply_diffs(
        &self,
        edits: Vec<FileEdit>,
        ai_identifiers: &AIIdentifiers,
        passive_diff: bool,
        ctx: &mut ModelContext<Self>,
    ) -> BoxFuture<'static, Result<Vec<AIRequestedCodeDiff>, Vec1<DiffApplicationError>>> {
        let session_context = SessionContext::from_session(self.active_session.as_ref(ctx), ctx);
        let background_executor = ctx.background_executor();
        let auth_state = AuthStateProvider::as_ref(ctx).get().clone();
        let ai_identifiers = ai_identifiers.clone();

        let is_remote = session_context.is_remote();
        let fut = async move {
            if is_remote {
                Err(vec1::vec1![
                    DiffApplicationError::RemoteFileOperationsUnsupported
                ])
            } else {
                apply_edits(
                    edits,
                    &session_context,
                    &ai_identifiers,
                    background_executor,
                    auth_state,
                    passive_diff,
                    |path| async move { FileReadResult::from(std::fs::read_to_string(path)) },
                )
                .await
            }
        };
        cfg_if::cfg_if! {
            if #[cfg(target_family = "wasm")] {
                fut.boxed_local()
            } else {
                fut.boxed()
            }
        }
    }
}
