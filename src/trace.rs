// Terminal-readable main-thread timings for the popover. A bundled app's
// stdout goes nowhere; these lines print to a terminal when WD40_TRACE or
// WD40_BENCH is set.
// Exports: `span`, `log`, `maybe_schedule`. Deps: crate UI modules, objc2.

use crate::hover_row::HoverRow;
use crate::state::{with_state, with_state_ret};
use crate::{live, popover};
use objc2::rc::Retained;
use objc2::{msg_send, ClassType, Message};
use objc2_app_kit::{NSApplication, NSView};
use objc2_foundation::MainThreadMarker;
use std::time::{Duration, Instant, SystemTime};
use wd40::scanner::{ArtifactKind, TargetDir};

/// Named interval. Prints on drop when WD40_TRACE or WD40_BENCH is set.
pub struct Span {
    name: &'static str,
    start: Instant,
    extra: String,
}

impl Span {
    pub fn extra(mut self, extra: String) -> Self {
        self.extra = extra;
        self
    }
}

impl Drop for Span {
    fn drop(&mut self) {
        emit(self.name, self.start.elapsed(), &self.extra);
    }
}

pub fn span(name: &'static str) -> Span {
    Span {
        name,
        start: Instant::now(),
        extra: String::new(),
    }
}

fn emit(name: &str, elapsed: Duration, extra: &str) {
    if !visible() {
        return;
    }
    let us = elapsed.as_secs_f64() * 1_000_000.0;
    if extra.is_empty() {
        eprintln!("wd40-perf {name} {us:.1}us");
    } else {
        eprintln!("wd40-perf {name} {us:.1}us {extra}");
    }
}

fn visible() -> bool {
    std::env::var_os("WD40_TRACE").is_some() || std::env::var_os("WD40_BENCH").is_some()
}

/// Seed a list and time the open / patch / hover paths, then quit.
pub fn maybe_schedule(_mtm: MainThreadMarker) {
    if std::env::var_os("WD40_BENCH").is_none() {
        return;
    }
    seed();
    crate::popover::refresh(_mtm);
    dispatch(run_bench);
}

pub fn active() -> bool {
    std::env::var_os("WD40_BENCH").is_some()
}

fn seed() {
    with_state(|state| {
        state.targets = bench_targets();
        state.measured = (0..state.targets.len()).collect();
        state.reclaim = None;
        state.reset_selection();
        state.screen = crate::state::UiScreen::Scan;
    });
}

fn bench_targets() -> Vec<TargetDir> {
    const KINDS: [ArtifactKind; 6] = [
        ArtifactKind::RustTarget,
        ArtifactKind::NodeModules,
        ArtifactKind::BuildOutput,
        ArtifactKind::Cache,
        ArtifactKind::Toolchain,
        ArtifactKind::Simulator,
    ];
    (0..30)
        .map(|i| TargetDir {
            path: std::path::PathBuf::from(format!("/Users/bench/proj{i}/target")),
            size_bytes: 80_000_000 * (30 - i as u64),
            last_modified: SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000 - i as u64 * 86_400),
            kind: KINDS[i % KINDS.len()],
        })
        .collect()
}

