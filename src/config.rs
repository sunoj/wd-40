// Config definitions for WD-40.
// Handles defaults and TOML parsing.
use crate::scanner::ArtifactGroup;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// Directory names recognized as cleanable dev artifacts.
pub const ARTIFACT_DIRS: &[&str] = &[
    "target", "node_modules", ".next", "dist", "build", ".build",
    ".gradle", ".turbo", ".svelte-kit", ".parcel-cache", ".nuxt", ".angular",
    ".mypy_cache", ".pytest_cache", ".ruff_cache", ".tox", ".nox",
    "zig-cache", ".zig-cache", "zig-out", ".dart_tool", "out", "artifacts",
];

#[derive(Deserialize, Serialize, Debug, Clone)]
#[serde(default)]
pub struct Config {
    pub scan_dirs: Vec<PathBuf>,
    pub max_age_days: u64,
    pub max_depth: usize,
    pub auto_clean_hours: u64,
    /// Which artifact groups are looked for at all, by `ArtifactGroup::key`.
    /// A group that is not in here is never discovered, so it costs nothing to
    /// walk and shows no figure it did not measure.
    pub scan_groups: Vec<String>,
    /// Print the reclaimable total beside the menu bar glyph.
    pub menu_bar_size: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            scan_dirs: default_scan_dirs(),
            max_age_days: 7,
            max_depth: 8,
            auto_clean_hours: 0,
            scan_groups: ArtifactGroup::ALL.iter().map(|g| g.key().to_string()).collect(),
            menu_bar_size: true,
        }
    }
}

impl Config {
    /// True when this group is one the scan looks for.
    pub fn scans(&self, group: ArtifactGroup) -> bool {
        self.scan_groups.iter().any(|key| key == group.key())
    }

    /// Turn a group on or off, leaving every other group as it was.
    pub fn set_scans(&mut self, group: ArtifactGroup, on: bool) {
        self.scan_groups.retain(|key| key != group.key());
        if on {
            self.scan_groups.push(group.key().to_string());
        }
    }

    pub fn load() -> Self {
        if let Some(path) = Self::config_path() {
            if let Ok(contents) = fs::read_to_string(&path) {
                if let Ok(parsed) = toml::from_str::<Config>(&contents) {
                    return parsed;
                } else {
                    eprintln!("wd-40: failed to parse {}", path.display());
                }
            }
        }
        Self::default()
    }

    pub fn save(&self) {
        if let Some(path) = Self::config_path() {
            if let Some(parent) = path.parent() {
                let _ = fs::create_dir_all(parent);
            }
            if let Ok(toml_str) = toml::to_string_pretty(self) {
                let _ = fs::write(&path, toml_str);
            }
        }
    }

    fn config_path() -> Option<PathBuf> {
        dirs::home_dir().map(|home| home.join(".config/wd-40/config.toml"))
    }
}

fn default_scan_dirs() -> Vec<PathBuf> {
    dirs::home_dir().map(|home| scan_dirs_in(&home)).unwrap_or_default()
}

fn scan_dirs_in(home: &std::path::Path) -> Vec<PathBuf> {
    const NAMES: &[&str] = &[
        "Develop", "Developer", "Projects", "projects", "code", "Code",
        "src", "dev", "workspace", "repos", "GitHub", "Documents/GitHub",
    ];
    let mut seen = std::collections::HashSet::new();
    let dirs: Vec<_> = NAMES.iter().map(|name| home.join(name)).filter(|path| {
        path.is_dir() && path.canonicalize().is_ok_and(|canonical| {
            seen.insert(canonical.to_string_lossy().to_lowercase())
        })
    }).collect();
    if dirs.is_empty() { vec![home.join("Develop")] } else { dirs }
}

#[cfg(test)]
mod tests {
    use super::{scan_dirs_in, Config};
    use crate::scanner::ArtifactGroup;
    use std::fs;

    #[test]
    fn every_group_is_scanned_by_default() {
        let config = Config::default();
        assert!(ArtifactGroup::ALL.iter().all(|group| config.scans(*group)));
    }

    #[test]
    fn switching_one_group_off_leaves_the_others_alone() {
        let mut config = Config::default();
        config.set_scans(ArtifactGroup::Caches, false);
        assert!(!config.scans(ArtifactGroup::Caches));
        assert!(config.scans(ArtifactGroup::Rust));
        assert!(config.scans(ArtifactGroup::NodeModules));
        assert!(config.scans(ArtifactGroup::BuildOutput));
    }

    #[test]
    fn switching_a_group_back_on_does_not_list_it_twice() {
        let mut config = Config::default();
        config.set_scans(ArtifactGroup::Rust, false);
        config.set_scans(ArtifactGroup::Rust, true);
        config.set_scans(ArtifactGroup::Rust, true);
        assert_eq!(config.scan_groups.iter().filter(|key| *key == "rust").count(), 1);
        assert!(config.scans(ArtifactGroup::Rust));
    }

    /// A config file written before `scan_groups` existed parses with every
    /// group on, rather than with an empty list that would find nothing.
    #[test]
    fn a_config_without_scan_groups_still_scans_everything() {
        let config: Config = toml::from_str(
            "artifact_types = [\"target\", \"node_modules\", \"dist\", \"build\"]\nmax_age_days = 3\n",
        )
        .expect("parses");
        assert_eq!(config.max_age_days, 3);
        assert!(ArtifactGroup::ALL.iter().all(|group| config.scans(*group)));
        assert!(config.menu_bar_size);
    }

    #[test]
    fn defaults_include_existing_roots_once_and_fall_back() {
        let home = std::env::temp_dir().join(format!("wd40-default-roots-{}", std::process::id()));
        let _ = fs::remove_dir_all(&home);
        fs::create_dir_all(&home).unwrap();
        assert_eq!(scan_dirs_in(&home), vec![home.join("Develop")]);
        fs::create_dir_all(home.join("Projects")).unwrap();
        fs::create_dir_all(home.join("Documents")).unwrap();
        std::os::unix::fs::symlink(home.join("Projects"), home.join("Documents/GitHub")).unwrap();
        fs::create_dir_all(home.join("code")).unwrap();
        assert_eq!(scan_dirs_in(&home), vec![home.join("Projects"), home.join("code")]);
        assert_eq!(Config::default().max_depth, 8);
        fs::remove_dir_all(home).unwrap();
    }
}
