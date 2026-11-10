// Background scan/clean orchestration for the WD-40 popover app.
// Exports: scan/clean spawners, timer control, main-thread completion hooks.
// Deps: objc2, crate::{live, mainthread, names, popover, state, tasks_clean}.

use crate::live;
use crate::popover;
use crate::state::{
    with_state, with_state_ret, CleanProgress, DoneSummary, UiScreen,
};
use crate::tasks_clean::{self, gather_job, initial_progress, CleanJob};
use objc2::rc::Retained;
use objc2::sel;
use objc2_foundation::{MainThreadMarker, NSTimer};
use crate::mainthread::{
    clean_done_trampoline, dispatch_to_main, progress_trampoline, scan_done_trampoline, schedule,
    sizes_done_trampoline, sizes_tick_trampoline, stop_timer,
};
use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Instant, SystemTime};
use wd40::disk::disk_space;
use wd40::discover::scan_discover;
use wd40::scanner::TargetDir;
use wd40::qos::Priority;
use wd40::sizes::size_targets;

/// How often the app rescans on its own, in seconds. Ten minutes rather than
/// five: nothing on screen goes stale enough in that time to be worth waking
/// the disk for, and the figures that cost the most to measure do not change
/// at all between two of them.
const AUTO_SCAN_INTERVAL: f64 = 10.0 * 60.0;

/// A clean that takes a moment is worth watching, and one that takes none at
/// all used to flash past before anything could be read. The floor is on the
/// screen alone: removal starts the instant it is asked for, nothing waits on
/// it, and every figure on the held screen is the finished one.
const MIN_CLEAN_SCREEN: f64 = 2.0;

static CLEANING: AtomicBool = AtomicBool::new(false);
static SCANNING: AtomicBool = AtomicBool::new(false);
static DISCOVERING: AtomicBool = AtomicBool::new(false);
static STOP_AFTER: AtomicBool = AtomicBool::new(false);
static RECLAIM_STARTED: AtomicBool = AtomicBool::new(false);
static RECLAIM_LANDED: AtomicBool = AtomicBool::new(false);
static SCAN_RESULT: Mutex<Option<Vec<TargetDir>>> = Mutex::new(None);
static RECLAIM_RESULT: Mutex<Option<wd40::reclaim::Reclaim>> = Mutex::new(None);
static DONE_RESULT: Mutex<Option<DoneSummary>> = Mutex::new(None);
/// The finished summary, waiting for the cleaning screen to have been up long
/// enough to have been read.
static PENDING_DONE: Mutex<Option<DoneSummary>> = Mutex::new(None);
static CLEAN_SHOWN: Mutex<Option<Instant>> = Mutex::new(None);
static PROGRESS: Mutex<Option<CleanProgress>> = Mutex::new(None);
/// Sizes that have settled but not yet reached the screen.
type SettledSize = (usize, u64, Option<SystemTime>);
static SIZES: Mutex<Vec<SettledSize>> = Mutex::new(Vec::new());
/// One pending hop to the main thread at a time, however fast sizes arrive.
static SIZE_HOP: AtomicBool = AtomicBool::new(false);
static PROGRESS_HOP: AtomicBool = AtomicBool::new(false);

thread_local! {
    static AUTO_TIMER: RefCell<Option<Retained<NSTimer>>> = const { RefCell::new(None) };
    static SCAN_TIMER: RefCell<Option<Retained<NSTimer>>> = const { RefCell::new(None) };
    /// Runs out the rest of the cleaning screen's minimum time. It carries no
    /// work; the run is already over by the time it is scheduled.
    static HOLD_TIMER: RefCell<Option<Retained<NSTimer>>> = const { RefCell::new(None) };
    static DISCOVERY_SWEEP_TIMER: RefCell<Option<Retained<NSTimer>>> = const { RefCell::new(None) };
}

pub fn is_busy() -> bool {
    CLEANING.load(Ordering::Relaxed) || SCANNING.load(Ordering::Relaxed)
}

pub fn is_scanning() -> bool {
    SCANNING.load(Ordering::Relaxed)
}

pub fn is_discovering() -> bool {
    DISCOVERING.load(Ordering::Relaxed)
}

fn scan_screen_is_visible(screen: UiScreen) -> bool {
    screen == UiScreen::Scan
}

fn discovery_sweep_should_run() -> bool {
    DISCOVERING.load(Ordering::Relaxed)
        && popover::is_open()
        && with_state_ret(|state| scan_screen_is_visible(state.screen)).unwrap_or(false)
}

