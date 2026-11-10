// The scan list's two kinds of line: a group header and a target row. Both hand
// back the handles that let a size arrive, or a tick land, without the list
// being rebuilt underneath.
// Exports: `ROW_H`, `GROUP_H`, `draw_group_header`, `draw_row`, `show_size`,
// `show_group_size`, `show_age`.
// Deps: crate::{checkbox, header, hover_row, live, names, reveal, widgets}.

use crate::checkbox::checkbox;
use crate::header::scan_size;
use crate::hover_row::{hover_row, HoverRow};
use crate::live::{Group, Row};
use crate::names::{age_short, display_path};
use crate::reveal::{reveal_button, SIDE as REVEAL_SIDE};
use crate::selection::{is_recent, GroupSelection};
use crate::theme::Theme;
use crate::widgets::{self, label, label_right, label_tracked, PAD_X, POPOVER_WIDTH};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{sel, MainThreadOnly};
use objc2_app_kit::{NSBox, NSLineBreakMode, NSTextField, NSView};
use objc2_foundation::{MainThreadMarker, NSPoint, NSRect, NSSize, NSString};
use wd40::scanner::{ArtifactGroup, TargetDir};

pub const ROW_H: f64 = 40.0;
pub const GROUP_H: f64 = 33.0;
/// Shown in place of a figure that is not known yet.
const UNKNOWN: &str = "\u{2026}";
/// Marks a total that is still being added to.
const FLOOR: &str = "\u{2265} ";
/// The reveal control sits at the row's right edge; the size reads up to it.
const REVEAL_X: f64 = POPOVER_WIDTH - PAD_X - REVEAL_SIDE;
const SIZE_X: f64 = PAD_X + 250.0;
const SIZE_W: f64 = REVEAL_X - 6.0 - SIZE_X;
/// The second line, which the pointer turns into the row's path. It stops short
/// of the RECENT badge where there is one, and of the size where there is not.
const DETAIL_X: f64 = PAD_X + 26.0;
const NAME_W: f64 = 220.0;
const DETAIL_W_BADGE: f64 = 150.0;
const DETAIL_W: f64 = SIZE_X - DETAIL_X - 6.0;

pub struct GroupModel {
    pub group: ArtifactGroup,
    pub count: usize,
    pub size: u64,
    /// Every target in the group has a settled size.
    pub settled: bool,
    pub selection: GroupSelection,
}

pub struct RowModel<'a> {
    pub index: usize,
    pub name: &'a str,
    pub target: &'a TargetDir,
    pub measured: bool,
    /// Largest settled size on screen, which the wash is drawn against.
    pub max_size: u64,
    pub on: bool,
    pub max_age_days: u64,
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

/// The kind · age line and the RECENT badge that shares its clock. A reopen
/// calls this so "2d ago" can become "3d ago" and the badge can leave with it,
/// without rebuilding the row.
pub fn show_age(field: &NSTextField, badge: &NSView, target: &TargetDir, max_age_days: u64) {
    field.setStringValue(&NSString::from_str(&meta(target)));
    let recent = is_recent(target, max_age_days);
    badge.setHidden(!recent);
    crate::hover_row::fit_detail(field, detail_width(recent));
}

fn detail_width(recent: bool) -> f64 {
    if recent { DETAIL_W_BADGE } else { DETAIL_W }
}

/// How wide the size wash is for `bytes` against the largest row on screen.
pub fn wash_frame(bytes: u64, max_size: u64) -> NSRect {
    let fraction = bytes as f64 / max_size.max(1) as f64;
    let width = (POPOVER_WIDTH * fraction.clamp(0.0, 1.0)).max(0.5);
    NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(width, ROW_H))
}

