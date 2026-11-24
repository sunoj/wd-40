// The app window: a resizable NSWindow showing the same screens as the popover
// with room for a sidebar and the whole list. Closing it only hides it; the app
// stays in the menu bar with no Dock icon. See docs/window-and-popover.md.
// Exports: `open`, `close`, `focus`, `is_visible`, `install`, `content_size`.

use crate::theme::Theme;
use crate::{popover, surface};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{define_class, msg_send, sel, MainThreadOnly};
use objc2_app_kit::{
    NSApplication, NSBackingStoreType, NSButton, NSEventModifierFlags, NSView, NSWindow,
    NSWindowStyleMask,
};
use objc2_foundation::{MainThreadMarker, NSObject, NSPoint, NSRect, NSSize, NSString};
use std::cell::RefCell;

const DEFAULT_SIZE: (f64, f64) = (960.0, 640.0);
/// The sidebar is a popover's width, and the list beside it needs its columns.
const MIN_SIZE: (f64, f64) = (820.0, 580.0);

thread_local! {
    static WINDOW: RefCell<Option<Retained<NSWindow>>> = const { RefCell::new(None) };
    static DELEGATE: RefCell<Option<Retained<WindowDelegate>>> = const { RefCell::new(None) };
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "WD40WindowDelegate"]
    struct WindowDelegate;

    impl WindowDelegate {
        /// Closing hides the window and hands the screens back to the popover.
        /// The window object is kept, so the next open is the same window.
        #[unsafe(method(windowShouldClose:))]
        fn should_close(&self, _sender: *mut AnyObject) -> bool {
            close(self.mtm());
            false
        }

        /// The layout is frame-based, so a new size is a new build.
        #[unsafe(method(windowDidResize:))]
        fn did_resize(&self, _notification: *mut AnyObject) {
            if is_visible() {
                surface::refresh(self.mtm());
            }
        }

        /// Free space, ages and the appearance can all have moved while the
        /// window sat behind something else.
        #[unsafe(method(windowDidBecomeKey:))]
        fn did_become_key(&self, _notification: *mut AnyObject) {
            bring_up_to_date(self.mtm());
        }

        #[unsafe(method(closeWindow:))]
        fn close_window(&self, _sender: *mut AnyObject) {
            close(self.mtm());
        }
    }
);

define_class!(
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "WD40WindowRoot"]
    struct WindowRoot;

    impl WindowRoot {
        /// Light to dark while the window is open. Deferred: AppKit sends this
        /// from inside its own layout pass, which is no place to swap a tree.
        #[unsafe(method(viewDidChangeEffectiveAppearance))]
        fn appearance_changed(&self) {
            crate::mainthread::run_on_main(appearance_later);
        }
    }
);

fn appearance_later() {
    if let Some(mtm) = MainThreadMarker::new() {
        if is_visible() {
            bring_up_to_date(mtm);
        }
    }
}

fn bring_up_to_date(mtm: MainThreadMarker) {
    if !surface::content_ready(mtm) {
        surface::refresh(mtm);
    } else {
        let _ = crate::live::chrome_changed(mtm);
    }
}

/// Read off the window itself, never a remembered flag.
pub fn is_visible() -> bool {
    WINDOW.with(|cell| cell.borrow().as_ref().is_some_and(|window| window.isVisible()))
}

/// Open the window, or bring it forward. The popover gives way: there is one
/// live surface at a time.
pub fn open(mtm: MainThreadMarker) {
    if is_visible() {
        return focus(mtm);
    }
    crate::auto_clean::apply_pending_snapshot();
    let _opening = crate::auto_clean::prepare_popover_opening();
    popover::close();
    let window = ensure(mtm);
    activate(mtm);
    window.makeKeyAndOrderFront(None);
    surface::refresh(mtm);
    crate::tasks::discovery_sweep_started();
}

pub fn focus(mtm: MainThreadMarker) {
    activate(mtm);
    WINDOW.with(|cell| {
        if let Some(window) = cell.borrow().as_ref() {
            window.makeKeyAndOrderFront(None);
        }
    });
}

