// The settings screen's frame: brand bar, scrolling body and footer. In the
// popover it is one column sized to its content; in the window the same column
// sits at a reading width in the middle of one surface that fills the window.
// Exports: `build_column`, `build_wide`. Deps: crate::{layout, settings_chrome, settings_view, widgets}.

use crate::layout::panel_w;
use crate::settings_chrome::{draw_brand, draw_footer, BRAND_H, FOOTER_H};
use crate::settings_view::{disclosure_height, draw_body};
use crate::state::AppState;
use crate::theme::Theme;
use crate::widgets;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_app_kit::NSView;
use objc2_foundation::{MainThreadMarker, NSPoint};

/// The settings column's width in the window: wide enough for a row's title
/// and its control to sit apart, narrow enough to read down.
const READING_W: f64 = 560.0;
/// Room for every section at once. What the body actually uses is measured off
/// the drawing rather than predicted by a second copy of the layout.
const SHEET_H: f64 = 1400.0;
const MAX_HEIGHT: f64 = 540.0;
/// The window: one surface edge to edge, with the settings in a column of
/// reading width down its middle and the rules of the brand bar and the footer
/// carried across the whole window.
pub fn build_wide(state: &AppState, theme: &Theme, target: &AnyObject, width: f64, height: f64, mtm: MainThreadMarker) -> Retained<NSView> {
    let root = widgets::sheet_view(width, height, theme, mtm);
    widgets::add_line(&root, 0.0, height - BRAND_H, width, theme.line, mtm);
    widgets::add_line(&root, 0.0, FOOTER_H, width, theme.line, mtm);
    let reading = READING_W.min(width);
    let (column, _) = crate::layout::with_width(reading, || build_column(state, theme, target, Some(height), mtm));
    column.setFrameOrigin(NSPoint::new(((width - reading) / 2.0).floor(), 0.0));
    root.addSubview(&column);
    // Last, so the label runs unbroken over the column's own surface.
    widgets::label_stripe(&root, mtm);
    root
}

/// Brand bar, scrolling body and footer in one column of the current width.
/// The popover sizes it to its content; the window gives it `fill` to fill.
pub fn build_column(
    state: &AppState,
    theme: &Theme,
    target: &AnyObject,
    fill: Option<f64>,
    mtm: MainThreadMarker,
) -> (Retained<NSView>, f64) {
    let sheet = widgets::sheet_view(panel_w(), SHEET_H, theme, mtm);
    let used = SHEET_H - draw_body(&sheet, SHEET_H, state, theme, target, mtm);

    let (body_h, root) = match fill {
        Some(height) => {
            let body_h = height - BRAND_H - FOOTER_H;
            (body_h, widgets::sheet_view(panel_w(), height, theme, mtm))
        }
        None => {
            let body_h = used.min(MAX_HEIGHT - BRAND_H - FOOTER_H + disclosure_height());
            (body_h, widgets::root_view(panel_w(), BRAND_H + body_h + FOOTER_H, theme, mtm))
        }
    };
    let height = BRAND_H + body_h + FOOTER_H;
    draw_brand(&root, height, theme, mtm);
    let document =
        crate::scrolling::scroll_document_view(&root, 0.0, FOOTER_H, panel_w(), body_h, used, mtm);
    // The sheet is taller than what it drew, so hang it from the document's top
    // edge and let the empty tail fall below the scroller.
    sheet.setFrameOrigin(NSPoint::new(0.0, document.frame().size.height - SHEET_H));
    document.addSubview(&sheet);
    draw_footer(&root, theme, target, mtm);
    (root, height)
}
