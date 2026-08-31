//! A reusable warning callout.
//!
//! This used to be a builder (`WarningBoxConfig`) with an optional description, icon override,
//! max width, and action button. The Environments settings page was the only caller that used
//! any of them; with it gone, both remaining callers pass a formatted title and nothing else,
//! so the config collapsed into this one argument.
use markdown_parser::{FormattedText, FormattedTextInline, FormattedTextLine};
use warp_core::ui::color::blend::Blend;
use warpui::color::ColorU;
use warpui::elements::{
    Border, ConstrainedBox, Container, CornerRadius, CrossAxisAlignment, Element, Expanded, Flex,
    FormattedTextElement, HyperlinkLens, MainAxisSize, ParentElement, Radius,
};

use crate::appearance::Appearance;
use crate::themes::theme::Fill as ThemeFill;
use crate::ui_components::icons::Icon;

pub fn render_warning_box(title: FormattedTextInline, appearance: &Appearance) -> Box<dyn Element> {
    let theme = appearance.theme();
    let icon_size = appearance.ui_font_size() * 1.1;

    let warning_color = theme.ui_warning_color();

    // Use a lighter yellow for readability while still clearly communicating “warning”.
    let text_color: ColorU = ThemeFill::Solid(theme.ui_yellow_color())
        .blend(&theme.foreground().with_opacity(70))
        .into();

    let warning_fill = ThemeFill::Solid(warning_color);
    let icon_fill = ThemeFill::Solid(text_color);

    let background = theme.surface_2().blend(&warning_fill.with_opacity(15));

    let title = FormattedTextElement::new(
        FormattedText::new([FormattedTextLine::Line(title)]),
        appearance.ui_font_size(),
        appearance.ui_font_family(),
        appearance.ui_font_family(),
        text_color,
        Default::default(),
    )
    .with_hyperlink_font_color(theme.accent().into())
    .register_default_click_handlers_with_action_support(|hyperlink, _event, app| {
        if let HyperlinkLens::Url(url) = hyperlink {
            app.open_url(url);
        }
    })
    .finish();

    // Warning boxes are flexible so they wrap and shrink with their container.
    let left = Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Start)
        .with_spacing(12.)
        .with_child(
            ConstrainedBox::new(Icon::AlertTriangle.to_warpui_icon(icon_fill).finish())
                .with_width(icon_size)
                .with_height(icon_size)
                .finish(),
        )
        .with_child(Expanded::new(1., title).finish())
        .finish();

    let row = Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Start)
        .with_spacing(12.)
        .with_main_axis_size(MainAxisSize::Max)
        .with_child(Expanded::new(1., left).finish())
        .finish();

    ConstrainedBox::new(
        Container::new(row)
            .with_margin_top(8.)
            .with_uniform_padding(12.)
            .with_corner_radius(CornerRadius::with_all(Radius::Pixels(6.)))
            .with_border(Border::all(1.).with_border_fill(theme.outline()))
            .with_background(background)
            .finish(),
    )
    .finish()
}
