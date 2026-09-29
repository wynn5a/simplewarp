use pathfinder_color::ColorU;
use pathfinder_geometry::vector::vec2f;
use warp_core::ui::icons::Icon as WarpIcon;
use warp_core::ui::theme::color::internal_colors;
use warp_core::ui::theme::{ColorScheme, Fill as WarpThemeFill, WarpTheme};
use warpui::elements::{
    ChildAnchor, ConstrainedBox, Container, CornerRadius, Element, OffsetPositioning, ParentAnchor,
    ParentElement, ParentOffsetBounds, Radius, Stack,
};

use crate::ai::agent::conversation::ConversationStatus;
use crate::terminal::CLIAgent;
use crate::themes::theme::Fill as ThemeFill;

// Sub-component size ratios, expressed as fractions of `total_size`. The brand circle is
// ~76% wide and the status badge is ~57% wide, with the badge's bottom-right anchored at
// the box's bottom-right corner. With these ratios the badge center sits *inside* the
// brand circle (not on its edge). `CIRCLE_RATIO` is `pub(crate)` so callers that
// pre-render their own avatar can size it consistently with the other variants.
pub(crate) const CIRCLE_RATIO: f32 = 0.76;
const ICON_RATIO: f32 = 0.43;
const DEFAULT_BADGE_RATIO: f32 = 0.57;
const DEFAULT_BADGE_ICON_RATIO: f32 = 0.34;

// Neutral variants have no overlay, so they fill the full `total_size` bounding box. The
// inner glyph occupies `NEUTRAL_GLYPH_RATIO * total_size`, matching the old sizing where
// a 24px container held a 16px glyph (16/24 ≈ 0.667).
const NEUTRAL_GLYPH_RATIO: f32 = 16.0 / 24.0;

/// Returns the brand-circle diameter for a given `total_size`.
pub(crate) fn circle_size(total: f32) -> f32 {
    total * CIRCLE_RATIO
}

fn icon_size(total: f32) -> f32 {
    total * ICON_RATIO
}

fn circle_padding(total: f32) -> f32 {
    (circle_size(total) - icon_size(total)) / 2.
}

/// Returns the diameter of the status badge's cutout ring, i.e. the badge's
/// full painted footprint.
fn badge_size(total: f32) -> f32 {
    total * DEFAULT_BADGE_RATIO
}

fn badge_icon_size(total: f32) -> f32 {
    total * DEFAULT_BADGE_ICON_RATIO
}

fn badge_padding(total: f32) -> f32 {
    (badge_size(total) - badge_icon_size(total)) / 4.
}

/// Default overhang of the overlay's BR past the circle's BR edge (toward the box's
/// BR), as a fraction of `total_size`. Baked into `corner_overlay_offset` so most
/// surfaces can just pass `0.0` for their `overlay_extra_overhang_ratio`.
const DEFAULT_OVERLAY_OVERHANG_PAST_CIRCLE_EDGE: f32 = 0.19;

/// Returns the pixel offset applied to the overlay's `BottomRight → BottomRight`
/// anchor.
/// The offset is measured from the bounding box's BR corner, so the returned value is
/// negative whenever the overlay sits up-and-left of the box's BR (which is the only
/// case we render).
///
/// `overlay_extra_overhang_ratio` is a signed fraction of `total` added to
/// `DEFAULT_OVERLAY_OVERHANG_PAST_CIRCLE_EDGE`:
/// * `0.0` — overlay BR sits `DEFAULT_OVERLAY_OVERHANG_PAST_CIRCLE_EDGE * total` past
///   the circle's BR (the position most surfaces want).
/// * Positive — overlay BR pushed further toward the box's BR. A value of
///   `1 - CIRCLE_RATIO - DEFAULT_OVERLAY_OVERHANG_PAST_CIRCLE_EDGE` (= 0.05) lands
///   exactly on the box's BR — the Figma-natural overhang.
/// * Negative — overlay BR pulled inward toward the circle's center.
fn corner_overlay_offset(total: f32, overlay_extra_overhang_ratio: f32) -> f32 {
    let total_overhang = DEFAULT_OVERLAY_OVERHANG_PAST_CIRCLE_EDGE + overlay_extra_overhang_ratio;
    -((1.0 - CIRCLE_RATIO) - total_overhang) * total
}

