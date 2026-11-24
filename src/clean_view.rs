// Cleaning-progress screen: disk header, job progress, and the rust plate that
// lifts a tile as each target is really removed. Stacked in the popover; in the
// window the figures form a sidebar and the plate takes the rest of the room.
// Exports: `build`, `TAG_STOP`, and the zone painters `live` calls each tick.
// Deps: crate::{can, controls, crust, header, layout, live, pace, plate, state, widgets}.

use crate::can::{can_view, STANDING};
use crate::controls::filled_button;
use crate::crust::PLATE_H;
use crate::header::{self, scan_size, HeaderModel};
use crate::layout::{self, content_w, panel_w, POPOVER_WIDTH};
use crate::live::{self, Clean, Zone};
use crate::pace::{elapsed, eta_label};
use crate::plate::{plate_view, PlateView};
use crate::state::{AppState, CleanItemStatus, CleanProgress};
use crate::theme::Theme;
use crate::widgets::{self, add_fill, add_line, label, label_right, label_wrap, PAD_X};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::sel;
use objc2_app_kit::{NSTextField, NSView};
use objc2_foundation::{MainThreadMarker, NSPoint, NSRect, NSSize, NSString};
use std::path::Path;
use wd40::disk::sum_bytes;

pub const TAG_STOP: isize = 2004;
const STRIP_H: f64 = 46.0;
/// The plate, the legend line under it, and the air around both.
const PLATE_BLOCK: f64 = PLATE_H + 46.0;
const PATH_H: f64 = 42.0;
const FOOTER_H: f64 = 78.0;
/// Room the window leaves round its plate.
const MARGIN: f64 = 24.0;

/// Everything the screen is built from, so the two layouts share one signature.
struct Parts<'a> {
    state: &'a AppState,
    progress: &'a CleanProgress,
    theme: &'a Theme,
    target: &'a AnyObject,
    mtm: MainThreadMarker,
}

pub fn build(state: &AppState, theme: &Theme, target: &AnyObject, mtm: MainThreadMarker) -> (Retained<NSView>, f64) {
    let progress = state.cleaning.clone().unwrap_or_default();
    let parts = Parts { state, progress: &progress, theme, target, mtm };
    match layout::window_size() {
        Some((width, height)) => (build_wide(&parts, width, height), height),
        None => build_stacked(&parts),
    }
}

/// The popover: header, progress, plate, path and button in one column.
fn build_stacked(parts: &Parts) -> (Retained<NSView>, f64) {
    let height = header::HEADER_HEIGHT + STRIP_H + PLATE_BLOCK + PATH_H + FOOTER_H;
    let root = widgets::root_view(panel_w(), height, parts.theme, parts.mtm);
    let (header, strip, below) = draw_figures(&root, height, parts);
    let frame = NSRect::new(NSPoint::new(PAD_X, below - PLATE_BLOCK + 34.0), NSSize::new(content_w(), PLATE_H));
    let plate = draw_plate(&root, frame, parts);
    let (caption, path) = draw_path(&root, below - PLATE_BLOCK, parts);
    let footer = draw_footer_zone(&root, parts);
    live::install_clean(Clean { theme: *parts.theme, header, strip, footer, caption, path, plate });
    (root, height)
}

/// The window: the popover's column, less the plate, as a sidebar, and the
/// plate over everything to the right of it.
fn build_wide(parts: &Parts, width: f64, height: f64) -> Retained<NSView> {
    let (theme, mtm) = (parts.theme, parts.mtm);
    let root = widgets::root_view(width, height, theme, mtm);
    let side = POPOVER_WIDTH;
    add_fill(&root, side, 0.0, 1.0, height, theme.line, 1.0, 0.0, mtm);
    let (header, strip, below) = draw_figures(&root, height, parts);
    let (caption, path) = draw_path(&root, below, parts);
    let footer = draw_footer_zone(&root, parts);
    draw_mark(&root, FOOTER_H, below - PATH_H, mtm);
    let frame = NSRect::new(
        NSPoint::new(side + MARGIN, MARGIN + 26.0),
        NSSize::new(width - side - MARGIN * 2.0, height - MARGIN * 2.0 - 26.0),
    );
    let plate = draw_plate(&root, frame, parts);
    live::install_clean(Clean { theme: *theme, header, strip, footer, caption, path, plate });
    root
}

