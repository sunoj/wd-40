// Artifact vocabulary for WD-40: what a target is, which group it belongs to,
// and how a size reads. Discovery lives in `discover`, measurement in `sizes`.
// Exports: `TargetDir`, `ArtifactKind`, `ArtifactGroup`, `human_size`,
// `sizes_may_overlap`. Deps: std only.

use std::path::PathBuf;
use std::time::SystemTime;

/// True when any target lives under a shared cargo target root. `aid` seeds
/// those per-session dirs with `cp -Rc` (APFS clone), so their reported sizes
/// share physical blocks and summing them overstates what a delete returns.
/// See docs/apfs-clone-overcount.md.
pub fn sizes_may_overlap(paths: &[PathBuf]) -> bool {
    paths.iter().any(|path| {
        path.components()
            .any(|component| component.as_os_str() == ".cargo-target")
    })
}

#[derive(Clone)]
pub struct TargetDir {
    pub path: PathBuf,
    pub size_bytes: u64,
    pub last_modified: SystemTime,
    pub kind: ArtifactKind,
}

#[derive(Clone, Copy)]
pub enum ArtifactKind {
    RustTarget,
    NodeModules,
    BuildOutput,
    Cache,
    Toolchain,
    Simulator,
    TmpTarget,
}

impl ArtifactKind {
    /// What a directory name discovered by the walk stands for. `discover` and
    /// the settings screen have to agree on this, or a group could be switched
    /// off and still be found.
    pub fn for_dir_name(name: &str) -> Self {
        match name {
            "target" => Self::RustTarget,
            "node_modules" => Self::NodeModules,
            _ => Self::BuildOutput,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::RustTarget => "target",
            Self::NodeModules => "node_modules",
            Self::BuildOutput => "build",
            Self::Cache => "cache",
            Self::Toolchain => "toolchain",
            Self::Simulator => "simulator",
            Self::TmpTarget => "tmp-target",
        }
    }

    pub fn group(self) -> ArtifactGroup {
        match self {
            Self::RustTarget | Self::TmpTarget => ArtifactGroup::Rust,
            Self::NodeModules => ArtifactGroup::NodeModules,
            Self::BuildOutput => ArtifactGroup::BuildOutput,
            Self::Cache => ArtifactGroup::Caches,
            Self::Toolchain => ArtifactGroup::Toolchains,
            Self::Simulator => ArtifactGroup::Simulators,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ArtifactGroup {
    Rust,
    NodeModules,
    BuildOutput,
    Caches,
    Toolchains,
    Simulators,
}

use crate::artifact_help::*;

impl ArtifactGroup {
    pub const ALL: &[ArtifactGroup] = &[
        ArtifactGroup::Rust,
        ArtifactGroup::NodeModules,
        ArtifactGroup::BuildOutput,
        ArtifactGroup::Caches,
        ArtifactGroup::Toolchains,
        ArtifactGroup::Simulators,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Rust => "Rust",
            Self::NodeModules => "Node Modules",
            Self::BuildOutput => "Build Output",
            Self::Caches => "Caches",
            Self::Toolchains => "Toolchains",
            Self::Simulators => "Simulators",
        }
    }

    /// How the group is named to the user in the popover.
    pub fn title(self) -> &'static str {
        match self {
            Self::Rust => "Rust targets",
            Self::NodeModules => "node_modules",
            Self::BuildOutput => "Build output",
            Self::Caches => "Caches",
            Self::Toolchains => "Rust toolchains",
            Self::Simulators => "Simulator devices",
        }
    }

    /// Stable key for the config file. Never derived from `label`, which is
    /// prose and may be reworded.
    pub fn key(self) -> &'static str {
        match self {
            Self::Rust => "rust",
            Self::NodeModules => "node_modules",
            Self::BuildOutput => "build_output",
            Self::Caches => "caches",
            Self::Toolchains => "toolchains",
            Self::Simulators => "simulators",
        }
    }

    /// SF Symbol shown next to the group header.
    pub fn symbol(self) -> &'static str {
        match self {
            Self::Rust => "gearshape.2",
            Self::NodeModules => "shippingbox",
            Self::BuildOutput => "hammer",
            // externaldrive.badge.clock does not exist on every macOS the app runs on,
            // and a missing symbol draws nothing at all.
            Self::Caches => "clock.arrow.circlepath",
            Self::Toolchains => "wrench.and.screwdriver",
            Self::Simulators => "iphone",
        }
    }

    pub fn tag(self) -> isize {
        match self {
            Self::Rust => 0,
            Self::NodeModules => 1,
            Self::BuildOutput => 2,
            Self::Caches => 3,
            Self::Toolchains => 4,
            Self::Simulators => 5,
        }
    }

    pub fn from_tag(tag: isize) -> Option<Self> {
        match tag {
            0 => Some(Self::Rust),
            1 => Some(Self::NodeModules),
            2 => Some(Self::BuildOutput),
            3 => Some(Self::Caches),
            4 => Some(Self::Toolchains),
            5 => Some(Self::Simulators),
            _ => None,
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::Rust => RUST_HELP,
            Self::NodeModules => NODE_HELP,
            Self::BuildOutput => BUILD_HELP,
            Self::Caches => CACHE_HELP,
            Self::Toolchains => TOOLCHAIN_HELP,
            Self::Simulators => SIMULATOR_HELP,
        }
    }
}

pub fn human_size(bytes: u64) -> String {
    let mut value = bytes as f64;
    let mut unit = 0;
    let units = ["B", "K", "M", "G", "T"];
    while value >= 1024.0 && unit < units.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{}{}", bytes, units[unit])
    } else {
        format!("{:.1}{}", value, units[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::sizes_may_overlap;
    use std::path::PathBuf;

    #[test]
    fn shared_cargo_root_targets_can_overlap() {
        let paths = vec![PathBuf::from("/Users/x/.cargo-target/proj/session-a")];
        assert!(sizes_may_overlap(&paths));
    }

    #[test]
    fn ordinary_project_targets_do_not_overlap() {
        let paths = vec![
            PathBuf::from("/Users/x/Develop/proj/target"),
            PathBuf::from("/Users/x/Develop/web/node_modules"),
        ];
        assert!(!sizes_may_overlap(&paths));
    }
}
