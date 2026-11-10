// Removal for WD-40. Deleting here is irreversible and skips the Trash, so a
// run reports exactly what left the disk: gone, partly gone, or untouched — a
// removal that failed half way is never counted as done.
// Exports: removals, cleaning operations, and verified-age lookup.
// Deps: std, crate::{scanner, sizes}.

use crate::scanner::{ArtifactKind, TargetDir};
use crate::sizes::measure_dir;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

/// Removals in flight. Unlinking is metadata-bound, so a handful overlap well;
/// past that they only queue behind the same volume.
const WORKERS: usize = 4;

/// What actually happened to one target.
pub enum Removal {
    Gone,
    /// Part of it left the disk and part is still there. Never reported as done.
    Partial { freed_bytes: u64, left_bytes: u64, reason: String },
    /// Nothing was removed.
    Refused(String),
}

impl Removal {
    /// Bytes this removal really returned, given what the scan measured.
    pub fn freed_bytes(&self, measured: u64) -> u64 {
        match self {
            Self::Gone => measured,
            Self::Partial { freed_bytes, .. } => *freed_bytes,
            Self::Refused(_) => 0,
        }
    }

    pub fn problem(&self) -> Option<&str> {
        match self {
            Self::Gone => None,
            Self::Partial { reason, .. } | Self::Refused(reason) => Some(reason),
        }
    }
}

/// Remove one target, re-checking the path: a symlink may have replaced it since.
///
/// Toolchains and simulators are not ours to unlink: rustup and CoreSimulator
/// keep their own records of what exists, and an `rm` would leave those lying.
pub fn remove_target(target: &TargetDir) -> Removal {
    if target.path.is_symlink() || !target.path.is_dir() {
        return Removal::Refused("path changed since scan (symlink or missing)".into());
    }
    crate::cache::forget(&target.path);
    match target.kind {
        ArtifactKind::Simulator => settle(target, crate::simulators::clean(&target.path).err()),
        ArtifactKind::Toolchain => settle(target, crate::toolchains::uninstall(&target.path).err()),
        _ => settle(target, fs::remove_dir_all(&target.path).err().map(|err| err.to_string())),
    }
}

/// What a removal actually came to, measured rather than assumed: one that got
/// part way is credited with the bytes it really returned, and one that got
/// nowhere is never counted as done.
fn settle(target: &TargetDir, problem: Option<String>) -> Removal {
    let Some(reason) = problem else {
        return Removal::Gone;
    };
    if !target.path.exists() {
        return Removal::Gone;
    }
    let left_bytes = measure_dir(&target.path);
    if left_bytes >= target.size_bytes {
        return Removal::Refused(reason);
    }
    Removal::Partial {
        freed_bytes: target.size_bytes.saturating_sub(left_bytes),
        left_bytes,
        reason,
    }
}

/// Remove `targets` with a bounded pool.
///
/// `may_start` is asked before each target is picked up; once it answers false
/// nothing further is started, and every target already in flight is finished
/// and reported. `on_start` therefore names exactly the targets that were
/// touched — anything it never named was left completely alone.
pub fn remove_targets(
    targets: &[TargetDir],
    may_start: impl Fn() -> bool + Sync,
    on_start: impl Fn(usize) + Sync,
    on_finish: impl Fn(usize, Removal) + Sync,
) {
    let next = AtomicUsize::new(0);
    std::thread::scope(|scope| {
        for _ in 0..WORKERS.min(targets.len().max(1)) {
            scope.spawn(|| {
                while may_start() {
                    let slot = next.fetch_add(1, Ordering::SeqCst);
                    let Some(target) = targets.get(slot) else { break };
                    on_start(slot);
                    on_finish(slot, remove_target(target));
                }
            });
        }
    });
}

#[derive(Default)]
pub struct CleanResult {
    pub freed_bytes: u64,
    pub removed_count: usize,
    pub errors: Vec<(PathBuf, String)>,
}

pub fn clean_all(targets: &[TargetDir]) -> CleanResult {
    let result = Mutex::new(CleanResult::default());
    remove_targets(
        targets,
        || true,
        |_| {},
        |slot, removal| {
            let target = &targets[slot];
            let mut result = result.lock().unwrap();
            result.freed_bytes = result
                .freed_bytes
                .saturating_add(removal.freed_bytes(target.size_bytes));
            if matches!(removal, Removal::Gone) {
                result.removed_count += 1;
            }
            if let Some(problem) = removal.problem() {
                result.errors.push((target.path.clone(), problem.to_string()));
            }
        },
    );
    result.into_inner().unwrap_or_default()
}

pub fn verified_age(
    target: &TargetDir,
    measured_ages: &HashMap<PathBuf, SystemTime>,
    now: SystemTime,
) -> Option<Duration> {
    now.duration_since(*measured_ages.get(&target.path)?).ok()
}

pub fn clean_old(
    targets: &[TargetDir],
    measured_ages: &HashMap<PathBuf, SystemTime>,
    max_age: Duration,
) -> CleanResult {
    let now = SystemTime::now();
    let old: Vec<TargetDir> = targets
        .iter()
        .filter(|target| verified_age(target, measured_ages, now).is_some_and(|age| age >= max_age))
        .cloned()
        .collect();
    clean_all(&old)
}

