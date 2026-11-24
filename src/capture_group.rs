// Debug run for a capped group: unfold every group in the real popover,
// capture the largest at its top, middle and bottom, drive it with real
// scroll-wheel events to show the wheel passing to the outer list at an end,
// hover and tick a row, then fold a group by a click on its header.
// WD40_CAPTURE_WINDOW=group. Deps: crate::{capture, group_scroll, hover_row, state}.

use crate::capture::{note_window, on_main, stage_crowd, WINDOW};
use crate::group_scroll::{live, GroupScroll};
use crate::hover_row::HoverRow;
use crate::mainthread::run_on_main;
use crate::state::{with_state, with_state_ret};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{class, msg_send};
use objc2_app_kit::{NSEvent, NSTextField, NSView};
use objc2_foundation::NSPoint;
use std::ffi::c_void;
use std::sync::atomic::Ordering;
use std::time::Duration;

/// A Core Graphics event, opaque here: only ever handed straight to AppKit.
#[repr(C)]
struct CGEvent {
    _opaque: [u8; 0],
}

unsafe impl objc2::encode::RefEncode for CGEvent {
    const ENCODING_REF: objc2::encode::Encoding =
        objc2::encode::Encoding::Pointer(&objc2::encode::Encoding::Struct("__CGEvent", &[]));
}

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGEventCreateScrollWheelEvent2(source: *const c_void, units: u32, wheels: u32, wheel1: i32, wheel2: i32, wheel3: i32) -> *mut CGEvent;
    fn CFRelease(object: *const c_void);
}

pub fn run(dir: &str, appearance: &str) {
    let shoot = |name: &str| {
        std::thread::sleep(Duration::from_millis(500));
        let path = format!("{dir}/group-{name}-{appearance}.png");
        let number = WINDOW.load(Ordering::Relaxed).to_string();
        let taken = std::process::Command::new("screencapture").args(["-x", "-o", "-l", &number, &path]).status();
        eprintln!("wd-40: captured {path} ({taken:?})");
    };
    run_on_main(|| on_main(stage));
    // The popover needs a moment on screen before anything can be captured.
    std::thread::sleep(Duration::from_millis(700));
    run_on_main(|| {
        let Some(group) = largest() else { return eprintln!("group: no capped group on show") };
        // Bring the group into the outer list's view, as a user would scroll to it.
        if let Some(document) = unsafe { group.superview() } {
            document.scrollRectToVisible(group.frame());
        }
        report("at the top", &group);
    });
    shoot("top");
    // AppKit applies a wheel event on a later turn of the run loop, so each
    // batch is sent, given time to land, and only then read back.
    let settle = || std::thread::sleep(Duration::from_millis(400));
    run_on_main(|| send(71, -40));
    settle();
    run_on_main(|| say("after 71 wheel events down (the middle)"));
    shoot("middle");
    run_on_main(check_row);
    shoot("hover");
    run_on_main(|| send(100, -40));
    settle();
    run_on_main(|| say("after 100 more down (the bottom)"));
    shoot("bottom");
    run_on_main(|| send(4, -40));
    settle();
    run_on_main(|| say("after 4 more down at the bottom: the outer list takes them"));
    run_on_main(|| send(2, 40));
    settle();
    run_on_main(|| say("after 2 back up: the group takes them"));
    run_on_main(fold_by_header);
    shoot("folded");
    run_on_main(|| say("after the header click and the rebuild"));
    std::thread::sleep(Duration::from_millis(300));
    run_on_main(|| on_main(|mtm| objc2_app_kit::NSApplication::sharedApplication(mtm).terminate(None)));
}

/// The crowd in the popover with the list on show and every group unfolded.
fn stage(mtm: objc2_foundation::MainThreadMarker) {
    stage_crowd();
    with_state(|state| {
        state.open.extend(wd40::scanner::ArtifactGroup::ALL.iter().map(|group| group.key()));
    });
    crate::surface::refresh(mtm);
    crate::popover::ensure_open(mtm);
    note_window(crate::popover::content_view());
}

/// The capped group with the most rows: the one the run is about.
fn largest() -> Option<Retained<GroupScroll>> {
    live().into_iter().max_by(|a, b| (a.room().0 + a.room().1).total_cmp(&(b.room().0 + b.room().1)))
}

/// A real scroll-wheel event of `delta` pixels, as the window server makes one.
fn wheel(delta: i32) -> Option<Retained<NSEvent>> {
    unsafe {
        let made = CGEventCreateScrollWheelEvent2(std::ptr::null(), 0, 1, delta, 0, 0);
        let event: Option<Retained<NSEvent>> = msg_send![class!(NSEvent), eventWithCGEvent: made];
        CFRelease(made as *const c_void);
        event
    }
}

/// How far the outer list is scrolled: its clip view's origin.
fn outer_at(group: &GroupScroll) -> f64 {
    let outer = unsafe { group.superview() }.and_then(|document| document.enclosingScrollView());
    outer.map_or(-1.0, |outer| outer.contentView().bounds().origin.y)
}

fn report(when: &str, group: &GroupScroll) {
    let (above, below) = group.room();
    eprintln!("group: {when}: inner has {above:.0}pt above and {below:.0}pt below; outer list at {:.0}", outer_at(group));
}

