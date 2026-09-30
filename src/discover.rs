// Phase 1 of a scan: find artifact directories without measuring them.
// Fast (<1s) because it never descends into an artifact, and because the roots
// that live under dot-directories are collected by name instead of walked.
// Exports: `scan_discover`. Deps: walkdir, crate::{config, roots, scanner}.

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
    let roots: Vec<_> = config.scan_dirs.iter()
        .filter_map(|dir| dir.canonicalize().ok().map(|path| (dir, path))).collect();
    let dirs: Vec<_> = roots.iter().enumerate()
        .filter(|(i, (_, path))| !types.is_empty() && !roots.iter().enumerate()
            .any(|(j, (_, other))| i != &j && path.starts_with(other)
                && (path != other || j < *i)))
        .map(|(_, (dir, _))| *dir).collect();
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
    let mut seen = HashSet::new();
    found.retain(|item| item.path.canonicalize().is_ok_and(|path| seen.insert(path)));
    found
}

fn walk(dir: &Path, types: &[&str], max_depth: usize) -> Vec<TargetDir> {
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
mod tests {
    use super::{is_dev_artifact, scan_discover, walk_types};
    use crate::config::Config;
    use crate::scanner::ArtifactGroup;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(name: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        path.push(format!("wd40-{name}-{}-{stamp}", std::process::id()));
        path
    }

    fn cleanup(path: &PathBuf) {
        let _ = fs::remove_dir_all(path);
    }

    #[test]
    fn pnpm_markers_allow_node_modules_detection() {
        let root = temp_dir("pnpm-markers");
        let node_modules = root.join("node_modules");
        let _ = fs::create_dir_all(node_modules.join(".pnpm"));
        let _ = fs::create_dir_all(node_modules.join(".bin"));
        let _ = fs::write(node_modules.join(".modules.yaml"), "hoistPattern: []");

        assert!(is_dev_artifact(&node_modules, "node_modules"));
        cleanup(&root);
    }

    #[test]
    fn a_group_switched_off_takes_its_dir_names_out_of_the_walk() {
        let mut config = Config::default();
        config.set_scans(ArtifactGroup::BuildOutput, false);
        let types = walk_types(&config);
        assert!(types.contains(&"target"));
        assert!(types.contains(&"node_modules"));
        assert!(!types.contains(&"dist"));
        assert!(!types.contains(&"build"));
        assert!(!types.contains(&".build"));
        assert!(!types.contains(&".next"));
    }

    #[test]
    fn node_modules_without_markers_are_ignored() {
        let root = temp_dir("node-modules-empty");
        let node_modules = root.join("node_modules");
        let _ = fs::create_dir_all(&node_modules);

        assert!(!is_dev_artifact(&node_modules, "node_modules"));
        cleanup(&root);
    }

    #[test]
    fn xcode_projects_mark_build_but_not_dist_as_artifacts() {
        let root = temp_dir("xcode-project-outputs");
        let _ = fs::create_dir_all(root.join("MyApp.xcodeproj"));
        let build = root.join("build");
        let dist = root.join("dist");
        let _ = fs::create_dir_all(&build);
        let _ = fs::create_dir_all(&dist);

        assert!(is_dev_artifact(&build, "build"));
        assert!(!is_dev_artifact(&dist, "dist"));
        cleanup(&root);
    }

    #[test]
    fn swiftpm_projects_mark_neither_build_nor_dist_as_artifacts() {
        let root = temp_dir("swiftpm-project-outputs");
        let build = root.join("build");
        let dist = root.join("dist");
        let _ = fs::create_dir_all(&build);
        let _ = fs::create_dir_all(&dist);
        let _ = fs::write(root.join("Package.swift"), "// swift-tools-version: 5.9\n");

        assert!(!is_dev_artifact(&build, "build"));
        assert!(!is_dev_artifact(&dist, "dist"));
        cleanup(&root);
    }

    #[test]
    fn swiftpm_build_is_found_as_a_builtin_output() {
        let root = temp_dir("swiftpm-build");
        let build = root.join(".build");
        let _ = fs::create_dir_all(&build);
        let _ = fs::write(root.join("Package.swift"), "// swift-tools-version: 5.9\n");

        let config = Config {
            scan_dirs: vec![root.clone()],
            scan_groups: vec![ArtifactGroup::BuildOutput.key().to_string()],
            ..Config::default()
        };
        assert!(scan_discover(&config).iter().any(|target| target.path == build));
        cleanup(&root);
    }

    #[test]
    fn unvalidated_generic_dirs_do_not_hide_nested_targets() {
        let root = temp_dir("nested-generic");
        for name in ["build", "dist", "out", "artifacts"] {
            let nested = root.join(name).join("project/target");
            fs::create_dir_all(nested.join("debug")).unwrap();
        }
        let config = Config {
            scan_dirs: vec![root.clone()],
            scan_groups: vec![ArtifactGroup::Rust.key().to_string()],
            ..Config::default()
        };
        let found = scan_discover(&config);
        for name in ["build", "dist", "out", "artifacts"] {
            assert!(found.iter().any(|item| item.path == root.join(name).join("project/target")), "{name}");
        }
        cleanup(&root);
    }

    #[test]
    fn hidden_worktrees_are_walked_but_other_hidden_dirs_are_not() {
        let root = temp_dir("hidden-worktrees");
        for container in [".claude/worktrees/branch", ".worktrees/branch", ".other/branch"] {
            fs::create_dir_all(root.join(container).join("target/debug")).unwrap();
            fs::create_dir_all(root.join(container).join("node_modules/.bin")).unwrap();
        }
        let config = Config { scan_dirs: vec![root.clone()], ..Config::default() };
        let found = scan_discover(&config);
        for container in [".claude/worktrees/branch", ".worktrees/branch"] {
            assert!(found.iter().any(|item| item.path == root.join(container).join("target")));
            assert!(found.iter().any(|item| item.path == root.join(container).join("node_modules")));
        }
        assert!(!found.iter().any(|item| item.path.starts_with(root.join(".other"))));
        cleanup(&root);
    }

    #[test]
    fn validated_artifact_prunes_its_contents() {
        let root = temp_dir("validated-prune");
        let build = root.join("build");
        fs::create_dir_all(build.join("target/debug")).unwrap();
        fs::write(root.join("package.json"), "{}").unwrap();
        let config = Config { scan_dirs: vec![root.clone()], ..Config::default() };
        let found = scan_discover(&config);
        assert!(found.iter().any(|item| item.path == build));
        assert!(!found.iter().any(|item| item.path == build.join("target")));
        cleanup(&root);
    }

    #[test]
    fn overlapping_symlink_roots_find_target_once() {
        let root = temp_dir("overlapping-roots");
        let a = root.join("A");
        let b = root.join("B");
        let target = a.join("apps/x/target");
        fs::create_dir_all(target.join("debug")).unwrap();
        std::os::unix::fs::symlink(a.join("apps"), &b).unwrap();
        let config = Config { scan_dirs: vec![a, b], scan_groups: vec!["rust".into()], ..Config::default() };
        assert_eq!(scan_discover(&config).iter().filter(|item| item.path == target).count(), 1);
        cleanup(&root);
    }

    #[test]
    fn disabled_artifacts_still_prune_nested_targets() {
        let root = temp_dir("disabled-prune");
        let nested = root.join("node_modules/pkg/target");
        fs::create_dir_all(nested.join("debug")).unwrap();
        fs::write(root.join("package.json"), "{}").unwrap();
        let config = Config { scan_dirs: vec![root.clone()], scan_groups: vec!["rust".into()], ..Config::default() };
        assert!(!scan_discover(&config).iter().any(|item| item.path.starts_with(&root)));
        cleanup(&root);
    }
}
