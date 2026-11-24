// Scan-results screen: grouped selectable targets and the clean CTA, or the
// all-clear screen when nothing is old. One column in the popover; the same
// column beside the usage map in the window. Exports: `build`, `header_model`,
// `membership`, tags. Deps: crate::{header, live, menu_rows, scan_rows, scan_verdict}.

use crate::group_scroll::{self, GROUP_CAP};
use crate::header::{self, ScanHeader, SCAN_HEADER_HEIGHT};
use crate::layout::{self, content_w, panel_w};
use crate::live::{self, Group, Row, Scan};
use crate::menu_rows::{plan_groups, Fold, GroupPlan};
use crate::names::display_names;
use crate::scan_footer::{draw_footer, footer_h};
use crate::scan_rows::{self, GroupModel, RowModel, GROUP_H, ROW_H};
use crate::scan_verdict::{self, VERDICT_H};
use crate::selection::{group_selection, nothing_old};
use crate::state::AppState;
use crate::theme::Theme;
use crate::usage_map;
use crate::widgets::{self, add_fill, add_line, label, PAD_X};
use crate::zone::Zone;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_app_kit::NSView;
use objc2_foundation::MainThreadMarker;
use wd40::scanner::ArtifactGroup;

pub const TAG_CLEAN: isize = 2000;
pub const TAG_RESCAN: isize = 2001;
pub const TAG_SETTINGS: isize = 2002;
pub const TAG_WINDOW: isize = 2007;
pub const TAG_ITEM_BASE: isize = 1000;
pub const TAG_GROUP_BASE: isize = 3000;
/// Well clear of the item tags, which run from `TAG_ITEM_BASE` one per row.
pub const TAG_REVEAL_BASE: isize = 10_000;
/// A header row that folds: one per group, then the all-clear screen's own.
pub const TAG_FOLD_BASE: isize = 100_000;

/// How many of its old targets a group shows in the popover while folded.
const ROWS_PER_GROUP: usize = 3;
const MAX_HEIGHT: f64 = 680.0;

pub fn build(state: &AppState, theme: &Theme, target: &AnyObject, mtm: MainThreadMarker) -> (Retained<NSView>, f64) {
    match layout::window_size() {
        Some((width, height)) => (build_wide(state, theme, target, width, height, mtm), height),
        None => build_stacked(state, theme, target, mtm),
    }
}

/// The popover: header, a capped list, and the footer in one column.
fn build_stacked(state: &AppState, theme: &Theme, target: &AnyObject, mtm: MainThreadMarker) -> (Retained<NSView>, f64) {
    let empty = state.empty_targets();
    let plans = plan_groups(&state.targets, &empty, fold(state));
    let list_h = measure_list(&plans);
    let body_h = (MAX_HEIGHT - SCAN_HEADER_HEIGHT - footer_h(state)).min(list_h);
    let height = SCAN_HEADER_HEIGHT + body_h + footer_h(state);
    let root = widgets::root_view(panel_w(), height, theme, mtm);
    let frame = draw_frame(&root, height, state, theme, target, mtm);
    let list = crate::scrolling::scroll_document_view(&root, 0.0, footer_h(state), panel_w(), body_h, list_h, mtm);
    let (rows, groups) = draw_list(&list, state, &plans, list_h, theme, target, mtm);
    install(state, theme, frame, rows, groups, None);
    (root, height)
}

/// The window: the popover's column at full height with every row in it, and
/// beside it the usage map — the same targets as tiles, sized by their bytes.
fn build_wide(state: &AppState, theme: &Theme, target: &AnyObject, width: f64, height: f64, mtm: MainThreadMarker) -> Retained<NSView> {
    let empty = state.empty_targets();
    let plans = plan_groups(&state.targets, &empty, fold(state));
    let root = widgets::root_view(width, height, theme, mtm);
    add_fill(&root, panel_w(), 0.0, 1.0, height, theme.line, 1.0, 0.0, mtm);
    let frame = draw_frame(&root, height, state, theme, target, mtm);
    let body_h = height - SCAN_HEADER_HEIGHT - footer_h(state);
    let list_h = measure_list(&plans);
    let list = crate::scrolling::scroll_document_view(&root, 0.0, footer_h(state), panel_w(), body_h, list_h, mtm);
    let (rows, groups) = draw_list(&list, state, &plans, list_h.max(body_h), theme, target, mtm);
    let map_frame = layout::map_frame(width, height);
    usage_map::draw_key(&root, state, map_frame, theme, mtm);
    let legend = label(&root, "", map_frame.origin.x, map_frame.origin.y - 26.0, map_frame.size.width, 16.0, 11.5, false, theme.ink_2, true, mtm);
    let map = usage_map::usage_map(map_frame, state, legend, theme.dark, mtm);
    root.addSubview(&map);
    install(state, theme, frame, rows, groups, Some(map));
    root
}

fn install(state: &AppState, theme: &Theme, frame: (Zone, Zone, Zone), rows: Vec<Row>, groups: Vec<Group>, map: Option<Retained<usage_map::MapView>>) {
    let (header, verdict, footer) = frame;
    live::install_scan(Scan { theme: *theme, header, verdict, footer, rows, groups, membership: membership(state), map });
}

