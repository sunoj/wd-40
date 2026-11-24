// The cleaning plate: steel painted once, the job's tiles over it, and the can.
// Removals are played one at a time — the can crosses to a tile, the tile comes
// up off the steel — so each is seen in full however fast the deletes land. A
// tile is queued only once its target has really gone: the plate may run behind
// the job, never ahead of it. Exports: `plate_view`, `PlateView`.

use crate::can::{can_view, CanView, SPRAY};
use crate::choreo::{can_origin, centre, drain_seconds, home, pace, LIFT, TRAVEL};
use crate::crust::paint_steel;
use crate::metal::reduce_motion;
use crate::motion;
use crate::state::CleanProgress;
use crate::tile::{tile_view, Patch, TileView};
use crate::treemap::{item_state, ACTIVE, DONE, IDLE, PENDING};
use crate::widgets::add_fill;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Sel};
use objc2::{define_class, msg_send, sel, DefinedClass, MainThreadOnly};
use objc2_app_kit::{NSBox, NSTextField, NSView};
use objc2_foundation::{MainThreadMarker, NSPoint, NSRect, NSString, NSTimer};
use std::cell::{Cell, OnceCell, RefCell};
use std::collections::VecDeque;

/// Diameter of the burst of spray over a tile as it starts to lift.
const PUFF: f64 = 46.0;

/// How many times the steel has been painted, so a debug run can show that a
/// whole clean costs one paint of it and not one per frame.
#[cfg(debug_assertions)]
pub static STEEL_PAINTS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// The views laid on the steel, made after the plate they are added to.
struct Laid {
    tiles: Vec<Retained<TileView>>,
    can: Retained<CanView>,
    puff: Retained<NSBox>,
}

pub struct PlateIvars {
    /// The tiles as planned, fixed for the run. `members` are slots in the
    /// job's item list; a tile with none is not part of the job.
    patches: Vec<Patch>,
    /// How much of the volume this job is, fixed for the run.
    share: String,
    /// Figures live under the plate, not on it: small tiles have no room.
    legend: Retained<NSTextField>,
    dark: bool,
    laid: OnceCell<Laid>,
    /// Tiles whose targets have really gone and whose lift is still to play.
    queue: RefCell<VecDeque<usize>>,
    /// True from the can setting off until its tile has finished lifting.
    playing: Cell<bool>,
}

define_class!(
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[ivars = PlateIvars]
    #[name = "WD40CleanPlate"]
    pub struct PlateView;

    impl PlateView {
        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            #[cfg(debug_assertions)]
            STEEL_PAINTS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let footprints: Vec<NSRect> = self.ivars().patches.iter().map(|patch| patch.rect).collect();
            paint_steel(self.bounds(), self.ivars().dark, &footprints);
        }

        /// The can has arrived: its tile comes off.
        #[unsafe(method(liftTick:))]
        fn lift_tick(&self, _timer: *mut AnyObject) {
            let Some(laid) = self.ivars().laid.get() else { return };
            let Some(index) = self.ivars().queue.borrow_mut().pop_front() else { return self.rest() };
            let tile = &laid.tiles[index];
            let aim = centre(tile.frame());
            laid.puff.setFrameOrigin(NSPoint::new(aim.x - PUFF / 2.0, aim.y - PUFF / 2.0));
            motion::puff(&laid.puff);
            tile.mark_gone();
            let seconds = LIFT * pace(self.waiting());
            motion::lift(tile, seconds);
            self.after(seconds, sel!(nextTick:));
        }

        /// That lift has played in full; the next one may start.
        #[unsafe(method(nextTick:))]
        fn next_tick(&self, _timer: *mut AnyObject) {
            self.rest();
        }
    }
);

impl PlateView {
    /// Take the run's new state. Progress hops are coalesced, so any number of
    /// tiles may have changed. A target that has gone joins the queue of lifts;
    /// every other change shows at once, since none of them claims a removal.
    pub fn update(&self, progress: &CleanProgress) {
        let ivars = self.ivars();
        let Some(laid) = ivars.laid.get() else { return };
        for (index, (patch, tile)) in ivars.patches.iter().zip(&laid.tiles).enumerate() {
            let real = patch_state(patch, progress);
            if real == tile.state() || ivars.queue.borrow().contains(&index) {
                continue;
            }
            match real {
                DONE if reduce_motion() => {
                    tile.mark_gone();
                    tile.setAlphaValue(0.0);
                }
                DONE => ivars.queue.borrow_mut().push_back(index),
                _ => tile.show(real),
            }
        }
        self.show_legend(progress);
        self.play();
    }

    /// Seconds until every queued lift has played, for whoever is waiting to
    /// leave this screen.
    pub fn seconds_left(&self) -> f64 {
        let waiting = self.waiting();
        drain_seconds(waiting) + if self.ivars().playing.get() { TRAVEL + LIFT } else { 0.0 }
    }

    fn waiting(&self) -> usize {
        self.ivars().queue.borrow().len()
    }

