// The popover's animation, and the single place Reduce Motion is honoured for
// it: with that setting on every helper here lands on the final state at once,
// so nothing is ever left mid-transition.
// Exports: `arrive`, `glide`, `lift`, `travel`, `fade`, `puff`, scan gauge and discovery
// sweep motion.
// Deps: objc2 AppKit animator proxy, crate::metal::reduce_motion.

use crate::metal::reduce_motion;
use objc2::rc::Retained;
use objc2_app_kit::{
    NSAnimatablePropertyContainer, NSAnimationContext, NSBox, NSView,
};
use objc2_foundation::{NSPoint, NSRect, NSSize};
use std::cell::{Cell, RefCell};
use std::time::Instant;

thread_local! {
    static SCAN_GAUGE: RefCell<Option<GaugeTransition>> = const { RefCell::new(None) };
    static DISCOVERY_SWEEP: RefCell<Option<Sweep>> = const { RefCell::new(None) };
}

/// A figure appearing. Short enough that a burst of arriving sizes still reads
/// as a list filling in rather than a light show.
const ARRIVE: f64 = 0.22;
/// Something moving or growing: a bar, a plate tile, a row.
pub const GLIDE: f64 = 0.3;
/// How far a tile rises as it leaves the plate.
const RISE: f64 = 46.0;
/// How long a burst of spray hangs in the air.
const PUFF: f64 = 0.5;
const SWEEP_PERIOD: f64 = 1.8;
const SWEEP_WIDTH: f64 = 38.0;
pub const DISCOVERY_SWEEP_INTERVAL: f64 = 1.0 / 36.0;

struct GaugeTransition {
    from: f64,
    to: f64,
    started: Instant,
}

pub struct GaugeAnimation {
    pub from: f64,
    pub to: f64,
    pub duration: f64,
}

struct Sweep {
    view: Retained<NSBox>,
    track_x: f64,
    track_width: f64,
    phase: Cell<f64>,
}

/// A value that has just become known: it fades up instead of blinking in.
pub fn arrive(view: &NSView) {
    if reduce_motion() {
        view.setAlphaValue(1.0);
        return;
    }
    view.setAlphaValue(0.0);
    grouped(ARRIVE, || view.animator().setAlphaValue(1.0));
}

/// A plate tile whose target has really been removed comes up off the steel:
/// it rises, swells a little toward the eye, and thins out as it goes. Under
/// Reduce Motion it is simply gone, which is the same final state.
pub fn lift(view: &NSView, seconds: f64) {
    if reduce_motion() {
        view.setAlphaValue(0.0);
        return;
    }
    let from = view.frame();
    let swell = (from.size.width.min(from.size.height) * 0.08).clamp(2.0, 8.0);
    let to = NSRect::new(
        NSPoint::new(from.origin.x - swell, from.origin.y - swell + RISE),
        NSSize::new(from.size.width + swell * 2.0, from.size.height + swell * 2.0),
    );
    grouped(seconds, || {
        view.animator().setFrame(to);
        view.animator().setAlphaValue(0.0);
    });
}

/// Move a view to `frame` over `seconds`: the can crossing the plate.
pub fn travel(view: &NSView, frame: NSRect, seconds: f64) {
    glide_for(view, frame, seconds);
}

/// Bring a view to `alpha`: at once under Reduce Motion, over `duration` otherwise.
pub fn fade(view: &NSView, alpha: f64, duration: f64) {
    if reduce_motion() {
        view.setAlphaValue(alpha);
        return;
    }
    grouped(duration, || view.animator().setAlphaValue(alpha));
}

/// One burst of spray: it shows, then thins out. Reduce Motion never shows it.
pub fn puff(view: &NSView) {
    view.setAlphaValue(0.0);
    if reduce_motion() {
        return;
    }
    view.setAlphaValue(0.8);
    grouped(PUFF, || view.animator().setAlphaValue(0.0));
}

/// Move or resize a view to `frame`.
pub fn glide(view: &NSView, frame: NSRect) {
    glide_for(view, frame, GLIDE);
}

fn glide_for(view: &NSView, frame: NSRect, duration: f64) {
    if reduce_motion() || duration <= 0.0 {
        view.setFrame(frame);
        return;
    }
    grouped(duration, || view.animator().setFrame(frame));
}

pub fn glide_for_gauge(view: &NSBox, x: f64, y: f64, width: f64, duration: f64) {
    glide_for(
        view,
        NSRect::new(NSPoint::new(x, y), NSSize::new(width.max(0.5), 11.0)),
        duration,
    );
}

pub fn glide_gauge_marker(view: &NSBox, x: f64, y: f64, duration: f64) {
    glide_for(
        view,
        NSRect::new(NSPoint::new(x, y), NSSize::new(1.0, 11.0)),
        duration,
    );
}

