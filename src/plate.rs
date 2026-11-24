// The cleaning screen's plate: steel painted once, a rust tile per target laid
// over it, and the can. Nothing here runs on a clock. A tile lifts when — and
// only when — its target's removal has really finished, and the can moves to
// that tile and nowhere else, so every step on screen is one target gone.
// Exports: `plate_view`, `PlateView`. Deps: crate::{can, crust, legend, motion, treemap}.

use crate::can::{can_view, CanView, SPRAY};
use crate::crust::{crust_region, paint_steel, paint_tile, share_line};
use crate::legend::{idle_text, tile_tip};
use crate::motion;
use crate::state::CleanProgress;
use crate::treemap::{tiles, Tile, ACTIVE, DONE, PENDING};
use crate::widgets::add_fill;
use objc2::rc::Retained;
use objc2::{define_class, msg_send, DefinedClass, MainThreadOnly};
use objc2_app_kit::{NSBox, NSTextField, NSView};
use objc2_foundation::{MainThreadMarker, NSPoint, NSRect, NSSize, NSString};
use std::cell::{Cell, OnceCell};
use wd40::disk::{sum_bytes, DiskSpace};

/// How much of a tile shows once the run has left its target on disk.
const LEFT_BEHIND: f64 = 0.6;
/// Diameter of the burst of spray over a tile that has just lifted.
const PUFF: f64 = 44.0;

/// How many times the steel has been painted, so a debug run can show that a
/// whole clean costs one paint of it and not one per frame.
#[cfg(debug_assertions)]
pub static STEEL_PAINTS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

pub struct TileIvars {
    index: usize,
    state: Cell<u8>,
}

define_class!(
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[ivars = TileIvars]
    #[name = "WD40RustTile"]
    pub struct TileView;

    impl TileView {
        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            paint_tile(self.bounds(), self.ivars().index, self.ivars().state.get());
        }
    }
);

/// The views laid on the steel. They are made after the plate exists, because
/// that is what they are added to.
struct Laid {
    /// One view per tile, in the treemap's own order, which is fixed for a run.
    tiles: Vec<Retained<TileView>>,
    can: Retained<CanView>,
    puff: Retained<NSBox>,
}

pub struct PlateIvars {
    /// The footprints scribed into the steel: the run's tiles as first laid out.
    scribed: Vec<Tile>,
    /// Region of the plate the crust covers, fixed for the run.
    crust: NSRect,
    /// How much of the volume this job is, fixed for the run.
    share: String,
    /// Figures live under the plate, not on it: small tiles have no room.
    legend: Retained<NSTextField>,
    dark: bool,
    laid: OnceCell<Laid>,
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
            paint_steel(self.bounds(), self.ivars().dark, &self.ivars().scribed);
        }
    }
);

impl PlateView {
    /// Take the run's new state in place. Progress hops are coalesced, so any
    /// number of tiles may have changed since the last call; each one that has
    /// is handled on its own and the rest are not touched.
    pub fn update(&self, progress: &CleanProgress) {
        let ivars = self.ivars();
        let Some(laid) = ivars.laid.get() else { return };
        let fresh = tiles(progress, ivars.crust);
        let mut lifted: Option<&Tile> = None;
        for (tile, view) in fresh.iter().zip(laid.tiles.iter()) {
            let before = view.ivars().state.replace(tile.state);
            if before == tile.state {
                continue;
            }
            view.setToolTip(Some(&NSString::from_str(&tile_tip(tile))));
            match tile.state {
                // Really gone. The largest of a hop is where the can goes.
                DONE => {
                    motion::lift(view);
                    lifted = lifted.or(Some(tile));
                }
                _ => {
                    view.setNeedsDisplay(true);
                    motion::fade(view, resting_alpha(tile.state), motion::GLIDE);
                }
            }
        }
        if let Some(tile) = lifted {
            self.spray(laid, tile.rect);
        }
        self.show_legend(progress);
    }

    /// Bring the can to ground a target has just left, and puff once over it.
    fn spray(&self, laid: &Laid, ground: NSRect) {
        let aim = centre(ground);
        motion::glide(&laid.can, NSRect::new(can_origin(aim, self.bounds()), laid.can.frame().size));
        laid.puff.setFrameOrigin(NSPoint::new(aim.x - PUFF / 2.0, aim.y - PUFF / 2.0));
        motion::puff(&laid.puff);
    }

    fn show_legend(&self, progress: &CleanProgress) {
        let text = idle_text(&self.ivars().share, progress);
        self.ivars().legend.setStringValue(&NSString::from_str(&text));
    }

    /// Lay the tiles, the puff and the can over the steel in their settled
    /// pose for the state the plate was built from. Nothing is animated here.
    fn lay(&self, mtm: MainThreadMarker) {
        let bounds = self.bounds();
        let scribed = &self.ivars().scribed;
        let tiles: Vec<Retained<TileView>> =
            scribed.iter().enumerate().map(|(index, tile)| tile_view(index, tile, mtm)).collect();
        for tile in &tiles {
            self.addSubview(tile);
        }
        let puff = add_fill(self, 0.0, 0.0, PUFF, PUFF, (1.0, 1.0, 1.0), 1.0, PUFF / 2.0, mtm);
        puff.setAlphaValue(0.0);
        // The can rests on the largest ground already cleared, if any has been.
        let rest = scribed.iter().find(|tile| tile.state == DONE).map_or(home(bounds), |tile| centre(tile.rect));
        let can = can_view(SPRAY, can_origin(rest, bounds), mtm);
        self.addSubview(&can);
        let _ = self.ivars().laid.set(Laid { tiles, can, puff });
    }
}