fn run_bench() {
    let Some(mtm) = MainThreadMarker::new() else {
        eprintln!("wd40-perf bench: not on main thread");
        return;
    };
    eprintln!("wd40-perf bench start");

    let refresh_us = time_repeat("refresh", 8, || {
        popover::refresh(mtm);
    });
    let status_us = time_repeat("refresh_status", 20, || {
        popover::refresh_status(mtm);
    });
    let show_first_ms = time_once("show-first", || {
        popover::ensure_open(mtm);
    });
    popover::close();
    let show_again_ms = time_once("show-again", || {
        popover::ensure_open(mtm);
    });
    popover::close();
    let reopen_old_ms = time_once("reopen-refresh+show", || {
        popover::refresh(mtm);
        popover::ensure_open(mtm);
    });
    popover::close();
    let reopen_new_ms = time_once("reopen-toggle", || {
        popover::toggle(mtm);
    });

    let rows = hover_rows(mtm);
    let hover_n = rows.len().max(1);
    let hover_enter = time_repeat("hover-enter", 40, || {
        for row in &rows {
            row.paint_hover(true);
        }
    });
    let hover_exit = time_repeat("hover-exit", 40, || {
        for row in &rows {
            row.paint_hover(false);
        }
    });
    let hover_paint = time_repeat("hover-paint", 20, || {
        for row in &rows {
            row.paint_hover(true);
            row.displayIfNeeded();
            row.paint_hover(false);
            row.displayIfNeeded();
        }
    });
    let hover_enter_us = hover_enter / hover_n as f64;
    let hover_exit_us = hover_exit / hover_n as f64;
    let hover_paint_us = hover_paint / hover_n as f64;

    let indices: Vec<usize> = with_state_ret(|state| {
        // Mid-scan: some figures still out, which is when sizes_arrived runs.
        let keep = state.targets.len().saturating_mul(2) / 3;
        state.measured = (0..keep).collect();
        (0..keep).collect()
    })
    .unwrap_or_default();
    let sizes_us = time_repeat("sizes_arrived", 12, || {
        let _ = live::sizes_arrived(&indices, mtm);
    });
    with_state(|state| state.measured = (0..state.targets.len()).collect());
    let settled: Vec<usize> =
        with_state_ret(|state| (0..state.targets.len()).collect()).unwrap_or_default();
    let sizes_settled_us = time_repeat("sizes_arrived_settled", 8, || {
        let _ = live::sizes_arrived(&settled, mtm);
    });
    let select_us = time_repeat("selection_changed", 12, || {
        let _ = live::selection_changed(mtm);
    });
    let totals_us = time_repeat("totals_changed", 12, || {
        let _ = live::totals_changed(mtm);
    });
    let chrome_us = time_repeat("chrome_changed", 12, || {
        let _ = live::chrome_changed(mtm);
    });

    with_state(|state| state.show_all = true);
    let refresh_all_us = time_repeat("refresh-show-all", 4, || {
        popover::refresh(mtm);
    });
    popover::ensure_open(mtm);
    let all_rows = hover_rows(mtm);
    let all_n = all_rows.len().max(1);
    let hover_all = time_repeat("hover-paint-all", 10, || {
        for row in &all_rows {
            row.paint_hover(true);
            row.displayIfNeeded();
            row.paint_hover(false);
            row.displayIfNeeded();
        }
    });

    eprintln!(
        "wd40-perf bench summary rows={} refresh_ms={:.2} show_first_ms={show_first_ms:.2} show_again_ms={show_again_ms:.2} reopen_old_ms={reopen_old_ms:.2} reopen_new_ms={reopen_new_ms:.2} status_us={status_us:.1} hover_enter_us={hover_enter_us:.1} hover_exit_us={hover_exit_us:.1} hover_paint_us={hover_paint_us:.1} sizes_arrived_us={sizes_us:.1} sizes_settled_us={sizes_settled_us:.1} selection_us={select_us:.1} totals_us={totals_us:.1} chrome_us={chrome_us:.1} show_all_rows={} refresh_all_ms={:.2} hover_paint_all_us={:.1}",
        rows.len(),
        refresh_us / 1000.0,
        all_rows.len(),
        refresh_all_us / 1000.0,
        hover_all / all_n as f64
    );
    eprintln!("wd40-perf bench done");
    NSApplication::sharedApplication(mtm).terminate(None);
}

fn time_repeat(name: &str, n: u32, mut body: impl FnMut()) -> f64 {
    // Warm the path so the first AppKit alloc is not the number we report.
    body();
    let start = Instant::now();
    for _ in 0..n {
        body();
    }
    let elapsed = start.elapsed();
    let mean = elapsed / n;
    emit(name, mean, &format!("n={n}"));
    mean.as_secs_f64() * 1_000_000.0
}

fn time_once(name: &str, body: impl FnOnce()) -> f64 {
    let start = Instant::now();
    body();
    let elapsed = start.elapsed();
    emit(name, elapsed, "n=1");
    elapsed.as_secs_f64() * 1_000.0
}

fn hover_rows(mtm: MainThreadMarker) -> Vec<Retained<HoverRow>> {
    let Some(root) = popover::content_view(mtm) else {
        return Vec::new();
    };
    let mut rows = Vec::new();
    collect_hover(&root, &mut rows);
    rows
}

fn collect_hover(view: &NSView, out: &mut Vec<Retained<HoverRow>>) {
    let is_row: bool = unsafe { msg_send![view, isKindOfClass: HoverRow::class()] };
    if is_row {
        let row: &HoverRow = unsafe { &*(view as *const NSView as *const HoverRow) };
        out.push(row.retain());
    }
    for child in view.subviews().to_vec() {
        collect_hover(&child, out);
    }
}

fn dispatch(work: fn()) {
    use std::ffi::c_void;
    static NEXT: std::sync::Mutex<Option<fn()>> = std::sync::Mutex::new(None);
    if let Ok(mut slot) = NEXT.lock() {
        *slot = Some(work);
    }
    extern "C" fn run(_ctx: *mut c_void) {
        if let Ok(mut slot) = NEXT.lock() {
            if let Some(work) = slot.take() {
                work();
            }
        }
    }
    unsafe {
        extern "C" {
            static _dispatch_main_q: c_void;
            fn dispatch_async_f(
                queue: *const c_void,
                context: *mut c_void,
                work: extern "C" fn(*mut c_void),
            );
        }
        dispatch_async_f(std::ptr::addr_of!(_dispatch_main_q), std::ptr::null_mut(), run);
    }
}
