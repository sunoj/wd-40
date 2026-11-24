// How a clean is going: how long this run has been working, and the only ETA
// the app is entitled to quote. Kept apart from the cleaning screen because it
// is arithmetic over what has really been removed, not layout.
// Exports: `elapsed`, `eta_label`. Deps: crate::state, wd40::disk.

use crate::state::{CleanItemStatus, CleanProgress};
use std::cell::Cell;
use std::time::Instant;
use wd40::disk::sum_bytes;

thread_local! {
    /// When this run started, kept only so throughput can be measured.
    static CLOCK: Cell<Option<(Instant, usize, usize)>> = const { Cell::new(None) };
}

/// Seconds this run has been measurably working. A run is new when nothing has
/// come off yet, when the finished count drops, or when the queue length
/// changes.
pub fn elapsed(p: &CleanProgress) -> f64 {
    let now = Instant::now();
    CLOCK.with(|cell| {
        let fresh = (p.done_count == 0 && p.freed_so_far == 0)
            || match cell.get() {
                Some((_, done, total)) => p.done_count < done || p.total_count != total,
                None => true,
            };
        let start = if fresh { now } else { cell.get().map_or(now, |value| value.0) };
        cell.set(Some((start, p.done_count, p.total_count)));
        now.duration_since(start).as_secs_f64()
    })
}

/// The only ETA this app is entitled to: bytes it has actually removed divided
/// by the seconds that took, applied to the bytes still queued. Until something
/// has been removed there is no rate to extrapolate, so there is no label.
pub fn eta_label(p: &CleanProgress, elapsed: f64) -> Option<String> {
    if p.done_count == 0 || p.freed_so_far == 0 || elapsed < 2.0 {
        return None;
    }
    let remaining = queued_bytes(p);
    if remaining == 0 {
        return None;
    }
    let seconds = remaining as f64 * elapsed / p.freed_so_far as f64;
    if !seconds.is_finite() || seconds > 3600.0 {
        return None;
    }
    Some(format!("~{} left", short_duration(seconds)))
}

/// Bytes still to be attempted, which is what an ETA may extrapolate over.
fn queued_bytes(p: &CleanProgress) -> u64 {
    sum_bytes(
        p.items
            .iter()
            .filter(|i| matches!(i.status, CleanItemStatus::Pending | CleanItemStatus::Active))
            .map(|i| i.size_bytes),
    )
}

fn short_duration(seconds: f64) -> String {
    if seconds < 90.0 {
        return format!("{}s", (seconds.ceil() as u64).max(1));
    }
    format!("{}m", (seconds / 60.0).ceil() as u64)
}

#[cfg(test)]
mod tests {
    use super::short_duration;

    #[test]
    fn durations_read_in_seconds_then_minutes() {
        assert_eq!(short_duration(0.2), "1s");
        assert_eq!(short_duration(47.1), "48s");
        assert_eq!(short_duration(200.0), "4m");
    }
}
