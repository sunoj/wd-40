// Group planning for popover scan lists: which rows fit within a visible budget.
// Exports: `GroupPlan`, `RowPlan`, `Fold`, `Membership`, `plan_groups`. Pure — no AppKit.
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
    pub size: u64,
}

/// How a group's rows are folded. The popover shows a few of each group's old
/// targets until its header is clicked; the window shows everything.
#[derive(Clone, Copy)]
pub struct Fold {
    /// Most rows a folded group shows. Per group, not shared: a shared budget
    /// left most groups with no rows at all once there were six of them.
    pub per_group: usize,
    /// "Keep builds newer than": targets newer than this fold away as recent,
    /// and an open group lists them after its old ones.
    pub keep_days: Option<u64>,
    /// Groups unfolded whole, in `ArtifactGroup::ALL` order.
    pub open: [bool; ArtifactGroup::ALL.len()],
}

impl Fold {
    pub fn all() -> Self {
        Self { per_group: usize::MAX, keep_days: None, open: [true; ArtifactGroup::ALL.len()] }
    }

    /// Every group folded to `per_group` of its old rows.
    pub fn folded(per_group: usize, keep_days: Option<u64>) -> Self {
        Self { per_group, keep_days, open: [false; ArtifactGroup::ALL.len()] }
    }

    fn is_open(&self, group: ArtifactGroup) -> bool {
        ArtifactGroup::ALL.iter().position(|g| *g == group).is_none_or(|at| self.open[at])
    }

    fn is_old(&self, target: &TargetDir) -> bool {
        self.keep_days.is_none_or(|days| !is_recent(target, days))
    }

    /// A group's rows. Folded, they are its first few old targets. Open, they
    /// are every target, old ones first, so unfolding moves no row on show.
    fn shown<'a>(&self, group: ArtifactGroup, found: &[(usize, &'a TargetDir)]) -> Vec<(usize, &'a TargetDir)> {
        let (aged, young): (Vec<_>, Vec<_>) = found.iter().copied().partition(|(_, td)| self.is_old(td));
        match self.is_open(group) {
            true => aged.into_iter().chain(young).collect(),
            false => aged.into_iter().take(self.per_group).collect(),
        }
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
            let old = |td: &TargetDir| fold.is_old(td);
            let shown = fold.shown(group, &found);
            // What the header's mark stands for: every member that is not a row.
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
                size: sum_bytes(found.iter().map(|(_, td)| td.size_bytes)),
                rows,
            }
        })
        .collect()
}

#[cfg(test)]
#[path = "menu_rows_tests.rs"]
mod tests;
