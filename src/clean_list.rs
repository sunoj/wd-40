// The window's list of a clean in progress: one line per target with a mark
// that says where it stands. Lines are built once; a progress tick changes the
// mark and colour of the lines whose status moved, and nothing else.
// Exports: `CleanList`, `build`. Deps: crate::{header, scrolling, state, widgets}.

use crate::header::scan_size;
use crate::layout::{content_w, panel_w};
use crate::state::{CleanItemStatus, CleanProgress};
use crate::theme::Theme;
use crate::widgets::{label, label_right, PAD_X};
use objc2::rc::Retained;
use objc2_app_kit::{NSLineBreakMode, NSTextField, NSView};
use objc2_foundation::{MainThreadMarker, NSString};
use std::cell::Cell;

const ROW_H: f64 = 26.0;
const SIZE_W: f64 = 84.0;

struct Line {
    status: Cell<CleanItemStatus>,
    mark: Retained<NSTextField>,
    name: Retained<NSTextField>,
}

pub struct CleanList {
    lines: Vec<Line>,
    theme: Theme,
}

impl CleanList {
    /// Take the run's new state: only lines whose status changed are touched.
    pub fn update(&self, progress: &CleanProgress) {
        for (line, item) in self.lines.iter().zip(&progress.items) {
            if line.status.replace(item.status) != item.status {
                paint(line, item.status, &self.theme);
            }
        }
    }
}

/// The glyph and colour a status reads as.
fn mark(status: CleanItemStatus, theme: &Theme) -> (&'static str, (f64, f64, f64)) {
    match status {
        CleanItemStatus::Pending => ("\u{00b7}", theme.ink_3),
        CleanItemStatus::Active => ("\u{25cb}", theme.accent),
        CleanItemStatus::Done => ("\u{2713}", theme.pos),
        CleanItemStatus::Partial => ("\u{25d0}", theme.danger),
        CleanItemStatus::Failed => ("\u{2715}", theme.danger),
        CleanItemStatus::Skipped => ("\u{2013}", theme.ink_3),
    }
}

fn paint(line: &Line, status: CleanItemStatus, theme: &Theme) {
    let (glyph, colour) = mark(status, theme);
    line.mark.setStringValue(&NSString::from_str(glyph));
    line.mark.setTextColor(Some(&Theme::color(colour)));
    let ink = match status {
        CleanItemStatus::Pending | CleanItemStatus::Skipped => theme.ink_2,
        _ => theme.ink,
    };
    line.name.setTextColor(Some(&Theme::color(ink)));
}

/// The list between `floor` and `ceiling` of a popover-wide column, scrolling
/// when the job has more targets than fit.
pub fn build(
    root: &NSView,
    floor: f64,
    ceiling: f64,
    progress: &CleanProgress,
    theme: &Theme,
    mtm: MainThreadMarker,
) -> CleanList {
    let document_h = progress.items.len() as f64 * ROW_H + 12.0;
    let room = ceiling - floor;
    let document = crate::scrolling::scroll_document_view(root, 0.0, floor, panel_w(), room, document_h, mtm);
    let top = document_h.max(room) - 6.0;
    let lines = progress
        .items
        .iter()
        .enumerate()
        .map(|(row, item)| {
            let y = top - (row + 1) as f64 * ROW_H + 5.0;
            let mark = label(&document, "", PAD_X, y, 18.0, 16.0, 13.0, true, theme.ink_3, false, mtm);
            let name_w = content_w() - 24.0 - SIZE_W;
            let name = label(&document, &item.name, PAD_X + 24.0, y, name_w, 16.0, 13.0, false, theme.ink, false, mtm);
            name.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
            label_right(&document, &scan_size(item.size_bytes), PAD_X + 24.0 + name_w, y, SIZE_W, 16.0, 12.0, theme.ink_2, true, mtm);
            let line = Line { status: Cell::new(item.status), mark, name };
            paint(&line, item.status, theme);
            line
        })
        .collect();
    CleanList { lines, theme: *theme }
}
