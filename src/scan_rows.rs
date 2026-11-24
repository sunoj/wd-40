// The scan list's two kinds of line: a group header and a target row. Both hand
// back the handles that let a size arrive, or a tick land, without the list
// being rebuilt underneath.
// Exports: `ROW_H`, `GROUP_H`, `draw_group_header`, `draw_row`, `show_size`,
// `show_group_size`, `show_age`.
// Deps: crate::{checkbox, header, hover_row, live, names, reveal, widgets}.

use crate::layout::panel_w;
use crate::checkbox::checkbox;
use crate::header::scan_size;
use crate::hover_row::{hover_row, HoverRow};
use crate::live::{Group, Row};
use crate::names::{age_short, display_path};
use crate::reveal::{reveal_button, SIDE as REVEAL_SIDE};
use crate::selection::GroupSelection;
use crate::theme::Theme;
use crate::widgets::{self, label, label_right, label_tracked, PAD_X};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{sel, MainThreadOnly};
use objc2_app_kit::{NSBox, NSLineBreakMode, NSTextField, NSView};
use objc2_foundation::{MainThreadMarker, NSPoint, NSRect, NSSize, NSString};
use wd40::scanner::{ArtifactGroup, TargetDir};

pub const ROW_H: f64 = 34.0;
pub const GROUP_H: f64 = 33.0;
/// Shown in place of a figure that is not known yet.
const UNKNOWN: &str = "\u{2026}";
/// Marks a total that is still being added to.
const FLOOR: &str = "\u{2265} ";
/// The gutter at the right edge: a row's reveal control while the pointer is
/// on it, a group's disclosure mark. Every size reads up to it, on one edge.
fn reveal_x() -> f64 {
    panel_w() - PAD_X - REVEAL_SIDE
}
const SIZE_W: f64 = 92.0 - REVEAL_SIDE;
fn size_x() -> f64 {
    reveal_x() - 6.0 - SIZE_W
}
/// The second line, which the pointer turns into the row's path.
const DETAIL_X: f64 = PAD_X + 26.0;
/// Thickness of the size meter along the bottom of a row.
const METER_H: f64 = 2.5;
fn name_w() -> f64 {
    size_x() - DETAIL_X - 4.0
}

pub struct GroupModel {
    pub group: ArtifactGroup,
    pub count: usize,
    /// Rows on show under the header; short of `count` while it is folded.
    pub shown: usize,
    pub size: u64,
    /// Every target in the group has a settled size.
    pub settled: bool,
    pub selection: GroupSelection,
    /// Nothing in the group is past the keep threshold, and that is news:
    /// another group has something that is.
    pub clear: bool,
    /// Whether the header folds its rows, and which way it is now.
    pub fold: Option<bool>,
}

pub struct RowModel<'a> {
    pub index: usize,
    pub name: &'a str,
    pub target: &'a TargetDir,
    pub measured: bool,
    /// Largest settled size on screen, which the meter is drawn against.
    pub max_size: u64,
    pub on: bool,
}

/// A figure, or the mark that says there is not one yet.
pub fn show_size(field: &NSTextField, bytes: Option<u64>) {
    let text = bytes.map_or_else(|| UNKNOWN.to_string(), scan_size);
    field.setStringValue(&NSString::from_str(&text));
}

/// A group total. Until every member has been measured it is a floor, and it
/// is drawn as one rather than as an answer that happens to be low.
pub fn show_group_size(field: &NSTextField, bytes: u64, settled: bool) {
    let prefix = if settled { "" } else { FLOOR };
    field.setStringValue(&NSString::from_str(&format!("{prefix}{}", scan_size(bytes))));
}

/// The kind · age line. A reopen calls this so "2d ago" can become "3d ago"
/// without rebuilding the row. Age is the row's one sign of old or recent.
pub fn show_age(field: &NSTextField, target: &TargetDir) {
    field.setStringValue(&NSString::from_str(&meta(target)));
}

/// The meter under a row: a thin bar from the text's left edge, as long as the
/// row's size is of the largest on screen. It replaces a wash behind the whole
/// row, which stepped down the list like a staircase and ran off its edge.
pub fn wash_frame(bytes: u64, max_size: u64) -> NSRect {
    let fraction = bytes as f64 / max_size.max(1) as f64;
    let width = (name_w() * fraction.clamp(0.0, 1.0)).max(METER_H);
    NSRect::new(NSPoint::new(DETAIL_X, 0.0), NSSize::new(width, METER_H))
}

