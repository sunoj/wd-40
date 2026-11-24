// What the plate says in words: how a tile's state reads, the tooltip each tile
// carries, and the line under the plate with the run's own figures.
// Exports: `phase`, `tile_tip`, `idle_text`.
// Deps: crate::{header, state, treemap}.

use crate::header::scan_size;
use crate::state::CleanProgress;
use crate::treemap::{Tile, ACTIVE, BLOCKED, DONE, PART_GONE, SKIPPED};

/// How a tile's state reads.
pub fn phase(state: u8) -> &'static str {
    match state {
        DONE => "removed",
        ACTIVE => "removing",
        SKIPPED => "left in place",
        PART_GONE => "partly removed \u{2014} the rest is still there",
        BLOCKED => "could not be removed",
        _ => "waiting",
    }
}

/// What hovering a tile says: which target it is, its size, where it stands.
pub fn tile_tip(tile: &Tile) -> String {
    format!("{} \u{00b7} {} \u{00b7} {}", tile.name, scan_size(tile.size), phase(tile.state))
}

/// The line under the plate: the share of the volume this job is, and how much
/// of it has really gone.
pub fn idle_text(share: &str, progress: &CleanProgress) -> String {
    format!("{share} \u{00b7} {}/{} clear", progress.done_count, progress.total_count)
}
