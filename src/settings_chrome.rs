// Fixed header and footer for the settings screen.
use crate::controls::{set_cmd_key, text_button};
use crate::theme::Theme;
use crate::widgets::{self, add_fill, add_line, fitted_width, label, PAD_X, POPOVER_WIDTH};
use objc2::runtime::AnyObject;
use objc2::sel;
use objc2_app_kit::NSView;
use objc2_foundation::MainThreadMarker;

pub(crate) const BRAND_H: f64 = 56.0;
pub(crate) const FOOTER_H: f64 = 44.0;

pub(crate) fn draw_brand(parent: &NSView, y_top: f64, theme: &Theme, mtm: MainThreadMarker) {
    let tile = y_top - 44.0;
    add_fill(parent, PAD_X, tile, 30.0, 30.0, theme.ink, 1.0, 8.0, mtm);
    let mark = fitted_width("40", 11.0, true, mtm);
    label(parent, "40", PAD_X + (30.0 - mark) / 2.0, tile + 7.0, mark + 2.0, 16.0, 11.0, false, theme.surface, true, mtm);
    label(parent, "WD-40", PAD_X + 41.0, tile + 14.0, 120.0, 18.0, 14.5, true, theme.ink, false, mtm);
    let version = format!("v{}", crate::updater::bundle_version());
    label(parent, &version, PAD_X + 41.0, tile - 1.0, 160.0, 15.0, 11.0, false, theme.ink_3, true, mtm);
    add_line(parent, 0.0, y_top - BRAND_H, POPOVER_WIDTH, theme.line, mtm);
}

pub(crate) fn draw_footer(parent: &NSView, theme: &Theme, target: &AnyObject, mtm: MainThreadMarker) {
    add_line(parent, 0.0, FOOTER_H, POPOVER_WIDTH, theme.line, mtm);
    widgets::symbol_view(parent, "chevron.left", PAD_X, 16.5, 11.0, theme.ink_4, mtm);
    let back = fitted_width("Back", 12.5, false, mtm) + 4.0;
    text_button(parent, "Back", PAD_X + 15.0, 11.0, back, sel!(settingsBack:), target, 0, theme.ink_2, mtm);

    let hint = fitted_width("\u{2318}Q", 11.0, true, mtm) + 4.0;
    let quit_w = fitted_width("Quit", 12.5, false, mtm) + 4.0;
    let quit_x = POPOVER_WIDTH - PAD_X - hint - 6.0 - quit_w;
    let quit = text_button(parent, "Quit", quit_x, 11.0, quit_w, sel!(quit:), target, 0, theme.ink_2, mtm);
    set_cmd_key(&quit, "q");
    label(parent, "\u{2318}Q", POPOVER_WIDTH - PAD_X - hint, 13.0, hint, 16.0, 11.0, false, theme.ink_4, true, mtm);
}
