// The menu bar item: the rusty can glyph and, past a gigabyte, the figure next
// to it. Repainted on its own while sizes arrive, where rebuilding a whole
// screen to move one number would defeat the point.
// Exports: `update`, `refresh`. Deps: crate::{icon, state, style}, objc2 AppKit.

use crate::icon::{rust_text_color, rusty_icon};
use crate::state::{with_state_ret, AppState};
use crate::style::tinted;
use objc2_app_kit::{NSColor, NSFont};
use objc2_foundation::{ns_string, MainThreadMarker};
use std::cell::Cell;
use wd40::scanner::human_size;

thread_local! {
    /// Rust-tint bucket last drawn into the status item, so a size tick that
    /// stays in the same band does not lockFocus a new glyph.
    static ICON_BUCKET: Cell<u8> = const { Cell::new(u8::MAX) };
}

/// Repaint only the menu bar item.
pub fn refresh(mtm: MainThreadMarker) {
    let _span = crate::trace::span("refresh_status");
    with_state_ret(|state| update(state, mtm));
}

pub fn update(state: &AppState, mtm: MainThreadMarker) {
    let total = state.total_size();
    let Some(button) = state.status_item.button(mtm) else { return };
    // With colour off the glyph is always the plain template, whatever the size.
    let color = state.config.menu_bar_color;
    let bucket = if color { icon_bucket(total) } else { 0 };
    let redraw = ICON_BUCKET.with(|cell| {
        let same = cell.get() == bucket && button.image().is_some();
        if !same {
            cell.set(bucket);
        }
        !same
    });
    if redraw {
        if let Some(image) = rusty_icon(total, color) {
            button.setImage(Some(&image));
        }
    }
    if !state.config.menu_bar_size || total < 1024 * 1024 * 1024 {
        button.setTitle(ns_string!(""));
        return;
    }
    let title = format!(" {}", human_size(total));
    let font = NSFont::menuBarFontOfSize(0.0);
    let tint = if color { rust_text_color(total) } else { NSColor::labelColor() };
    button.setAttributedTitle(&tinted(&title, &font, &tint));
}

fn icon_bucket(total_bytes: u64) -> u8 {
    match total_bytes / (1024 * 1024 * 1024) {
        0..=4 => 0,
        5..=19 => 1,
        20..=49 => 2,
        _ => 3,
    }
}
