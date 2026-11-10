// Phase 1 of a scan: find artifact directories without measuring them.
// Fast because it never descends into a validated artifact; dot-dir roots are
// collected by name. Exports: `scan_discover`, `walk_types`.
// Deps: crate::{config, discover_walk, roots, scanner, simulators}.

use crate::config::{Config, ARTIFACT_DIRS};
use crate::roots;
use crate::scanner::{ArtifactGroup, ArtifactKind, TargetDir};
use std::collections::HashSet;
use std::path::PathBuf;

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
    let dirs: Vec<&PathBuf> = dirs
        .into_iter()
        .filter(|dir| !crate::simulators::is_device_fs(dir))
        .collect();
    let mut found = crate::discover_walk::walk_roots(&dirs, &types, config.max_depth);
    // Name-collected roots still need their own group gate.
    if config.scans(ArtifactGroup::Rust) {
        roots::collect_tmp_targets(&mut found);
        roots::collect_shared_cargo_target(&mut found);
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

#[cfg(test)]
#[path = "discover_tests.rs"]
mod tests;