pub fn draw_group_header(
    parent: &NSView,
    y_top: f64,
    model: &GroupModel,
    theme: &Theme,
    target: &AnyObject,
    mtm: MainThreadMarker,
) -> (f64, Group) {
    let y = y_top - GROUP_H;
    // The mock draws no checkbox here, but selecting a whole group at once is a
    // requirement of its own; the header is the only place it can live.
    let index = ArtifactGroup::ALL.iter().position(|g| *g == model.group).unwrap_or(0);
    let check = checkbox(
        parent, model.selection, PAD_X, y + 6.0, sel!(handleToggleGroup:), target,
        crate::scan_view::TAG_GROUP_BASE + index as isize, theme, mtm,
    );
    widgets::symbol_view(parent, model.group.symbol(), PAD_X + 23.0, y + 6.0, 15.0, theme.ink_3, mtm);
    // Boxes centre on y + 13.5; all-caps text carries descender space it never
    // uses, so its frame sits a point lower to land on the same optical line.
    label_tracked(parent, title(model.group), PAD_X + 46.0, y + 4.0, 120.0, 17.0, 11.5, false, theme.ink_3, true, 0.69, mtm);
    widgets::add_fill(parent, PAD_X + 166.0, y + 5.0, 28.0, 17.0, theme.surface_2, 1.0, 4.0, mtm);
    label(parent, &model.count.to_string(), PAD_X + 171.0, y + 6.0, 18.0, 15.0, 10.5, false, theme.ink_3, true, mtm);
    let size = label_right(parent, "", PAD_X + 203.0, y + 4.0, widgets::CONTENT_WIDTH - 203.0, 17.0, 12.0, theme.ink_2, true, mtm);
    show_group_size(&size, model.size, model.settled);
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
    // Full row height, not inset. The mock insets it because nothing else sits
    // behind the row; here the hover tint does, and a shorter wash on top of it
    // banded every row into three tones.
    let wash = draw_wash(&row, model, theme, mtm);
    let check = checkbox(&row, GroupSelection::from(model.on), PAD_X, 13.0, sel!(handleToggleItem:), target, tag, theme, mtm);
    let name = label(&row, model.name, DETAIL_X, 18.0, NAME_W, 16.0, 13.5, false, theme.ink, false, mtm);
    // A device or project name can outrun the column; ending it in an ellipsis
    // says so, where a hard clip reads as a name that happens to look odd.
    name.setLineBreakMode(NSLineBreakMode::ByTruncatingTail);
    let recent = is_recent(model.target, model.max_age_days);
    let age = draw_detail(&row, model, recent, theme, mtm);
    let badge = draw_recent_badge(&row, theme, mtm);
    badge.setHidden(!recent);
    let size = label_right(&row, "", SIZE_X, 12.0, SIZE_W, 16.0, 13.0, theme.ink, true, mtm);
    show_size(&size, model.measured.then_some(model.target.size_bytes));
    // Its own action, so `HoverRow::hitTest` leaves the click with it: opening
    // Finder must not also tick the row it sits in.
    reveal_button(
        &row, REVEAL_X, (ROW_H - REVEAL_SIDE) / 2.0, sel!(handleRevealItem:), target,
        crate::scan_view::TAG_REVEAL_BASE + model.index as isize, theme.ink_3, mtm,
    );
    (y, Row { index: model.index, size, check, wash, age, badge })
}

/// The line under the name: what the row is, and — while the pointer is on it —
/// which directory it is. A path is longer than the line, so it is cut in the
/// middle, where a path carries the least; the tooltip holds the whole of it.
fn draw_detail(
    row: &HoverRow,
    model: &RowModel<'_>,
    recent: bool,
    theme: &Theme,
    mtm: MainThreadMarker,
) -> Retained<NSTextField> {
    let width = if recent { DETAIL_W_BADGE } else { DETAIL_W };
    let rest = meta(model.target);
    let path = display_path(&model.target.path);
    let field = label(row, &rest, DETAIL_X, 4.0, width, 14.0, 11.0, false, theme.ink_3, true, mtm);
    field.setUsesSingleLineMode(true);
    field.setLineBreakMode(NSLineBreakMode::ByTruncatingMiddle);
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
    let frame = match model.measured {
        true => wash_frame(model.target.size_bytes, model.max_size),
        false => NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(0.5, ROW_H)),
    };
    widgets::add_fill(row, 0.0, 0.0, frame.size.width, ROW_H, theme.accent, 0.09, 0.0, mtm)
}

fn meta(target: &TargetDir) -> String {
    if target.kind.group() == ArtifactGroup::Caches {
        return format!("{} \u{00b7} network downloads to rebuild", target.kind.label());
    }
    format!("{} \u{00b7} {}", target.kind.label(), age_short(target.last_modified))
}

fn draw_recent_badge(row: &NSView, theme: &Theme, mtm: MainThreadMarker) -> Retained<NSView> {
    // One host so a reopen can hide the fill and the word together.
    let host = NSView::initWithFrame(
        NSView::alloc(mtm),
        NSRect::new(NSPoint::new(PAD_X + 182.0, 3.0), NSSize::new(58.0, 17.0)),
    );
    let badge = widgets::add_fill(&host, 0.0, 0.0, 50.0, 17.0, theme.surface, 1.0, 3.0, mtm);
    badge.setBorderWidth(1.0);
    badge.setBorderColor(&Theme::color_alpha(theme.warn, 0.35));
    label_tracked(&host, "RECENT", 4.0, 2.0, 54.0, 13.0, 10.0, false, theme.warn, true, 0.4, mtm);
    row.addSubview(&host);
    host
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
    fn a_cache_row_has_no_age_to_go_stale() {
        let line = meta(&target(ArtifactKind::Cache, SystemTime::UNIX_EPOCH));
        assert_eq!(line, "cache \u{00b7} network downloads to rebuild");
    }

    #[test]
    fn aging_out_of_recent_widens_the_detail_to_the_badge_slot() {
        assert!(super::DETAIL_W > super::DETAIL_W_BADGE);
        assert_eq!(super::detail_width(true), super::DETAIL_W_BADGE);
        assert_eq!(super::detail_width(false), super::DETAIL_W);
    }
}
