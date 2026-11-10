// Scheduling class for the threads that walk the disk, chosen by who is
// waiting: a timer scan must never take the machine from a build, but on a
// loaded Mac a background-class pass someone is watching barely runs at all.
// Exports: `Priority`, `background`, `utility`, `workers`.
// Deps: libc.

/// `qos_class_t` from <sys/qos.h>.
const QOS_CLASS_UTILITY: libc::c_uint = 0x11;
/// Efficiency cores, lowest CPU priority, and disk I/O throttled by the kernel
/// — the last of which is what a directory walk actually competes for.
const QOS_CLASS_BACKGROUND: libc::c_uint = 0x09;

extern "C" {
    fn pthread_set_qos_class_self_np(
        qos_class: libc::c_uint,
        relative_priority: libc::c_int,
    ) -> libc::c_int;
}

/// Who a pass is for. Background class on a Mac with load far above its core
/// count left a CLI scan at 0.7 s of CPU after 21 minutes; a person waiting is
/// owed the utility class, which still yields to the foreground.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Priority {
    /// The CLI, or a scan whose popover is open.
    Waited,
    /// A timer scan or auto-clean nobody is looking at.
    Housekeeping,
}

impl Priority {
    /// Put the calling thread in this priority's class.
    pub fn apply(self) {
        match self {
            Self::Waited => utility(),
            Self::Housekeeping => background(),
        }
    }
}

/// Put the calling thread in the background class. For work nobody asked for:
/// the scan that runs on a timer.
pub fn background() {
    set(QOS_CLASS_BACKGROUND);
}

/// Put the calling thread in the utility class. For work someone is waiting on
/// — a scan they asked for by opening the popover or pressing Rescan — which
/// should still yield to the foreground, just not be throttled to a crawl.
pub fn utility() {
    set(QOS_CLASS_UTILITY);
}

fn set(class: libc::c_uint) {
    // A failure here costs nothing but scheduling politeness, so it is not
    // worth propagating: the walk is correct at any priority.
    unsafe { pthread_set_qos_class_self_np(class, 0) };
}

/// How many directory reads to keep in flight.
///
/// This was 16, chosen as the point where an internal SSD stops overlapping
/// reads. That is the right number for finishing fastest and the wrong one for
/// a menu bar app: sixteen threads reading at once is a visible stall on a
/// machine that is already compiling. Half the cores, capped, leaves the
/// machine usable and — with sizes remembered between scans — costs a second
/// on the one scan that has to do the work.
pub fn workers() -> usize {
    let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
    (cores / 2).clamp(2, 6)
}

#[cfg(test)]
mod tests {
    use super::{background, utility, workers};

    #[test]
    fn the_pool_stays_small_enough_to_share_the_machine() {
        let count = workers();
        assert!((2..=6).contains(&count), "{count}");
    }

    /// Setting a class must not panic or poison the thread whatever the OS
    /// makes of it — the walk has to keep working at any priority.
    #[test]
    fn setting_a_class_is_harmless() {
        background();
        utility();
        background();
    }
}
