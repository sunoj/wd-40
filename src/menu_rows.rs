// Group planning for popover scan lists: which rows fit within a visible budget.
// Exports: `GroupPlan`, `RowPlan`, `Membership`, `plan_groups`. Pure — no AppKit.
// Deps: crate::names, wd40::{disk, scanner}.

use crate::names::display_names;
use crate::selection::is_recent;
use std::collections::HashSet;
use wd40::disk::sum_bytes;
use wd40::scanner::{ArtifactGroup, TargetDir};


/// One project row, resolved down to what the popover draws.
pub struct RowPlan<'a> {
    pub index: usize,
    pub target: &'a TargetDir,
    #[allow(dead_code)]
    pub name: String,
    #[allow(dead_code)]
    pub kind: &'static str,
}

/// One artifact group and the rows the popover has room to show for it.
pub struct GroupPlan<'a> {
    pub group: ArtifactGroup,
    pub rows: Vec<RowPlan<'a>>,
    pub count: usize,
    /// Rows this group has but the budget could not fit.
    /// Old targets past the group's row budget, folded behind the link.
    pub more: usize,
    /// Recent targets, which the popover folds away whole.
    pub recent: usize,
    pub hidden_bytes: u64,
    pub size: u64,
}

/// How a group's rows are folded. The popover shows a few of each group's old
/// targets; the window, and "show all", show everything.
#[derive(Clone, Copy)]
pub struct Fold {
    /// Most rows any one group shows. Per group, not shared: a shared budget
    /// left most groups with no rows at all once there were six of them.
    pub per_group: usize,
    /// "Keep builds newer than": targets newer than this fold away as recent.
    pub keep_days: Option<u64>,
}

impl Fold {
    pub fn all() -> Self {
        Self { per_group: usize::MAX, keep_days: None }
    }
}

impl<'a> GroupPlan<'a> {
    pub fn hidden(&self) -> usize {
        self.more + self.recent
    }
}

/// The visible row set and the group counts it was planned from. A later open
/// rebuilds only when this no longer matches — size hops patch labels, not
/// which rows belong on screen.
#[derive(Clone, PartialEq, Eq)]
pub struct Membership {
    rows: Vec<usize>,
    counts: Vec<(ArtifactGroup, usize)>,
}

impl Membership {
    pub fn of(targets: &[TargetDir], empty: &HashSet<usize>, fold: Fold) -> Self {
        let plans = plan_groups(targets, empty, fold);
        Self {
            rows: plans.iter().flat_map(|p| p.rows.iter().map(|r| r.index)).collect(),
            counts: plans.iter().map(|p| (p.group, p.count)).collect(),
        }
    }
}

impl std::fmt::Debug for Membership {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let counts: Vec<(&str, usize)> = self.counts.iter().map(|(g, n)| (g.key(), *n)).collect();
        f.debug_struct("Membership")
            .field("rows", &self.rows)
            .field("counts", &counts)
            .finish()
    }
}