pub(crate) fn discovery_sweep_started() {
    if !discovery_sweep_should_run() {
        stop_discovery_sweep();
        return;
    }
    if !crate::motion::discovery_sweep_enabled() {
        stop_discovery_sweep();
        return;
    }
    if DISCOVERY_SWEEP_TIMER.with(|cell| cell.borrow().is_some()) {
        return;
    }
    schedule(&DISCOVERY_SWEEP_TIMER, crate::motion::DISCOVERY_SWEEP_INTERVAL, sel!(discoverySweepTick:), true);
}

pub(crate) fn discovery_sweep_stopped() {
    stop_discovery_sweep();
    crate::motion::reset_scan_gauge();
}

fn stop_discovery_sweep() {
    stop_timer(&DISCOVERY_SWEEP_TIMER);
    crate::motion::stop_discovery_sweep();
}

pub fn on_discovery_sweep_tick(_mtm: MainThreadMarker) {
    if !discovery_sweep_should_run() {
        stop_discovery_sweep();
        return;
    }
    if !crate::motion::advance_discovery_sweep() {
        stop_discovery_sweep();
    }
}

/// True once the user has asked the current run to stop starting new targets.
pub fn stop_requested() -> bool {
    STOP_AFTER.load(Ordering::Relaxed)
}

pub(crate) fn clean_stop_flag() -> &'static AtomicBool {
    &STOP_AFTER
}

pub(crate) fn reset_clean_stop() {
    STOP_AFTER.store(false, Ordering::Relaxed);
}

pub fn start_scan() {
    if crate::trace::active() {
        return;
    }
    let Some(config) = with_state_ret(|state| state.config.clone()) else { return };
    if CLEANING.load(Ordering::Relaxed) || SCANNING.swap(true, Ordering::Relaxed) {
        return;
    }
    DISCOVERING.store(true, Ordering::Relaxed);
    crate::auto_clean::discard_pending_snapshot();
    crate::auto_clean::clear_receipt();
    if let Some(mtm) = MainThreadMarker::new() {
        let left_done = with_state_ret(|state| {
            let was = state.screen == UiScreen::Done;
            if was {
                state.screen = UiScreen::Scan;
            }
            was
        })
        .unwrap_or(false);
        // An open scan needs the in-progress copy immediately; a closed one
        // catches the same state through popover::content_ready on reopen.
        if left_done || popover::is_open() {
            popover::refresh(mtm);
            discovery_sweep_started();
        }
    }
    std::thread::spawn(move || {
        *SCAN_RESULT.lock().unwrap() = Some(scan_discover(&config));
        dispatch_to_main(scan_done_trampoline);
    });
}

pub fn on_scan_done(mtm: MainThreadMarker) {
    DISCOVERING.store(false, Ordering::Relaxed);
    stop_discovery_sweep();
    // A new scan replaces the rows, so the remembered offset would point at
    // targets that are no longer there.
    crate::scrolling::reset_scroll();
    let Some(targets) = SCAN_RESULT.lock().unwrap().take() else {
        SCANNING.store(false, Ordering::Relaxed);
        popover::refresh(mtm);
        return;
    };
    SIZES.lock().unwrap().clear();
    with_state(|state| {
        state.targets = targets;
        state.measured.clear();
        state.reclaim = None;
        state.reset_selection();
    });
    RECLAIM_STARTED.store(false, Ordering::Relaxed);
    RECLAIM_LANDED.store(false, Ordering::Relaxed);
    popover::refresh(mtm);
    let pending: Vec<TargetDir> =
        with_state_ret(|state| state.targets.clone()).unwrap_or_default();
    // Someone looking at the popover is waiting on this pass; a timer scan is not.
    let priority = if popover::is_open() { Priority::Waited } else { Priority::Housekeeping };
    std::thread::spawn(move || {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            size_targets(&pending, priority, |sized| {
                SIZES.lock().unwrap().push(settled_size(sized));
                if !SIZE_HOP.swap(true, Ordering::SeqCst) {
                    dispatch_to_main(sizes_tick_trampoline);
                }
            });
        }));
        dispatch_to_main(sizes_done_trampoline);
    });
}

/// Sizes that settled since the last hop. Patches the rows in place — the list
/// keeps its order and its scroll position until the whole pass is done.
pub fn on_sizes_tick(mtm: MainThreadMarker) {
    SIZE_HOP.store(false, Ordering::SeqCst);
    let arrived = std::mem::take(&mut *SIZES.lock().unwrap());
    if arrived.is_empty() {
        return;
    }
    let indices: Vec<usize> = arrived.iter().map(|(index, _, _)| *index).collect();
    with_state(|state| {
        for (index, bytes, last_modified) in arrived {
            state.apply_size(index, bytes, last_modified);
        }
    });
    if !live::sizes_arrived(&indices, mtm) {
        popover::refresh(mtm);
    }
    popover::refresh_status(mtm);
}