/// Tiles that are queued or coming off are solid; a removed one is not there;
/// anything the run left on disk stays, dimmed.
fn resting_alpha(state: u8) -> f64 {
    match state {
        PENDING | ACTIVE => 1.0,
        DONE => 0.0,
        _ => LEFT_BEHIND,
    }
}

fn centre(rect: NSRect) -> NSPoint {
    NSPoint::new(rect.origin.x + rect.size.width / 2.0, rect.origin.y + rect.size.height / 2.0)
}

/// Where the can's view goes so its straw tip lands on `aim`, kept whole inside
/// the plate: against the right or top edge the can stops and the tip falls
/// short, which reads better than a can sliced by the plate's boundary.
fn can_origin(aim: NSPoint, plate: NSRect) -> NSPoint {
    let tip = SPRAY.tip();
    NSPoint::new(
        (aim.x - tip.x).clamp(0.0, (plate.size.width - SPRAY.size.0).max(0.0)),
        (aim.y - tip.y).clamp(0.0, (plate.size.height - SPRAY.size.1).max(0.0)),
    )
}

/// Where the can waits before anything has been removed: over the plate's
/// bottom right corner, where the treemap puts its smallest tiles.
fn home(plate: NSRect) -> NSPoint {
    NSPoint::new(plate.size.width - 60.0, 12.0)
}

/// The plate in its settled pose for `progress`: built from the state alone,
/// so what it shows never depends on an animation having finished.
pub fn plate_view(
    frame: NSRect,
    progress: &CleanProgress,
    disk: Option<DiskSpace>,
    legend: Retained<NSTextField>,
    dark: bool,
    mtm: MainThreadMarker,
) -> Retained<PlateView> {
    let job = sum_bytes(progress.items.iter().map(|item| item.size_bytes));
    let crust = crust_region(frame.size);
    let view = mtm.alloc::<PlateView>().set_ivars(PlateIvars {
        scribed: tiles(progress, crust),
        crust,
        share: share_line(job, disk),
        legend,
        dark,
        laid: OnceCell::new(),
    });
    let view: Retained<PlateView> = unsafe { msg_send![super(view), initWithFrame: frame] };
    // Layer-backed, so a tile fading or the can gliding is composited from
    // what was already painted and the steel under them is never redrawn.
    view.setWantsLayer(true);
    view.lay(mtm);
    view.show_legend(progress);
    view
}

fn tile_view(index: usize, tile: &Tile, mtm: MainThreadMarker) -> Retained<TileView> {
    let view = mtm.alloc::<TileView>().set_ivars(TileIvars { index, state: Cell::new(tile.state) });
    let size = NSSize::new(tile.rect.size.width.max(0.5), tile.rect.size.height.max(0.5));
    let view: Retained<TileView> = unsafe { msg_send![super(view), initWithFrame: NSRect::new(tile.rect.origin, size)] };
    view.setAlphaValue(resting_alpha(tile.state));
    view.setToolTip(Some(&NSString::from_str(&tile_tip(tile))));
    view
}

#[cfg(test)]
mod tests {
    use super::{can_origin, resting_alpha, LEFT_BEHIND};
    use crate::can::SPRAY;
    use crate::treemap::{ACTIVE, BLOCKED, DONE, PART_GONE, PENDING, SKIPPED};
    use objc2_foundation::{NSPoint, NSRect, NSSize};

    fn plate() -> NSRect {
        NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(348.0, 190.0))
    }

    #[test]
    fn only_a_removed_target_leaves_the_plate() {
        assert_eq!(resting_alpha(PENDING), 1.0);
        assert_eq!(resting_alpha(ACTIVE), 1.0);
        assert_eq!(resting_alpha(DONE), 0.0);
        for stays in [SKIPPED, PART_GONE, BLOCKED] {
            assert_eq!(resting_alpha(stays), LEFT_BEHIND);
        }
    }

    #[test]
    fn the_can_is_never_cut_off_by_the_plate() {
        for (x, y) in [(0.0, 0.0), (348.0, 190.0), (174.0, 95.0), (348.0, 0.0), (0.0, 190.0)] {
            let at = can_origin(NSPoint::new(x, y), plate());
            assert!(at.x >= 0.0 && at.x + SPRAY.size.0 <= 348.0, "{at:?}");
            assert!(at.y >= 0.0 && at.y + SPRAY.size.1 <= 190.0, "{at:?}");
        }
    }

    /// Away from the edges the straw tip sits exactly on the point aimed at.
    #[test]
    fn with_room_the_tip_lands_on_the_aim() {
        let at = can_origin(NSPoint::new(120.0, 60.0), plate());
        let tip = SPRAY.tip();
        assert!((at.x + tip.x - 120.0).abs() < 1e-9 && (at.y + tip.y - 60.0).abs() < 1e-9);
    }
}
