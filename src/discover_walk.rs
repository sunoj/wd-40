// The discovery walk, shared by a small pool: one checkout container can hold
// hundreds of checkouts and ~90k directories, which one thread per root walked
// serially. Workers take directories from one stack, deepest-first per pop.
// Exports: `walk_roots` (crate-internal). Deps: crate::{config, qos, rules, scanner}.

use crate::config::ARTIFACT_DIRS;
use crate::rules::is_dev_artifact;
use crate::scanner::{ArtifactKind, TargetDir};
use std::path::{Path, PathBuf};
use std::sync::{Condvar, Mutex};
use std::time::SystemTime;

/// Directories that never contain dev artifacts — skip to avoid slow traversal.
const SKIP_DIRS: &[&str] = &[
    // Media & personal
    "Music", "Movies", "Photos", "Pictures",
    // macOS system
    "Library", "Applications", "System",
    // iCloud & cloud storage
    "Mobile Documents", "iCloud Drive", "Google Drive", "OneDrive", "Dropbox",
    // Network & external volumes
    "Volumes",
];

type Job = (PathBuf, usize, usize);

struct Pending {
    /// Directories still to read: depth below their root, and which root.
    stack: Vec<Job>,
    /// Workers reading a directory right now: an empty stack only ends the
    /// walk once none of them can push more.
    busy: usize,
}

/// Every validated artifact under `roots` whose name is in `types`, never
/// descending into one, a symlink, or a skipped directory, and no deeper than
/// `max_depth` below its root. Results come back in root order, so the caller's
/// dedupe keeps the earlier root's path, whichever worker found it first.
pub(crate) fn walk_roots(roots: &[&PathBuf], types: &[&str], max_depth: usize) -> Vec<TargetDir> {
    let pending = Mutex::new(Pending {
        stack: roots.iter().enumerate().map(|(index, root)| ((*root).clone(), 0, index)).collect(),
        busy: 0,
    });
    let wake = Condvar::new();
    let found = Mutex::new(Vec::new());
    std::thread::scope(|scope| {
        for _ in 0..crate::qos::workers() {
            scope.spawn(|| {
                while let Some((dir, depth, root)) = take(&pending, &wake) {
                    let mut local = Vec::new();
                    let children = read(&dir, (depth, root), types, max_depth, &mut local);
                    found.lock().unwrap_or_else(|error| error.into_inner())
                        .extend(local.into_iter().map(|target| (root, target)));
                    let mut guard = pending.lock().unwrap_or_else(|error| error.into_inner());
                    guard.stack.extend(children);
                    guard.busy -= 1;
                    wake.notify_all();
                }
            });
        }
    });
    let mut found = found.into_inner().unwrap_or_else(|error| error.into_inner());
    found.sort_by_key(|(root, _)| *root);
    found.into_iter().map(|(_, target)| target).collect()
}

fn take(pending: &Mutex<Pending>, wake: &Condvar) -> Option<Job> {
    let mut guard = pending.lock().unwrap_or_else(|error| error.into_inner());
    loop {
        if let Some(job) = guard.stack.pop() {
            guard.busy += 1;
            return Some(job);
        }
        if guard.busy == 0 {
            return None;
        }
        guard = wake.wait(guard).unwrap_or_else(|error| error.into_inner());
    }
}

/// Read one directory: record the artifacts directly in it and hand back the
/// subdirectories still worth reading.
fn read(
    dir: &Path,
    (depth, root): (usize, usize),
    types: &[&str],
    max_depth: usize,
    found: &mut Vec<TargetDir>,
) -> Vec<Job> {
    let depth = depth + 1;
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    if depth > max_depth {
        return Vec::new();
    }
    let mut children = Vec::new();
    for entry in entries.filter_map(Result::ok) {
        // The type from the directory itself: a symlink is never a dir here.
        if !entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            continue;
        }
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if should_skip(&path, &name) {
            continue;
        }
        if ARTIFACT_DIRS.contains(&name.as_ref()) && is_dev_artifact(&path, &name) {
            if types.contains(&name.as_ref()) {
                found.push(artifact(path, &name));
            }
            continue;
        }
        if depth < max_depth {
            children.push((path, depth, root));
        }
    }
    children
}

fn artifact(path: PathBuf, name: &str) -> TargetDir {
    let last_modified = std::fs::symlink_metadata(&path)
        .and_then(|meta| meta.modified())
        .unwrap_or(SystemTime::UNIX_EPOCH);
    TargetDir { path, size_bytes: 0, last_modified, kind: ArtifactKind::for_dir_name(name) }
}

fn should_skip(path: &Path, name: &str) -> bool {
    if name == "DeviceFS" && crate::simulators::is_device_fs(path) {
        return true;
    }
    // The two hidden checkout containers can hold artifacts at any depth.
    if name.starts_with('.') && name != ".claude" && name != ".worktrees"
        && !ARTIFACT_DIRS.contains(&name) {
        return true;
    }
    SKIP_DIRS.contains(&name)
}