/// Hide the window. Nothing stops: a clean in flight carries on, and the
/// popover shows it from the same state the next time it is opened.
pub fn close(mtm: MainThreadMarker) {
    crate::tasks::discovery_sweep_stopped();
    WINDOW.with(|cell| {
        if let Some(window) = cell.borrow().as_ref() {
            window.orderOut(None);
        }
    });
    surface::refresh(mtm);
}

pub fn content_size(mtm: MainThreadMarker) -> Option<(f64, f64)> {
    content_view(mtm).map(|view| (view.bounds().size.width, view.bounds().size.height))
}

pub fn content_view(_mtm: MainThreadMarker) -> Option<Retained<NSView>> {
    WINDOW.with(|cell| cell.borrow().as_ref().and_then(|window| window.contentView()))
}

/// Take freshly built content. A screen narrower than the window is a card,
/// centred on the window's backdrop.
pub fn install(view: &NSView, mtm: MainThreadMarker) {
    let Some(root) = content_view(mtm) else { return };
    for old in root.subviews().to_vec() {
        old.removeFromSuperview();
    }
    let (room, size) = (root.bounds().size, view.frame().size);
    view.setFrameOrigin(NSPoint::new(
        ((room.width - size.width) / 2.0).floor().max(0.0),
        ((room.height - size.height) / 2.0).floor().max(0.0),
    ));
    crate::widgets::add_fill(&root, 0.0, 0.0, room.width, room.height, Theme::for_app(mtm).paper, 1.0, 0.0, mtm);
    root.addSubview(view);
    add_close_key(&root, mtm);
}

/// An accessory app has no main menu, so nothing answers Command-W unless a
/// control in the window does.
fn add_close_key(root: &NSView, mtm: MainThreadMarker) {
    let Some(delegate) = DELEGATE.with(|cell| cell.borrow().clone()) else { return };
    let target: &AnyObject = unsafe { &*(&*delegate as *const WindowDelegate as *const AnyObject) };
    let button = unsafe {
        NSButton::buttonWithTitle_target_action(&NSString::from_str(""), Some(target), Some(sel!(closeWindow:)), mtm)
    };
    button.setBordered(false);
    button.setFrame(NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(0.0, 0.0)));
    button.setKeyEquivalent(&NSString::from_str("w"));
    button.setKeyEquivalentModifierMask(NSEventModifierFlags::Command);
    root.addSubview(&button);
}

fn ensure(mtm: MainThreadMarker) -> Retained<NSWindow> {
    if let Some(window) = WINDOW.with(|cell| cell.borrow().clone()) {
        return window;
    }
    let style = NSWindowStyleMask::Titled
        | NSWindowStyleMask::Closable
        | NSWindowStyleMask::Miniaturizable
        | NSWindowStyleMask::Resizable;
    let rect = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(DEFAULT_SIZE.0, DEFAULT_SIZE.1));
    let window = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
            NSWindow::alloc(mtm), rect, style, NSBackingStoreType::Buffered, false,
        )
    };
    // The default hands the window back to AppKit on close, under the
    // reference this module keeps.
    unsafe { window.setReleasedWhenClosed(false) };
    window.setTitle(&NSString::from_str("WD-40"));
    window.setContentMinSize(NSSize::new(MIN_SIZE.0, MIN_SIZE.1));
    let root: Retained<WindowRoot> = unsafe { msg_send![WindowRoot::alloc(mtm), initWithFrame: rect] };
    window.setContentView(Some(&root));
    window.center();
    let delegate: Retained<WindowDelegate> = unsafe { msg_send![WindowDelegate::alloc(mtm), init] };
    let _: () = unsafe { msg_send![&*window, setDelegate: &*delegate] };
    DELEGATE.with(|cell| *cell.borrow_mut() = Some(delegate));
    WINDOW.with(|cell| *cell.borrow_mut() = Some(window.clone()));
    window
}

#[allow(deprecated)]
fn activate(mtm: MainThreadMarker) {
    NSApplication::sharedApplication(mtm).activateIgnoringOtherApps(true);
}
