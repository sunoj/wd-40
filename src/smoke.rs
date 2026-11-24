// Smoke run for the window: open it, resize it, clean a throwaway fixture
// through the real clean path, close it, quit. Debug builds only; activated by WD40_SMOKE_ROOT,
// which names the only directory the run is allowed to remove anything under.
// Deps: crate::{mainthread, plate, state, surface, tasks, window}.

use crate::mainthread::run_on_main;
use crate::state::{with_state_ret, UiScreen};
use crate::{surface, tasks, window};
use objc2::msg_send;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_app_kit::NSEvent;
use objc2_foundation::{MainThreadMarker, NSSize};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

static STARTED: AtomicBool = AtomicBool::new(false);

fn root() -> Option<PathBuf> {
    std::env::var_os("WD40_SMOKE_ROOT").and_then(|root| PathBuf::from(root).canonicalize().ok())
}

pub fn maybe_start() {
    if root().is_none() || STARTED.swap(true, Ordering::Relaxed) {
        return;
    }
    std::thread::spawn(|| {
        let pause = |ms: u64| std::thread::sleep(Duration::from_millis(ms));
        // The startup scan has to settle first: its sizes are what gets ticked.
        pause(500);
        while tasks::is_busy() {
            pause(100);
        }
        run_on_main(open);
        pause(500);
        run_on_main(|| resize(1180.0, 720.0));
        pause(500);
        run_on_main(poke_map);
        pause(300);
        run_on_main(clean);
        // A frame every 60ms for as long as the cleaning screen is up.
        while !with_done() {
            pause(60);
            run_on_main(sample);
        }
        run_on_main(close);
        pause(500);
        run_on_main(quit);
    });
}

fn with_done() -> bool {
    static DONE: AtomicBool = AtomicBool::new(false);
    run_on_main(|| {
        let done = with_state_ret(|state| state.screen == UiScreen::Done).unwrap_or(false);
        DONE.store(done, Ordering::Relaxed);
    });
    DONE.load(Ordering::Relaxed)
}

/// Open the window the way the popover's button and the status menu do: by
/// sending the handler its action.
fn open() {
    let Some(mtm) = MainThreadMarker::new() else { return };
    if std::env::var_os("WD40_SMOKE_POPOVER").is_some() {
        crate::popover::ensure_open(mtm);
        return eprintln!("smoke: popover opened, shown={}", crate::popover::is_shown());
    }
    let Some(handler) = crate::HANDLER.with(|cell| cell.borrow().clone()) else { return };
    let _: () = unsafe { msg_send![&*handler, openWindow: &*handler] };
    let number = window::content_view(mtm).and_then(|view| view.window()).map_or(0, |window| window.windowNumber());
    eprintln!("smoke: openWindow: sent, visible={}, window number {number}", window::is_visible());
}

/// Hover a tile of the scan map, then click it twice, with real mouse events
/// sent through the window: the legend should name the tile, and the tick
/// count should move and move back.
fn poke_map() {
    let Some(map) = crate::live::scan_map() else { return eprintln!("smoke: no scan map on show") };
    let (Some(point), before) = map.probe() else { return eprintln!("smoke: map has no single tile") };
    let Some(window) = map.window() else { return };
    let ticked = || with_state_ret(|state| state.selected.len()).unwrap_or(0);
    let event = |kind: usize| -> Option<Retained<NSEvent>> {
        unsafe {
            msg_send![objc2::class!(NSEvent), mouseEventWithType: kind, location: point, modifierFlags: 0_usize,
                timestamp: 0.0_f64, windowNumber: window.windowNumber(), context: std::ptr::null::<AnyObject>(),
                eventNumber: 0_isize, clickCount: 1_isize, pressure: 1.0_f32]
        }
    };
    if let Some(moved) = event(5) {
        let _: () = unsafe { msg_send![&*map, mouseMoved: &*moved] };
    }
    eprintln!("smoke: map legend before hover: {before:?}");
    eprintln!("smoke: map legend on hover:     {:?}", map.probe().1);
    let start = ticked();
    if let Some(down) = event(1) {
        window.sendEvent(&down);
        let after_one = ticked();
        window.sendEvent(&down);
        eprintln!("smoke: ticked {start} -> {after_one} after one click on the tile -> {} after a second", ticked());
    }
}