/// What to render inside the circle.
pub(crate) enum IconWithStatusVariant {
    /// A generic icon with a given color on an overlay background.
    Neutral {
        icon: WarpIcon,
        icon_color: WarpThemeFill,
    },
    /// A pre-built icon element on an overlay background.
    NeutralElement { icon_element: Box<dyn Element> },
    /// A Warp agent conversation: monochrome Warp glyph and circle, using a
    /// foreground/background pair that flips for light and dark themes.
    OzAgent { status: Option<ConversationStatus> },
    /// A CLI agent icon on the agent's brand color background.
    CLIAgent {
        agent: CLIAgent,
        status: Option<ConversationStatus>,
    },
}

/// Renders an icon-with-status component sized entirely from a single `total_size`. All
/// sub-components (brand circle, status badge) are derived proportionally,
/// so callers only need to pick the size they want.
///
/// `overlay_extra_overhang_ratio` is a signed fraction of `total_size` added to the
/// default overlay overhang past the circle's BR edge. Most surfaces pass `0.0` to
/// get the default position; positive values push the overlay further toward the box's
/// BR (more overhang) and negative values pull it inward toward the circle's center.
pub(crate) fn render_icon_with_status(
    variant: IconWithStatusVariant,
    total_size: f32,
    overlay_extra_overhang_ratio: f32,
    theme: &WarpTheme,
    status_container_background: WarpThemeFill,
) -> Box<dyn Element> {
    let sub_text = theme.sub_text_color(theme.background());

    match variant {
        IconWithStatusVariant::Neutral { icon, icon_color } => render_neutral_circle(
            icon.to_warpui_icon(icon_color).finish(),
            internal_colors::fg_overlay_2(theme),
            total_size,
        ),
        IconWithStatusVariant::NeutralElement { icon_element } => render_neutral_circle(
            icon_element,
            internal_colors::fg_overlay_2(theme),
            total_size,
        ),
        IconWithStatusVariant::OzAgent { status } => {
            let (circle_background, glyph_color) = warp_agent_circle_colors(theme);
            let circle = render_circle(
                WarpIcon::Agent.to_warpui_icon(glyph_color).finish(),
                circle_background,
                total_size,
            );
            render_with_optional_status_badge(
                circle,
                status.as_ref(),
                total_size,
                overlay_extra_overhang_ratio,
                theme,
                status_container_background,
            )
        }
        IconWithStatusVariant::CLIAgent { agent, status } => {
            let brand_color = agent
                .brand_color()
                .unwrap_or(ColorU::new(100, 100, 100, 255));
            let icon_color = agent.brand_icon_color();
            let icon_element = agent
                .icon()
                .map(|icon| {
                    icon.to_warpui_icon(WarpThemeFill::Solid(icon_color))
                        .finish()
                })
                .unwrap_or_else(|| WarpIcon::Terminal.to_warpui_icon(sub_text).finish());
            let circle = render_circle(icon_element, ThemeFill::Solid(brand_color), total_size);
            render_with_optional_status_badge(
                circle,
                status.as_ref(),
                total_size,
                overlay_extra_overhang_ratio,
                theme,
                status_container_background,
            )
        }
    }
}

fn warp_agent_circle_colors(theme: &WarpTheme) -> (WarpThemeFill, WarpThemeFill) {
    match theme.inferred_color_scheme() {
        ColorScheme::LightOnDark => (WarpThemeFill::black(), WarpThemeFill::white()),
        ColorScheme::DarkOnLight => (WarpThemeFill::white(), WarpThemeFill::black()),
    }
}

