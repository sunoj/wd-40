// Scan-results screen: grouped selectable targets and the clean CTA. One column
// in the popover; the same column beside the usage map in the window.
// Exports: `build`, `header_model`, `membership`, the screen's tags.
// Deps: crate::{controls, header, live, menu_rows, scan_rows, state, widgets}.

use crate::layout::{self, content_w, panel_w};
use crate::usage_map;
use crate::controls::text_button_underlined;
use crate::scan_footer::{draw_footer, footer_h};
use crate::header::{self, scan_size, ScanHeader};
use crate::live::{self, Group, Row, Scan};
use crate::zone::Zone;
use crate::group_scroll::{self, GROUP_CAP};
use crate::selection::nothing_old;
use crate::menu_rows::{plan_groups, Fold, GroupPlan};
use crate::names::display_names;
use crate::scan_rows::{self, GroupModel, RowModel, GROUP_H, ROW_H};
use crate::selection::group_selection;
use crate::state::AppState;
use crate::theme::Theme;
use crate::widgets::{
    self, add_fill, add_line, label, PAD_X,
};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::sel;
use objc2_app_kit::NSView;
use objc2_foundation::MainThreadMarker;

pub const TAG_CLEAN: isize = 2000;
pub const TAG_RESCAN: isize = 2001;
pub const TAG_SETTINGS: isize = 2002;
pub const TAG_SHOW_MORE: isize = 2006;
pub const TAG_WINDOW: isize = 2007;
pub const TAG_ITEM_BASE: isize = 1000;
pub const TAG_GROUP_BASE: isize = 3000;
/// Well clear of the item tags, which run from `TAG_ITEM_BASE` one per row.
pub const TAG_REVEAL_BASE: isize = 10_000;

/// How many of its old targets a group shows in the popover before folding.
/// The line that says a group has nothing old, and the banner for the list.
const CLEAR_H: f64 = 24.0;
const BANNER_H: f64 = 86.0;
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
    let list_h = measure_list(state, &plans);
    let body_h = (MAX_HEIGHT - header::SCAN_HEADER_HEIGHT - footer_h(state)).min(list_h);
    let height = header::SCAN_HEADER_HEIGHT + body_h + footer_h(state);
    let root = widgets::root_view(panel_w(), height, theme, mtm);
    let (header, footer) = draw_frame(&root, height, state, theme, target, mtm);
    let list = crate::scrolling::scroll_document_view(&root, 0.0, footer_h(state), panel_w(), body_h, list_h, mtm);
    let (rows, groups) = draw_list(&list, state, &plans, list_h, theme, target, mtm);
    live::install_scan(Scan { theme: *theme, header, footer, rows, groups, membership: membership(state), map: None });
    (root, height)
}

/// The window: the popover's column at full height with every row in it, and
/// beside it the usage map — the same targets as tiles, sized by their bytes.
fn build_wide(state: &AppState, theme: &Theme, target: &AnyObject, width: f64, height: f64, mtm: MainThreadMarker) -> Retained<NSView> {
    let empty = state.empty_targets();
    let plans = plan_groups(&state.targets, &empty, fold(state));
    let root = widgets::root_view(width, height, theme, mtm);
    add_fill(&root, panel_w(), 0.0, 1.0, height, theme.line, 1.0, 0.0, mtm);
    let (header, footer) = draw_frame(&root, height, state, theme, target, mtm);
    let body_h = height - header::SCAN_HEADER_HEIGHT - footer_h(state);
    let list_h = measure_list(state, &plans);
    let list = crate::scrolling::scroll_document_view(&root, 0.0, footer_h(state), panel_w(), body_h, list_h, mtm);
    let (rows, groups) = draw_list(&list, state, &plans, list_h.max(body_h), theme, target, mtm);
    let frame = layout::map_frame(width, height);
    usage_map::draw_key(&root, state, frame, theme, mtm);
    let legend = label(&root, "", frame.origin.x, frame.origin.y - 26.0, frame.size.width, 16.0, 11.5, false, theme.ink_2, true, mtm);
    let map = usage_map::usage_map(frame, state, legend, theme.dark, mtm);
    root.addSubview(&map);
    live::install_scan(Scan { theme: *theme, header, footer, rows, groups, membership: membership(state), map: Some(map) });
    root
}

