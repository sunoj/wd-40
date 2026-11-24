// The scan screen's footer: the clean button, the action that ticks every old
// target, the caveat and receipt lines, and the shortcuts. Shared by the
// popover and the window's column. Exports: `footer_h`, `draw_footer`.
// Deps: crate::{controls, header, selection, state, widgets}.

use crate::controls::{clean_button, set_cmd_key, text_button_hint, text_button_underlined, CleanCta};
use crate::header::scan_size;
use crate::layout::{content_w, panel_w};
use crate::state::AppState;
use crate::theme::Theme;
use crate::widgets::{self, add_line, label, PAD_X};
use objc2::runtime::AnyObject;
use objc2::sel;
use objc2_app_kit::NSView;
use objc2_foundation::MainThreadMarker;
use std::collections::HashSet;
use wd40::disk::sum_bytes;

const TAG_TICK_OLD: isize = 2008;

/// Button, shortcuts and padding. The caveat line adds its own height only
/// when there is a caveat.
const FOOTER_BASE_H: f64 = 95.0;
const RECEIPT_H: f64 = 20.0;
const TICK_H: f64 = 24.0;

pub fn footer_h(state: &AppState) -> f64 {
    FOOTER_BASE_H
        + if old_batch(state).0.is_empty() { 0.0 } else { TICK_H }
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
    clean_button(parent, &clean_cta(state), PAD_X, footer_h(state) - 47.0, content_w(), sel!(handleCleanSelected:), target, crate::scan_view::TAG_CLEAN, theme, mtm);
    let mut line_y = footer_h(state) - 69.0;
    let (old, bytes) = old_batch(state);
    if !old.is_empty() {
        draw_tick_old(parent, line_y - 3.0, state, &old, bytes, theme, target, mtm);
        line_y -= TICK_H;
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

/// The targets past the keep threshold, and what they add up to.
fn old_batch(state: &AppState) -> (HashSet<usize>, u64) {
    let old = crate::selection::old_targets(&state.targets, &state.empty_targets(), state.config.max_age_days);
    let bytes = sum_bytes(old.iter().map(|index| state.targets[*index].size_bytes));
    (old, bytes)
}

/// One action that ticks every old target, in every group, with the count and
/// size it stands for written on it. It only ticks: nothing is removed until
/// Clean is pressed. Once the whole batch is ticked it says so instead.
#[allow(clippy::too_many_arguments)]
fn draw_tick_old(parent: &NSView, y: f64, state: &AppState, old: &HashSet<usize>, bytes: u64, theme: &Theme, target: &AnyObject, mtm: MainThreadMarker) {
    let days = state.config.max_age_days;
    let what = format!("{} old \u{00b7} {}", old.len(), scan_size(bytes));
    if old.is_subset(&state.selected) {
        let done = format!("All {what} ticked (older than {days}d)");
        label(parent, &done, PAD_X, y, content_w(), 16.0, 12.0, false, theme.ink_3, false, mtm);
        return;
    }
    let title = format!("Tick {what}");
    let width = widgets::fitted_width(&title, 12.5, false, mtm) + 8.0;
    text_button_underlined(parent, &title, PAD_X, y - 3.0, width, sel!(handleTickOld:), target, TAG_TICK_OLD, theme.accent, mtm);
    let hint = format!("older than {days}d, sizes as scanned");
    label(parent, &hint, PAD_X + width + 6.0, y, content_w() - width - 6.0, 16.0, 11.5, false, theme.ink_3, false, mtm);
}

fn clean_cta(state: &AppState) -> CleanCta {
    if state.sizing() || crate::tasks::is_scanning() {
        return CleanCta::Measuring;
    }
    if state.selected.is_empty() {
        let days = state.config.max_age_days;
        let clear = !state.targets.is_empty() && crate::selection::nothing_old(&state.targets, &state.empty_targets(), None, days);
        return if clear { CleanCta::Clear } else { CleanCta::Empty };
    }
    CleanCta::Ready {
        count: state.selected.len(),
        size: scan_size(state.selected_size()),
    }
}