/// Builds the brand-circle container around `icon_element`. The circle's diameter is
/// `circle_size(total)` and the icon glyph is `icon_size(total)`, with the rest going
/// to symmetric padding around the glyph.
/// The returned element is `circle_size(total)` wide; agent callers wrap it via
/// `render_with_optional_status_badge` to occupy the full `total_size` footprint.
fn render_circle(
    icon_element: Box<dyn Element>,
    background: WarpThemeFill,
    total_size: f32,
) -> Box<dyn Element> {
    let icon = icon_size(total_size);
    let padding = circle_padding(total_size);
    let inner = ConstrainedBox::new(icon_element)
        .with_width(icon)
        .with_height(icon)
        .finish();
    Container::new(inner)
        .with_uniform_padding(padding)
        .with_background(background)
        .with_corner_radius(CornerRadius::with_all(Radius::Pixels(
            circle_size(total_size) / 2.,
        )))
        .finish()
}

/// Builds the neutral circle: a full-`total_size` container with the glyph at
/// `NEUTRAL_GLYPH_RATIO * total_size`. Used for non-agent surfaces (plain terminal,
/// code, file tabs, etc.) which have no status overlay and therefore should fill the
/// requested bounding box rather than shrinking to `circle_size(total)`.
fn render_neutral_circle(
    icon_element: Box<dyn Element>,
    background: WarpThemeFill,
    total_size: f32,
) -> Box<dyn Element> {
    let glyph = total_size * NEUTRAL_GLYPH_RATIO;
    let padding = (total_size - glyph) / 2.;
    let inner = ConstrainedBox::new(icon_element)
        .with_width(glyph)
        .with_height(glyph)
        .finish();
    Container::new(inner)
        .with_uniform_padding(padding)
        .with_background(background)
        .with_corner_radius(CornerRadius::with_all(Radius::Pixels(total_size / 2.)))
        .finish()
}

/// Adds a status badge with a cutout ring to the bottom-right of the circle.
fn render_with_optional_status_badge(
    circle: Box<dyn Element>,
    status: Option<&ConversationStatus>,
    total_size: f32,
    overlay_extra_overhang_ratio: f32,
    theme: &WarpTheme,
    status_container_background: WarpThemeFill,
) -> Box<dyn Element> {
    let Some(status) = status else {
        // No status badge: still reserve the full `total_size` footprint the caller
        // asked for, so badged and un-badged variants occupy identical space.
        // `ConstrainedBox` only tightens constraints — it does not center — so the
        // circle (which is only `circle_size(total)` wide) is painted at the box's
        // top-left. Callers that need it centered must wrap it in `Align`.
        return ConstrainedBox::new(circle)
            .with_width(total_size)
            .with_height(total_size)
            .finish();
    };
    let (icon, color) = status.status_icon_and_color(theme);
    let badge_icon_diameter = badge_icon_size(total_size);
    let pad = badge_padding(total_size);
    let badge_icon = ConstrainedBox::new(icon.to_warpui_icon(WarpThemeFill::Solid(color)).finish())
        .with_width(badge_icon_diameter)
        .with_height(badge_icon_diameter)
        .finish();
    let badge = Container::new(badge_icon)
        .with_uniform_padding(pad)
        .with_corner_radius(CornerRadius::with_all(Radius::Percentage(50.)))
        .finish();
    let badge_with_ring = Container::new(badge)
        .with_uniform_padding(pad)
        .with_background(status_container_background)
        .with_corner_radius(CornerRadius::with_all(Radius::Percentage(50.)))
        .finish();

    let badge_corner_offset = corner_overlay_offset(total_size, overlay_extra_overhang_ratio);
    let mut stack = Stack::new().with_child(
        ConstrainedBox::new(circle)
            .with_width(total_size)
            .with_height(total_size)
            .finish(),
    );
    stack.add_positioned_child(
        badge_with_ring,
        OffsetPositioning::offset_from_parent(
            vec2f(badge_corner_offset, badge_corner_offset),
            ParentOffsetBounds::Unbounded,
            ParentAnchor::BottomRight,
            ChildAnchor::BottomRight,
        ),
    );
    ConstrainedBox::new(stack.finish())
        .with_width(total_size)
        .with_height(total_size)
        .finish()
}

#[cfg(test)]
#[path = "icon_with_status_tests.rs"]
mod tests;
