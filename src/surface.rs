// Where the screens are shown. There is one live surface at a time: the window
// while it is open, the popover otherwise. Both are filled by the same screen
// builders from the same `AppState`, so nothing here knows what a screen does.
// Exports: `refresh`, `is_open`, `content_ready`, `content_view`.
// Deps: crate::{layout, live, popover, state, status, theme, window}, the views.

use crate::state::{with_state_ret, AppState, UiScreen};
use crate::theme::Theme;
use crate::{clean_view, done_view, layout, popover, scan_view, settings_view, window};
use crate::{MenuHandler, HANDLER};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::MainThreadOnly;
use objc2_app_kit::NSView;
use objc2_foundation::{MainThreadMarker, NSPoint, NSRect, NSSize};
use std::cell::Cell;

/// What the content on show was built for. Any difference means rebuild.
#[derive(Clone, Copy, PartialEq)]
struct Built {
    dark: bool,
    scanning: bool,
    in_window: bool,
}

thread_local! {
    static BUILT: Cell<Option<Built>> = const { Cell::new(None) };
    /// A rebuild can be asked for from inside one (a resize it caused).
    static BUILDING: Cell<bool> = const { Cell::new(false) };
}

/// True while someone can be looking at a screen, in either surface.
pub fn is_open() -> bool {
    window::is_visible() || popover::is_shown()
}

/// Rebuild the live surface from the current `AppState`. With the window shut
/// the popover's content is rebuilt even while hidden, so opening it is instant.
pub fn refresh(mtm: MainThreadMarker) {
    if BUILDING.replace(true) {
        return;
    }
    let _span = crate::trace::span("refresh");
    crate::scrolling::remember_scroll();
    // The handles are about to point at views that have left the tree.
    crate::live::clear();
    let pane = window::is_visible().then(|| window::content_size(mtm)).flatten();
    let (view, height) = with_state_ret(|state| {
        crate::status::update(state, mtm);
        layout::with_window(pane, || build_for_state(state, mtm))
    })
    .unwrap_or_else(|| empty_view(mtm));
    match pane {
        Some(_) => window::install(&view, mtm),
        None => popover::install(&view, height, mtm),
    }
    BUILT.set(with_state_ret(|state| built_for(state, pane.is_some(), mtm)));
    BUILDING.set(false);
}

/// True when the content on show still describes the state, so an open can
/// reuse it instead of rebuilding under the pointer.
pub fn content_ready(mtm: MainThreadMarker) -> bool {
    let in_window = window::is_visible();
    with_state_ret(|state| {
        if state.screen == UiScreen::Scan && !crate::live::scan_matches(state) {
            return false;
        }
        BUILT.get() == Some(built_for(state, in_window, mtm))
    })
    .unwrap_or(false)
}

/// Content view of the live surface, for screenshot capture and tracing.
pub fn content_view(mtm: MainThreadMarker) -> Option<Retained<NSView>> {
    match window::is_visible() {
        true => window::content_view(mtm),
        false => popover::content_view(),
    }
}

fn built_for(state: &AppState, in_window: bool, mtm: MainThreadMarker) -> Built {
    Built {
        dark: Theme::for_app(mtm).dark,
        scanning: state.screen == UiScreen::Scan && crate::tasks::is_scanning(),
        in_window,
    }
}

fn build_for_state(state: &AppState, mtm: MainThreadMarker) -> (Retained<NSView>, f64) {
    let Some(handler) = HANDLER.with(|cell| cell.borrow().clone()) else {
        return empty_view(mtm);
    };
    let target: &AnyObject =
        unsafe { &*(handler.as_ref() as *const MenuHandler as *const AnyObject) };
    let theme = Theme::for_app(mtm);
    match state.screen {
        UiScreen::Scan => scan_view::build(state, &theme, target, mtm),
        UiScreen::Cleaning => clean_view::build(state, &theme, target, mtm),
        UiScreen::Done => done_view::build(state, &theme, target, mtm),
        UiScreen::Settings => settings_view::build(state, &theme, target, mtm),
    }
}

fn empty_view(mtm: MainThreadMarker) -> (Retained<NSView>, f64) {
    let view = NSView::initWithFrame(
        NSView::alloc(mtm),
        NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(layout::POPOVER_WIDTH, 80.0)),
    );
    (view, 80.0)
}
