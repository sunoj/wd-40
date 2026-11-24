// Handles on the few views a small state change actually touches, so a size
// arriving or a tile coming off the plate repaints those instead of rebuilding
// the popover. The old full rebuild also threw away the scroll position, the
// hover and the spray in flight, which is why those states jumped.
// Exports: `Scan`, `Clean`, `install_*`, `clear`, `scan_matches`, `chrome_changed`, the `*_changed` hooks.
// Deps: crate view modules + state + theme; objc2 AppKit.

use crate::checkbox::CheckBox;
use crate::menu_rows::Membership;
use crate::plate::PlateView;
use crate::state::{with_state_ret, AppState, UiScreen};
use crate::theme::Theme;
use crate::zone::Zone;
use crate::{MenuHandler, HANDLER};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_app_kit::{NSBox, NSTextField, NSView};
use objc2_foundation::MainThreadMarker;
use std::cell::RefCell;
use wd40::scanner::ArtifactGroup;

thread_local! {
    static SCAN: RefCell<Option<Scan>> = const { RefCell::new(None) };
    static CLEAN: RefCell<Option<Clean>> = const { RefCell::new(None) };
}

pub struct Row {
    pub index: usize,
    pub size: Retained<NSTextField>,
    pub check: CheckBox,
    /// Meter under the row, drawn to the size's share of the largest row.
    pub wash: Retained<NSBox>,
    /// Second line: kind and age. Ages go stale while the popover is shut.
    pub age: Retained<NSTextField>,
    /// RECENT marker. Same clock as the age line; hidden once the row is old.
    pub badge: Retained<NSView>,
}

pub struct Group {
    pub group: ArtifactGroup,
    pub size: Retained<NSTextField>,
    pub check: CheckBox,
}

pub struct Scan {
    pub theme: Theme,
    pub header: Zone,
    pub footer: Zone,
    pub rows: Vec<Row>,
    pub groups: Vec<Group>,
    pub membership: Membership,
    /// The window's map of the same targets; the popover has none.
    pub map: Option<Retained<crate::usage_map::MapView>>,
}

pub struct Clean {
    pub theme: Theme,
    pub header: Zone,
    pub strip: Zone,
    pub footer: Zone,
    /// The line above the path, which stops saying "removing" when nothing is.
    pub caption: Retained<NSTextField>,
    pub path: Retained<NSTextField>,
    pub plate: Retained<PlateView>,
    /// The window's line-per-target list; the popover has none.
    pub list: Option<crate::clean_list::CleanList>,
}

pub fn install_scan(scan: Scan) {
    clear();
    SCAN.with(|cell| *cell.borrow_mut() = Some(scan));
}

pub fn install_clean(clean: Clean) {
    clear();
    CLEAN.with(|cell| *cell.borrow_mut() = Some(clean));
}

/// Seconds until the plate has played every removal it has been told of.
pub fn lifts_left() -> f64 {
    CLEAN.with(|cell| cell.borrow().as_ref().map_or(0.0, |clean| clean.plate.seconds_left()))
}

/// The plate on show, for the debug run that samples its layers.
#[cfg(debug_assertions)]
pub fn clean_plate() -> Option<Retained<PlateView>> {
    CLEAN.with(|cell| cell.borrow().as_ref().map(|clean| clean.plate.clone()))
}

/// Drop every handle. Called before each rebuild — the views they point at are
/// about to leave the tree.
pub fn clear() {
    SCAN.with(|cell| *cell.borrow_mut() = None);
    CLEAN.with(|cell| *cell.borrow_mut() = None);
}

/// Sizes that have just settled. Nothing here changes the layout, so no row
/// moves while the user is reading it.
///
/// False means there is no live scan screen to patch and the caller should
/// rebuild instead.
pub fn sizes_arrived(arrived: &[usize], mtm: MainThreadMarker) -> bool {
    let _span = crate::trace::span("sizes_arrived").extra(format!("n={}", arrived.len()));
    patch(UiScreen::Scan, |state, target| {
        SCAN.with(|cell| {
            let held = cell.borrow();
            let Some(scan) = held.as_ref() else { return false };
            let max_size = largest(state);
            for row in &scan.rows {
                let Some(bytes) = state.measured.contains(&row.index).then(|| {
                    state.targets.get(row.index).map_or(0, |t| t.size_bytes)
                }) else {
                    continue;
                };
                if arrived.contains(&row.index) {
                    crate::scan_rows::show_size(&row.size, Some(bytes));
                    crate::motion::arrive(&row.size);
                }
                // Every wash is drawn against the largest row, so one arrival
                // can rescale the rest; they glide rather than snap.
                crate::motion::glide(&row.wash, crate::scan_rows::wash_frame(bytes, max_size));
            }
            for group in &scan.groups {
                crate::scan_rows::show_group_size(
                    &group.size,
                    state.group_size(group.group),
                    state.group_settled(group.group),
                );
            }
            // The footer stays on "Measuring" until every size is in. Rebuilding
            // it on each hop is the same button, and it is the expensive half.
            if state.sizing() {
                redraw_scan_header(scan, state, mtm);
            } else {
                redraw_scan_frame(scan, state, target, mtm);
            }
            true
        })
    })
}

