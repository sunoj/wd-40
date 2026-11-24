// Done-summary screen: the space the volume really gained, the disk before and
// after, and what was removed from each group. One column in the popover; in
// the window the same column beside the map as the run left it. No rebuild-time
// estimate — nothing here can know it. Exports: `build`.
// Deps: crate::{clean_view, controls, header, layout, medal, state, widgets}.

use crate::controls::filled_button;
use crate::header::scan_size;
use crate::layout::{self, content_w, panel_w};
use crate::medal::{medal, start_sheen, stop_sheen};
use crate::state::{AppState, DoneSummary};
use crate::theme::Theme;
use crate::widgets::{self, add_fill, add_line, label, label_right, label_tracked, label_wrap, PAD_X};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::sel;
use objc2_app_kit::{NSTextAlignment, NSTextField, NSView};
use objc2_foundation::{MainThreadMarker, NSPoint, NSRect, NSSize};
use wd40::disk::sum_bytes;
use wd40::scanner::ArtifactGroup;

pub const TAG_DONE: isize = 2003;
const DISC: f64 = 76.0;
const HERO_H: f64 = 226.0;
const HEADINGS_H: f64 = 28.0;
const ROW_H: f64 = 34.0;
const NOTE_H: f64 = 86.0;
const FOOTER_H: f64 = 62.0;
const GAP: f64 = 10.0;

pub fn build(state: &AppState, theme: &Theme, target: &AnyObject, mtm: MainThreadMarker) -> (Retained<NSView>, f64) {
    stop_sheen();
    let summary = state.done.as_ref();
    match layout::window_size() {
        Some((width, height)) => {
            let root = widgets::root_view(width, height, theme, mtm);
            add_fill(&root, panel_w(), 0.0, 1.0, height, theme.line, 1.0, 0.0, mtm);
            draw_column(&root, height, summary, theme, target, mtm);
            // The map as the run left it: scan, clean and done are one picture.
            if let Some(run) = state.last_run.as_ref() {
                crate::clean_view::end_plate(&root, layout::map_frame(width, height), state, run, theme, target, mtm);
            }
            (root, height)
        }
        None => {
            let height = summary.map_or(64.0 + FOOTER_H, |s| column_height(s));
            let root = widgets::root_view(panel_w(), height, theme, mtm);
            draw_column(&root, height, summary, theme, target, mtm);
            (root, height)
        }
    }
}

/// Height the column needs for `s`, which is all of it the popover gives it.
fn column_height(s: &DoneSummary) -> f64 {
    let rows = s.removed.len() + usize::from(s.skipped_count > 0) + usize::from(s.troubled_count > 0);
    let note = if size_note(s).is_some() { NOTE_H } else { 0.0 };
    HERO_H + HEADINGS_H + rows as f64 * ROW_H + note + GAP + FOOTER_H
}

/// The result, stacked down from `top`, with the buttons at the bottom.
fn draw_column(root: &NSView, top: f64, summary: Option<&DoneSummary>, theme: &Theme, target: &AnyObject, mtm: MainThreadMarker) {
    draw_footer(root, theme, target, mtm);
    let Some(s) = summary else {
        label(root, "Nothing was removed", PAD_X, top - 44.0, content_w(), 22.0, 15.0, false, theme.ink_2, false, mtm);
        return;
    };
    draw_hero(root, top, s, theme, mtm);
    let mut y = top - HERO_H - HEADINGS_H;
    label_tracked(root, "REMOVED", PAD_X, y + 4.0, 160.0, 14.0, 10.5, false, theme.ink_3, true, 0.6, mtm);
    label_right(root, "SIZE AS SCANNED", PAD_X + 160.0, y + 4.0, content_w() - 160.0, 14.0, 10.5, theme.ink_3, true, mtm);
    for row in &s.removed {
        y = draw_row(root, y, row.count, ArtifactGroup::title(row.group), &scan_size(row.bytes), false, theme, mtm);
    }
    if s.skipped_count > 0 {
        y = draw_row(root, y, s.skipped_count, "Left in place \u{2014} not selected", &scan_size(s.skipped_bytes), true, theme, mtm);
    }
    // A target that would not come off, or only came off part way, is still
    // on disk. Saying so is the whole point of the screen.
    if s.troubled_count > 0 {
        y = draw_row(root, y, s.troubled_count, "Still there \u{2014} could not be removed", "", true, theme, mtm);
    }
    if let Some(note) = size_note(s) {
        add_line(root, PAD_X, y - 6.0, content_w(), theme.line, mtm);
        label_wrap(root, &note, PAD_X, y - NOTE_H + 4.0, content_w(), NOTE_H - 16.0, 11.5, theme.ink_3, mtm);
    }
}

