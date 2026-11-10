// Regression tests for unattended-clean policy, cancellation, and receipts.
// Exercises private policy helpers through `auto_clean`'s child test module.
// Deps: std, crate::{auto_clean, tasks}, wd40 config/scanner types.

use super::{
    apply_outcome, can_start, candidate_indices, clean, clear_receipt, format_receipt,
    prepare_popover_opening, receipt_line, AutoCleanJob, CleanOutcome, Receipt, RECEIPT, RUNNING,
};
use std::collections::{HashMap, HashSet};
use std::fs::{File, FileTimes};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime};
use wd40::config::Config;
use wd40::scanner::{ArtifactGroup, ArtifactKind, TargetDir};

fn target(now: SystemTime, age_days: u64, kind: ArtifactKind) -> TargetDir {
    TargetDir {
        path: PathBuf::from(format!("/tmp/auto-clean-{age_days}")),
        size_bytes: 1024,
        last_modified: now - Duration::from_secs(age_days * 86_400),
        kind,
    }
}

fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("wd40-auto-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&path);
    path
}

fn measured_ages(targets: &[TargetDir]) -> HashMap<PathBuf, SystemTime> {
    targets
        .iter()
        .map(|target| (target.path.clone(), target.last_modified))
        .collect()
}

fn set_modified(path: &Path, modified: SystemTime) {
    File::open(path)
        .and_then(|file| file.set_times(FileTimes::new().set_modified(modified)))
        .expect("set modification time");
}

fn stale_tree(name: &str, bytes: usize) -> PathBuf {
    let root = scratch(name);
    std::fs::create_dir_all(&root).expect("target");
    let file = root.join("artifact");
    std::fs::write(&file, vec![b'x'; bytes]).expect("artifact");
    let modified = SystemTime::now() - Duration::from_secs(30 * 86_400);
    set_modified(&file, modified);
    set_modified(&root, modified);
    root
}

#[test]
fn manual_selection_does_not_affect_auto_clean_policy() {
    let now = SystemTime::now();
    let targets = vec![
        target(now, 8, ArtifactKind::RustTarget),
        target(now, 2, ArtifactKind::RustTarget),
    ];
    let manual_selection = HashSet::from([1]);
    let candidates = candidate_indices(&targets, &measured_ages(&targets), &Config::default(), now);
    assert_eq!(candidates, [0]);
    assert!(!manual_selection.contains(&candidates[0]));
}

#[test]
fn target_one_day_under_keep_threshold_survives() {
    let now = SystemTime::now();
    let config = Config { max_age_days: 7, ..Config::default() };
    let targets = vec![target(now, 6, ArtifactKind::RustTarget)];
    assert!(candidate_indices(&targets, &measured_ages(&targets), &config, now).is_empty());
}

#[test]
fn zero_keep_threshold_yields_no_auto_clean_candidates() {
    let now = SystemTime::now();
    let config = Config { max_age_days: 0, ..Config::default() };
    let targets = vec![target(now, 30, ArtifactKind::RustTarget)];
    assert!(candidate_indices(&targets, &measured_ages(&targets), &config, now).is_empty());
}

#[test]
fn disabled_group_is_untouched() {
    let now = SystemTime::now();
    let mut config = Config::default();
    config.set_scans(ArtifactGroup::Caches, false);
    let targets = vec![target(now, 30, ArtifactKind::Cache)];
    assert!(candidate_indices(&targets, &measured_ages(&targets), &config, now).is_empty());
}

#[test]
fn toolchains_are_never_auto_clean_candidates() {
    let now = SystemTime::now();
    let config = Config { max_age_days: 7, ..Config::default() };
    let targets = vec![target(now, 1000, ArtifactKind::Toolchain)];
    assert!(config.scans(ArtifactGroup::Toolchains));
    assert!(candidate_indices(&targets, &measured_ages(&targets), &config, now).is_empty());
}

#[test]
fn simulators_are_never_auto_clean_candidates() {
    let now = SystemTime::now();
    let targets = vec![target(now, 1000, ArtifactKind::Simulator)];
    assert!(candidate_indices(&targets, &measured_ages(&targets), &Config::default(), now).is_empty());
}

#[test]
fn missing_age_evidence_keeps_an_unattended_target() {
    let now = SystemTime::now();
    let targets = vec![target(now, 30, ArtifactKind::RustTarget)];
    assert!(candidate_indices(&targets, &HashMap::new(), &Config::default(), now).is_empty());
}