/// Send `count` wheel events of `delta` pixels each to the group.
fn send(count: usize, delta: i32) {
    let Some(group) = largest() else { return };
    for _ in 0..count {
        if let Some(event) = wheel(delta) {
            group.scrollWheel(&event);
        }
    }
}

fn say(when: &str) {
    if let Some(group) = largest() {
        report(when, &group);
    }
}

/// Hover and click the row in the middle of the group's visible rows, with a
/// real mouse-down sent through the window.
fn check_row() {
    let Some(group) = largest() else { return };
    let Some(window) = group.window() else { return };
    let size = group.bounds().size;
    let point = group.convertPoint_toView(NSPoint::new(size.width / 3.0, size.height / 2.0), None);
    let event = |kind: usize| -> Option<Retained<NSEvent>> {
        unsafe {
            msg_send![class!(NSEvent), mouseEventWithType: kind, location: point, modifierFlags: 0_usize,
                timestamp: 0.0_f64, windowNumber: window.windowNumber(), context: std::ptr::null::<AnyObject>(),
                eventNumber: 0_isize, clickCount: 1_isize, pressure: 1.0_f32]
        }
    };
    let Some(row) = row_at(&window.contentView(), point) else { return eprintln!("group: no row under the point") };
    let lines = |row: &NSView| -> Vec<String> {
        let shown = row.subviews().to_vec().into_iter().filter(|view| !view.isHidden());
        shown.filter_map(|view| view.downcast_ref::<NSTextField>().map(|field| field.stringValue().to_string())).collect()
    };
    eprintln!("group: row before hover: {:?}", lines(&row));
    if let Some(moved) = event(5) {
        let _: () = unsafe { msg_send![&*row, mouseEntered: &*moved] };
    }
    eprintln!("group: row on hover:     {:?}", lines(&row));
    let ticked = || with_state_ret(|state| state.selected.len()).unwrap_or(0);
    let before = ticked();
    // Through the window first, as a click arrives; a popover that is not key
    // swallows that, so the row is then handed the same event directly.
    let mut how = "sent through the window";
    if let Some(down) = event(1) {
        window.sendEvent(&down);
        if ticked() == before {
            how = "handed to the row";
            let _: () = unsafe { msg_send![&*row, mouseDown: &*down] };
        }
    }
    let (above, below) = group.room();
    eprintln!("group: ticked {before} -> {} after a mouse-down {how}; inner still at {above:.0}/{below:.0}", ticked());
}

/// Fold the largest group with a real mouse-down on its header line, and say
/// what the info mark's tooltip is, since no capture can show one.
fn fold_by_header() {
    let Some(group) = largest() else { return };
    let (Some(document), Some(window)) = (unsafe { group.superview() }, group.window()) else { return };
    if let Some(content) = window.contentView() {
        say_tooltips(&content);
    }
    // The header is the fold row whose lower edge is the group's upper one.
    let top = group.frame().origin.y + group.frame().size.height;
    let rows = document.subviews().to_vec().into_iter().filter_map(|view| view.downcast::<HoverRow>().ok());
    let Some(header) = rows.into_iter().find(|row| row.tag() >= crate::scan_view::TAG_FOLD_BASE && (row.frame().origin.y - top).abs() < 1.0) else {
        return eprintln!("group: no header row above the group");
    };
    let open = || with_state_ret(|state| { let mut open: Vec<_> = state.open.iter().copied().collect(); open.sort(); open }).unwrap_or_default();
    let (before, outer) = (open(), outer_at(&group));
    let size = header.bounds().size;
    let point = header.convertPoint_toView(NSPoint::new(size.width / 2.0, size.height / 2.0), None);
    let down: Option<Retained<NSEvent>> = unsafe {
        msg_send![class!(NSEvent), mouseEventWithType: 1_usize, location: point, modifierFlags: 0_usize,
            timestamp: 0.0_f64, windowNumber: window.windowNumber(), context: std::ptr::null::<AnyObject>(),
            eventNumber: 0_isize, clickCount: 1_isize, pressure: 1.0_f32]
    };
    let mut how = "sent through the window";
    if let Some(down) = down {
        window.sendEvent(&down);
        if open() == before {
            how = "handed to the header";
            let _: () = unsafe { msg_send![&*header, mouseDown: &*down] };
        }
    }
    eprintln!("group: header mouse-down {how}: open {before:?} -> {:?}; outer list was at {outer:.0}", open());
}

/// Every image on show that carries a tooltip, with what the tooltip says.
fn say_tooltips(view: &NSView) {
    for child in view.subviews().to_vec() {
        match (child.downcast_ref::<objc2_app_kit::NSImageView>(), child.toolTip()) {
            (Some(_), Some(tip)) if !child.isHidden() => eprintln!("group: tooltip on a visible image: {tip:?}"),
            _ => say_tooltips(&child),
        }
    }
}

/// The hover row under a window point, found the way a click finds it.
fn row_at(content: &Option<Retained<NSView>>, point: NSPoint) -> Option<Retained<NSView>> {
    let content = content.as_ref()?;
    let parent = unsafe { content.superview() };
    let local = parent.as_ref().map_or(point, |parent| parent.convertPoint_fromView(point, None));
    let mut view = content.hitTest(local)?;
    while view.class().name().to_str() != Ok("WD40HoverRow") {
        view = unsafe { view.superview() }?;
    }
    Some(view)
}