#[cfg(test)]
mod tests {
    use super::{remove_target, remove_targets, Removal};
    use crate::scanner::{ArtifactKind, TargetDir};
    use std::path::PathBuf;
    use std::sync::Mutex;
    use std::time::SystemTime;
    fn scratch(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("wd40-clean-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        path
    }

    fn target(path: PathBuf, size_bytes: u64) -> TargetDir {
        TargetDir {
            path,
            size_bytes,
            last_modified: SystemTime::UNIX_EPOCH,
            kind: ArtifactKind::RustTarget,
        }
    }

    /// The red line for the toolchain kind: it must reach rustup and nothing
    /// else. Pointed at a directory rustup has never heard of, the removal has
    /// to come back empty-handed — an `rm` would have taken the directory with
    /// it, so the directory still standing is the proof.
    #[test]
    fn a_toolchain_is_never_taken_off_by_an_unlink() {
        let path = scratch("not-a-toolchain");
        std::fs::create_dir_all(&path).expect("scratch dir");
        std::fs::write(path.join("file"), vec![b'x'; 4096]).expect("scratch file");
        let mut td = target(path.clone(), 4096);
        td.kind = ArtifactKind::Toolchain;

        let removal = remove_target(&td);
        assert!(!matches!(removal, Removal::Gone), "rustup owns no such toolchain");
        assert!(path.is_dir(), "the directory must survive — nothing may unlink it");
        assert!(path.join("file").is_file(), "and so must what is inside it");
        let _ = std::fs::remove_dir_all(&path);
    }

    #[test]
    fn a_missing_target_is_refused_rather_than_counted() {
        let removal = remove_target(&target(scratch("absent"), 10));
        assert!(matches!(removal, Removal::Refused(_)));
        assert_eq!(removal.freed_bytes(10), 0);
    }

    #[test]
    fn a_symlink_where_a_target_was_is_never_followed() {
        let root = scratch("symlink");
        let _ = std::fs::create_dir_all(&root);
        let real = root.join("real");
        let link = root.join("link");
        let _ = std::fs::create_dir_all(&real);
        let _ = std::os::unix::fs::symlink(&real, &link);
        assert!(matches!(remove_target(&target(link, 10)), Removal::Refused(_)));
        assert!(real.is_dir(), "the directory the link pointed at must survive");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn stopping_starts_nothing_further_and_names_what_it_touched() {
        let root = scratch("stop");
        let targets: Vec<TargetDir> = (0..12)
            .map(|n| {
                let path = root.join(format!("t{n}"));
                let _ = std::fs::create_dir_all(&path);
                target(path, 0)
            })
            .collect();
        let started = Mutex::new(Vec::new());
        let finished = Mutex::new(Vec::new());
        remove_targets(
            &targets,
            // Stop as soon as anything at all has been picked up.
            || started.lock().unwrap().is_empty(),
            |slot| started.lock().unwrap().push(slot),
            |slot, _| finished.lock().unwrap().push(slot),
        );
        let started = started.into_inner().unwrap();
        let finished = finished.into_inner().unwrap();
        assert!(!started.is_empty() && started.len() < targets.len(), "{started:?}");
        assert_eq!(started.len(), finished.len(), "everything started is reported");
        for (slot, target) in targets.iter().enumerate() {
            assert_eq!(
                started.contains(&slot),
                !target.path.is_dir(),
                "target {slot} was removed exactly when it was started"
            );
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The red line: a target still sitting on disk must never come back as
    /// `Gone`, and the bytes claimed must never exceed what actually left.
    #[test]
    fn a_target_that_will_not_come_off_is_never_reported_gone() {
        use std::os::unix::fs::PermissionsExt;
        let root = scratch("locked");
        let locked = root.join("locked");
        let _ = std::fs::create_dir_all(&locked);
        let _ = std::fs::write(locked.join("pinned"), vec![b'x'; 8192]);
        let _ = std::fs::write(root.join("loose"), vec![b'x'; 8192]);
        // Removing a file needs write permission on the directory holding it.
        let _ = std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o555));

        let measured = 64 * 1024;
        let removal = remove_target(&target(root.clone(), measured));
        assert!(!matches!(removal, Removal::Gone), "the target is still there");
        assert!(root.is_dir(), "the target is still on disk");
        assert!(removal.freed_bytes(measured) < measured, "cannot claim the whole target");
        assert!(removal.problem().is_some(), "the trouble is reported, not swallowed");

        let _ = std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn every_target_is_removed_when_nothing_stops_it() {
        let root = scratch("all");
        let targets: Vec<TargetDir> = (0..9)
            .map(|n| {
                let path = root.join(format!("t{n}"));
                let _ = std::fs::create_dir_all(path.join("nested"));
                let _ = std::fs::write(path.join("nested/file"), b"x");
                target(path, 1)
            })
            .collect();
        let done = Mutex::new(0_usize);
        remove_targets(&targets, || true, |_| {}, |_, removal| {
            assert!(matches!(removal, Removal::Gone));
            *done.lock().unwrap() += 1;
        });
        assert_eq!(done.into_inner().unwrap(), targets.len());
        let _ = std::fs::remove_dir_all(&root);
    }
}