/// The two figures on this screen measure different things, and when they
/// disagree the screen has to say which is which. The headline is the volume's
/// own free space, read before and after. A row is the size the scan measured
/// for the directories that were removed: it counts blocks a directory shares
/// with APFS clones of it, which deleting one copy does not return, and the
/// volume figure also moves with whatever else was written meanwhile.
fn size_note(s: &DoneSummary) -> Option<String> {
    let scanned = sum_bytes(s.removed.iter().map(|row| row.bytes));
    let apart = scanned.abs_diff(s.freed_bytes);
    if scanned == 0 || apart * 20 < scanned.max(s.freed_bytes) {
        return None;
    }
    Some(format!(
        "The removed directories measured {} when scanned; the volume's free space rose by {}. \
         A scanned size counts blocks shared with APFS clones, which removing one copy does not \
         give back, and free space also moves with whatever else was written meanwhile.",
        scan_size(scanned),
        scan_size(s.freed_bytes),
    ))
}

/// Medal, the space gained, and the disk bar, all on the screen's one surface.
fn draw_hero(root: &NSView, y_top: f64, s: &DoneSummary, theme: &Theme, mtm: MainThreadMarker) {
    let disc = medal(NSRect::new(NSPoint::new((panel_w() - DISC) / 2.0, y_top - 96.0), NSSize::new(DISC, DISC)), theme.dark, mtm);
    root.addSubview(&disc);
    start_sheen(&disc);

    centred(root, &format!("+{}", scan_size(s.freed_bytes)), y_top - 140.0, 32.0, true, theme.ink, mtm);
    let caption = format!("free space gained on the volume, in {}", elapsed_text(s.duration.as_secs()));
    centred(root, &caption, y_top - 162.0, 13.0, false, theme.ink_2, mtm);

    draw_bar(root, y_top - 188.0, s, theme, mtm);
    label(root, &format!("{} free", scan_size(s.free_after)), PAD_X, y_top - 210.0, 180.0, 18.0, 13.0, false, theme.ink, false, mtm);
    label_right(root, &format!("was {}", scan_size(s.free_before)), PAD_X + 180.0, y_top - 209.0, content_w() - 180.0, 16.0, 11.5, theme.ink_3, true, mtm);
    add_line(root, 0.0, y_top - HERO_H, panel_w(), theme.line, mtm);
}

/// Used / just-reclaimed / already-free, straight from the volume's own before
/// and after figures — the middle slice is exactly what this run gave back.
fn draw_bar(root: &NSView, y: f64, s: &DoneSummary, theme: &Theme, mtm: MainThreadMarker) {
    if s.total_bytes == 0 {
        return;
    }
    let scale = content_w() / s.total_bytes as f64;
    let used = (s.total_bytes.saturating_sub(s.free_after) as f64 * scale).clamp(0.0, content_w());
    let back = (s.freed_bytes as f64 * scale).clamp(0.0, content_w() - used);
    add_fill(root, PAD_X, y, content_w(), 11.0, theme.pos, 0.26, 3.0, mtm);
    add_fill(root, PAD_X, y, used.max(0.5), 11.0, theme.ink_4, 1.0, 3.0, mtm);
    add_fill(root, PAD_X + used, y, back.max(0.5), 11.0, theme.pos, 0.7, 0.0, mtm);
}