/// Split `targets` into groups and decide which rows fit within `limit`.
///
/// `empty` is left out of the rows and out of the count with it: a target that
/// was measured and holds nothing frees nothing, so counting it would promise
/// a row the list does not have. The totals are untouched — an empty target
/// contributes no bytes to add or lose.
pub fn plan_groups<'a>(
    targets: &'a [TargetDir],
    empty: &HashSet<usize>,
    fold: Fold,
) -> Vec<GroupPlan<'a>> {
    let names = display_names(targets);
    let members: Vec<(ArtifactGroup, Vec<(usize, &TargetDir)>)> = ArtifactGroup::ALL
        .iter()
        .map(|&group| {
            let found: Vec<(usize, &TargetDir)> = targets
                .iter()
                .enumerate()
                .filter(|(index, td)| td.kind.group() == group && !empty.contains(index))
                .collect();
            (group, found)
        })
        .filter(|(_, found)| !found.is_empty())
        .collect();

    members
        .into_iter()
        .map(|(group, found)| {
            // With a threshold, the rows are the old targets and the recent
            // ones fold away; without one, every target is a candidate row.
            let old = |td: &TargetDir| fold.keep_days.is_none_or(|days| !is_recent(td, days));
            let shown: Vec<(usize, &TargetDir)> =
                found.iter().copied().filter(|(_, td)| old(td)).take(fold.per_group).collect();
            // What the link stands for: every member that is not a row.
            let folded: Vec<&TargetDir> =
                found.iter().filter(|(i, _)| !shown.iter().any(|(s, _)| s == i)).map(|(_, td)| *td).collect();
            let more = folded.iter().filter(|td| old(td)).count();
            let first_kind = found[0].1.kind.label();
            let mixed = found.iter().any(|(_, td)| td.kind.label() != first_kind);
            let rows: Vec<RowPlan> = shown
                .iter()
                .map(|&(index, target)| RowPlan {
                    index,
                    target,
                    name: names[index].clone(),
                    kind: if mixed { target.kind.label() } else { "" },
                })
                .collect();
            GroupPlan {
                group,
                count: found.len(),
                more,
                recent: folded.len() - more,
                hidden_bytes: sum_bytes(folded.iter().map(|td| td.size_bytes)),
                size: sum_bytes(found.iter().map(|(_, td)| td.size_bytes)),
                rows,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{plan_groups, Fold, Membership};
    use std::collections::HashSet;
    use std::path::PathBuf;
    use std::time::SystemTime;
    use wd40::scanner::{ArtifactKind, TargetDir};

    fn target(path: &str, kind: ArtifactKind, size: u64) -> TargetDir {
        TargetDir {
            path: PathBuf::from(path),
            size_bytes: size,
            last_modified: SystemTime::UNIX_EPOCH,
            kind,
        }
    }

    #[test]
    fn a_target_that_measures_empty_changes_membership() {
        let targets = vec![
            target("/w/a/target", ArtifactKind::RustTarget, 9),
            target("/w/b/target", ArtifactKind::RustTarget, 4),
        ];
        let before = Membership::of(&targets, &HashSet::new(), Fold::all());
        let after = Membership::of(&targets, &HashSet::from([1]), Fold::all());
        assert_ne!(before, after);
        assert_eq!(after, Membership::of(&targets[..1], &HashSet::new(), Fold::all()));
    }

    #[test]
    fn membership_ignores_how_old_a_target_is() {
        let now = SystemTime::now();
        let young = TargetDir {
            path: PathBuf::from("/w/a/target"),
            size_bytes: 9,
            last_modified: now,
            kind: ArtifactKind::RustTarget,
        };
        let old = TargetDir {
            last_modified: now
                .checked_sub(std::time::Duration::from_secs(3 * 86_400))
                .unwrap_or(SystemTime::UNIX_EPOCH),
            ..young.clone()
        };
        assert_eq!(
            Membership::of(&[young], &HashSet::new(), Fold::all()),
            Membership::of(&[old], &HashSet::new(), Fold::all()),
        );
    }

    #[test]
    fn a_nonzero_size_leaves_membership_alone() {
        let unmeasured = vec![
            target("/w/a/target", ArtifactKind::RustTarget, 0),
            target("/w/b/target", ArtifactKind::RustTarget, 8),
        ];
        let settled = vec![
            target("/w/a/target", ArtifactKind::RustTarget, 12),
            target("/w/b/target", ArtifactKind::RustTarget, 8),
        ];
        assert_eq!(
            Membership::of(&unmeasured, &HashSet::new(), Fold::all()),
            Membership::of(&settled, &HashSet::new(), Fold::all()),
        );
    }

    #[test]
    fn a_hidden_empty_target_still_changes_the_count() {
        let targets: Vec<TargetDir> = (0..7)
            .map(|i| target(&format!("/w/{i}/target"), ArtifactKind::RustTarget, 10 - i as u64))
            .collect();
        let before = Membership::of(&targets, &HashSet::new(), Fold { per_group: 6, keep_days: None });
        let after = Membership::of(&targets, &HashSet::from([6]), Fold { per_group: 6, keep_days: None });
        assert_ne!(before, after);
        assert_eq!(after, Membership::of(&targets[..6], &HashSet::new(), Fold { per_group: 6, keep_days: None }));
    }

    /// A row that frees nothing is not a row, and the count says so — the
    /// figure beside the group title is what the list can show, not what the
    /// scan happened to walk past.
    #[test]
    fn an_empty_target_leaves_the_rows_and_the_count() {
        let targets = vec![
            target("/w/a/target", ArtifactKind::RustTarget, 9),
            target("/w/b/target", ArtifactKind::RustTarget, 0),
        ];
        let plans = plan_groups(&targets, &HashSet::from([1]), Fold::all());
        assert_eq!(plans[0].rows.len(), 1);
        assert_eq!(plans[0].rows[0].index, 0);
        assert_eq!(plans[0].count, 1);
        assert_eq!(plans[0].hidden(), 0);
        assert_eq!(plans[0].size, 9);
    }

    #[test]
    fn a_group_of_one_kind_leaves_the_kind_suffix_off() {
        let targets = vec![
            target("/w/a/target", ArtifactKind::RustTarget, 9),
            target("/tmp/cc-target-b", ArtifactKind::TmpTarget, 8),
        ];
        let mixed = plan_groups(&targets, &HashSet::new(), Fold::all());
        assert_eq!(mixed[0].rows[0].kind, "target");
        let single = plan_groups(&targets[..1], &HashSet::new(), Fold::all());
        assert_eq!(single[0].rows[0].kind, "");
    }

    fn aged(path: &str, kind: ArtifactKind, days: u64) -> TargetDir {
        let mut made = target(path, kind, 10);
        made.last_modified = SystemTime::now() - std::time::Duration::from_secs(days * 86_400);
        made
    }

    /// Six groups and a budget of two rows each: every group with an old
    /// target shows one, however crowded its neighbours are, and the recent
    /// ones fold away and are counted as recent.
    #[test]
    fn every_group_with_old_targets_shows_at_least_one_row() {
        use ArtifactKind::{BuildOutput, Cache, NodeModules, RustTarget, Simulator, Toolchain};
        let mut targets = Vec::new();
        for (kind, old, recent) in [(RustTarget, 13, 2), (NodeModules, 138, 9), (BuildOutput, 17, 0), (Cache, 62, 1), (Simulator, 1, 4), (Toolchain, 0, 3)] {
            targets.extend((0..old).map(|n| aged(&format!("/w/{}/old{n}/target", kind.label()), kind, 60)));
            targets.extend((0..recent).map(|n| aged(&format!("/w/{}/new{n}/target", kind.label()), kind, 1)));
        }
        let plans = plan_groups(&targets, &HashSet::new(), Fold { per_group: 2, keep_days: Some(7) });
        assert_eq!(plans.len(), 6);
        for plan in &plans {
            let old = plan.count - plan.recent;
            assert_eq!(plan.rows.len(), old.min(2), "{:?}", plan.group.key());
            assert_eq!(plan.rows.len() + plan.more + plan.recent, plan.count);
            assert!(plan.rows.iter().all(|row| row.target.last_modified < SystemTime::now() - std::time::Duration::from_secs(7 * 86_400)));
        }
        let of = |group: wd40::scanner::ArtifactGroup| plans.iter().find(|plan| plan.group == group).unwrap();
        let crowded = of(wd40::scanner::ArtifactGroup::NodeModules);
        assert_eq!((crowded.more, crowded.recent), (136, 9));
        // Only recent targets: no rows, and the link has to say "recent".
        let young = of(wd40::scanner::ArtifactGroup::Toolchains);
        assert_eq!((young.rows.len(), young.more, young.recent), (0, 0, 3));
    }
}