/// One line per frame: when it was taken, how many targets have really gone,
/// and where the render tree has the can and each tile at that instant.
fn sample() {
    let Some(plate) = crate::live::clean_plate() else { return };
    let gone = with_state_ret(|state| state.cleaning.as_ref().map_or(0, |run| run.done_count)).unwrap_or(0);
    let line = plate.sample();
    let lifted: usize = line.split(['=', ' ']).nth(1).and_then(|count| count.parse().ok()).unwrap_or(0);
    let lead = if lifted > gone { "  PLATE AHEAD OF JOB" } else { "" };
    eprintln!("frame t={:>5}ms gone={gone} {line}{lead}", clock());
}

/// Milliseconds since the run started.
fn clock() -> u128 {
    static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    START.get_or_init(std::time::Instant::now).elapsed().as_millis()
}

/// Resize the window, which rebuilds whatever screen is up at the new size.
fn resize(width: f64, height: f64) {
    let Some(mtm) = MainThreadMarker::new() else { return };
    let Some(window) = window::content_view(mtm).and_then(|view| view.window()) else { return };
    window.setContentSize(NSSize::new(width, height));
    eprintln!("smoke: resized to {width}x{height}, content is {:?}", window::content_size(mtm));
}

/// Tick exactly the targets under the smoke root and start the real clean.
/// Refuses to start if that would leave anything else ticked.
fn clean() {
    let (Some(mtm), Some(root)) = (MainThreadMarker::new(), root()) else { return };
    // Ticked the way a user ticks: through the row action, which is also what
    // starts the physical-size pass that can land while the clean runs.
    let wanted: Vec<usize> = with_state_ret(|state| {
        state.selected.clear();
        (0..state.targets.len())
            .filter(|index| state.targets[*index].path.canonicalize().is_ok_and(|path| path.starts_with(&root)))
            .collect()
    })
    .unwrap_or_default();
    for index in wanted {
        crate::actions::toggle_item(crate::scan_view::TAG_ITEM_BASE + index as isize, mtm);
    }
    let job = with_state_ret(|state| {
        let paths: Vec<PathBuf> = state.selected.iter().map(|index| state.targets[*index].path.clone()).collect();
        (paths, state.targets.len())
    });
    let Some((paths, found)) = job else { return };
    let safe = !paths.is_empty()
        && paths.iter().all(|path| path.canonicalize().is_ok_and(|path| path.starts_with(&root)));
    eprintln!("smoke: scan found {found} targets, {} under {}", paths.len(), root.display());
    for path in &paths {
        eprintln!("smoke:   will remove {}", path.display());
    }
    if !safe {
        eprintln!("smoke: refusing to clean, nothing under the smoke root");
        return quit();
    }
    tasks::spawn_clean_selected();
    eprintln!("smoke: clean started at t={}ms, screen is cleaning={}", clock(), on_screen(UiScreen::Cleaning));
}

fn close() {
    let Some(mtm) = MainThreadMarker::new() else { return };
    let freed = with_state_ret(|state| state.done.as_ref().map(|done| done.freed_bytes)).flatten();
    eprintln!("smoke: done screen={} at t={}ms, summary freed={freed:?}", on_screen(UiScreen::Done), clock());
    let paints = crate::plate::STEEL_PAINTS.load(Ordering::Relaxed);
    eprintln!("smoke: plate steel painted {paints} time(s) during the run");
    // The close button's own path, through the delegate's windowShouldClose:.
    if let Some(window) = window::content_view(mtm).and_then(|view| view.window()) {
        window.performClose(None);
    }
    eprintln!("smoke: performClose: sent, visible={}, any surface open={}", window::is_visible(), surface::is_open());
}

fn quit() {
    let Some(mtm) = MainThreadMarker::new() else { return };
    eprintln!("smoke: quitting cleanly");
    objc2_app_kit::NSApplication::sharedApplication(mtm).terminate(None);
}

fn on_screen(screen: UiScreen) -> bool {
    with_state_ret(|state| state.screen == screen).unwrap_or(false)
}