fn settled_size(sized: wd40::sizes::SizedTarget) -> SettledSize {
    (sized.index, sized.bytes, sized.last_modified)
}

pub fn on_sizes_done(mtm: MainThreadMarker) {
    SCANNING.store(false, Ordering::Relaxed);
    on_sizes_tick(mtm);
    // Only now is there a full set of figures to sort by, so this is the one
    // point at which rows are allowed to move.
    with_state(|state| state.finish_sizing());
    popover::refresh(mtm);
    #[cfg(debug_assertions)]
    crate::screenshot::maybe_start(mtm);
}

/// Start the physical accounting pass after the first selection intent. It is
/// deliberately not part of scan completion: the allocated total is enough
/// for the scan, and cleaning can start before this background work finishes.
pub(crate) fn reclaim_for_selection_intent(was_empty: bool, is_empty: bool) {
    if should_start_reclaim(was_empty, is_empty) {
        start_reclaim();
    }
}

fn should_start_reclaim(was_empty: bool, is_empty: bool) -> bool {
    was_empty && !is_empty
}

fn start_reclaim() {
    if RECLAIM_STARTED.swap(true, Ordering::Relaxed) {
        return;
    }
    let targets = with_state_ret(|state| state.targets.clone()).unwrap_or_default();
    if targets.is_empty() {
        return;
    }
    std::thread::spawn(move || {
        // `measure` answers from the two-level store when the same directories
        // come back at the same sizes, so this is free far more often than it
        // is expensive — including on the first scan after a restart.
        *RECLAIM_RESULT.lock().unwrap() = wd40::reclaim::Reclaim::measure(&targets);
        wd40::cache::flush();
        dispatch_to_main(crate::mainthread::reclaim_done_trampoline);
    });
}

/// The accounting is in: totals stop being a sum and start being a promise.
pub fn on_reclaim_done(mtm: MainThreadMarker) {
    let Some(found) = RECLAIM_RESULT.lock().unwrap().take() else { return };
    with_state(|state| {
        // Printed because the difference between these two numbers is the whole
        // point of the pass, and a run where it never lands looks from outside
        // exactly like one where the disk really does hold that much.
        let summed = wd40::scanner::human_size(state.total_size());
        state.reclaim = Some(found);
        println!("Reclaimable {} on the device, {summed} summed", wd40::scanner::human_size(state.total_size()));
    });
    RECLAIM_LANDED.store(true, Ordering::Relaxed);
    if !live::totals_changed(mtm) {
        popover::refresh(mtm);
    }
    popover::refresh_status(mtm);
}

pub(crate) fn take_reclaim_landed() -> bool {
    RECLAIM_LANDED.swap(false, Ordering::Relaxed)
}

/// Clean exactly the checked items. Irreversible; nothing else is touched.
pub fn spawn_clean_selected() {
    let Some(job) = gather_job() else { return };
    let CleanJob { items, skipped_count, skipped_bytes, reference } = job;
    if items.is_empty() || !claim_cleaning() {
        return;
    }
    STOP_AFTER.store(false, Ordering::Relaxed);
    let before = reference.as_deref().and_then(disk_space);
    let progress = initial_progress(&items);
    *PROGRESS.lock().unwrap() = Some(progress.clone());
    with_state(|state| {
        state.screen = UiScreen::Cleaning;
        state.cleaning = Some(progress.clone());
        state.done = None;
    });
    if let Some(mtm) = MainThreadMarker::new() {
        // A run of its own cancels whatever the last one was still showing.
        stop_timer(&HOLD_TIMER);
        *PENDING_DONE.lock().unwrap() = None;
        *CLEAN_SHOWN.lock().unwrap() = Some(Instant::now());
        popover::refresh(mtm);
    }

    std::thread::spawn(move || {
        let summary = tasks_clean::run_clean(
            items,
            skipped_count,
            skipped_bytes,
            reference,
            before,
            progress,
            |p| {
                *PROGRESS.lock().unwrap() = Some(p.clone());
                if !PROGRESS_HOP.swap(true, Ordering::SeqCst) {
                    dispatch_to_main(progress_trampoline);
                }
            },
            &STOP_AFTER,
        );
        *DONE_RESULT.lock().unwrap() = Some(summary);
        dispatch_to_main(clean_done_trampoline);
    });
}