    /// Start the next lift, unless one is playing or none is waiting.
    fn play(&self) {
        let ivars = self.ivars();
        let (Some(laid), Some(index)) = (ivars.laid.get(), ivars.queue.borrow().front().copied()) else { return };
        if ivars.playing.replace(true) {
            return;
        }
        let seconds = TRAVEL * pace(self.waiting());
        let to = can_origin(centre(laid.tiles[index].frame()), self.bounds());
        motion::travel(&laid.can, NSRect::new(to, laid.can.frame().size), seconds);
        self.after(seconds, sel!(liftTick:));
    }

    fn rest(&self) {
        self.ivars().playing.set(false);
        // A plate that has left the screen has nobody to play to.
        match self.window().is_some() {
            true => self.play(),
            false => self.ivars().queue.borrow_mut().clear(),
        }
    }

    /// One shot, once per step of a lift: there is no per-frame clock.
    fn after(&self, seconds: f64, selector: Sel) {
        let target: &AnyObject = unsafe { &*(self as *const Self as *const AnyObject) };
        unsafe {
            NSTimer::scheduledTimerWithTimeInterval_target_selector_userInfo_repeats(seconds, target, selector, None, false);
        }
    }

    fn show_legend(&self, progress: &CleanProgress) {
        let text = format!("{} \u{00b7} {}/{} clear", self.ivars().share, progress.done_count, progress.total_count);
        self.ivars().legend.setStringValue(&NSString::from_str(&text));
    }

    /// Lay the tiles, the puff and the can over the steel in their settled
    /// pose for `progress`. Nothing is animated here.
    fn lay(&self, progress: &CleanProgress, mtm: MainThreadMarker) {
        let bounds = self.bounds();
        let patches = &self.ivars().patches;
        let tiles: Vec<Retained<TileView>> = patches
            .iter()
            .enumerate()
            .map(|(index, patch)| tile_view(index, patch, patch_state(patch, progress), mtm))
            .collect();
        for tile in &tiles {
            self.addSubview(tile);
        }
        let puff = add_fill(self, 0.0, 0.0, PUFF, PUFF, (1.0, 1.0, 1.0), 1.0, PUFF / 2.0, mtm);
        puff.setAlphaValue(0.0);
        // The can rests on the largest ground already cleared, if any has been.
        let cleared = tiles.iter().find(|tile| tile.state() == DONE).map(|tile| centre(tile.frame()));
        let can = can_view(SPRAY, can_origin(cleared.unwrap_or(home(bounds)), bounds), mtm);
        self.addSubview(&can);
        let _ = self.ivars().laid.set(Laid { tiles, can, puff });
    }

    /// Where the render tree has each tile right now — alpha, then the corner
    /// of its frame — and the can. Read off the presentation layers, which is
    /// what is on screen mid-animation.
    #[cfg(debug_assertions)]
    pub fn sample(&self) -> String {
        let shown = |view: &NSView| -> (f32, NSRect) {
            unsafe {
                let layer: *mut AnyObject = msg_send![view, layer];
                if layer.is_null() {
                    return (-1.0, view.frame());
                }
                let live: *mut AnyObject = msg_send![layer, presentationLayer];
                let from = if live.is_null() { layer } else { live };
                (msg_send![from, opacity], msg_send![from, frame])
            }
        };
        let Some(laid) = self.ivars().laid.get() else { return String::new() };
        let tiles: Vec<String> = laid.tiles.iter().map(|tile| {
            let (alpha, frame) = shown(tile);
            format!("{alpha:.2}@{:.0},{:.0}", frame.origin.x, frame.origin.y)
        }).collect();
        let (_, can) = shown(&laid.can);
        format!("can {:.0},{:.0} tiles [{}]", can.origin.x, can.origin.y, tiles.join(" "))
    }
}

/// What a tile should show for the job as it really stands. A tile shared by
/// the tail is gone only when every target in it is.
fn patch_state(patch: &Patch, progress: &CleanProgress) -> u8 {
    let states: Vec<u8> = patch.members.iter().filter_map(|slot| progress.items.get(*slot)).map(|item| item_state(item.status)).collect();
    match states.as_slice() {
        [] => IDLE,
        [one] => *one,
        all if all.iter().all(|state| *state == DONE) => DONE,
        all if all.iter().all(|state| *state == PENDING) => PENDING,
        all if all.iter().any(|state| *state == ACTIVE || *state == PENDING) => ACTIVE,
        all => *all.iter().find(|state| **state != DONE).unwrap_or(&DONE),
    }
}

/// The plate in its settled pose for `progress`: built from the state alone,
/// so what it shows never depends on an animation having finished.
pub fn plate_view(
    frame: NSRect,
    patches: Vec<Patch>,
    progress: &CleanProgress,
    share: String,
    legend: Retained<NSTextField>,
    dark: bool,
    mtm: MainThreadMarker,
) -> Retained<PlateView> {
    let view = mtm.alloc::<PlateView>().set_ivars(PlateIvars {
        patches,
        share,
        legend,
        dark,
        laid: OnceCell::new(),
        queue: RefCell::new(VecDeque::new()),
        playing: Cell::new(false),
    });
    let view: Retained<PlateView> = unsafe { msg_send![super(view), initWithFrame: frame] };
    // Layer-backed, so a tile lifting or the can crossing is composited from
    // what was already painted and the steel under them is never redrawn.
    view.setWantsLayer(true);
    view.lay(progress, mtm);
    view.show_legend(progress);
    view
}
