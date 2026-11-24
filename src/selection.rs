// Per-item clean selection: nothing starts checked; the user opts in. Also
// which targets the list is entitled to show at all.
// Exports: `default_selection`, `selected_bytes`, `is_recent`, `empty_measured`,
// `settled_order`. Deps: std, wd40::scanner::TargetDir.

use std::collections::HashSet;
use std::time::{Duration, SystemTime};
use wd40::scanner::{ArtifactGroup, ArtifactKind, TargetDir};

const SECONDS_PER_DAY: u64 = 86_400;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum GroupSelection {
    Off,
    Mixed,
    On,
}

/// A single row is only ever on or off; only a group can be part-ticked.
impl From<bool> for GroupSelection {
    fn from(on: bool) -> Self {
        match on {
            true => Self::On,
            false => Self::Off,
        }
    }
}

/// True when the artifact is newer than `max_age_days` (still "recent").
pub fn is_recent(target: &TargetDir, max_age_days: u64) -> bool {
    let max_age = Duration::from_secs(max_age_days.saturating_mul(SECONDS_PER_DAY));
    SystemTime::now()
        .duration_since(target.last_modified)
        .map(|age| age < max_age)
        .unwrap_or(false)
}

/// The batch "tick the old ones" stands for: every listed target that is past
/// the keep threshold by the same test as the RECENT badge. It only names
/// positions; ticking them changes a selection and nothing else. Toolchains
/// and simulator devices stay out, as they do for auto-clean: their age says
/// nothing about whether they are still in use, so they are ticked by hand.
pub fn old_targets(targets: &[TargetDir], empty: &HashSet<usize>, max_age_days: u64) -> HashSet<usize> {
    (0..targets.len())
        .filter(|index| !empty.contains(index) && in_old_batch(&targets[*index], max_age_days))
        .collect()
}

fn in_old_batch(target: &TargetDir, max_age_days: u64) -> bool {
    !matches!(target.kind, ArtifactKind::Toolchain | ArtifactKind::Simulator)
        && !is_recent(target, max_age_days)
}

/// True when no listed target — of `group`, or of any group — is past the keep
/// threshold. This is the list's own per-row test, the one that folds a row
/// away as recent, so a group reads as clear exactly when it would show no
/// old rows. It is deliberately not `old_targets`: an old simulator is an old
/// row even though the tick-old batch leaves it alone.
pub fn nothing_old(targets: &[TargetDir], empty: &HashSet<usize>, group: Option<ArtifactGroup>, max_age_days: u64) -> bool {
    !targets.iter().enumerate().any(|(index, target)| {
        !empty.contains(&index)
            && group.is_none_or(|group| target.kind.group() == group)
            && !is_recent(target, max_age_days)
    })
}

/// Nothing is checked after a scan. Deleting here is irreversible and skips the
/// Trash, so a popover that opens already armed to delete is not a default
/// anyone should be handed — the user opts in, per row or per group.
pub fn default_selection(_targets: &[TargetDir], _max_age_days: u64) -> HashSet<usize> {
    HashSet::new()
}

/// Targets the list leaves out: measured, and holding nothing. A directory that
/// returns nothing is noise in a list you scroll.
///
/// Measured is the whole point of the test. Sizes land one at a time, and a
/// target reads zero until its own figure arrives — filtering on "zero bytes"
/// would hide every row that has not been measured yet and pop them into
/// existence one by one as the pass ran.
pub fn empty_measured(targets: &[TargetDir], measured: &HashSet<usize>) -> HashSet<usize> {
    measured
        .iter()
        .copied()
        .filter(|index| targets.get(*index).is_some_and(|target| target.size_bytes == 0))
        .collect()
}

/// The order the list settles into once every figure is in: largest first, with
/// the measured-empty left out for good. Returns positions in `targets`.
pub fn settled_order(targets: &[TargetDir], measured: &HashSet<usize>) -> Vec<usize> {
    let empty = empty_measured(targets, measured);
    let mut order: Vec<usize> = (0..targets.len()).filter(|index| !empty.contains(index)).collect();
    order.sort_by_key(|index| std::cmp::Reverse(targets[*index].size_bytes));
    order
}

pub fn selected_bytes(targets: &[TargetDir], selected: &HashSet<usize>) -> u64 {
    selected
        .iter()
        .filter_map(|&i| targets.get(i).map(|td| td.size_bytes))
        .fold(0_u64, |sum, n| sum.saturating_add(n))
}

/// The rows of one group that are on screen. An empty target never takes part
/// in a group's state or its toggle: the box says what the visible rows say,
/// and ticking it can only ever select what the user can see.
fn group_rows(targets: &[TargetDir], group: ArtifactGroup, empty: &HashSet<usize>) -> Vec<usize> {
    targets
        .iter()
        .enumerate()
        .filter(|(index, target)| target.kind.group() == group && !empty.contains(index))
        .map(|(index, _)| index)
        .collect()
}

pub fn group_selection(
    targets: &[TargetDir],
    selected: &HashSet<usize>,
    group: ArtifactGroup,
    empty: &HashSet<usize>,
) -> GroupSelection {
    let indices = group_rows(targets, group, empty);
    let count = indices.len();
    let selected_count = indices.iter().filter(|index| selected.contains(index)).count();
    match selected_count {
        0 => GroupSelection::Off,
        selected_count if selected_count == count => GroupSelection::On,
        _ => GroupSelection::Mixed,
    }
}

pub fn toggle_group(
    targets: &[TargetDir],
    selected: &mut HashSet<usize>,
    group: ArtifactGroup,
    empty: &HashSet<usize>,
) {
    let indices = group_rows(targets, group, empty);
    let clear = indices.iter().all(|index| selected.contains(index));
    for index in indices {
        if clear {
            selected.remove(&index);
        } else {
            selected.insert(index);
        }
    }
}