/// Header at the top and footer at the bottom of a popover-wide column. The
/// header shows everything found; only the clean button counts the selection.
fn draw_frame(root: &NSView, height: f64, state: &AppState, theme: &Theme, target: &AnyObject, mtm: MainThreadMarker) -> (Zone, Zone) {
    let header = Zone::new(root, 0.0, height - header::SCAN_HEADER_HEIGHT, header::SCAN_HEADER_HEIGHT, mtm);
    header.paint(|view, top| header::draw_scan_header(view, top, &header_model(state), theme, mtm));
    let footer = Zone::new(root, 0.0, 0.0, footer_h(state) + 1.0, mtm);
    footer.paint(|view, _| draw_footer(view, state, theme, target, mtm));
    (header, footer)
}

/// The popover shows a few of each group's old targets and folds the rest
/// behind a link; the window has the room, so it shows them all.
fn fold(state: &AppState) -> Fold {
    match state.show_all || crate::window::is_visible() {
        true => Fold::all(),
        false => Fold { per_group: ROWS_PER_GROUP, keep_days: Some(state.config.max_age_days) },
    }
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
        approximate: !state.reclaim.as_ref().is_some_and(|found| found.covers(&state.targets)),
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
    let list_bottom = 4.0;
    if all_clear(state) {
        draw_all_clear(root, y, state, theme, mtm);
        y -= BANNER_H;
    }
    let names = display_names(&state.targets);
    let empty = state.empty_targets();
    let max_size = state.measured.iter().filter_map(|i| state.targets.get(*i)).map(|t| t.size_bytes).max().unwrap_or(1).max(1);
    let (mut rows, mut groups) = (Vec::new(), Vec::new());
    for plan in plans {
        if y <= list_bottom + ROW_H {
            break;
        }
        let model = GroupModel {
            group: plan.group,
            count: plan.count,
            size: plan.size,
            settled: state.group_settled(plan.group),
            selection: group_selection(&state.targets, &state.selected, plan.group, &empty),
        };
        let (next, group) = scan_rows::draw_group_header(root, y, &model, theme, target, mtm);
        y = next;
        groups.push(group);
        if group_clear(state, plan) {
            let line = format!("\u{2713} Nothing older than {}d", state.config.max_age_days);
            label(root, &line, PAD_X + 46.0, y - 20.0, content_w() - 46.0, 16.0, 12.5, false, theme.ok, false, mtm);
            y -= CLEAR_H;
        }
        // A group past the cap gets a scroll view of its own to draw into.
        let shown_h = rows_height(plan);
        let all_h = plan.rows.len() as f64 * ROW_H;
        let inner = (all_h > shown_h).then(|| group_scroll::capped(root, y, shown_h, all_h, plan.group.key(), theme, mtm));
        let mut row_y = if inner.is_some() { all_h } else { y };
        for row in &plan.rows {
            let model = row_model(state, row, &names, max_size);
            let (next, drawn) = scan_rows::draw_row(inner.as_deref().unwrap_or(root), row_y, &model, theme, target, mtm);
            row_y = next;
            rows.push(drawn);
        }
        y -= shown_h;
        y = draw_show_more(root, state, plan, y, list_bottom, theme, target, mtm);
    }
    if plans.is_empty() && !state.sizing() && !crate::tasks::is_scanning() {
        label(
            root, "Nothing to clean \u{2014} everything is tidy", PAD_X, list_bottom + 20.0,
            content_w(), 20.0, 13.0, false, theme.ink_2, false, mtm,
        );
    }
    (rows, groups)
}