#[test]
fn unreadable_fresh_content_cannot_be_misread_as_old() {
    let root = scratch("unreadable-age");
    let sealed = root.join("sealed");
    std::fs::create_dir_all(&sealed).expect("sealed subtree");
    std::fs::write(sealed.join("fresh"), b"active build").expect("fresh content");
    let stale = SystemTime::now() - Duration::from_secs(30 * 86_400);
    set_modified(&sealed, stale);
    set_modified(&root, stale);
    std::fs::set_permissions(&sealed, std::fs::Permissions::from_mode(0o000)).expect("seal");

    let mut targets = vec![target(SystemTime::now(), 30, ArtifactKind::RustTarget)];
    targets[0].path = root.clone();
    let ages = wd40::sizes::scan_sizes(&mut targets, wd40::qos::Priority::Waited);
    std::fs::set_permissions(&sealed, std::fs::Permissions::from_mode(0o700)).expect("restore");
    assert!(!ages.contains_key(&root), "an incomplete walk has no age evidence");
    assert!(candidate_indices(&targets, &ages, &Config::default(), SystemTime::now()).is_empty());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn successfully_measured_old_content_is_an_unattended_candidate() {
    let root = stale_tree("known-old", 1024);
    let mut targets = vec![target(SystemTime::now(), 30, ArtifactKind::RustTarget)];
    targets[0].path = root.clone();
    let ages = wd40::sizes::scan_sizes(&mut targets, wd40::qos::Priority::Waited);
    assert_eq!(candidate_indices(&targets, &ages, &Config::default(), SystemTime::now()), [0]);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn start_guard_rejects_every_unsafe_ui_state() {
    assert!(can_start(false, false, false));
    assert!(!can_start(true, false, false));
    assert!(!can_start(false, true, false));
    assert!(!can_start(false, false, true));
}

#[test]
fn fresh_nested_content_is_rechecked_before_removal() {
    let root = scratch("fresh-recheck");
    std::fs::create_dir_all(root.join("stable")).expect("target");
    std::fs::write(root.join("stable/active"), b"building").expect("fresh content");
    let mut stale = target(SystemTime::now(), 30, ArtifactKind::RustTarget);
    stale.path = root.clone();
    let job = AutoCleanJob { items: vec![(stale, "active target".into())], reference: None };
    let config = Config { max_age_days: 7, ..Config::default() };
    let outcome = clean(job, &config, &AtomicBool::new(false));
    assert!(root.is_dir(), "fresh content must survive the final age check");
    assert!(outcome.receipt.groups.is_empty());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn shared_stop_prevents_the_next_unattended_removal() {
    let root = stale_tree("shared-stop", 1024);
    let mut stale = target(SystemTime::now(), 30, ArtifactKind::RustTarget);
    stale.path = root.clone();
    let job = AutoCleanJob { items: vec![(stale, "stopped target".into())], reference: None };
    let outcome = clean(job, &Config::default(), &AtomicBool::new(true));
    assert!(root.is_dir());
    assert!(outcome.receipt.groups.is_empty());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn opening_the_popover_requests_the_shared_stop() {
    crate::tasks::reset_clean_stop();
    RUNNING.store(true, Ordering::Relaxed);
    let _opening = prepare_popover_opening();
    assert!(crate::tasks::stop_requested());
    RUNNING.store(false, Ordering::Relaxed);
    crate::tasks::reset_clean_stop();
}

#[test]
fn receipt_uses_the_fresh_measurement_instead_of_the_scan_size() {
    let root = stale_tree("receipt-size", 8192);
    let measured = wd40::sizes::measure_dir_with_modified(&root).bytes;
    let mut stale = target(SystemTime::now(), 30, ArtifactKind::RustTarget);
    stale.path = root.clone();
    stale.size_bytes = 5_000_000_000;
    let job = AutoCleanJob { items: vec![(stale, "shrunk".into())], reference: None };
    let outcome = clean(job, &Config::default(), &AtomicBool::new(false));
    assert_eq!(outcome.receipt.freed_bytes, measured);
    assert!(!root.exists());
}

#[test]
fn partial_removal_revises_the_surviving_snapshot_size() {
    let now = SystemTime::now();
    let mut targets = vec![target(now, 30, ArtifactKind::RustTarget)];
    targets[0].size_bytes = 2_629_632;
    let receipt = Receipt { groups: Vec::new(), names: Vec::new(), freed_bytes: 655_360 };
    let outcome = CleanOutcome {
        receipt,
        remaining: vec![(targets[0].path.clone(), 1_974_272)],
    };
    let _receipt = apply_outcome(&mut targets, outcome);
    assert_eq!(targets[0].size_bytes, 1_974_272);
}

#[test]
fn receipt_is_visible_then_cleared_by_its_lifecycle() {
    clear_receipt();
    assert!(format_receipt(None).is_none());
    *RECEIPT.lock().unwrap() = Some(Receipt {
        groups: vec![(ArtifactGroup::Rust, 2), (ArtifactGroup::Caches, 1)],
        names: vec!["alpha".into(), "beta".into(), "registry".into()],
        freed_bytes: 1536,
    });
    let line = receipt_line().expect("receipt line");
    assert!(line.contains("3 targets (alpha, beta, registry) \u{00b7} Rust/Caches"));
    assert!(line.ends_with("1.5K"));
    clear_receipt();
    assert!(receipt_line().is_none());
}