#[allow(dead_code)]
pub fn age_days(target: &TargetDir) -> u64 {
    SystemTime::now()
        .duration_since(target.last_modified)
        .map(|d| d.as_secs() / SECONDS_PER_DAY)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::{
        default_selection, empty_measured, group_selection, is_recent, settled_order, toggle_group,
        GroupSelection,
    };
    use std::collections::HashSet;
    use std::path::PathBuf;
    use std::time::{Duration, SystemTime};
    use wd40::scanner::{ArtifactGroup, ArtifactKind, TargetDir};

    fn target(age_days: u64) -> TargetDir {
        sized(age_days, 10)
    }

    fn sized(age_days: u64, size_bytes: u64) -> TargetDir {
        TargetDir {
            path: PathBuf::from("/tmp/cc-target-x"),
            size_bytes,
            last_modified: SystemTime::now() - Duration::from_secs(age_days * 86_400),
            kind: ArtifactKind::TmpTarget,
        }
    }

    #[test]
    fn nothing_is_armed_to_delete_by_default() {
        // Age still drives the "recent" badge, but it must not pre-arm a row:
        // the popover opens with an empty selection whatever the ages are.
        let targets = vec![target(1), target(10)];
        assert!(default_selection(&targets, 3).is_empty());
        assert!(is_recent(&targets[0], 3));
        assert!(!is_recent(&targets[1], 3));
    }

    #[test]
    fn group_toggle_selects_all_then_clears_all() {
        let targets = vec![target(1), target(10)];
        let none = HashSet::new();
        let mut selected = HashSet::from([0]);
        let state = |selected: &HashSet<usize>| {
            group_selection(&targets, selected, ArtifactGroup::Rust, &none)
        };
        assert!(state(&selected) == GroupSelection::Mixed);
        toggle_group(&targets, &mut selected, ArtifactGroup::Rust, &none);
        assert!(state(&selected) == GroupSelection::On);
        toggle_group(&targets, &mut selected, ArtifactGroup::Rust, &none);
        assert!(state(&selected) == GroupSelection::Off);
    }

    /// The group box can only ever tick what the list shows. A target that is
    /// not on screen must not end up in a job the user cannot see.
    #[test]
    fn a_hidden_target_is_outside_the_group_box() {
        let targets = vec![sized(1, 10), sized(10, 0)];
        let empty = HashSet::from([1]);
        let mut selected = HashSet::new();
        toggle_group(&targets, &mut selected, ArtifactGroup::Rust, &empty);
        assert_eq!(selected, HashSet::from([0]));
        assert!(group_selection(&targets, &selected, ArtifactGroup::Rust, &empty) == GroupSelection::On);
    }

    /// Zero on its own means nothing: it is what every row reads before its own
    /// figure lands.
    #[test]
    fn only_a_measured_zero_counts_as_empty() {
        let targets = vec![sized(1, 0), sized(1, 0), sized(1, 40)];
        let measured = HashSet::from([0, 2]);
        assert_eq!(empty_measured(&targets, &measured), HashSet::from([0]));
        assert_eq!(settled_order(&targets, &measured), vec![2, 1]);
    }

    /// Ticking the old batch takes exactly the targets past the threshold:
    /// none that are recent, and none the list has dropped as empty.
    #[test]
    fn the_old_batch_is_exactly_the_targets_past_the_threshold() {
        use std::time::{Duration, SystemTime};
        let at = |days: u64| TargetDir {
            path: std::path::PathBuf::from(format!("/w/{days}/target")),
            size_bytes: 1,
            last_modified: SystemTime::now() - Duration::from_secs(days * 86_400 + 60),
            kind: wd40::scanner::ArtifactKind::RustTarget,
        };
        let kind = |kind, target: TargetDir| TargetDir { kind, ..target };
        let targets = [
            at(0), at(6), at(7), at(8), at(400), at(90),
            kind(wd40::scanner::ArtifactKind::Simulator, at(90)),
            kind(wd40::scanner::ArtifactKind::Toolchain, at(90)),
        ];
        let empty = std::collections::HashSet::from([5]);
        let old = super::old_targets(&targets, &empty, 7);
        assert_eq!(old, std::collections::HashSet::from([2, 3, 4]));
        assert!(old.iter().all(|index| !super::is_recent(&targets[*index], 7)));
    }

    /// A group is clear exactly when it has no old rows, whatever tick-old
    /// would do with them: old simulators keep their group from being clear.
    #[test]
    fn clear_follows_the_rows_not_the_tick_old_batch() {
        use std::time::{Duration, SystemTime};
        use wd40::scanner::{ArtifactGroup, ArtifactKind};
        let at = |kind: ArtifactKind, days: u64| TargetDir {
            path: std::path::PathBuf::from(format!("/w/{days}/x")),
            size_bytes: 1,
            last_modified: SystemTime::now() - Duration::from_secs(days * 86_400 + 60),
            kind,
        };
        let none = std::collections::HashSet::new();
        let targets = [at(ArtifactKind::RustTarget, 1), at(ArtifactKind::NodeModules, 2), at(ArtifactKind::Simulator, 90)];
        assert!(super::nothing_old(&targets, &none, Some(ArtifactGroup::Rust), 7));
        assert!(super::old_targets(&targets, &none, 7).is_empty());
        assert!(!super::nothing_old(&targets, &none, Some(ArtifactGroup::Simulators), 7));
        assert!(!super::nothing_old(&targets, &none, None, 7));
        assert!(super::nothing_old(&targets[..2], &none, None, 7));
        // An old target the list has dropped as empty is not a row either.
        assert!(super::nothing_old(&targets, &std::collections::HashSet::from([2]), None, 7));
    }
}