/// The can, standing in whatever the sidebar has left between the figures and
/// the button. Left out when there is not the room for the whole of it.
fn draw_mark(root: &NSView, floor: f64, ceiling: f64, mtm: MainThreadMarker) {
    let (w, h) = STANDING.size;
    if ceiling - floor < h + 32.0 {
        return;
    }
    let at = NSPoint::new((POPOVER_WIDTH - w) / 2.0, floor + (ceiling - floor - h) / 2.0);
    root.addSubview(&can_view(STANDING, at, mtm));
}

/// Disk header and progress strip, stacked down from `top`. Returns the zones
/// and the y the next block starts at.
fn draw_figures(root: &NSView, top: f64, parts: &Parts) -> (Zone, Zone, f64) {
    let (state, progress, theme, mtm) = (parts.state, parts.progress, parts.theme, parts.mtm);
    let header_y = top - header::HEADER_HEIGHT;
    let header = Zone::new(root, 0.0, header_y, header::HEADER_HEIGHT, mtm);
    header.paint(|view, y_top| draw_disk_header(view, y_top, state, progress, theme, mtm));
    let strip = Zone::new(root, 0.0, header_y - STRIP_H, STRIP_H, mtm);
    strip.paint(|view, y_top| draw_strip(view, y_top, progress, theme, mtm));
    (header, strip, header_y - STRIP_H)
}

fn draw_footer_zone(root: &NSView, parts: &Parts) -> Zone {
    let footer = Zone::new(root, 0.0, 0.0, FOOTER_H, parts.mtm);
    footer.paint(|view, _| draw_footer(view, parts.progress, parts.theme, parts.target, parts.mtm));
    footer
}

pub fn draw_disk_header(
    root: &NSView,
    y_top: f64,
    state: &AppState,
    p: &CleanProgress,
    theme: &Theme,
    mtm: MainThreadMarker,
) {
    header::draw_header(
        root,
        y_top,
        &HeaderModel {
            disk: state.disk_stats(),
            // What this job has not taken off the disk — the freed part has
            // already moved into the free slice by the time we redraw.
            reclaimable: left_bytes(p),
            sizing: false,
            approximate: false,
            trailing: Some(format!("{} of {}", p.done_count, p.total_count)),
            detail_left: Some(match p.working() {
                true => format!("{} freed so far", scan_size(p.freed_so_far)),
                false => format!("{} freed", scan_size(p.freed_so_far)),
            }),
            detail_right: None,
        },
        theme,
        mtm,
    );
}

pub fn draw_strip(root: &NSView, y_top: f64, p: &CleanProgress, theme: &Theme, mtm: MainThreadMarker) {
    let so_far = elapsed(p);
    let total = sum_bytes(p.items.iter().map(|i| i.size_bytes));
    let done = if total == 0 { 0.0 } else { p.freed_so_far as f64 / total as f64 };
    add_fill(root, PAD_X, y_top - 22.0, content_w(), 11.0, theme.surface_3, 1.0, 3.0, mtm);
    // Nothing is drawn for nothing freed: a sliver of bar would claim a start.
    if done > 0.0 {
        add_fill(root, PAD_X, y_top - 22.0, (content_w() * done.min(1.0)).max(6.0), 11.0, theme.pos, 1.0, 3.0, mtm);
    }
    label(root, &status_line(p), PAD_X, y_top - 42.0, 230.0, 16.0, 12.5, false, theme.ink, false, mtm);
    if let Some(eta) = eta_label(p, so_far) {
        label_right(root, &eta, PAD_X + 230.0, y_top - 42.0, content_w() - 230.0, 16.0, 11.5, theme.ink_3, true, mtm);
    }
}

fn status_line(p: &CleanProgress) -> String {
    if p.total_count == 0 {
        return "Nothing to remove".to_string();
    }
    let troubled = p.troubled_count();
    if troubled > 0 && p.items.iter().all(|item| item.status.settled()) {
        return format!("{troubled} of {} could not be removed", p.total_count);
    }
    let skipped = p.items.iter().filter(|i| i.status == CleanItemStatus::Skipped).count();
    if skipped > 0 && p.items.iter().all(|item| item.status.settled()) {
        return format!("Stopped \u{2014} {skipped} left in place");
    }
    match left_bytes(p) {
        0 if p.done_count == p.total_count => format!("All {} targets clear", p.total_count),
        left => format!("{} of crust left", scan_size(left)),
    }
}