/// Keep the gauge transition continuous when its header is rebuilt.
///
/// A remembered transition only means anything while a scan is running. The
/// popover is `Transient`, so dismissing it by clicking away never reaches
/// `popover::close` and nothing forgets the width the last scan was part way
/// to — a settled reopen would then glide up to a figure that has been final
/// for minutes.
pub fn scan_gauge_animation(
    target: f64,
    sizing: bool,
    in_progress: bool,
    reclaim_landed: bool,
) -> GaugeAnimation {
    let target = target.max(0.0);
    if reduce_motion() || (!in_progress && !reclaim_landed) {
        SCAN_GAUGE.with(|cell| {
            *cell.borrow_mut() = Some(GaugeTransition { from: target, to: target, started: Instant::now() });
        });
        return GaugeAnimation { from: target, to: target, duration: 0.0 };
    }

    SCAN_GAUGE.with(|cell| {
        let now = Instant::now();
        let mut state = cell.borrow_mut();
        let Some(previous) = state.as_mut() else {
            *state = Some(GaugeTransition { from: target, to: target, started: now });
            return GaugeAnimation { from: target, to: target, duration: 0.0 };
        };
        let current = gauge_width(previous, now);
        if reclaim_landed {
            *previous = GaugeTransition { from: current, to: target, started: now };
            return GaugeAnimation { from: current, to: target, duration: GLIDE };
        }
        let target = if sizing { target.max(previous.to).max(current) } else { target };
        if target < current {
            *previous = GaugeTransition { from: target, to: target, started: now };
            return GaugeAnimation { from: target, to: target, duration: 0.0 };
        }
        let duration = if (target - previous.to).abs() < f64::EPSILON {
            (GLIDE - previous.started.elapsed().as_secs_f64()).max(0.0)
        } else {
            *previous = GaugeTransition { from: current, to: target, started: now };
            GLIDE
        };
        GaugeAnimation { from: current, to: target, duration }
    })
}

pub fn reset_scan_gauge() {
    SCAN_GAUGE.with(|cell| *cell.borrow_mut() = None);
}

pub fn discovery_sweep_enabled() -> bool {
    !reduce_motion()
}

pub fn install_discovery_sweep(view: Retained<NSBox>, track_x: f64, track_width: f64) {
    if reduce_motion() {
        view.removeFromSuperview();
        stop_discovery_sweep();
        return;
    }
    let phase = DISCOVERY_SWEEP.with(|cell| cell.borrow().as_ref().map_or(0.0, |sweep| sweep.phase.get()));
    let sweep = Sweep { view, track_x, track_width, phase: Cell::new(phase) };
    set_sweep_frame(&sweep);
    DISCOVERY_SWEEP.with(|cell| *cell.borrow_mut() = Some(sweep));
}

pub fn advance_discovery_sweep() -> bool {
    if reduce_motion() {
        stop_discovery_sweep();
        return false;
    }
    DISCOVERY_SWEEP.with(|cell| {
        let state = cell.borrow();
        let Some(sweep) = state.as_ref() else { return false };
        sweep.phase.set((sweep.phase.get() + DISCOVERY_SWEEP_INTERVAL / SWEEP_PERIOD) % 1.0);
        set_sweep_frame(sweep);
        true
    })
}

pub fn stop_discovery_sweep() {
    DISCOVERY_SWEEP.with(|cell| {
        if let Some(sweep) = cell.borrow_mut().take() {
            sweep.view.removeFromSuperview();
        }
    });
}

fn gauge_width(transition: &GaugeTransition, now: Instant) -> f64 {
    let progress = (now.duration_since(transition.started).as_secs_f64() / GLIDE).clamp(0.0, 1.0);
    transition.from + (transition.to - transition.from) * progress
}

fn set_sweep_frame(sweep: &Sweep) {
    let travel = sweep.track_width + SWEEP_WIDTH;
    let x = sweep.track_x - SWEEP_WIDTH + travel * sweep.phase.get();
    sweep.view.setFrame(NSRect::new(
        NSPoint::new(x, sweep.view.frame().origin.y),
        NSSize::new(SWEEP_WIDTH, sweep.view.frame().size.height),
    ));
}

fn grouped(duration: f64, body: impl FnOnce()) {
    NSAnimationContext::beginGrouping();
    NSAnimationContext::currentContext().setDuration(duration);
    body();
    NSAnimationContext::endGrouping();
}

#[cfg(test)]
mod tests {
    use super::{reset_scan_gauge, scan_gauge_animation};

    #[test]
    fn closing_scan_forgets_gauge_transition_before_settled_reopen() {
        reset_scan_gauge();
        let _ = scan_gauge_animation(4.0, true, true, false);
        let _ = scan_gauge_animation(8.0, true, true, false);
        crate::tasks::discovery_sweep_stopped();

        let animation = scan_gauge_animation(16.0, false, false, false);
        assert_eq!(animation.from, 16.0);
        assert_eq!(animation.to, 16.0);
        assert_eq!(animation.duration, 0.0);
    }

    /// Clicking away dismisses a `Transient` popover without reaching
    /// `popover::close`, so nothing clears the transition. A settled header
    /// must draw its bar outright rather than glide to a figure that stopped
    /// moving while the popover was shut.
    #[test]
    fn a_settled_reopen_draws_outright_even_though_nothing_was_forgotten() {
        reset_scan_gauge();
        let _ = scan_gauge_animation(4.0, true, true, false);
        let _ = scan_gauge_animation(8.0, true, true, false);

        let animation = scan_gauge_animation(16.0, false, false, false);
        assert_eq!(animation.from, 16.0);
        assert_eq!(animation.to, 16.0);
        assert_eq!(animation.duration, 0.0);
    }

    #[test]
    fn reclaim_landing_glides_once_but_settled_redraw_still_snaps() {
        reset_scan_gauge();
        let _ = scan_gauge_animation(16.0, false, false, false);

        let landing = scan_gauge_animation(6.0, false, false, true);
        assert_eq!(landing.from, 16.0);
        assert_eq!(landing.to, 6.0);
        assert!(landing.duration > 0.0);

        let settled = scan_gauge_animation(4.0, false, false, false);
        assert_eq!(settled.from, 4.0);
        assert_eq!(settled.to, 4.0);
        assert_eq!(settled.duration, 0.0);
    }
}
