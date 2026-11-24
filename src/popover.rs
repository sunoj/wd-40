// NSPopover shell for WD-40: toggled from the status item, filled by `surface`.
// While the window is open the status item brings that forward instead.
// Exports: `attach`, `toggle`, `close`, `ensure_open`, `install`, `is_shown`.
// Deps: objc2 AppKit; crate::{layout, state, surface, window}.

use crate::layout::POPOVER_WIDTH;
use crate::state::with_state_ret;
use crate::{surface, window, MenuHandler, HANDLER};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{sel, MainThreadOnly};
use objc2_app_kit::{NSEventMask, NSPopover, NSPopoverBehavior, NSView, NSViewController};
use objc2_foundation::{MainThreadMarker, NSRectEdge, NSSize, NSString};
use std::cell::RefCell;

thread_local! {
    static POPOVER: RefCell<Option<Retained<NSPopover>>> = const { RefCell::new(None) };
    static CONTROLLER: RefCell<Option<Retained<NSViewController>>> = const { RefCell::new(None) };
}

/// Wire the status button to toggle the popover instead of showing an NSMenu.
pub fn attach(mtm: MainThreadMarker) {
    let handler = HANDLER.with(|cell| cell.borrow().clone());
    let button = with_state_ret(|state| state.status_item.button(mtm)).flatten();
    if let (Some(handler), Some(button)) = (handler, button) {
        let target: &AnyObject = unsafe { &*(handler.as_ref() as *const MenuHandler as *const AnyObject) };
        unsafe {
            button.setTarget(Some(target));
            button.setAction(Some(sel!(togglePopover:)));
        }
        // Fire on mouse-down, not mouse-up. A transient popover dismisses itself
        // the moment you press outside it — including on this button — so by
        // mouse-up it is already closed and the toggle would just reopen it,
        // which reads as a popover that cannot be clicked shut.
        // The right button asks for the status menu instead.
        button.sendActionOn(NSEventMask::LeftMouseDown | NSEventMask::RightMouseDown);
        button.setToolTip(Some(&NSString::from_str("WD-40")));
    }
    ensure_popover(mtm);
    surface::refresh(mtm);
    crate::trace::maybe_schedule(mtm);
}

pub fn toggle(mtm: MainThreadMarker) {
    let _span = crate::trace::span("toggle");
    // One surface at a time: an open window is the app, so the status item
    // brings it forward rather than opening a second view of the same state.
    if window::is_visible() {
        return window::focus(mtm);
    }
    if is_shown() {
        close();
        return;
    }
    // Publish first: a pass that removed rows while we were shut has to reach
    // the state before the staleness test reads it.
    crate::auto_clean::apply_pending_snapshot();
    // The tree is already current whenever a scan, a tick or a screen change
    // has rebuilt or patched it. Rebuilding here is what made the pointer
    // wait on the status item. Disk free space and row ages still go stale
    // while the popover is shut, so those patch through the live handles.
    if CONTROLLER.with(|cell| cell.borrow().is_none()) || !surface::content_ready(mtm) {
        surface::refresh(mtm);
    } else {
        let _ = crate::live::chrome_changed(mtm);
    }
    show(mtm);
}

/// Read off the popover itself: a transient one dismisses without telling us.
pub fn is_shown() -> bool {
    POPOVER.with(|cell| cell.borrow().as_ref().is_some_and(|popover| popover.isShown()))
}

pub fn close() {
    crate::tasks::discovery_sweep_stopped();
    POPOVER.with(|cell| {
        if let Some(popover) = cell.borrow().as_ref() {
            popover.close();
        }
    });
}

/// Open the popover if it is not already shown.
pub fn ensure_open(mtm: MainThreadMarker) {
    if !is_shown() {
        show(mtm);
    }
}

pub fn content_view() -> Option<Retained<NSView>> {
    CONTROLLER.with(|cell| cell.borrow().as_ref().map(|c| c.view()))
}

/// Take freshly built content, keeping the popover open if it was.
pub fn install(view: &NSView, height: f64, mtm: MainThreadMarker) {
    ensure_popover(mtm);
    let was_shown = is_shown();
    CONTROLLER.with(|cell| {
        if let Some(controller) = cell.borrow().as_ref() {
            controller.setView(view);
        }
    });
    POPOVER.with(|cell| {
        if let Some(popover) = cell.borrow().as_ref() {
            popover.setContentSize(NSSize::new(POPOVER_WIDTH, height));
        }
    });
    if was_shown {
        show(mtm);
    }
}

fn ensure_popover(mtm: MainThreadMarker) {
    if POPOVER.with(|cell| cell.borrow().is_some()) {
        return;
    }
    let controller =
        NSViewController::initWithNibName_bundle(NSViewController::alloc(mtm), None, None);
    let popover = NSPopover::new(mtm);
    popover.setBehavior(behavior());
    popover.setAnimates(false);
    popover.setContentViewController(Some(&controller));
    CONTROLLER.with(|cell| *cell.borrow_mut() = Some(controller));
    POPOVER.with(|cell| *cell.borrow_mut() = Some(popover));
}

/// Transient, except under the screenshot harness, which must not have the
/// popover dismissed by whatever else takes focus during a capture.
fn behavior() -> NSPopoverBehavior {
    #[cfg(debug_assertions)]
    if crate::screenshot::enabled() {
        return NSPopoverBehavior::ApplicationDefined;
    }
    NSPopoverBehavior::Transient
}

fn show(mtm: MainThreadMarker) {
    let _span = crate::trace::span("show");
    let _opening = crate::auto_clean::prepare_popover_opening();
    let button = with_state_ret(|state| state.status_item.button(mtm)).flatten();
    let Some(button) = button else { return };
    let popover = POPOVER.with(|cell| cell.borrow().clone());
    if let Some(popover) = popover {
        popover.showRelativeToRect_ofView_preferredEdge(button.bounds(), &button, NSRectEdge::MaxY);
    }
    crate::tasks::discovery_sweep_started();
}
