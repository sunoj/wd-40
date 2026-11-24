// Done-summary popover body: the polished medal, the space actually recovered,
// and what each group gave back. No rebuild-time estimate — nothing here can
// know it. Exports: `build`.
// Deps: crate::{controls, header, medal, state, theme, widgets}, objc2, wd40.

use crate::layout::{content_w, panel_w};
use crate::controls::{filled_button, text_button};
use crate::header::scan_size;
use crate::medal::{medal, start_sheen, stop_sheen};
use crate::state::{AppState, DoneSummary};
use crate::theme::Theme;
use crate::widgets::{
    self, add_fill, add_line, label, label_right, PAD_X,
};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::sel;
use objc2_app_kit::{NSTextAlignment, NSTextField, NSView};
use objc2_foundation::{MainThreadMarker, NSPoint, NSRect, NSSize};
use wd40::scanner::ArtifactGroup;

pub const TAG_DONE: isize = 2003;
const DISC: f64 = 76.0;
const HERO_H: f64 = 224.0;
const ROW_H: f64 = 34.0;
const FOOTER_H: f64 = 74.0;

pub fn build(state: &AppState, theme: &Theme, target: &AnyObject, mtm: MainThreadMarker) -> (Retained<NSView>, f64) {
    stop_sheen();
    let summary = state.done.as_ref();
    let rows = summary.map_or(0, |s| {
        s.removed.len() + usize::from(s.skipped_count > 0) + usize::from(s.troubled_count > 0)
    });
    let height = match summary {
        Some(_) => HERO_H + rows as f64 * ROW_H + FOOTER_H,
        None => 64.0 + FOOTER_H,
    };
    let root = widgets::root_view(panel_w(), height, theme, mtm);

    match summary {
        Some(s) => {
            draw_hero(&root, height, s, theme, mtm);
            let mut y = height - HERO_H;
            for row in &s.removed {
                y = draw_row(&root, y, row.count, ArtifactGroup::title(row.group), &scan_size(row.bytes), false, theme, mtm);
            }
            if s.skipped_count > 0 {
                y = draw_row(&root, y, s.skipped_count, "Left in place \u{2014} not selected", &scan_size(s.skipped_bytes), true, theme, mtm);
            }
            // A target that would not come off, or only came off part way, is
            // still on disk. Saying so is the whole point of the screen.
            if s.troubled_count > 0 {
                y = draw_row(&root, y, s.troubled_count, "Still there \u{2014} could not be removed", "", true, theme, mtm);
            }
            let _ = y;
        }
        None => {
            label(&root, "Nothing was removed", PAD_X, height - 44.0, content_w(), 22.0, 15.0, false, theme.ink_2, false, mtm);
        }
    }
    draw_footer(&root, theme, target, mtm);
    (root, height)
}

fn draw_hero(root: &NSView, y_top: f64, s: &DoneSummary, theme: &Theme, mtm: MainThreadMarker) {
    add_fill(root, 0.0, y_top - HERO_H, panel_w(), HERO_H, theme.pos, 0.05, 0.0, mtm);
    add_fill(root, 0.0, y_top - 104.0, panel_w(), 104.0, theme.pos, 0.04, 0.0, mtm);

    let disc = medal(NSRect::new(NSPoint::new((panel_w() - DISC) / 2.0, y_top - 94.0), NSSize::new(DISC, DISC)), theme.dark, mtm);
    root.addSubview(&disc);
    start_sheen(&disc);

    centred(root, &format!("+{}", scan_size(s.freed_bytes)), y_top - 134.0, 32.0, true, theme.ink, mtm);
    let caption = format!("of space back, in {}", elapsed_text(s.duration.as_secs()));
    centred(root, &caption, y_top - 156.0, 14.0, false, theme.ink_2, mtm);

    draw_bar(root, y_top - 182.0, s, theme, mtm);
    label(root, &format!("{} free", scan_size(s.free_after)), PAD_X, y_top - 204.0, 180.0, 18.0, 13.0, false, theme.ink, false, mtm);
    label_right(root, &format!("was {}", scan_size(s.free_before)), PAD_X + 180.0, y_top - 203.0, content_w() - 180.0, 16.0, 11.5, theme.ink_3, true, mtm);
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


fn draw_footer(root: &NSView, theme: &Theme, target: &AnyObject, mtm: MainThreadMarker) {
    add_line(root, 0.0, FOOTER_H, panel_w(), theme.line, mtm);
    filled_button(root, "Done", PAD_X, 30.0, content_w(), sel!(handleDoneAck:), target, TAG_DONE, theme.cta, theme.cta_ink, mtm);
    text_button(root, "Rescan", PAD_X, 6.0, 60.0, sel!(handleRescan:), target, crate::scan_view::TAG_RESCAN, theme.ink_2, mtm);
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
    use super::elapsed_text;

    #[test]
    fn elapsed_reads_in_seconds_then_minutes() {
        assert_eq!(elapsed_text(0), "1s");
        assert_eq!(elapsed_text(48), "48s");
        assert_eq!(elapsed_text(192), "3m 12s");
    }
}
