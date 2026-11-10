// Phase 1 of a scan: find artifact directories without measuring them.
// Fast because it never descends into a validated artifact; dot-dir roots are
// collected by name. Exports: `scan_discover`. Deps: walkdir, crate::{config, roots, rules, scanner}.

use crate::config::{Config, ARTIFACT_DIRS};
use crate::roots;
use crate::rules::is_dev_artifact;
use crate::scanner::{ArtifactGroup, ArtifactKind, TargetDir};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use walkdir::{DirEntry, WalkDir};

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

/// Built-in directory names for enabled groups.
pub fn walk_types(config: &Config) -> Vec<&str> {
    ARTIFACT_DIRS
        .iter()
        .copied()
        .filter(|name| config.scans(ArtifactKind::for_dir_name(name).group()))
        .collect()
}

/// Discover artifact directories. Sizes are all zero on return.
pub fn scan_discover(config: &Config) -> Vec<TargetDir> {
    let types = walk_types(config);
    // Nested roots are walked too (the outer walk may not reach them); dedupe is below.
    let mut seen_roots = HashSet::new();
    let dirs: Vec<&PathBuf> = match types.is_empty() {
        true => Vec::new(),
        false => config.scan_dirs.iter()
            .filter(|dir| dir.canonicalize().is_ok_and(|path| seen_roots.insert(path)))
            .collect(),
    };
    let mut found: Vec<TargetDir> = std::thread::scope(|scope| {
        let handles: Vec<_> = dirs
            .iter()
            .map(|dir| {
                let types = &types;
                let max_depth = config.max_depth;
                scope.spawn(move || walk(dir, types, max_depth))
            })
            .collect();
        handles.into_iter().flat_map(|handle| handle.join().unwrap_or_default()).collect()
    });
    // Name-collected roots still need their own group gate.
    if config.scans(ArtifactGroup::Rust) {
        roots::collect_tmp_targets(&mut found);
        roots::collect_shared_cargo_target(&mut found);
        roots::collect_aid_worktrees(&mut found);
    }
    if config.scans(ArtifactGroup::Caches) {
        roots::collect_dev_caches(&mut found);
    }
    if config.scans(ArtifactGroup::Toolchains) {
        roots::collect_toolchains(&mut found, &config.scan_dirs, config.max_depth);
    }
    if config.scans(ArtifactGroup::Simulators) {
        crate::simulators::collect(&mut found);
    }
    let mut seen = HashSet::new();
    found.retain(|item| item.path.canonicalize().is_ok_and(|path| seen.insert(path)));
    found
}

fn walk(dir: &Path, types: &[&str], max_depth: usize) -> Vec<TargetDir> {
    if crate::simulators::is_device_fs(dir) { return Vec::new(); }
    let mut local = Vec::new();
    let mut walker = WalkDir::new(dir).max_depth(max_depth).into_iter();
    while let Some(result) = walker.next() {
        let Ok(entry) = result else { continue };
        if entry.depth() == 0 || !entry.file_type().is_dir() {
            continue;
        }
        if should_skip(&entry) {
            walker.skip_current_dir();
            continue;
        }
        let name = entry.file_name().to_string_lossy();
        if !ARTIFACT_DIRS.contains(&name.as_ref()) || !is_dev_artifact(entry.path(), &name) {
            continue;
        }
        walker.skip_current_dir();
        if !types.contains(&name.as_ref()) {
            continue;
        }
        let kind = ArtifactKind::for_dir_name(name.as_ref());
        let last_modified = entry
            .metadata()
            .ok()
            .and_then(|meta| meta.modified().ok())
            .unwrap_or(SystemTime::UNIX_EPOCH);
        local.push(TargetDir { path: entry.into_path(), size_bytes: 0, last_modified, kind });
    }
    local
}

fn should_skip(entry: &DirEntry) -> bool {
    if entry.depth() == 0 || !entry.file_type().is_dir() {
        return false;
    }
    if crate::simulators::is_device_fs(entry.path()) { return true; }
    // Skip symlinks — don't follow into network mounts or iCloud aliases
    if entry.path_is_symlink() {
        return true;
    }
    let name = entry.file_name().to_string_lossy();
    // The two hidden checkout containers can hold artifacts at any depth.
    if name.starts_with('.') && name != ".claude" && name != ".worktrees"
        && !ARTIFACT_DIRS.contains(&name.as_ref()) {
        return true;
    }
    // Skip non-dev directories (media, cloud, system)
    if SKIP_DIRS.contains(&name.as_ref()) {
        return true;
    }
    false
}

#[cfg(test)]
#[path = "discover_tests.rs"]
mod tests;
