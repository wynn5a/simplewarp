use pathfinder_color::ColorU;
use warp_core::ui::theme::Fill;
use warp_core::ui::theme::phenomenon::PhenomenonStyle;
use warpui::elements::{CornerRadius, MouseStateHandle, Radius};
use warpui::ui_components::checkbox::Checkbox;
use warpui::ui_components::components::UiComponentStyles;

use crate::appearance::Appearance;

pub fn phenomenon_background_color() -> ColorU {
    PhenomenonStyle::background()
}

pub fn phenomenon_foreground_color() -> ColorU {
    PhenomenonStyle::foreground()
}

pub fn phenomenon_accent_color() -> ColorU {
    PhenomenonStyle::accent()
}

pub fn phenomenon_body_text_color() -> ColorU {
    PhenomenonStyle::body_text()
}

pub fn phenomenon_label_text_color() -> ColorU {
    PhenomenonStyle::label_text()
}

pub fn phenomenon_disabled_label_text_color() -> ColorU {
    PhenomenonStyle::disabled_label_text()
}

pub fn phenomenon_subtle_border_color() -> ColorU {
    PhenomenonStyle::subtle_border()
}

/// Label/secondary text color for callout content in the Phenomenon palette.
pub fn callout_label_color(appearance: &Appearance) -> ColorU {
    let _ = appearance;
    phenomenon_label_text_color()
}

/// Creates a checkbox styled for callout bubbles using the Phenomenon palette.
///
/// Unchecked: foreground border, no fill.
/// Checked: foreground fill, background-colored check icon.
pub fn callout_checkbox(
    mouse_state: MouseStateHandle,
    size: Option<f32>,
    appearance: &Appearance,
) -> Checkbox {
    let _ = appearance;
    let foreground_color = phenomenon_foreground_color();
    let foreground_fill = Fill::Solid(foreground_color);
    let background_color = phenomenon_background_color();
    let disabled_color = phenomenon_subtle_border_color();
    let checkbox_size = size.or(Some(12.));
    let corner_radius = CornerRadius::with_all(Radius::Pixels(2.));

    Checkbox::new(
        mouse_state,
        UiComponentStyles {
            font_size: checkbox_size,
            border_color: Some(Fill::Solid(foreground_color).into()),
            font_color: Some(foreground_color),
            border_width: Some(1.),
            border_radius: Some(corner_radius),
            ..Default::default()
        },
        None,
        Some(UiComponentStyles {
            font_size: checkbox_size,
            background: Some(foreground_fill.into()),
            border_color: Some(foreground_fill.into()),
            font_color: Some(background_color),
            border_radius: Some(corner_radius),
            ..Default::default()
        }),
        Some(UiComponentStyles {
            font_size: checkbox_size,
            border_color: Some(Fill::Solid(disabled_color).into()),
            font_color: Some(disabled_color),
            border_width: Some(1.),
            border_radius: Some(corner_radius),
            ..Default::default()
        }),
    )
}
