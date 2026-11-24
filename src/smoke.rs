// Smoke run for the window: open it, resize it, clean a throwaway fixture
// through the real clean path, close it, quit. Debug builds only; activated by WD40_SMOKE_ROOT,
// which names the only directory the run is allowed to remove anything under.
// Deps: crate::{mainthread, plate, state, surface, tasks, window}.

use crate::mainthread::run_on_main;
use crate::state::{with_state_ret, UiScreen};
use crate::{surface, tasks, window};
use objc2::msg_send;
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
        run_on_main(clean);
        pause(150);
        run_on_main(|| resize(900.0, 600.0));
        // The cleaning screen holds for its minimum time, then hands over.
        for _ in 0..150 {
            pause(200);
            if with_done() {
                break;
            }
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
    let Some(handler) = crate::HANDLER.with(|cell| cell.borrow().clone()) else { return };
    let _: () = unsafe { msg_send![&*handler, openWindow: &*handler] };
    eprintln!("smoke: openWindow: sent, visible={}", window::is_visible());
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
    let job = with_state_ret(|state| {
        state.selected = (0..state.targets.len())
            .filter(|index| state.targets[*index].path.canonicalize().is_ok_and(|path| path.starts_with(&root)))
            .collect();
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
    surface::refresh(mtm);
    tasks::spawn_clean_selected();
    eprintln!("smoke: clean started, screen is cleaning={}", on_screen(UiScreen::Cleaning));
}

fn close() {
    let Some(mtm) = MainThreadMarker::new() else { return };
    let freed = with_state_ret(|state| state.done.as_ref().map(|done| done.freed_bytes)).flatten();
    eprintln!("smoke: done screen={}, summary freed={freed:?}", on_screen(UiScreen::Done));
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
