// Tests for the scan list's planning: which targets are rows, how a group
// folds and unfolds, and when the planned rows count as a different list.
// Deps: super (menu_rows), wd40::scanner.

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
    let before = Membership::of(&targets, &HashSet::new(), Fold::folded(6, None));
    let after = Membership::of(&targets, &HashSet::from([6]), Fold::folded(6, None));
    assert_ne!(before, after);
    assert_eq!(after, Membership::of(&targets[..6], &HashSet::new(), Fold::folded(6, None)));
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
    let plans = plan_groups(&targets, &HashSet::new(), Fold::folded(2, Some(7)));
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

/// Unfolding a group lists every target with the old ones first, so the
/// rows that were on show while it was folded stay where they were.
#[test]
fn an_open_group_lists_everything_with_its_old_targets_first() {
    let targets = vec![
        aged("/w/new/target", ArtifactKind::RustTarget, 1),
        aged("/w/old-a/target", ArtifactKind::RustTarget, 60),
        aged("/w/old-b/target", ArtifactKind::RustTarget, 90),
        aged("/w/site/node_modules", ArtifactKind::NodeModules, 1),
    ];
    let mut fold = Fold::folded(1, Some(7));
    let rows = |fold| plan_groups(&targets, &HashSet::new(), fold).iter().map(|plan| plan.rows.iter().map(|row| row.index).collect()).collect::<Vec<Vec<usize>>>();
    assert_eq!(rows(fold), [vec![1], vec![]]);
    fold.open[0] = true;
    assert_eq!(rows(fold), [vec![1, 2, 0], vec![]]);
}