/// Header at the top and footer at the bottom of a popover-wide column, with
/// the verdict line as a zone of its own along the header's lower edge. The
/// header shows everything found; only the clean button counts the selection.
fn draw_frame(root: &NSView, height: f64, state: &AppState, theme: &Theme, target: &AnyObject, mtm: MainThreadMarker) -> (Zone, Zone, Zone) {
    let header = Zone::new(root, 0.0, height - SCAN_HEADER_HEIGHT, SCAN_HEADER_HEIGHT, mtm);
    header.paint(|view, top| header::draw_scan_header(view, top, &header_model(state), theme, mtm));
    let verdict = Zone::new(root, 0.0, height - SCAN_HEADER_HEIGHT + 1.0, VERDICT_H, mtm);
    verdict.paint(|view, _| scan_verdict::draw(view, &scan_verdict::verdict(state), theme, target, mtm));
    let footer = Zone::new(root, 0.0, 0.0, footer_h(state) + 1.0, mtm);
    footer.paint(|view, _| draw_footer(view, state, theme, target, mtm));
    (header, verdict, footer)
}

/// The popover shows a few of each group's old targets and folds the rest
/// under the group's header until it is clicked; the window has the room, so
/// it shows them all. Either way a group's old targets come first.
fn fold(state: &AppState) -> Fold {
    let days = Some(state.config.max_age_days);
    if crate::window::is_visible() {
        return Fold { keep_days: days, ..Fold::all() };
    }
    let mut fold = Fold::folded(ROWS_PER_GROUP, days);
    for (at, group) in ArtifactGroup::ALL.iter().enumerate() {
        fold.open[at] = state.open.contains(group.key());
    }
    fold
}

/// What this state would put on the scan list. Compared to the tree we last
/// built so an open can reuse it only when the rows and counts still match.
pub fn membership(state: &AppState) -> crate::menu_rows::Membership {
    let empty = state.empty_targets();
    crate::menu_rows::Membership::of(&state.targets, &empty, fold(state))
}

pub fn header_model(state: &AppState) -> ScanHeader {
    ScanHeader {
        disk: state.disk_stats(),
        reclaimable: state.total_size(),
        measured: state.measured.len(),
        total: state.targets.len(),
        discovering: crate::tasks::is_discovering(),
        all_recent: all_clear(state),
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_list(
    root: &NSView,
    state: &AppState,
    plans: &[GroupPlan<'_>],
    mut y: f64,
    theme: &Theme,
    target: &AnyObject,
    mtm: MainThreadMarker,
) -> (Vec<Row>, Vec<Group>) {
    let names = display_names(&state.targets);
    let max_size = state.measured.iter().filter_map(|i| state.targets.get(*i)).map(|t| t.size_bytes).max().unwrap_or(1).max(1);
    let (mut rows, mut groups) = (Vec::new(), Vec::new());
    for plan in plans {
        let (next, group) = scan_rows::draw_group_header(root, y, &group_model(state, plan), theme, target, mtm);
        y = next;
        groups.push(group);
        // A group past the cap gets a scroll view of its own to draw into.
        let shown_h = rows_height(plan);
        let all_h = plan.rows.len() as f64 * ROW_H;
        let inner = (all_h > shown_h).then(|| group_scroll::capped(root, y, shown_h, all_h, plan.group.key(), theme, mtm));
        let mut row_y = if inner.is_some() { all_h } else { y };
        for row in &plan.rows {
            let model = RowModel {
                index: row.index,
                name: &names[row.index],
                target: row.target,
                measured: state.measured.contains(&row.index),
                max_size,
                on: state.selected.contains(&row.index),
            };
            let (next, drawn) = scan_rows::draw_row(inner.as_deref().unwrap_or(root), row_y, &model, theme, target, mtm);
            row_y = next;
            rows.push(drawn);
        }
        y -= shown_h + GROUP_GAP;
        add_line(root, PAD_X, y + GROUP_GAP / 2.0, content_w(), theme.line, mtm);
    }
    if plans.is_empty() && !state.sizing() && !crate::tasks::is_scanning() {
        label(root, "Nothing to clean \u{2014} everything is tidy", PAD_X, 24.0, content_w(), 20.0, 13.0, false, theme.ink_2, false, mtm);
    }
    (rows, groups)
}

/// A group's header line. In the popover it folds whenever there is something
/// folded under it or it has been unfolded; in the window nothing folds.
fn group_model(state: &AppState, plan: &GroupPlan<'_>) -> GroupModel {
    let empty = state.empty_targets();
    let open = state.open.contains(plan.group.key());
    let folds = !crate::window::is_visible() && (open || plan.hidden() > 0);
    let clear = nothing_old(&state.targets, &empty, Some(plan.group), state.config.max_age_days);
    GroupModel {
        group: plan.group,
        count: plan.count,
        shown: plan.rows.len(),
        size: plan.size,
        settled: state.group_settled(plan.group),
        // The verdict line already says so when every group is clear.
        clear: clear && !all_clear(state),
        selection: group_selection(&state.targets, &state.selected, plan.group, &empty),
        fold: folds.then_some(open),
    }
}

/// Nothing listed is past the keep threshold: the screen says so, once.
fn all_clear(state: &AppState) -> bool {
    !state.targets.is_empty() && !state.sizing() && nothing_old(&state.targets, &state.empty_targets(), None, state.config.max_age_days)
}

/// Height a group's rows take in the list: all of them, up to the cap.
fn rows_height(plan: &GroupPlan<'_>) -> f64 {
    plan.rows.len().min(GROUP_CAP) as f64 * ROW_H
}

/// Between one group's last line and the next group's header, rule included.
const GROUP_GAP: f64 = 8.0;

fn measure_list(plans: &[GroupPlan<'_>]) -> f64 {
    if plans.is_empty() {
        return 48.0;
    }
    4.0 + plans.iter().map(|plan| GROUP_H + rows_height(plan) + GROUP_GAP).sum::<f64>()
}
