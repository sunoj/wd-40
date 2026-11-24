// Real captures of the real window: every screen at three sizes, taken with
// `screencapture -l` so the title bar and the window's own compositing are in
// the picture. The offscreen renders in `screenshot` cannot show a layout that
// only goes wrong inside a window. Debug builds only; WD40_CAPTURE_WINDOW.
// Deps: crate::{mainthread, screenshot, state, surface, window}.

use crate::mainthread::run_on_main;
use crate::screenshot::{stage_cleaning, stage_done, stage_fixture, CLEAN_FRAMES};
use crate::state::{with_state, UiScreen};
use crate::{surface, window};
use objc2_foundation::{MainThreadMarker, NSSize};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use wd40::scanner::{ArtifactKind, TargetDir};

/// Content sizes: the window's minimum, its default, and a large display.
const SIZES: [(&str, f64, f64); 3] = [("min", 820.0, 580.0), ("default", 960.0, 640.0), ("large", 1600.0, 1000.0)];
const SCREENS: [(&str, UiScreen); 4] = [
    ("scan", UiScreen::Scan),
    ("cleaning", UiScreen::Cleaning),
    ("done", UiScreen::Done),
    ("settings", UiScreen::Settings),
];

/// Which capture is being staged, and the window's number once it is open.
static STEP: AtomicUsize = AtomicUsize::new(0);
pub static WINDOW: AtomicUsize = AtomicUsize::new(0);

/// Runs on its own thread: stage on the main thread, give the window server
/// time to put it on screen, capture, move on.
pub fn run() {
    let dir = std::env::var("WD40_SCREENSHOT_DIR").unwrap_or_default();
    let appearance = std::env::var("WD40_APPEARANCE").unwrap_or_else(|_| "light".into());
    match std::env::var("WD40_CAPTURE_WINDOW").as_deref() {
        Ok("list") => return run_list(&dir, &appearance),
        Ok("group") => return crate::capture_group::run(&dir, &appearance),
        _ => {}
    }
    run_on_main(open);
    std::thread::sleep(Duration::from_millis(600));
    for step in 0..SIZES.len() * SCREENS.len() {
        STEP.store(step, Ordering::Relaxed);
        run_on_main(stage);
        std::thread::sleep(Duration::from_millis(450));
        let (size, screen) = (SIZES[step / SCREENS.len()].0, SCREENS[step % SCREENS.len()].0);
        let path = format!("{dir}/win-{size}-{screen}-{appearance}.png");
        let number = WINDOW.load(Ordering::Relaxed).to_string();
        let taken = std::process::Command::new("screencapture").args(["-x", "-o", "-l", &number, &path]).status();
        eprintln!("wd-40: captured {path} ({taken:?})");
    }
    run_on_main(|| {
        if let Some(mtm) = MainThreadMarker::new() {
            objc2_app_kit::NSApplication::sharedApplication(mtm).terminate(None);
        }
    });
}

fn open() {
    let Some(mtm) = MainThreadMarker::new() else { return };
    stage_fixture();
    window::open(mtm);
    let number = window::content_view(mtm).and_then(|view| view.window()).map_or(0, |window| window.windowNumber());
    WINDOW.store(number as usize, Ordering::Relaxed);
}

fn stage() {
    let Some(mtm) = MainThreadMarker::new() else { return };
    let step = STEP.load(Ordering::Relaxed);
    let (_, width, height) = SIZES[step / SCREENS.len()];
    let screen = SCREENS[step % SCREENS.len()].1;
    // Staged in run order, so the done screen's map is the cleaning one ended.
    match screen {
        UiScreen::Cleaning => stage_cleaning(CLEAN_FRAMES[1].1),
        UiScreen::Done => stage_done(),
        _ => {}
    }
    with_state(|state| state.screen = screen);
    if let Some(window) = window::content_view(mtm).and_then(|view| view.window()) {
        window.setContentSize(NSSize::new(width, height));
        window.center();
    }
    surface::refresh(mtm);
}

/// A crowded machine: six groups, 237 targets, most of them old.
const CROWD: [(ArtifactKind, u64, u64); 6] = [
    (ArtifactKind::RustTarget, 13, 4),
    (ArtifactKind::NodeModules, 138, 11),
    (ArtifactKind::BuildOutput, 17, 2),
    (ArtifactKind::Cache, 62, 0),
    (ArtifactKind::Simulator, 5, 3),
    (ArtifactKind::Toolchain, 0, 3),
];

/// The scan list with the crowd in it: the popover, the popover with the old
/// batch ticked, then the window.
fn run_list(dir: &str, appearance: &str) {
    let shoot = |name: &str| {
        std::thread::sleep(Duration::from_millis(500));
        let crowd = std::env::var("WD40_CROWD").unwrap_or_else(|_| "old".into());
        let path = format!("{dir}/list-{crowd}-{name}-{appearance}.png");
        let number = WINDOW.load(Ordering::Relaxed).to_string();
        let taken = std::process::Command::new("screencapture").args(["-x", "-o", "-l", &number, &path]).status();
        eprintln!("wd-40: captured {path} ({taken:?})");
    };
    run_on_main(|| on_main(|mtm| {
        stage_crowd();
        surface::refresh(mtm);
        crate::popover::ensure_open(mtm);
        note_window(crate::popover::content_view());
    }));
    shoot("popover");
    run_on_main(|| on_main(crate::actions::tick_old));
    shoot("popover-ticked");
    run_on_main(|| on_main(|mtm| {
        window::open(mtm);
        note_window(window::content_view(mtm));
    }));
    // The window takes longer than the popover to reach the screen.
    std::thread::sleep(Duration::from_millis(700));
    shoot("window");
    run_on_main(|| on_main(|mtm| objc2_app_kit::NSApplication::sharedApplication(mtm).terminate(None)));
}

pub fn on_main(body: fn(MainThreadMarker)) {
    if let Some(mtm) = MainThreadMarker::new() {
        body(mtm);
    }
}

pub fn note_window(view: Option<objc2::rc::Retained<objc2_app_kit::NSView>>) {
    let number = view.and_then(|view| view.window()).map_or(0, |window| window.windowNumber());
    WINDOW.store(number as usize, Ordering::Relaxed);
}

pub fn stage_crowd() {
    let now = std::time::SystemTime::now();
    with_state(|state| {
        state.targets.clear();
        for (kind, old, recent) in CROWD {
            for n in 0..old + recent {
                // WD40_CROWD=clear makes every target recent; =mix, every other group.
                let young = match std::env::var("WD40_CROWD").as_deref() {
                    Ok("clear") => true,
                    Ok("mix") => kind.group() as usize % 2 == 1,
                    _ => false,
                };
                let days = if n < old && !young { 9 + (n * 7) % 300 } else { n % 6 };
                state.targets.push(TargetDir {
                    path: format!("/Users/dev/code/{}-{n:03}/{}", kind.label().replace(' ', "-"), kind.label()).into(),
                    size_bytes: (3 + (n * 37) % 900) * 1024 * 1024 * if n % 9 == 0 { 8 } else { 1 },
                    last_modified: now - Duration::from_secs(days * 86_400 + 3_600),
                    kind,
                });
            }
        }
        state.measured = (0..state.targets.len()).collect();
        state.reclaim = None;
        state.finish_sizing();
        state.selected.clear();
        state.show_all = false;
        state.screen = UiScreen::Scan;
    });
}