/// True when the cached scan tree was built from this state's rows and counts.
pub fn scan_matches(state: &AppState) -> bool {
    SCAN.with(|cell| {
        cell.borrow()
            .as_ref()
            .is_some_and(|scan| scan.membership == crate::scan_view::membership(state))
    })
}

/// A row or group was ticked: the boxes and the button, and nothing else.
pub fn selection_changed(mtm: MainThreadMarker) -> bool {
    let _span = crate::trace::span("selection_changed");
    patch(UiScreen::Scan, |state, target| {
        SCAN.with(|cell| {
            let held = cell.borrow();
            let Some(scan) = held.as_ref() else { return false };
            for row in &scan.rows {
                let on = state.selected.contains(&row.index);
                row.check.set(crate::selection::GroupSelection::from(on), &scan.theme);
            }
            let empty = state.empty_targets();
            for group in &scan.groups {
                let selection = crate::selection::group_selection(
                    &state.targets, &state.selected, group.group, &empty,
                );
                group.check.set(selection, &scan.theme);
            }
            if let Some(map) = &scan.map {
                map.sync(&state.selected);
            }
            scan.footer.repaint(|view, _| crate::scan_view::draw_footer(view, state, &scan.theme, target, mtm));
            true
        })
    })
}

/// Reclaim or a header figure landed. Rows stay put; only the chrome is redrawn.
pub fn totals_changed(mtm: MainThreadMarker) -> bool {
    let _span = crate::trace::span("totals_changed");
    patch(UiScreen::Scan, |state, target| {
        SCAN.with(|cell| {
            let held = cell.borrow();
            let Some(scan) = held.as_ref() else { return false };
            redraw_scan_frame(scan, state, target, mtm);
            true
        })
    })
}

/// Disk free space, row ages and RECENT badges go stale while the popover is
/// shut. Membership is unchanged, so the tree stays; only the header and the
/// age line move.
pub fn chrome_changed(mtm: MainThreadMarker) -> bool {
    let _span = crate::trace::span("chrome_changed");
    patch(UiScreen::Scan, |state, _target| {
        SCAN.with(|cell| {
            let held = cell.borrow();
            let Some(scan) = held.as_ref() else { return false };
            redraw_scan_header(scan, state, mtm);
            for row in &scan.rows {
                if let Some(target) = state.targets.get(row.index) {
                    crate::scan_rows::show_age(&row.age, &row.badge, target, state.config.max_age_days);
                }
            }
            true
        })
    })
}

/// One tick of a clean: the strip, the header, the path and the plate's tiles.
/// The plate takes the new state in place; only tiles that changed are touched.
pub fn progress_changed(mtm: MainThreadMarker) -> bool {
    patch(UiScreen::Cleaning, |state, target| {
        CLEAN.with(|cell| {
            let held = cell.borrow();
            let Some(live) = held.as_ref() else { return false };
            let Some(progress) = state.cleaning.clone() else { return false };
            let theme = &live.theme;
            live.header.repaint(|view, top| crate::clean_view::draw_disk_header(view, top, state, &progress, theme, mtm));
            live.strip.repaint(|view, top| crate::clean_view::draw_strip(view, top, &progress, theme, mtm));
            live.footer.repaint(|view, _| crate::clean_view::draw_footer(view, &progress, theme, target, mtm));
            crate::clean_view::show_path(&live.caption, &live.path, &progress);
            live.plate.update(&progress);
            if let Some(list) = &live.list {
                list.update(&progress);
            }
            true
        })
    })
}

/// Largest settled size on screen — nothing is drawn to scale against a figure
/// that has not arrived.
fn largest(state: &AppState) -> u64 {
    state
        .measured
        .iter()
        .filter_map(|index| state.targets.get(*index))
        .map(|target| target.size_bytes)
        .max()
        .unwrap_or(1)
        .max(1)
}

fn redraw_scan_header(scan: &Scan, state: &AppState, mtm: MainThreadMarker) {
    let model = crate::scan_view::header_model(state);
    scan.header.repaint(|view, top| crate::header::draw_scan_header(view, top, &model, &scan.theme, mtm));
}

fn redraw_scan_frame(scan: &Scan, state: &AppState, target: &AnyObject, mtm: MainThreadMarker) {
    redraw_scan_header(scan, state, mtm);
    scan.footer.repaint(|view, _| crate::scan_view::draw_footer(view, state, &scan.theme, target, mtm));
}

/// Run `body` only while `screen` is the one on show, with the state and the
/// action target it needs.
fn patch(screen: UiScreen, body: impl FnOnce(&AppState, &AnyObject) -> bool) -> bool {
    let Some(handler) = HANDLER.with(|cell| cell.borrow().clone()) else { return false };
    let target: &AnyObject =
        unsafe { &*(handler.as_ref() as *const MenuHandler as *const AnyObject) };
    with_state_ret(|state| match state.screen == screen {
        true => body(state, target),
        false => false,
    })
    .unwrap_or(false)
}