/// One line per group: tick box, name, count, total, and the mark that says
/// its rows unfold. A header that folds is a row of its own, so the whole line
/// takes the click; the tick box keeps its own.
pub fn draw_group_header(
    parent: &NSView,
    y_top: f64,
    model: &GroupModel,
    theme: &Theme,
    target: &AnyObject,
    mtm: MainThreadMarker,
) -> (f64, Group) {
    let y = y_top - GROUP_H;
    let index = ArtifactGroup::ALL.iter().position(|g| *g == model.group).unwrap_or(0);
    let frame = NSRect::new(NSPoint::new(0.0, y), NSSize::new(panel_w(), GROUP_H));
    let host: Retained<NSView> = match model.fold {
        Some(_) => Retained::into_super(hover_row(parent, y, GROUP_H, theme.surface_2, crate::scan_view::TAG_FOLD_BASE + index as isize, mtm)),
        None => {
            let plain = NSView::initWithFrame(NSView::alloc(mtm), frame);
            parent.addSubview(&plain);
            plain
        }
    };
    let check = checkbox(
        &host, model.selection, PAD_X, 6.0, sel!(handleToggleGroup:), target,
        crate::scan_view::TAG_GROUP_BASE + index as isize, theme, mtm,
    );
    // Boxes centre on 13.5; all-caps text carries descender space it never
    // uses, so its frame sits a point lower to land on the same optical line.
    // The chip follows the title, however long the group's name is.
    let name = title(model.group);
    let title_w = widgets::fitted_width(name, 11.5, true, mtm) + name.len() as f64 * 0.69 + 4.0;
    let chip_x = DETAIL_X + title_w + 6.0;
    label_tracked(&host, name, DETAIL_X, 4.0, title_w, 17.0, 11.5, false, theme.ink_3, true, 0.69, mtm);
    // As wide as its figure. A folded group with rows on show says how many.
    let count = match model.shown > 0 && model.shown < model.count {
        true => format!("{} of {}", model.shown, model.count),
        false => model.count.to_string(),
    };
    let count_w = widgets::fitted_width(&count, 10.5, true, mtm);
    widgets::add_fill(&host, chip_x, 5.0, count_w + 10.0, 17.0, theme.surface_3, 1.0, 4.0, mtm);
    label(&host, &count, chip_x + 5.0, 6.0, count_w + 2.0, 15.0, 10.5, false, theme.ink_3, true, mtm);
    if model.clear {
        label(&host, "\u{2713}", chip_x + 18.0 + count_w, 5.0, 16.0, 16.0, 12.5, true, theme.ok, false, mtm);
    }
    let size = label_right(&host, "", size_x(), 4.0, SIZE_W, 17.0, 12.0, theme.ink_2, true, mtm);
    show_group_size(&size, model.size, model.settled);
    if let Some(open) = model.fold {
        let mark = if open { "chevron.down" } else { "chevron.right" };
        widgets::symbol_view(&host, mark, reveal_x() + 4.0, 7.5, 11.0, theme.ink_3, mtm);
    }
    (y, Group { group: model.group, size, check })
}

fn title(group: ArtifactGroup) -> &'static str {
    match group {
        ArtifactGroup::Rust => "RUST TARGETS",
        ArtifactGroup::NodeModules => "NODE MODULES",
        ArtifactGroup::BuildOutput => "BUILD OUTPUT",
        ArtifactGroup::Caches => "CACHES",
        ArtifactGroup::Simulators => "SIMULATOR DEVICES",
        ArtifactGroup::Toolchains => "RUST TOOLCHAINS",
    }
}

