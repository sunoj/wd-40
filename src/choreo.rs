// The plate's choreography as plain arithmetic: how long each step of a lift
// takes, how a backlog is paced, and where the can goes to reach a tile.
// Exports: `TRAVEL`, `LIFT`, `pace`, `drain_seconds`, `centre`, `can_origin`, `home`.
// Deps: crate::can, objc2 Foundation geometry.

use crate::can::SPRAY;
use objc2_foundation::{NSPoint, NSRect};

/// The can crossing to a tile, then the tile coming off, at an unhurried pace.
pub const TRAVEL: f64 = 0.38;
pub const LIFT: f64 = 0.62;

/// How fast to play with `waiting` lifts queued: at full length while the plate
/// is keeping up, quicker — never skipped — once a backlog builds.
pub fn pace(waiting: usize) -> f64 {
    (3.0 / waiting.max(1) as f64).clamp(0.5, 1.0)
}

/// How long `waiting` queued lifts take to play out, one after another.
pub fn drain_seconds(waiting: usize) -> f64 {
    (1..=waiting).map(|left| (TRAVEL + LIFT) * pace(left)).sum()
}

pub fn centre(rect: NSRect) -> NSPoint {
    NSPoint::new(rect.origin.x + rect.size.width / 2.0, rect.origin.y + rect.size.height / 2.0)
}

/// Where the can's view goes so its straw tip lands on `aim`, kept whole inside
/// the plate: against the right or top edge the can stops and the tip falls
/// short, which reads better than a can sliced by the plate's boundary.
pub fn can_origin(aim: NSPoint, plate: NSRect) -> NSPoint {
    let tip = SPRAY.tip();
    NSPoint::new(
        (aim.x - tip.x).clamp(0.0, (plate.size.width - SPRAY.size.0).max(0.0)),
        (aim.y - tip.y).clamp(0.0, (plate.size.height - SPRAY.size.1).max(0.0)),
    )
}

/// Where the can waits before anything has been removed: over the plate's
/// bottom right corner, where the treemap puts its smallest tiles.
pub fn home(plate: NSRect) -> NSPoint {
    NSPoint::new(plate.size.width - 60.0, 12.0)
}

#[cfg(test)]
mod tests {
    use super::{can_origin, drain_seconds, pace, LIFT, TRAVEL};
    use crate::can::SPRAY;
    use objc2_foundation::{NSPoint, NSRect, NSSize};

    fn plate() -> NSRect {
        NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(348.0, 190.0))
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

    /// A backlog plays faster but every lift still takes real time, and the
    /// wait for the done screen is the sum of them.
    #[test]
    fn a_backlog_speeds_up_but_never_skips() {
        assert_eq!(pace(1), 1.0);
        assert_eq!(pace(30), 0.5);
        assert!((drain_seconds(1) - (TRAVEL + LIFT)).abs() < 1e-9);
        assert!(drain_seconds(8) > 8.0 * (TRAVEL + LIFT) * 0.5);
        assert_eq!(drain_seconds(0), 0.0);
    }
}
