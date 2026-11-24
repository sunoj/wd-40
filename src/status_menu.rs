// The status item's menu, shown on a right click or a Control-click. A plain
// click still toggles the popover; this is the way to the window, and to Quit,
// without opening anything first.
// Exports: `requested`, `show`. Deps: objc2 AppKit, crate::{state, HANDLER}.

use crate::state::with_state_ret;
use crate::{MenuHandler, HANDLER};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Sel};
use objc2::sel;
use objc2_app_kit::{NSApplication, NSEventModifierFlags, NSEventType, NSMenu, NSMenuItem};
use objc2_foundation::{MainThreadMarker, NSPoint, NSString};

/// True when the click being handled asks for the menu rather than the popover.
pub fn requested(mtm: MainThreadMarker) -> bool {
    let Some(event) = NSApplication::sharedApplication(mtm).currentEvent() else { return false };
    event.r#type() == NSEventType::RightMouseDown
        || event.modifierFlags().contains(NSEventModifierFlags::Control)
}

pub fn show(mtm: MainThreadMarker) {
    let Some(handler) = HANDLER.with(|cell| cell.borrow().clone()) else { return };
    let Some(button) = with_state_ret(|state| state.status_item.button(mtm)).flatten() else { return };
    let target: &AnyObject = unsafe { &*(handler.as_ref() as *const MenuHandler as *const AnyObject) };
    let menu = NSMenu::new(mtm);
    menu.addItem(&item("Open WD-40 Window", sel!(openWindow:), "o", target, mtm));
    menu.addItem(&NSMenuItem::separatorItem(mtm));
    menu.addItem(&item("Quit WD-40", sel!(quit:), "q", target, mtm));
    // Hung just below the menu bar, under the item that was clicked.
    let below = NSPoint::new(0.0, button.bounds().size.height + 6.0);
    menu.popUpMenuPositioningItem_atLocation_inView(None, below, Some(&button));
}

fn item(title: &str, action: Sel, key: &str, target: &AnyObject, mtm: MainThreadMarker) -> Retained<NSMenuItem> {
    let item = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            mtm.alloc::<NSMenuItem>(),
            &NSString::from_str(title),
            Some(action),
            &NSString::from_str(key),
        )
    };
    unsafe { item.setTarget(Some(target)) };
    item
}
