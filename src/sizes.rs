// Directory sizing for WD-40. A bounded pool walks every target concurrently,
// handing each size back as soon as it is final.
// Exports: `SizedTarget`, `size_targets`, `scan_sizes`, directory measurement.
// Deps: walkdir, crate::{bulk, disk, nesting, scanner, walk}.
use crate::disk::disk_space;
use crate::nesting::Publisher;
use crate::qos::Priority;
use crate::scanner::TargetDir;
use crate::cache;
use crate::walk::Walk;
use std::collections::HashMap;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;
use walkdir::WalkDir;
/// A target whose size is settled: anything nested inside it has already been
/// subtracted, so this figure is never revised.
pub struct SizedTarget {
    pub index: usize,
    pub bytes: u64,
    pub last_modified: Option<SystemTime>,
}

/// Allocated bytes and the newest content timestamp found below one target.
#[derive(Clone, Copy)]
pub struct Measurement {
    pub bytes: u64,
    pub last_modified: Option<SystemTime>,
}

/// Measure every target, calling `on_size` once per target as its size settles.
/// Indices are positions in `targets`. The callback runs on worker threads.
pub fn size_targets(
    targets: &[TargetDir],
    priority: Priority,
    on_size: impl Fn(SizedTarget) + Sync,
) {
    if targets.is_empty() {
        return;
    }
    let paths: Vec<PathBuf> = targets.iter().map(|target| target.path.clone()).collect();
    let publisher = Mutex::new(Publisher::new(&paths));

    // Anything already known goes to the publisher before the walk starts, so
    // a target enclosing one of them still settles correctly.
    let cached: Vec<Option<(u64, SystemTime)>> = targets
        .iter()
        .map(|target| cache::measurement_of(&target.path, target.kind))
        .collect();
    let modified = Mutex::new(vec![None; targets.len()]);
    for (index, measurement) in cached.iter().copied().enumerate() {
        let Some((bytes, last_modified)) = measurement else { continue };
        modified.lock().unwrap_or_else(|error| error.into_inner())[index] = Some(last_modified);
        let settled = publisher
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .record(index, bytes);
        for (index, bytes) in settled {
            let last_modified = modified
                .lock()
                .unwrap_or_else(|error| error.into_inner())[index];
            on_size(SizedTarget { index, bytes, last_modified });
        }
    }

    let known: Vec<bool> = cached.iter().map(Option::is_some).collect();
    let file_aged: Vec<bool> = targets
        .iter()
        .map(|target| matches!(target.kind, crate::scanner::ArtifactKind::Simulator))
        .collect();
    let walk = Walk::new(&paths, &known, &file_aged);
    let remember = |mut sized: SizedTarget| {
        sized.last_modified = sized.last_modified.map(|modified| modified.min(SystemTime::now()));
        if let Some(target) = targets.get(sized.index) {
            if let Some(last_modified) = sized.last_modified {
                cache::put_measurement(&target.path, target.kind, sized.bytes, last_modified);
            }
        }
        on_size(sized);
    };
    std::thread::scope(|scope| {
        for _ in 0..crate::qos::workers() {
            scope.spawn(|| {
                priority.apply();
                walk.run(&paths, &publisher, &modified, &remember);
            });
        }
    });
}

/// Blocking sizing for callers with nothing to show in the meantime (the CLI).
pub fn scan_sizes(targets: &mut [TargetDir], priority: Priority) -> HashMap<PathBuf, SystemTime> {
    let initial = vec![(0_u64, None); targets.len()];
    let settled = Mutex::new(initial);
    size_targets(targets, priority, |sized| {
        settled.lock().unwrap_or_else(|error| error.into_inner())[sized.index] = (sized.bytes, sized.last_modified);
    });
    let settled = settled.into_inner().unwrap_or_default();
    let mut measured_ages = HashMap::new();
    for (target, (bytes, last_modified)) in targets.iter_mut().zip(settled) {
        target.size_bytes = bytes;
        if let Some(last_modified) = last_modified {
            target.last_modified = last_modified;
            measured_ages.insert(target.path.clone(), last_modified);
        }
    }
    targets.sort_by_key(|target| std::cmp::Reverse(target.size_bytes));
    measured_ages
}

/// What one directory contributed.
pub(crate) struct Found {
    pub bytes: u64,
    pub entry_count: usize,
    pub last_modified: Option<SystemTime>,
    /// Newest regular file only: the age of a target whose owner touches its
    /// directories without using it (CoreSimulator does).
    pub newest_file: Option<SystemTime>,
    pub subdirs: Vec<PathBuf>,
    pub broken: bool,
}

/// One directory measured on the calling thread. Used to find out how much of
/// a target a failed removal actually took off, where spinning up the pool for
/// a single path would cost more than the walk.
pub fn measure_dir(path: &Path) -> u64 {
    measure_dir_with_modified(path).bytes
}

/// Measure a target and retain the newest timestamp anywhere in its contents.
pub fn measure_dir_with_modified(path: &Path) -> Measurement {
    let limit = disk_space(path).map(|stats| stats.total_bytes);
    let mut total: u64 = 0;
    let mut last_modified = None;
    let mut stack = vec![path.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let found = read_dir(&dir);
        if found.broken {
            let mut fallback = dir_measurement_fallback(path);
            fallback.bytes = limit.map_or(fallback.bytes, |max| fallback.bytes.min(max));
            fallback.last_modified = fallback.last_modified.map(|modified| modified.min(SystemTime::now()));
            return fallback;
        }
        total = total.saturating_add(found.bytes);
        last_modified = last_modified.max(found.last_modified);
        stack.extend(found.subdirs);
    }
    Measurement {
        bytes: limit.map_or(total, |max| total.min(max)),
        last_modified: last_modified.map(|modified| modified.min(SystemTime::now())),
    }
}

