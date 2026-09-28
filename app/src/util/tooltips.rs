//! Shared tooltip UI components for file path and link tooltips

use std::path::Path;

use warpui::elements::{
    Border, Container, CornerRadius, Flex, MouseStateHandle, ParentElement, Radius, Text,
};
use warpui::ui_components::components::{Coords, UiComponent, UiComponentStyles};
use warpui::{AppContext, Element, EventContext};

use crate::appearance::Appearance;

/// A link to be shown in a tooltip
pub struct TooltipLink<OnClick> {
    pub text: String,
    pub on_click: OnClick,
    /// Optional detail text to show after the link (e.g., "[Cmd Click]")
    pub detail: Option<String>,
    pub mouse_state: MouseStateHandle,
}

impl<OnClick> TooltipLink<OnClick> {
    pub fn new(text: String, on_click: OnClick, mouse_state: MouseStateHandle) -> Self {
        Self {
            text,
            on_click,
            detail: None,
            mouse_state,
        }
    }

    pub fn with_detail(mut self, detail: String) -> Self {
        self.detail = Some(detail);
        self
    }
}

/// Configuration for redaction messaging in tooltips
pub enum TooltipRedaction {
    /// When sending text to an LLM, we want to ensure users this secret
    /// was obfuscated and not sent to the LLM.
    SecretNotSentToLLMMessaging,
    /// When displaying text which is secret and could be added to an Agent Mode
    /// conversation, we want to ensure users this secret will not be sent to
    /// the LLM.
    SecretWillNotBeSentToLLMMessaging,
    NoRedaction,
}

/// Render a tooltip with one or more links and optional redaction messaging.
///
/// This is generic over the click handler type to support different action dispatch mechanisms.
pub fn render_tooltip<OnClick>(
    tooltip_links: impl IntoIterator<Item = TooltipLink<OnClick>>,
    redaction: TooltipRedaction,
    appearance: &Appearance,
) -> Box<dyn Element>
where
    OnClick: 'static + Fn(&mut EventContext),
{
    let mut tooltip = Flex::column();
    let mut links = Vec::new();
    let mut first = true;
    let background_color = appearance.theme().tooltip_background();

    for link in tooltip_links.into_iter() {
        if !first {
            links.push(
                Container::new(
                    appearance
                        .ui_builder()
                        .span(" | ".to_string())
                        .build()
                        .finish(),
                )
                .with_horizontal_padding(8.)
                .finish(),
            );
        }

        let on_click = link.on_click;
        links.push(
            appearance
                .ui_builder()
                .tooltip_link(
                    link.text,
                    None,
                    Some(Box::new(move |ctx| {
                        on_click(ctx);
                    })),
                    link.mouse_state,
                )
                .soft_wrap(false)
                .build()
                .finish(),
        );

        if let Some(detail) = link.detail {
            links.push(
                appearance
                    .ui_builder()
                    .span(detail)
                    .with_style(UiComponentStyles {
                        margin: Some(Coords::default().left(4.)),
                        ..Default::default()
                    })
                    .build()
                    .finish(),
            );
        }

        first = false;
    }

    let link_row = if links.is_empty() {
        None
    } else {
        Some(Flex::row().with_children(links).finish())
    };

    match redaction {
        TooltipRedaction::SecretNotSentToLLMMessaging
        | TooltipRedaction::SecretWillNotBeSentToLLMMessaging => {
            let theme = appearance.theme();
            let title = if matches!(redaction, TooltipRedaction::SecretNotSentToLLMMessaging) {
                "This wasn't included in the AI conversation."
            } else {
                "This won't be included in any AI conversations or shared blocks."
            };

            let secret_message = "Pattern matched your secret redaction regex list.";

            tooltip.add_child(
                Flex::column()
                    .with_child(
                        Text::new(
                            title,
                            appearance.ui_font_family(),
                            appearance.ui_font_size() + 1.,
                        )
                        .with_color(theme.main_text_color(background_color.into()).into_solid())
                        .finish(),
                    )
                    .with_child(
                        Container::new(
                            Text::new(
                                secret_message,
                                appearance.ui_font_family(),
                                appearance.ui_font_size(),
                            )
                            .with_color(theme.sub_text_color(background_color.into()).into_solid())
                            .finish(),
                        )
                        .with_margin_top(4.)
                        .finish(),
                    )
                    .finish(),
            );
            if let Some(link_row) = link_row {
                tooltip.add_child(Container::new(link_row).with_margin_top(4.).finish());
            }
        }
        TooltipRedaction::NoRedaction => {
            if let Some(link_row) = link_row {
                tooltip.add_child(link_row);
            }
        }
    }

    let tooltip_element = Container::new(tooltip.finish())
        .with_vertical_padding(4.)
        .with_horizontal_padding(6.)
        .finish();

    Container::new(tooltip_element)
        .with_background(background_color)
        .with_corner_radius(CornerRadius::with_all(Radius::Pixels(4.)))
        .with_border(Border::all(1.).with_border_fill(appearance.theme().outline()))
        .finish()
}

/// Returns whether "Open in Warp" should be offered for the given file path.
///
/// This checks:
/// - Whether Warp is already the default editor (skip if so)
/// - Whether this file is openable in Warp (skips binary files and directories)
/// - Whether the file renders in Warp's notebook viewer, which is reached via a
///   different affordance (skips Markdown and, when enabled, Jupyter notebooks)
pub fn should_show_open_in_warp_link(path: &Path, app: &AppContext) -> bool {
    use warpui::SingletonEntity;

    use crate::code::view::is_binary_file;
    use crate::notebooks::file::renders_in_warp_notebook_viewer;
    use crate::util::file::external_editor::EditorSettings;
    use crate::util::file::external_editor::settings::EditorChoice;

    let settings = EditorSettings::as_ref(app);

    if matches!(*settings.open_file_editor, EditorChoice::Warp) {
        return false;
    }

    !renders_in_warp_notebook_viewer(path) && !is_binary_file(path) && !path.is_dir()
}
