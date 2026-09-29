use serde::Serialize;
use warpui::Element;
use warpui::elements::MouseStateHandle;
use warpui::notification::RequestPermissionsOutcome;

use super::{
    InlineBannerButtonState, InlineBannerCloseButton, InlineBannerContent, InlineBannerStyle,
    InlineBannerTextButton, InlineBannerTextButtonVariant, render_inline_block_list_banner,
};
use crate::appearance::Appearance;
use crate::terminal::session_settings::NotificationsMode;
use crate::terminal::view::{InlineBannerId, NotificationsTrigger, TerminalAction};

#[derive(Clone, Copy, Debug, Serialize)]
pub enum NotificationsDiscoveryBannerAction {
    TurnOn(NotificationsTrigger),
    Configure,
    Close,
}

#[derive(Default)]
pub struct NotificationsDiscoveryBannerMouseStates {
    pub turn_on: MouseStateHandle,
    pub configure: MouseStateHandle,
    pub close: MouseStateHandle,
}

/// State necessary to render the (singleton) notifications discovery banner.
pub struct NotificationsDiscoveryBannerState {
    pub banner_id: InlineBannerId,
    pub mouse_states: NotificationsDiscoveryBannerMouseStates,
}

pub fn render_inline_notifications_discovery_banner(
    trigger: NotificationsTrigger,
    request_outcome: Option<RequestPermissionsOutcome>,
    state: &NotificationsDiscoveryBannerState,
    notifications_mode: NotificationsMode,
    appearance: &Appearance,
) -> Box<dyn Element> {
    let active_ui_text_color = appearance.theme().active_ui_text_color().into_solid();

    let (title, buttons) = match notifications_mode {
        NotificationsMode::Dismissed => (
            "We won't show this banner again, but you can always go to Settings to enable notifications.",
            vec![],
        ),
        NotificationsMode::Disabled => (
            "Notifications were turned off, but you can always go to Settings to enable notifications.",
            vec![],
        ),
        NotificationsMode::Unset => (
            trigger.discovery_banner_copy(),
            vec![InlineBannerTextButton {
                text: "Enable".to_string(),
                text_color: active_ui_text_color,
                button_state: InlineBannerButtonState {
                    on_click_event: TerminalAction::NotificationsDiscoveryBanner(
                        NotificationsDiscoveryBannerAction::TurnOn(trigger),
                    ),
                    mouse_state_handle: state.mouse_states.turn_on.clone(),
                },
                font: Default::default(),
                position_id: None,
                variant: InlineBannerTextButtonVariant::Primary,
            }],
        ),
        NotificationsMode::Enabled => {
            // Determine the messaging based on what the user's response was to the
            // permissions request (if any)
            let title = match request_outcome {
                Some(RequestPermissionsOutcome::Accepted) => {
                    "Success! You are now ready to receive desktop notifications."
                }
                Some(RequestPermissionsOutcome::PermissionsDenied) => {
                    "Warp was denied permissions to send you notifications."
                }
                Some(RequestPermissionsOutcome::OtherError { .. }) => {
                    "Something went wrong while requesting permissions."
                }
                None => {
                    "Don't forget to 'Allow' the permissions request to finish setting up notifications."
                }
            };

            (
                title,
                vec![InlineBannerTextButton {
                    text: "Configure notifications".to_string(),
                    text_color: active_ui_text_color,
                    button_state: InlineBannerButtonState {
                        on_click_event: TerminalAction::NotificationsDiscoveryBanner(
                            NotificationsDiscoveryBannerAction::Configure,
                        ),
                        mouse_state_handle: state.mouse_states.configure.clone(),
                    },
                    font: Default::default(),
                    position_id: None,
                    variant: InlineBannerTextButtonVariant::Secondary,
                }],
            )
        }
    };

    let close_button = InlineBannerCloseButton(InlineBannerButtonState {
        on_click_event: TerminalAction::NotificationsDiscoveryBanner(
            NotificationsDiscoveryBannerAction::Close,
        ),
        mouse_state_handle: state.mouse_states.close.clone(),
    });

    render_inline_block_list_banner(
        InlineBannerStyle::CallToAction,
        appearance,
        InlineBannerContent {
            title: title.to_owned(),
            buttons,
            close_button: Some(close_button),
            ..Default::default()
        },
    )
}