pub(crate) fn read_dir(dir: &Path) -> Found {
    crate::bulk::read_dir(dir, crate::bulk::BUFFER_SIZE).unwrap_or_else(|| read_dir_standard(dir))
}

pub(crate) fn read_dir_standard(dir: &Path) -> Found {
    let Some(mut found) = empty_found(dir) else { return broken() };
    let Ok(entries) = std::fs::read_dir(dir) else { return broken() };
    for entry in entries {
        let Ok(entry) = entry else { return broken() };
        let Ok(file_type) = entry.file_type() else { return broken() };
        let Ok(metadata) = entry.metadata() else { return broken() };
        let Ok(modified) = metadata.modified() else { return broken() };
        found.entry_count += 1;
        found.last_modified = found.last_modified.max(Some(modified));
        if file_type.is_file() {
            found.newest_file = found.newest_file.max(Some(modified));
        }
        if file_type.is_dir() {
            found.subdirs.push(entry.path());
        } else if !file_type.is_symlink() {
            found.bytes = found.bytes.saturating_add(metadata.blocks().saturating_mul(512));
        }
    }
    found
}

fn empty_found(dir: &Path) -> Option<Found> {
    let last_modified = std::fs::metadata(dir).ok()?.modified().ok()?;
    Some(Found {
        bytes: 0,
        entry_count: 0,
        last_modified: Some(last_modified),
        newest_file: None,
        subdirs: Vec::new(),
        broken: false,
    })
}

fn broken() -> Found {
    Found {
        bytes: 0, entry_count: 0, last_modified: None, newest_file: None,
        subdirs: Vec::new(), broken: true,
    }
}

pub(crate) fn dir_measurement_fallback(path: &Path) -> Measurement {
    let mut measurement = Measurement { bytes: 0, last_modified: None };
    for entry in WalkDir::new(path).follow_links(false) {
        let Ok(entry) = entry else { return incomplete(measurement.bytes) };
        let Ok(metadata) = entry.metadata() else { return incomplete(measurement.bytes) };
        let Ok(modified) = metadata.modified() else { return incomplete(measurement.bytes) };
        measurement.bytes = measurement.bytes.saturating_add(metadata.blocks().saturating_mul(512));
        measurement.last_modified = measurement.last_modified.max(Some(modified));
    }
    measurement
}

fn incomplete(bytes: u64) -> Measurement {
    Measurement { bytes, last_modified: None }
}

#[cfg(test)]
mod tests {
    use super::{scan_sizes, size_targets, Priority};
    use crate::scanner::{ArtifactKind, TargetDir};
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;
    use std::time::SystemTime;

    fn tree(root: &Path, files: &[(&str, usize)]) {
        for (relative, bytes) in files {
            let path = root.join(relative);
            let _ = std::fs::create_dir_all(path.parent().unwrap_or(root));
            let _ = std::fs::write(path, vec![b'x'; *bytes]);
        }
    }

    fn target(path: PathBuf) -> TargetDir {
        TargetDir {
            path,
            size_bytes: 0,
            last_modified: SystemTime::UNIX_EPOCH,
            kind: ArtifactKind::RustTarget,
        }
    }

    fn scratch(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("wd40-sizes-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        path
    }

    /// CoreSimulator touches a device's directories without anyone using it, so
    /// a simulator is as old as its newest file; anything else counts every entry.
    #[test]
    fn a_simulator_is_aged_by_its_newest_file_alone() {
        let root = scratch("file-aged");
        tree(&root, &[("sim/data/old.db", 4096), ("other/old.db", 4096)]);
        let old = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000);
        for file in ["sim/data/old.db", "other/old.db"] {
            let handle = std::fs::File::options().write(true).open(root.join(file)).unwrap();
            handle.set_modified(old).unwrap();
        }
        std::fs::create_dir_all(root.join("sim/data/touched")).unwrap();
        std::fs::create_dir_all(root.join("other/touched")).unwrap();
        let mut simulator = target(root.join("sim"));
        simulator.kind = ArtifactKind::Simulator;
        let mut targets = vec![simulator, target(root.join("other"))];
        let ages = scan_sizes(&mut targets, Priority::Waited);
        assert_eq!(ages.get(&root.join("sim")), Some(&old));
        assert!(ages.get(&root.join("other")).is_some_and(|age| *age > old));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn every_target_is_reported_exactly_once() {
        let root = scratch("once");
        tree(&root, &[("a/one.bin", 4096), ("b/two.bin", 4096), ("b/deep/three.bin", 4096)]);
        let targets = vec![target(root.join("a")), target(root.join("b"))];
        let seen = Mutex::new(Vec::new());
        size_targets(&targets, Priority::Waited, |sized| seen.lock().unwrap().push(sized.index));
        let mut seen = seen.into_inner().unwrap();
        seen.sort_unstable();
        assert_eq!(seen, [0, 1]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_missing_directory_sizes_to_nothing_rather_than_hanging() {
        let mut targets = vec![target(scratch("absent"))];
        scan_sizes(&mut targets, Priority::Waited);
        assert_eq!(targets[0].size_bytes, 0);
    }
}