fn row_model<'a>(
    state: &'a AppState,
    row: &'a crate::menu_rows::RowPlan<'a>,
    names: &'a [String],
    max_size: u64,
) -> RowModel<'a> {
    RowModel {
        index: row.index,
        name: &names[row.index],
        target: row.target,
        measured: state.measured.contains(&row.index),
        max_size,
        on: state.selected.contains(&row.index),
        max_age_days: state.config.max_age_days,
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_show_more(
    root: &NSView,
    state: &AppState,
    plan: &GroupPlan<'_>,
    mut y: f64,
    list_bottom: f64,
    theme: &Theme,
    target: &AnyObject,
    mtm: MainThreadMarker,
) -> f64 {
    if plan.hidden() > 0 && y > list_bottom + 40.0 {
        text_button_underlined(
            root, &show_more_title(state, plan), PAD_X, y - 28.0, content_w(),
            sel!(handleShowMore:), target, TAG_SHOW_MORE, theme.ink_2, mtm,
        );
        y -= 40.0;
    }
    if y > list_bottom + 8.0 {
        add_line(root, PAD_X, y - 4.0, content_w(), theme.line, mtm);
        y -= 8.0;
    }
    y
}

/// What the link under a group unfolds, said as it is: old targets past the
/// row budget are "more", recent ones are "recent", and neither is called
/// smaller. The size is left off until every figure in the group is in.
fn show_more_title(state: &AppState, plan: &GroupPlan<'_>) -> String {
    let what = match (plan.more, plan.recent) {
        (more, 0) => format!("Show {more} more"),
        (0, recent) => format!("Show {recent} recent"),
        (more, recent) => format!("Show {more} more and {recent} recent"),
    };
    match state.group_settled(plan.group) {
        true => format!("{what} \u{00b7} {}", scan_size(plan.hidden_bytes)),
        false => what,
    }
}

/// A group none of whose targets is past the keep threshold: healthy.
fn group_clear(state: &AppState, plan: &GroupPlan<'_>) -> bool {
    nothing_old(&state.targets, &state.empty_targets(), Some(plan.group), state.config.max_age_days)
}

/// The whole list is healthy: said once, at its head, before the groups.
fn draw_all_clear(root: &NSView, y: f64, state: &AppState, theme: &Theme, mtm: MainThreadMarker) {
    let days = state.config.max_age_days;
    widgets::add_fill(root, PAD_X, y - BANNER_H + 8.0, content_w(), BANNER_H - 14.0, theme.ok, 0.12, 8.0, mtm);
    label(root, "\u{2713}  Nothing to clean", PAD_X + 14.0, y - 34.0, content_w() - 28.0, 20.0, 15.0, true, theme.ok, false, mtm);
    let detail = format!("Everything here was touched in the last {days} days. Recent builds stay listed below.");
    widgets::label_wrap(root, &detail, PAD_X + 14.0, y - BANNER_H + 14.0, content_w() - 28.0, 32.0, 11.5, theme.ink_2, mtm);
}

fn all_clear(state: &AppState) -> bool {
    !state.targets.is_empty() && !state.sizing() && nothing_old(&state.targets, &state.empty_targets(), None, state.config.max_age_days)
}

/// Height a group's rows take in the list: all of them, up to the cap.
fn rows_height(plan: &GroupPlan<'_>) -> f64 {
    plan.rows.len().min(GROUP_CAP) as f64 * ROW_H
}

fn measure_list(state: &AppState, plans: &[GroupPlan<'_>]) -> f64 {
    if plans.is_empty() {
        return 48.0;
    }
    let mut h = 8.0 + if all_clear(state) { BANNER_H } else { 0.0 };
    for plan in plans {
        h += GROUP_H + rows_height(plan) + if group_clear(state, plan) { CLEAR_H } else { 0.0 };
        if plan.hidden() > 0 {
            h += 40.0;
        }
        h += 8.0;
    }
    h
}