#[allow(clippy::too_many_arguments)]
fn draw_row(
    root: &NSView,
    y_top: f64,
    count: usize,
    title: &str,
    size: &str,
    muted: bool,
    theme: &Theme,
    mtm: MainThreadMarker,
) -> f64 {
    let y = y_top - ROW_H;
    let (chip, ink) = if muted { (theme.accent_weak, theme.warn) } else { (theme.surface_2, theme.ink_2) };
    add_fill(root, PAD_X, y + 6.0, 22.0, 22.0, chip, 1.0, 6.0, mtm);
    label(root, &count.to_string(), PAD_X + 2.0, y + 9.0, 18.0, 16.0, 11.0, false, ink, true, mtm);
    label(root, title, PAD_X + 32.0, y + 9.0, 216.0, 16.0, 13.5, false, theme.ink, false, mtm);
    label_right(
        root, size, PAD_X + 248.0, y + 9.0, content_w() - 248.0, 16.0, 13.0,
        if muted { theme.ink_3 } else { theme.ink }, true, mtm,
    );
    y
}

/// Two buttons sharing the column: the way on, and the way back to a scan.
fn draw_footer(root: &NSView, theme: &Theme, target: &AnyObject, mtm: MainThreadMarker) {
    add_line(root, 0.0, FOOTER_H, panel_w(), theme.line, mtm);
    let half = (content_w() - GAP) / 2.0;
    filled_button(root, "Rescan", PAD_X, 14.0, half, sel!(handleRescan:), target, crate::scan_view::TAG_RESCAN, theme.surface_2, theme.ink, mtm);
    filled_button(root, "Done", PAD_X + half + GAP, 14.0, half, sel!(handleDoneAck:), target, TAG_DONE, theme.cta, theme.cta_ink, mtm);
}

fn centred(
    root: &NSView,
    text: &str,
    y: f64,
    size: f64,
    bold: bool,
    color: (f64, f64, f64),
    mtm: MainThreadMarker,
) -> Retained<NSTextField> {
    let field = label(root, text, PAD_X, y, content_w(), size + 8.0, size, bold, color, false, mtm);
    field.setAlignment(NSTextAlignment::Center);
    field
}

fn elapsed_text(seconds: u64) -> String {
    match seconds {
        0..=89 => format!("{}s", seconds.max(1)),
        _ => format!("{}m {}s", seconds / 60, seconds % 60),
    }
}

#[cfg(test)]
mod tests {
    use super::{elapsed_text, size_note};
    use crate::state::{DoneSummary, GroupSummary};
    use std::time::Duration;
    use wd40::scanner::ArtifactGroup;

    fn summary(freed: u64, scanned: u64) -> DoneSummary {
        DoneSummary {
            freed_bytes: freed,
            duration: Duration::from_secs(1),
            free_before: 0,
            free_after: freed,
            total_bytes: 1 << 40,
            removed: vec![GroupSummary { group: ArtifactGroup::NodeModules, count: 6, bytes: scanned }],
            skipped_count: 0,
            skipped_bytes: 0,
            troubled_count: 0,
        }
    }

    #[test]
    fn elapsed_reads_in_seconds_then_minutes() {
        assert_eq!(elapsed_text(0), "1s");
        assert_eq!(elapsed_text(48), "48s");
        assert_eq!(elapsed_text(192), "3m 12s");
    }

    /// 603.8 MB gained against 1.3 GB scanned is the case a user reads as a
    /// bug: the screen has to name both figures. Agreement needs no note.
    #[test]
    fn figures_that_disagree_are_both_named() {
        let note = size_note(&summary(633_000_000, 1_395_000_000)).expect("a note");
        assert!(note.contains("measured") && note.contains("free space rose"));
        assert!(size_note(&summary(1_000_000_000, 1_020_000_000)).is_none());
    }
}