pub(crate) fn claim_cleaning() -> bool {
    !CLEANING.swap(true, Ordering::Relaxed)
}

pub(crate) fn finish_cleaning() {
    CLEANING.store(false, Ordering::Relaxed);
}

pub fn on_progress(mtm: MainThreadMarker) {
    PROGRESS_HOP.store(false, Ordering::SeqCst);
    let snapshot = PROGRESS.lock().unwrap().clone();
    with_state(|state| state.cleaning = snapshot);
    if !live::progress_changed(mtm) {
        popover::refresh(mtm);
    }
}

pub fn request_stop() {
    STOP_AFTER.store(true, Ordering::Relaxed);
    // Say so straight away: the button has to stop offering what it has
    // already been asked to do.
    if let Some(mtm) = MainThreadMarker::new() {
        if !live::progress_changed(mtm) {
            popover::refresh(mtm);
        }
    }
}

pub fn on_clean_done(mtm: MainThreadMarker) {
    finish_cleaning();
    *PENDING_DONE.lock().unwrap() = DONE_RESULT.lock().unwrap().take();
    // The last thing the run published is what really happened. It goes on
    // screen before anything is held back, so a held screen is a finished one:
    // the counts, the bar and the plate all read done because they are.
    let settled = PROGRESS.lock().unwrap().clone();
    with_state(|state| state.cleaning = settled);
    let Some(seconds) = hold_left() else { return show_result(mtm) };
    if !live::progress_changed(mtm) {
        popover::refresh(mtm);
    }
    schedule(&HOLD_TIMER, seconds, sel!(cleanHoldTick:), false);
}

/// Seconds the cleaning screen has still to run, or None when it owes none —
/// a job that already took longer than the floor, or Reduce Motion, which is
/// never made to wait for a screen.
fn hold_left() -> Option<f64> {
    if crate::metal::reduce_motion() {
        return None;
    }
    let shown = (*CLEAN_SHOWN.lock().unwrap())?;
    let left = MIN_CLEAN_SCREEN - shown.elapsed().as_secs_f64();
    (left > 0.05).then_some(left)
}

/// Hand the finished run to the done screen. Called by the floor's timer, and
/// straight away when there is no floor left to run out.
pub fn show_result(mtm: MainThreadMarker) {
    stop_timer(&HOLD_TIMER);
    // A new run started while this one was being shown; its screen wins.
    if CLEANING.load(Ordering::Relaxed) {
        return;
    }
    let summary = PENDING_DONE.lock().unwrap().take();
    *PROGRESS.lock().unwrap() = None;
    *CLEAN_SHOWN.lock().unwrap() = None;
    with_state(|state| {
        state.cleaning = None;
        state.done = summary;
        state.screen = UiScreen::Done;
    });
    popover::refresh(mtm);
}

/// The button on the held screen. It only skips the wait — there is nothing to
/// show until the run has handed its summary over.
pub fn show_result_now(mtm: MainThreadMarker) {
    if PENDING_DONE.lock().unwrap().is_some() {
        show_result(mtm);
    }
}

pub fn start_auto_scan() {
    schedule(&SCAN_TIMER, AUTO_SCAN_INTERVAL, sel!(autoScanTick:), true);
}

pub fn start_auto_clean(hours: u64) {
    stop_auto_clean();
    schedule(&AUTO_TIMER, hours as f64 * 3600.0, sel!(autoCleanTick:), true);
}

pub fn stop_auto_clean() {
    stop_timer(&AUTO_TIMER);
}

#[cfg(test)]
mod tests {
    use super::settled_size;
    use crate::state::UiScreen;
    use std::time::{Duration, SystemTime};

    #[test]
    fn sizing_handoff_keeps_descendant_timestamp() {
        let last_modified = Some(SystemTime::UNIX_EPOCH + Duration::from_nanos(7));
        let queued = settled_size(wd40::sizes::SizedTarget { index: 3, bytes: 11, last_modified });
        assert_eq!(queued, (3, 11, last_modified));
    }

    #[test]
    fn discovery_sweep_belongs_to_scan_screen() {
        assert!(super::scan_screen_is_visible(UiScreen::Scan));
        assert!(!super::scan_screen_is_visible(UiScreen::Settings));
    }

    #[test]
    fn automatic_rescan_does_not_start_a_physical_pass() {
        assert!(!super::should_start_reclaim(false, false));
    }

    #[test]
    fn first_selection_intent_starts_a_physical_pass() {
        assert!(super::should_start_reclaim(true, false));
    }
}
