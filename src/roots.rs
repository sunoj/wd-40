// Artifact roots that live where the ordinary walk will not go: /tmp, and the
// dot-directories under $HOME that `should_skip` deliberately refuses to enter.
// Exports: the `collect_*` gatherers and `is_cargo_target`.
// Deps: std, dirs, crate::scanner.

use crate::scanner::{ArtifactKind, TargetDir};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// A cargo target dir always has something built in it.
pub(crate) fn is_cargo_target(path: &Path) -> bool {
    path.join("debug").is_dir() || path.join("release").is_dir()
}

/// Collect /tmp Cargo target dirs.
///
/// Recognized name patterns (all confirmed by a `debug/` or `release/` subdir):
///   * `cc-target-*`               — Claude Code per-session targets
///   * `*-target`                  — ad-hoc CARGO_TARGET_DIR (e.g. `smart-router-target`)
///   * `*-target-*`                — ad-hoc CARGO_TARGET_DIR with a suffix
pub(crate) fn collect_tmp_targets(found: &mut Vec<TargetDir>) {
    // macOS: /tmp is a symlink to /private/tmp. read_dir on either works.
    let Ok(entries) = std::fs::read_dir(Path::new("/tmp")) else { return };
    for entry in entries.filter_map(Result::ok) {
        let name = entry.file_name();
        if !is_tmp_target_name(&name.to_string_lossy()) {
            continue;
        }
        // Validate it's actually a cargo target dir, not an unrelated dir that
        // happens to carry "-target" in its name.
        let path = entry.path();
        if path.is_symlink() || !path.is_dir() || !is_cargo_target(&path) {
            continue;
        }
        found.push(TargetDir {
            last_modified: modified(&path),
            path,
            size_bytes: 0,
            kind: ArtifactKind::TmpTarget,
        });
    }
}

fn is_tmp_target_name(name: &str) -> bool {
    name.starts_with("cc-target-") || name.ends_with("-target") || name.contains("-target-")
}

/// Collect ~/.aid/worktrees/<repo>/<branch>/target directories.
/// These live under a dot-prefixed dir so the main WalkDir skips them.
pub(crate) fn collect_aid_worktrees(found: &mut Vec<TargetDir>) {
    let Some(home) = dirs::home_dir() else { return };
    for repo in subdirs(&home.join(".aid").join("worktrees")) {
        for branch in subdirs(&repo) {
            let target = branch.join("target");
            if target.is_dir() && is_cargo_target(&target) {
                push_dir(found, target, ArtifactKind::RustTarget);
            }
        }
    }
}

/// Collect per-project subdirs under ~/.cargo-target/ (shared CARGO_TARGET_DIR
/// root). Supports both <project>/debug and <project>/<session>/debug layouts.
pub(crate) fn collect_shared_cargo_target(found: &mut Vec<TargetDir>) {
    let Some(home) = dirs::home_dir() else { return };
    for project in subdirs(&home.join(".cargo-target")) {
        if is_cargo_target(&project) {
            push_dir(found, project.clone(), ArtifactKind::RustTarget);
        }
        for session in subdirs(&project) {
            if is_cargo_target(&session) {
                push_dir(found, session, ArtifactKind::RustTarget);
            }
        }
    }
}

/// Collect the Rust toolchains a scan may offer. Which ones those are is
/// rustup's business, not a path pattern's, so the decision lives in
/// `toolchains` and this only turns the answer into rows.
pub(crate) fn collect_toolchains(found: &mut Vec<TargetDir>, scan_dirs: &[PathBuf], max_depth: usize) {
    for path in crate::toolchains::removable(scan_dirs, max_depth) {
        push_dir(found, path, ArtifactKind::Toolchain);
    }
}

pub(crate) fn collect_dev_caches(found: &mut Vec<TargetDir>) {
    let Some(home) = dirs::home_dir() else { return };
    collect_home_caches(found, &home);
}

fn collect_home_caches(found: &mut Vec<TargetDir>, home: &Path) {
    for path in [
        home.join("Library/Developer/Xcode/DerivedData"),
        home.join("Library/Developer/Xcode/ModuleCache.noindex"),
        home.join("Library/Caches/org.swift.swiftpm"),
        home.join("Library/Caches/com.apple.dt.Xcode"),
        home.join("Library/Caches/Homebrew"),
        home.join("Library/Caches/pip"),
        home.join(".cache/uv"),
        home.join("Library/Caches/uv"),
        home.join("Library/Caches/go-build"),
        home.join(".gradle/caches"),
        home.join(".gradle/wrapper/dists"),
        home.join(".bun/install/cache"),
        home.join("Library/Caches/CocoaPods"),
        home.join("Library/Caches/node-gyp"),
        home.join("Library/Caches/Mozilla.sccache"),
        home.join("Library/Caches/deno"),
        home.join(".npm/_npx"),
        home.join("Library/Developer/CoreSimulator/Caches"),
        // ~/.npm is npm's cache *root*, but it also holds _logs and npx state.
        // Only the content-addressable store is safe to drop wholesale.
        home.join(".npm/_cacache"),
        home.join("Library/pnpm/store"),
        home.join(".cache/pnpm"),
        home.join(".local/share/pnpm/store"),
        home.join(".pnpm-store"),
        home.join("Library/Caches/Yarn"),
        home.join(".cache/yarn"),
        PathBuf::from("/opt/homebrew/var/homebrew/cache"),
        PathBuf::from("/usr/local/var/homebrew/cache"),
    ] {
        if path.is_dir() && !path.is_symlink() {
            push_dir(found, path, ArtifactKind::Cache);
        }
    }
    collect_dynamic_caches(found, home);
}

