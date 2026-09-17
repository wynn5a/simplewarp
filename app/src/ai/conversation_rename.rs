use warpui::{SingletonEntity, View, ViewContext};

use crate::ai::agent::conversation::AIConversationId;
use crate::ai::blocklist::BlocklistAIHistoryModel;
use crate::view_components::DismissibleToast;
use crate::workspace::ToastStack;

const CONVERSATION_TITLE_MAX_CHARS: usize = 500;

const EMPTY_TITLE_MESSAGE: &str = "Please provide a conversation title";
const EMPTY_CONVERSATION_MESSAGE: &str = "You can't rename an empty conversation";

/// Renames a conversation locally.
///
/// Renaming is only exposed for open conversations, so the conversation is expected
/// to already be loaded in the history model.
pub(crate) fn rename_conversation<T: View>(
    conversation_id: AIConversationId,
    title: String,
    ctx: &mut ViewContext<T>,
) {
    let title = match validate_conversation_title(title) {
        Ok(title) => title,
        Err(message) => {
            let window_id = ctx.window_id();
            ToastStack::handle(ctx).update(ctx, |toast_stack, ctx| {
                toast_stack.add_ephemeral_toast(DismissibleToast::error(message), window_id, ctx);
            });
            return;
        }
    };
    if BlocklistAIHistoryModel::as_ref(ctx)
        .conversation(&conversation_id)
        .is_some_and(|conversation| conversation.is_empty())
    {
        let window_id = ctx.window_id();
        ToastStack::handle(ctx).update(ctx, |toast_stack, ctx| {
            toast_stack.add_ephemeral_toast(
                DismissibleToast::error(EMPTY_CONVERSATION_MESSAGE.to_owned()),
                window_id,
                ctx,
            );
        });
        return;
    }
    if conversation_already_has_title(conversation_id, &title, ctx) {
        return;
    }

    let renamed = BlocklistAIHistoryModel::handle(ctx).update(ctx, |history, ctx| {
        history.rename_conversation_locally(conversation_id, title, ctx)
    });
    let window_id = ctx.window_id();
    match renamed {
        Ok(()) => {
            ToastStack::handle(ctx).update(ctx, |toast_stack, ctx| {
                toast_stack.add_ephemeral_toast(
                    DismissibleToast::success("Conversation renamed".to_owned()),
                    window_id,
                    ctx,
                );
            });
        }
        Err(message) => {
            ToastStack::handle(ctx).update(ctx, |toast_stack, ctx| {
                toast_stack.add_ephemeral_toast(DismissibleToast::error(message), window_id, ctx);
            });
        }
    }
}

/// Returns whether the conversation's current local title already matches `title`,
/// making the rename a no-op.
fn conversation_already_has_title<T: View>(
    conversation_id: AIConversationId,
    title: &str,
    ctx: &ViewContext<T>,
) -> bool {
    BlocklistAIHistoryModel::as_ref(ctx)
        .conversation(&conversation_id)
        .and_then(|conversation| conversation.title())
        .is_some_and(|current_title| current_title == title)
}

/// Trims and validates a requested conversation title, returning a user-facing
/// error message when the title is invalid.
fn validate_conversation_title(title: String) -> Result<String, String> {
    let title = title.trim();
    if title.is_empty() {
        return Err(EMPTY_TITLE_MESSAGE.to_owned());
    }

    if title.chars().count() > CONVERSATION_TITLE_MAX_CHARS {
        return Err(format!(
            "Conversation title must be {CONVERSATION_TITLE_MAX_CHARS} characters or fewer",
        ));
    }

    Ok(title.to_owned())
}