/// Bytes this job has not taken off the disk: what is still queued, what is
/// running, and whatever a part-way removal left behind.
fn left_bytes(p: &CleanProgress) -> u64 {
    sum_bytes(
        p.items
            .iter()
            .map(|item| item.size_bytes.saturating_sub(item.freed_bytes)),
    )
}

/// The plate at `frame`, with its legend line directly under it.
fn draw_plate(root: &NSView, frame: NSRect, parts: &Parts) -> Retained<PlateView> {
    let (theme, mtm) = (parts.theme, parts.mtm);
    let legend = label(
        root, "", frame.origin.x, frame.origin.y - 26.0, frame.size.width, 16.0, 11.5, false,
        theme.ink_2, true, mtm,
    );
    let view = plate_view(frame, parts.progress, parts.state.disk_stats(), legend, theme.dark, mtm);
    root.addSubview(&view);
    view
}

fn draw_path(root: &NSView, y_top: f64, parts: &Parts) -> (Retained<NSTextField>, Retained<NSTextField>) {
    let (theme, mtm) = (parts.theme, parts.mtm);
    let caption = label(root, "", PAD_X, y_top - 20.0, content_w(), 14.0, 11.0, false, theme.ink_3, true, mtm);
    let path = label(root, "", PAD_X, y_top - 38.0, content_w(), 16.0, 12.0, false, theme.ink, true, mtm);
    show_path(&caption, &path, parts.progress);
    add_line(root, 0.0, y_top - PATH_H, panel_w(), theme.line, mtm);
    (caption, path)
}

/// Several targets come off at once, so name the one most recently picked up
/// and say how many others are going with it rather than implying it is alone.
/// Once nothing is running the caption stops saying otherwise: the same path is
/// then the last target the run touched, not one being worked.
pub fn show_path(caption: &NSTextField, field: &NSTextField, p: &CleanProgress) {
    let heading = if p.working() { "REMOVING" } else { "LAST TARGET" };
    caption.setStringValue(&NSString::from_str(heading));
    let text = match p.current_path.is_empty() {
        true => "\u{2026}".to_string(),
        false => {
            let named = ellipsize(&crate::names::display_path(Path::new(&p.current_path)), 44);
            match p.active_count().saturating_sub(1) {
                0 => named,
                others => format!("{named}  +{others} more"),
            }
        }
    };
    field.setStringValue(&NSString::from_str(&text));
}

pub fn draw_footer(
    root: &NSView,
    p: &CleanProgress,
    theme: &Theme,
    target: &AnyObject,
    mtm: MainThreadMarker,
) {
    // Once the last target has settled there is nothing left to stop, and a
    // Stop button would say there was. The screen is only still up because it
    // is being shown, so the button offers the way out of it.
    match p.working() {
        true => filled_button(
            root, &stop_title(p), PAD_X, 38.0, content_w(), sel!(handleStopClean:),
            target, TAG_STOP, theme.surface_2, theme.danger, mtm,
        ),
        false => filled_button(
            root, "Show the result", PAD_X, 38.0, content_w(), sel!(handleShowResult:),
            target, TAG_STOP, theme.cta, theme.cta_ink, mtm,
        ),
    };
    label_wrap(
        root,
        "A tile lifts off the steel only when its target is really gone. Nothing reaches the Trash.",
        PAD_X, 6.0, content_w(), 30.0, 11.5, theme.ink_3, mtm,
    );
}

/// Several targets are removed at once, so the button has to name how many it
/// will let finish. Stopping starts nothing further; it never abandons a
/// removal half way, which would leave a target neither there nor gone.
fn stop_title(p: &CleanProgress) -> String {
    let active = p.active_count();
    if crate::tasks::stop_requested() {
        return match active {
            0 => "Stopping\u{2026}".to_string(),
            1 => "Stopping after this target".to_string(),
            n => format!("Stopping after these {n} targets"),
        };
    }
    match active {
        0 | 1 => "Stop after this target".to_string(),
        n => format!("Stop after these {n} targets"),
    }
}

fn ellipsize(path: &str, max_chars: usize) -> String {
    let chars: Vec<char> = path.chars().collect();
    if chars.len() <= max_chars {
        return path.to_string();
    }
    let tail: String = chars[chars.len() - (max_chars - 1)..].iter().collect();
    format!("\u{2026}{tail}")
}
