// Cross-feature regressions for sizing correctness and age-gated deletion.
// Exercises directory refill fallback and verified CLI cleaning policy.
// Deps: crate::{cleaner, scanner, sizes}, std filesystem metadata.

use crate::cleaner::{clean_old, verified_age};
use crate::scanner::{ArtifactKind, TargetDir};
use crate::sizes::{read_dir, scan_sizes};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("wd40-safety-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&path);
    path
}

fn target(path: PathBuf) -> TargetDir {
    TargetDir {
        path,
        size_bytes: 0,
        last_modified: SystemTime::UNIX_EPOCH,
        kind: ArtifactKind::RustTarget,
    }
}

fn tree(root: &Path, files: &[(&str, usize)]) {
    for (relative, bytes) in files {
        let path = root.join(relative);
        let _ = std::fs::create_dir_all(path.parent().unwrap_or(root));
        let _ = std::fs::write(path, vec![b'x'; *bytes]);
    }
}

#[test]
fn a_nested_target_is_not_counted_in_the_one_around_it() {
    let root = scratch("nested");
    tree(&root, &[("outer/own.bin", 4096), ("outer/inner/kept.bin", 4096)]);
    let mut targets = vec![target(root.join("outer")), target(root.join("outer/inner"))];
    scan_sizes(&mut targets, crate::qos::Priority::Waited);
    let total: u64 = targets.iter().map(|target| target.size_bytes).sum();
    let outer = targets.iter().find(|target| target.path.ends_with("outer")).unwrap();
    let inner = targets.iter().find(|target| target.path.ends_with("inner")).unwrap();
    assert!(inner.size_bytes > 0, "inner measured");
    assert_eq!(outer.size_bytes, total - inner.size_bytes);
    assert!(outer.size_bytes < total, "outer must not re-count the inner target");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn an_empty_target_list_does_no_work() {
    let mut targets: Vec<TargetDir> = Vec::new();
    scan_sizes(&mut targets, crate::qos::Priority::Waited);
    assert!(targets.is_empty());
}

#[test]
fn newest_nested_content_replaces_the_target_directory_mtime() {
    let root = scratch("content-age");
    let nested = root.join("stable");
    std::fs::create_dir_all(&nested).expect("nested dir");
    let directory_mtime = std::fs::metadata(&root).and_then(|metadata| metadata.modified()).expect("mtime");
    std::thread::sleep(Duration::from_millis(20));
    std::fs::write(nested.join("fresh"), b"new").expect("nested content");
    let mut targets = vec![target(root.clone())];
    scan_sizes(&mut targets, crate::qos::Priority::Waited);
    assert!(targets[0].last_modified > directory_mtime);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn bulk_refill_matches_standard_entry_count_and_bytes() {
    let root = scratch("bulk-refill");
    std::fs::create_dir_all(&root).expect("scratch dir");
    for index in 0..5_000 {
        let name = format!("artifact-{index:05}-with-a-name-long-enough-to-fill-the-buffer");
        std::fs::write(root.join(name), b"x").expect("artifact");
    }
    let expected: Vec<_> = std::fs::read_dir(&root)
        .expect("standard reader")
        .map(|entry| entry.expect("standard entry").metadata().expect("metadata"))
        .collect();
    let expected_bytes: u64 = expected.iter().map(|metadata| metadata.blocks() * 512).sum();
    let found = read_dir(&root);

    assert_eq!(found.entry_count, expected.len());
    assert_eq!(found.bytes, expected_bytes);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn clean_old_keeps_a_target_without_verified_age_evidence() {
    let unverified = scratch("unverified-age");
    let verified = scratch("verified-age");
    for path in [&unverified, &verified] {
        std::fs::create_dir_all(path).expect("scratch dir");
        std::fs::write(path.join("artifact"), b"x").expect("artifact");
    }
    let targets = vec![target(unverified.clone()), target(verified.clone())];
    let measured_ages = std::collections::HashMap::from([(
        verified.clone(),
        SystemTime::UNIX_EPOCH,
    )]);
    let now = SystemTime::now();

    assert!(verified_age(&targets[0], &measured_ages, now).is_none());
    assert!(verified_age(&targets[1], &measured_ages, now).is_some());
    let result = clean_old(&targets, &measured_ages, Duration::from_secs(86_400));

    assert_eq!(result.removed_count, 1);
    assert!(unverified.is_dir(), "unverified discovery-time age must not authorize deletion");
    assert!(!verified.exists(), "verified old target should still be deleted");
    let _ = std::fs::remove_dir_all(&unverified);
}