fn collect_dynamic_caches(found: &mut Vec<TargetDir>, home: &Path) {
    collect_xcode_device_support(found, &home.join("Library/Developer/Xcode"));

    // Cargo's downloads: the .crate tarballs, the sources unpacked from them and
    // the registry index. All of it comes back on the next build, so it goes in
    // whole rather than by part — but from CARGO_HOME, which can be moved.
    if let Some(registry) = crate::toolchains::cargo_home().map(|home| home.join("registry")) {
        if registry.is_dir() && !registry.is_symlink() {
            push_dir(found, registry, ArtifactKind::Cache);
        }
    }

    for device in subdirs(&home.join("Library/Developer/CoreSimulator/Devices")) {
        let caches = device.join("data/Library/Caches");
        if caches.is_dir() && !caches.is_symlink() {
            push_dir(found, caches, ArtifactKind::Cache);
        }
    }
}

fn collect_xcode_device_support(found: &mut Vec<TargetDir>, xcode_root: &Path) {
    for path in subdirs(xcode_root) {
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else { continue };
        if name.ends_with(" DeviceSupport") {
            push_dir(found, path, ArtifactKind::Cache);
        }
    }
}

/// Real subdirectories of `root`, symlinks excluded.
fn subdirs(root: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(root) else { return Vec::new() };
    entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| !path.is_symlink() && path.is_dir())
        .collect()
}

fn push_dir(found: &mut Vec<TargetDir>, path: PathBuf, kind: ArtifactKind) {
    found.push(TargetDir { last_modified: modified(&path), path, size_bytes: 0, kind });
}

fn modified(path: &Path) -> SystemTime {
    std::fs::metadata(path)
        .ok()
        .and_then(|meta| meta.modified().ok())
        .unwrap_or(SystemTime::UNIX_EPOCH)
}

#[cfg(test)]
mod tests {
    use super::{collect_home_caches, collect_xcode_device_support, is_tmp_target_name};
    use std::fs;
    use std::path::PathBuf;

    #[test]
    fn tmp_target_names_cover_the_three_layouts() {
        assert!(is_tmp_target_name("cc-target-wd40"));
        assert!(is_tmp_target_name("smart-router-target"));
        assert!(is_tmp_target_name("filler-target-issue144"));
        assert!(!is_tmp_target_name("com.apple.launchd.abc"));
    }

    #[test]
    fn all_xcode_device_support_siblings_are_collected() {
        let root = std::env::temp_dir().join(format!("wd40-device-support-{}", std::process::id()));
        let xcode = root.join("Xcode");
        let _ = fs::remove_dir_all(&root);
        let _ = fs::create_dir_all(xcode.join("iOS DeviceSupport"));
        let _ = fs::create_dir_all(xcode.join("watchOS DeviceSupport"));
        let _ = fs::create_dir_all(xcode.join("Archives"));

        let mut found = Vec::new();
        collect_xcode_device_support(&mut found, &xcode);

        let paths: Vec<PathBuf> = found.into_iter().map(|target| target.path).collect();
        assert!(paths.contains(&xcode.join("iOS DeviceSupport")));
        assert!(paths.contains(&xcode.join("watchOS DeviceSupport")));
        assert!(!paths.contains(&xcode.join("Archives")));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn fixed_cache_roots_require_their_exact_path() {
        let home = std::env::temp_dir().join(format!("wd40-cache-roots-{}", std::process::id()));
        let _ = fs::remove_dir_all(&home);
        fs::create_dir_all(&home).unwrap();
        let paths = [
            "Library/Caches/pip", ".cache/uv", "Library/Caches/uv",
            "Library/Caches/go-build", ".gradle/caches", ".gradle/wrapper/dists",
            ".bun/install/cache", "Library/Caches/CocoaPods", "Library/Caches/node-gyp",
            "Library/Caches/Mozilla.sccache", "Library/Caches/deno", ".npm/_npx",
            "Library/Developer/CoreSimulator/Caches",
        ];
        let mut found = Vec::new();
        collect_home_caches(&mut found, &home);
        assert!(!found.iter().any(|item| item.path.starts_with(&home)));
        for relative in paths {
            fs::create_dir_all(home.join(relative)).unwrap();
            fs::create_dir_all(home.join("elsewhere").join(relative)).unwrap();
        }
        collect_home_caches(&mut found, &home);
        for relative in paths {
            assert!(found.iter().any(|item| item.path == home.join(relative)), "{relative}");
        }
        assert_eq!(found.iter().filter(|item| item.path.starts_with(&home)).count(), paths.len());
        fs::remove_dir_all(home).unwrap();
    }
}
