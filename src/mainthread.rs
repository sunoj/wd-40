// Getting work onto the main thread: straight away from a worker thread, or
// later off a timer. Scanning and cleaning both run off it, and every one of
// their results has to be applied where AppKit lives.
// Exports: `dispatch_to_main`, `run_on_main`, the trampolines, `schedule`,
// `stop_timer`.
// Deps: libdispatch FFI, objc2, crate::{MenuHandler, HANDLER}.

use crate::{MenuHandler, HANDLER};
use objc2::msg_send;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_foundation::NSTimer;
use std::cell::RefCell;
use std::ffi::c_void;

pub(crate) fn dispatch_to_main(work: extern "C" fn(*mut c_void)) {
    unsafe {
        dispatch_async_f(std::ptr::addr_of!(_dispatch_main_q), std::ptr::null_mut(), work);
    }
}

/// Run a plain function on the main thread, once the current event has been
/// dealt with. The function pointer itself rides in the context argument.
pub(crate) fn run_on_main(work: fn()) {
    extern "C" fn call(context: *mut c_void) {
        let work: fn() = unsafe { std::mem::transmute(context) };
        work();
    }
    unsafe {
        dispatch_async_f(std::ptr::addr_of!(_dispatch_main_q), work as *mut c_void, call);
    }
}

macro_rules! trampoline {
    ($name:ident, $selector:ident) => {
        pub(crate) extern "C" fn $name(_ctx: *mut c_void) {
            HANDLER.with(|cell| {
                if let Some(handler) = cell.borrow().as_ref() {
                    let obj: &AnyObject =
                        unsafe { &*(handler.as_ref() as *const MenuHandler as *const AnyObject) };
                    let _: () = unsafe { msg_send![obj, $selector: std::ptr::null::<AnyObject>()] };
                }
            });
        }
    };
}

trampoline!(scan_done_trampoline, scanDone);
trampoline!(sizes_tick_trampoline, sizesTick);
trampoline!(sizes_done_trampoline, sizesDone);
trampoline!(reclaim_done_trampoline, reclaimDone);
trampoline!(clean_done_trampoline, cleanDone);
trampoline!(progress_trampoline, cleanProgress);

#[link(name = "System", kind = "dylib")]
extern "C" {
    static _dispatch_main_q: c_void;
    fn dispatch_async_f(queue: *const c_void, context: *mut c_void, work: extern "C" fn(*mut c_void));
}

/// A timer the app owns exactly one of: it replaces whatever was in the slot,
/// so a screen or a poll can never end up with two of them running.
pub(crate) type TimerSlot = std::thread::LocalKey<RefCell<Option<Retained<NSTimer>>>>;

pub(crate) fn schedule(slot: &'static TimerSlot, interval: f64, selector: objc2::runtime::Sel, repeats: bool) {
    HANDLER.with(|cell| {
        let Some(handler) = cell.borrow().as_ref().map(Retained::clone) else { return };
        let target: &AnyObject = unsafe { &*(&*handler as *const MenuHandler as *const AnyObject) };
        let timer = unsafe {
            NSTimer::scheduledTimerWithTimeInterval_target_selector_userInfo_repeats(
                interval, target, selector, None, repeats,
            )
        };
        slot.with(|cell| *cell.borrow_mut() = Some(timer));
    });
}

pub(crate) fn stop_timer(slot: &'static TimerSlot) {
    slot.with(|cell| {
        if let Some(timer) = cell.borrow_mut().take() {
            timer.invalidate();
        }
    });
}
