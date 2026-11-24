// The width a screen is being laid out for. The popover is always 380pt; the
// window lays the same screens out in wider panes. Builders read the width from
// here instead of a constant, and a `live::Zone` re-enters the width it was
// built at before it repaints.
// Exports: `panel_w`, `content_w`, `with_width`, `window_size`, `with_window`,
// `map_frame`.

use crate::widgets::PAD_X;
use objc2_foundation::{NSPoint, NSRect, NSSize};
use std::cell::Cell;

/// The popover's fixed width, and the width of any pane nobody has resized.
pub const POPOVER_WIDTH: f64 = 380.0;

thread_local! {
    static WIDTH: Cell<f64> = const { Cell::new(POPOVER_WIDTH) };
    /// The window's content size while a screen is being built for it.
    static WINDOW: Cell<Option<(f64, f64)>> = const { Cell::new(None) };
}

/// Full width of the pane being drawn, edge to edge.
pub fn panel_w() -> f64 {
    WIDTH.with(Cell::get)
}

/// Width between the pane's side paddings.
pub fn content_w() -> f64 {
    panel_w() - PAD_X * 2.0
}

/// Draw `body` at `width`, then put back whatever width was in force before.
pub fn with_width<R>(width: f64, body: impl FnOnce() -> R) -> R {
    let before = WIDTH.with(|cell| cell.replace(width));
    let out = body();
    WIDTH.with(|cell| cell.set(before));
    out
}

/// Width and height of the window being built for; `None` in the popover. A
/// screen with a wide layout reads this to choose it.
pub fn window_size() -> Option<(f64, f64)> {
    WINDOW.with(Cell::get)
}

/// Build `body` for a window of `size`, or for the popover when there is none.
pub fn with_window<R>(size: Option<(f64, f64)>, body: impl FnOnce() -> R) -> R {
    let before = WINDOW.with(|cell| cell.replace(size));
    let out = body();
    WINDOW.with(|cell| cell.set(before));
    out
}

/// Where the window puts its map: everything right of the popover-wide column,
/// inside a margin, with a line of room above for the colour key and below for
/// the legend. The scan map and the cleaning plate both use exactly this
/// frame, which is what makes one hand over to the other without a jump.
pub fn map_frame(width: f64, height: f64) -> NSRect {
    const MARGIN: f64 = 22.0;
    const LINE: f64 = 26.0;
    let x = POPOVER_WIDTH + 1.0 + MARGIN;
    let y = MARGIN + LINE;
    NSRect::new(NSPoint::new(x, y), NSSize::new(width - x - MARGIN, height - y - MARGIN - LINE))
}
