// The scan screen's footer: the clean button, the auto-clean receipt line and
// the shortcuts. Shared by the popover and the window's column; the all-clear
// screen keeps only the shortcuts. Exports: `footer_h`, `draw_footer`.
// Deps: crate::{controls, header, scan_calm, state, widgets}.

use crate::controls::{clean_button, set_cmd_key, text_button_hint, CleanCta};
use crate::header::scan_size;
use crate::layout::{content_w, panel_w};
use crate::scan_calm::calm;
use crate::state::AppState;
use crate::theme::Theme;
use crate::widgets::{add_line, label, PAD_X};
use objc2::runtime::AnyObject;
use objc2::sel;
use objc2_app_kit::NSView;
use objc2_foundation::MainThreadMarker;

/// The shortcuts and their padding; the clean button and the receipt line add
/// their own height when they are there.
const SHORTCUTS_H: f64 = 44.0;
const CLEAN_H: f64 = 51.0;
const RECEIPT_H: f64 = 20.0;

/// The all-clear screen has nothing to arm, so it has no clean button: a grey
/// one there read as a control that had broken.
pub fn footer_h(state: &AppState) -> f64 {
    SHORTCUTS_H
        + if calm(state) { 0.0 } else { CLEAN_H }
        + if crate::auto_clean::receipt_line().is_some() { RECEIPT_H } else { 0.0 }
}

pub fn draw_footer(
    parent: &NSView,
    state: &AppState,
    theme: &Theme,
    target: &AnyObject,
    mtm: MainThreadMarker,
) {
    add_line(parent, 0.0, footer_h(state), panel_w(), theme.line, mtm);
    let mut line_y = footer_h(state) - 22.0;
    if !calm(state) {
        clean_button(parent, &clean_cta(state), PAD_X, footer_h(state) - 47.0, content_w(), sel!(handleCleanSelected:), target, crate::scan_view::TAG_CLEAN, theme, mtm);
        line_y -= CLEAN_H - 4.0;
    }
    if let Some(receipt) = crate::auto_clean::receipt_line() {
        label(parent, &receipt, PAD_X, line_y, content_w(), 16.0, 11.0, false, theme.ink_3, false, mtm);
    }
    let rescan = text_button_hint(parent, "Rescan", "\u{2318}R", PAD_X, 13.0, sel!(handleRescan:), target, crate::scan_view::TAG_RESCAN, theme.ink_2, theme.ink_4, mtm);
    set_cmd_key(&rescan, "r");
    // The window is where this footer already is when it is open.
    if !crate::window::is_visible() {
        let window = text_button_hint(parent, "Window", "\u{2318}O", (panel_w() - 86.0) / 2.0, 13.0, sel!(openWindow:), target, crate::scan_view::TAG_WINDOW, theme.ink_2, theme.ink_4, mtm);
        set_cmd_key(&window, "o");
    }
    let settings = text_button_hint(parent, "Settings", "\u{2318},", panel_w() - PAD_X - 96.0, 13.0, sel!(openSettings:), target, crate::scan_view::TAG_SETTINGS, theme.ink_2, theme.ink_4, mtm);
    set_cmd_key(&settings, ",");
}

fn clean_cta(state: &AppState) -> CleanCta {
    if state.sizing() || crate::tasks::is_scanning() {
        return CleanCta::Measuring;
    }
    if state.selected.is_empty() {
        return CleanCta::Empty;
    }
    CleanCta::Ready {
        count: state.selected.len(),
        size: scan_size(state.selected_size()),
    }
}