pub fn draw_row(
    parent: &NSView,
    y_top: f64,
    model: &RowModel<'_>,
    theme: &Theme,
    target: &AnyObject,
    mtm: MainThreadMarker,
) -> (f64, Row) {
    let y = y_top - ROW_H;
    let tag = crate::scan_view::TAG_ITEM_BASE + model.index as isize;
    let row = hover_row(parent, y, ROW_H, theme.surface_2, tag, mtm);
    let wash = draw_wash(&row, model, theme, mtm);
    let check = checkbox(&row, GroupSelection::from(model.on), PAD_X, 9.5, sel!(handleToggleItem:), target, tag, theme, mtm);
    let name = label(&row, model.name, DETAIL_X, 15.0, name_w(), 16.0, 13.5, false, theme.ink, false, mtm);
    // A device or project name can outrun the column; ending it in an ellipsis
    // says so, where a hard clip reads as a name that happens to look odd.
    name.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
    let age = draw_detail(&row, model, theme, mtm);
    let size = label_right(&row, "", size_x(), 9.0, SIZE_W, 16.0, 13.0, theme.ink, true, mtm);
    show_size(&size, model.measured.then_some(model.target.size_bytes));
    // Its own action, so `HoverRow::hitTest` leaves the click with it: opening
    // Finder must not also tick the row it sits in. On show only under the
    // pointer: a folder on every row was a column of noise.
    let reveal = reveal_button(
        &row, reveal_x(), (ROW_H - REVEAL_SIDE) / 2.0, sel!(handleRevealItem:), target,
        crate::scan_view::TAG_REVEAL_BASE + model.index as isize, theme.ink_2, mtm,
    );
    if let Some(reveal) = reveal {
        row.show_on_hover(&reveal);
    }
    (y, Row { index: model.index, size, check, wash, age })
}

/// The line under the name: what the row is, and — while the pointer is on it —
/// which directory it is. A path is longer than the line, so it is cut in the
/// middle, where a path carries the least; the tooltip holds the whole of it.
fn draw_detail(
    row: &HoverRow,
    model: &RowModel<'_>,
    theme: &Theme,
    mtm: MainThreadMarker,
) -> Retained<NSTextField> {
    let width = size_x() - DETAIL_X - 6.0;
    let rest = meta(model.target);
    let path = display_path(&model.target.path);
    let field = label(row, &rest, DETAIL_X, 2.0, width, 14.0, 11.0, false, theme.ink_3, true, mtm);
    field.setUsesSingleLineMode(true);
    field.setLineBreakMode(NSLineBreakMode::ByTruncatingHead);
    row.set_detail(&field, &rest, &path);
    row.setToolTip(Some(&NSString::from_str(&model.target.path.to_string_lossy())));
    field
}

fn draw_wash(
    row: &NSView,
    model: &RowModel<'_>,
    theme: &Theme,
    mtm: MainThreadMarker,
) -> Retained<NSBox> {
    let bytes = if model.measured { model.target.size_bytes } else { 0 };
    let frame = wash_frame(bytes, model.max_size);
    // Kept for the live handle, not shown: under every row it read as noise.
    let meter = widgets::add_fill(row, frame.origin.x, frame.origin.y, frame.size.width, METER_H, theme.accent, 0.75, METER_H / 2.0, mtm);
    meter.setHidden(true);
    meter
}

/// What the row is and how long since it was touched. A cache also says what
/// removing it costs: a download, where a build only costs a rebuild.
fn meta(target: &TargetDir) -> String {
    let aged = format!("{} \u{00b7} {}", target.kind.label(), age_short(target.last_modified));
    match target.kind.group() {
        ArtifactGroup::Caches => format!("{aged} \u{00b7} re-downloads"),
        _ => aged,
    }
}

#[cfg(test)]
mod tests {
    use super::meta;
    use std::path::PathBuf;
    use std::time::{Duration, SystemTime};
    use wd40::scanner::{ArtifactKind, TargetDir};

    fn target(kind: ArtifactKind, modified: SystemTime) -> TargetDir {
        TargetDir {
            path: PathBuf::from("/w/a/target"),
            size_bytes: 1,
            last_modified: modified,
            kind,
        }
    }

    #[test]
    fn a_row_age_moves_from_today_to_days() {
        let now = SystemTime::now();
        assert_eq!(meta(&target(ArtifactKind::RustTarget, now)), "target \u{00b7} today");
        let three = now.checked_sub(Duration::from_secs(3 * 86_400)).expect("clock");
        assert_eq!(meta(&target(ArtifactKind::RustTarget, three)), "target \u{00b7} 3d ago");
    }

    #[test]
    fn a_cache_row_says_its_age_and_what_removing_it_costs() {
        let line = meta(&target(ArtifactKind::Cache, SystemTime::now()));
        assert_eq!(line, "cache \u{00b7} today \u{00b7} re-downloads");
    }
}
