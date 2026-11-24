// One tile of crust: a view painted once in its group's colour, which the scan
// map ticks and the cleaning plate lifts. Also the plan both are built from —
// which entries share a tile, and what that tile is called.
// Exports: `Entry`, `Patch`, `patches`, `TileView`, `tile_view`, `resting_alpha`.

use crate::crust::{crust_region, paint_tile, Tone};
use crate::header::scan_size;
use crate::treemap::{cells, ACTIVE, DONE, IDLE, PENDING};
use objc2::rc::Retained;
use objc2::{define_class, msg_send, DefinedClass, MainThreadOnly};
use objc2_app_kit::{NSView, NSViewLayerContentsRedrawPolicy};
use objc2_foundation::{MainThreadMarker, NSPoint, NSRect, NSSize};
use std::cell::Cell;

/// How much of a tile shows once the run has left its target on disk.
const LEFT_BEHIND: f64 = 0.6;
/// A tile that is not ticked, or not in the job: there, but out of the way.
const IDLE_ALPHA: f64 = 0.42;

/// One thing the map shows: a scan target, or an item of a clean.
pub struct Entry {
    pub name: String,
    /// Where it is on disk, for the hover line. Empty when not known.
    pub path: String,
    pub bytes: u64,
    pub tone: Tone,
}

/// A tile as planned: where it goes and which entries it stands for.
pub struct Patch {
    pub rect: NSRect,
    pub members: Vec<usize>,
    pub name: String,
    pub path: String,
    pub bytes: u64,
    pub tone: Tone,
}

impl Patch {
    pub fn tip(&self) -> String {
        match self.path.is_empty() {
            true => format!("{} \u{00b7} {}", self.name, scan_size(self.bytes)),
            false => format!("{} \u{00b7} {} \u{00b7} {}", self.name, scan_size(self.bytes), self.path),
        }
    }
}

/// Plan the tiles for `entries` on a plate of `size`. The tail of small
/// entries shares one tile, named for how many it holds.
pub fn patches(entries: &[Entry], size: NSSize) -> Vec<Patch> {
    let sizes: Vec<u64> = entries.iter().map(|entry| entry.bytes).collect();
    cells(&sizes, crust_region(size))
        .into_iter()
        .map(|cell| {
            let first = &entries[cell.members[0]];
            let (name, path) = match cell.members.len() {
                1 => (first.name.clone(), first.path.clone()),
                n => (format!("{n} smaller targets"), String::new()),
            };
            Patch { rect: cell.rect, name, path, bytes: cell.bytes, tone: first.tone, members: cell.members }
        })
        .collect()
}

pub struct TileIvars {
    index: usize,
    tone: Tone,
    state: Cell<u8>,
}

define_class!(
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[ivars = TileIvars]
    #[name = "WD40CrustTile"]
    pub struct TileView;

    impl TileView {
        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            let ivars = self.ivars();
            paint_tile(self.bounds(), ivars.tone, ivars.index, ivars.state.get());
        }

        /// Clicks and hovers belong to the map the tile lies on.
        #[unsafe(method(hitTest:))]
        fn hit_test(&self, _point: NSPoint) -> *mut NSView {
            std::ptr::null_mut()
        }
    }
);

impl TileView {
    pub fn state(&self) -> u8 {
        self.ivars().state.get()
    }

    /// Change how the tile reads without moving it: repaint, and bring it to
    /// the alpha that state rests at.
    pub fn show(&self, state: u8) {
        if self.ivars().state.replace(state) == state {
            return;
        }
        self.setNeedsDisplay(true);
        crate::motion::fade(self, resting_alpha(state), crate::motion::GLIDE);
    }

    /// Record that the tile's target has gone. The caller moves it off.
    pub fn mark_gone(&self) {
        self.ivars().state.set(DONE);
    }
}

/// Tiles that are ticked, queued or coming off are solid; a removed one is not
/// there; an unticked one is faint; anything a run left on disk stays, dimmed.
pub fn resting_alpha(state: u8) -> f64 {
    match state {
        PENDING | ACTIVE => 1.0,
        DONE => 0.0,
        IDLE => IDLE_ALPHA,
        _ => LEFT_BEHIND,
    }
}

/// A tile in its settled pose for `state`, with nothing animated.
pub fn tile_view(index: usize, patch: &Patch, state: u8, mtm: MainThreadMarker) -> Retained<TileView> {
    let view = mtm.alloc::<TileView>().set_ivars(TileIvars { index, tone: patch.tone, state: Cell::new(state) });
    let size = NSSize::new(patch.rect.size.width.max(0.5), patch.rect.size.height.max(0.5));
    let view: Retained<TileView> = unsafe { msg_send![super(view), initWithFrame: NSRect::new(patch.rect.origin, size)] };
    // A tile that swells as it lifts is its own picture scaled, not a repaint.
    view.setLayerContentsRedrawPolicy(NSViewLayerContentsRedrawPolicy::OnSetNeedsDisplay);
    view.setAlphaValue(resting_alpha(state));
    view
}

#[cfg(test)]
mod tests {
    use super::{patches, resting_alpha, Entry, IDLE_ALPHA, LEFT_BEHIND};
    use crate::treemap::{ACTIVE, BLOCKED, DONE, IDLE, PART_GONE, PENDING, SKIPPED};
    use objc2_foundation::NSSize;

    #[test]
    fn only_a_removed_target_leaves_the_plate() {
        assert_eq!(resting_alpha(PENDING), 1.0);
        assert_eq!(resting_alpha(ACTIVE), 1.0);
        assert_eq!(resting_alpha(DONE), 0.0);
        assert_eq!(resting_alpha(IDLE), IDLE_ALPHA);
        for stays in [SKIPPED, PART_GONE, BLOCKED] {
            assert_eq!(resting_alpha(stays), LEFT_BEHIND);
        }
    }

    #[test]
    fn a_shared_tile_says_how_many_targets_it_holds() {
        let entries: Vec<Entry> = [50_u64, 40, 30, 20, 10]
            .iter()
            .map(|bytes| Entry { name: format!("t{bytes}"), path: "/p".into(), bytes: *bytes, tone: (0.5, 0.3, 0.2) })
            .collect();
        let laid = patches(&entries, NSSize::new(134.0, 119.0));
        assert_eq!(laid[0].name, "t50");
        assert_eq!(laid.last().unwrap().name, "3 smaller targets");
        assert_eq!(laid.last().unwrap().bytes, 60);
    }
}
