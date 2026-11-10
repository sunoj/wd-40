// Human-readable labels and paths for scanned artifact directories, kept
// distinct: two rows that read the same tell the user nothing about which one
// they are about to delete.
// Exports: `display_names`, `display_path`, `project_name`, `age`.
// Deps: dirs, crate::cache_names, wd40::scanner.

use std::collections::HashMap;
use std::path::Path;
use std::time::SystemTime;
use wd40::scanner::{ArtifactKind, TargetDir};

const SECONDS_PER_DAY: u64 = 86_400;

/// Directory names that hold something rather than name it, so they cannot tell
/// two rows apart on their own.
const CONTAINERS: &[&str] =
    &["data", "library", "caches", "cache", "store", "var", "share", "tmp", "developer"];

/// How far up the tree to look for something distinguishing before giving up.
const TAG_DEPTH: usize = 3;

/// One label per target, disambiguated so no two rows read the same.
pub fn display_names(targets: &[TargetDir]) -> Vec<String> {
    let mut names: Vec<String> = targets.iter().map(project_name).collect();
    let mut seen: HashMap<&str, usize> = HashMap::new();
    for name in &names {
        *seen.entry(name.as_str()).or_insert(0) += 1;
    }
    let repeated: Vec<String> = seen
        .iter()
        .filter(|(_, &count)| count > 1)
        .map(|(name, _)| (*name).to_string())
        .collect();

    for (index, name) in names.iter_mut().enumerate() {
        if !repeated.iter().any(|dup| dup == name) {
            continue;
        }
        let path = &targets[index].path;
        // Two projects share a directory name — say which tree each lives in.
        if let Some(context) = context_dir(path, name) {
            *name = format!("{context}/{name}");
        } else if let Some(tag) = distinguishing_tag(path, name) {
            *name = format!("{name} ({tag})");
        }
    }
    names
}

/// Something short out of the path for rows the label alone cannot separate:
/// the nearest ancestor that names anything, skipping the containers and
/// anything the label already says. A UUID directory is cut to its first block
/// so the row still fits. The walk stops at the home directory — the user's own
/// account name distinguishes nothing from anything.
fn distinguishing_tag(path: &Path, name: &str) -> Option<String> {
    let home = dirs::home_dir();
    let label = name.to_lowercase();
    path.ancestors()
        .skip(1)
        .take(TAG_DEPTH)
        .take_while(|dir| !home.as_deref().is_some_and(|home| home == *dir))
        .filter_map(|dir| dir.file_name()?.to_str())
        .find(|part| {
            let part = part.to_lowercase();
            !CONTAINERS.contains(&part.as_str()) && !label.contains(&part)
        })
        .map(short_tag)
}

fn short_tag(part: &str) -> String {
    let uuid = part.len() == 36 && part.matches('-').count() == 4;
    match uuid {
        true => part.split('-').next().unwrap_or(part).to_string(),
        false => part.to_string(),
    }
}

/// The directory one level above the project, when the project dir is what the
/// name came from. Anything else (tmp targets, shared cargo roots) is already
/// unique by construction.
fn context_dir(path: &Path, name: &str) -> Option<String> {
    let project = path.parent()?;
    if project.file_name()?.to_str()? != name {
        return None;
    }
    Some(project.parent()?.file_name()?.to_str()?.to_string())
}

pub fn display_path(path: &Path) -> String {
    let text = path.to_string_lossy().into_owned();
    let Some(home) = dirs::home_dir() else { return text };
    match path.strip_prefix(&home) {
        Ok(rel) => format!("~/{}", rel.display()),
        Err(_) => text,
    }
}

pub fn age_short(modified: SystemTime) -> String {
    let Ok(elapsed) = SystemTime::now().duration_since(modified) else {
        return "now".to_string();
    };
    match elapsed.as_secs() / SECONDS_PER_DAY {
        0 => "today".to_string(),
        1 => "1d ago".to_string(),
        days => format!("{days}d ago"),
    }
}

/// Longer age phrase kept for the CLI / future menus.
#[allow(dead_code)]
pub fn age(modified: SystemTime) -> String {
    let Ok(elapsed) = SystemTime::now().duration_since(modified) else {
        return "modified just now".to_string();
    };
    match elapsed.as_secs() / SECONDS_PER_DAY {
        0 => "modified today".to_string(),
        1 => "modified yesterday".to_string(),
        days => format!("modified {days} days ago"),
    }
}

pub fn project_name(td: &TargetDir) -> String {
    match td.kind {
        ArtifactKind::TmpTarget => {
            let dir_name = td.path.file_name().and_then(|n| n.to_str()).unwrap_or("unknown");
            // Strip the cc-target- prefix when present; otherwise show as-is so
            // names like "smart-router-target" stay recognizable.
            dir_name.strip_prefix("cc-target-").unwrap_or(dir_name).to_string()
        }
        ArtifactKind::Simulator => wd40::simulators::label(&td.path),
        ArtifactKind::Toolchain => wd40::toolchains::label(&td.path),
        // A cache is named after the tool that fills it: its own directory is
        // called "Caches" or "store" on every one of them.
        ArtifactKind::Cache => crate::cache_names::cache_label(&td.path).unwrap_or_else(|| {
            td.path.file_name().and_then(|n| n.to_str()).unwrap_or("cache").to_string()
        }),
        _ => {
            if let Some(home) = dirs::home_dir() {
                // Under ~/.cargo-target/<project>[/<session>]/, show path relative to the root.
                let shared_root = home.join(".cargo-target");
                if let Ok(rel) = td.path.strip_prefix(&shared_root) {
                    return rel.to_string_lossy().into_owned();
                }
                // Under ~/.aid/worktrees/<repo>/<branch>/target, show "<repo>/<branch>".
                let aid_root = home.join(".aid").join("worktrees");
                if let Ok(rel) = td.path.strip_prefix(&aid_root) {
                    if let Some(parent) = rel.parent() {
                        let name = parent.to_string_lossy();
                        if !name.is_empty() {
                            return name.into_owned();
                        }
                    }
                }
            }
            // Standard target/node_modules/.next — show the containing project dir name.
            td.path
                .parent()
                .and_then(|p| p.file_name())
                .and_then(|n| n.to_str())
                .unwrap_or("unknown")
                .to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::display_names;
    use std::path::PathBuf;
    use std::time::SystemTime;
    use wd40::scanner::{ArtifactKind, TargetDir};

    fn target(path: &str, kind: ArtifactKind) -> TargetDir {
        TargetDir {
            path: PathBuf::from(path),
            size_bytes: 0,
            last_modified: SystemTime::UNIX_EPOCH,
            kind,
        }
    }

    #[test]
    fn repeated_project_names_gain_their_parent_directory() {
        let targets = vec![
            target("/w/alpha/web/node_modules", ArtifactKind::NodeModules),
            target("/w/beta/web/node_modules", ArtifactKind::NodeModules),
            target("/w/gamma/api/node_modules", ArtifactKind::NodeModules),
        ];
        assert_eq!(display_names(&targets), ["alpha/web", "beta/web", "api"]);
    }

    #[test]
    fn a_name_outside_the_home_directory_survives_display() {
        let targets = vec![target("/tmp/cc-target-solo", ArtifactKind::TmpTarget)];
        assert_eq!(display_names(&targets), ["solo"]);
    }

    /// The seven rows that all read "Caches": every simulator keeps its cache
    /// at the same three path components, so the name has to come from further
    /// up. No device.plist exists under these paths, so this is the fallback.
    #[test]
    fn simulator_caches_no_longer_all_read_the_same() {
        let devices = "/Users/x/Library/Developer/CoreSimulator/Devices";
        let targets = vec![
            target(
                &format!("{devices}/03203F31-0A9E-4B47-8680-1B500C8FDF81/data/Library/Caches"),
                ArtifactKind::Cache,
            ),
            target(
                &format!("{devices}/AB12CD34-0A9E-4B47-8680-1B500C8FDF81/data/Library/Caches"),
                ArtifactKind::Cache,
            ),
        ];
        assert_eq!(display_names(&targets), ["Simulator 03203F31", "Simulator AB12CD34"]);
    }

    /// Two rows that a label alone cannot separate take a tag from their path,
    /// and never the account name every path on the machine shares.
    #[test]
    fn a_repeated_label_gains_a_tag_from_its_own_path() {
        let Some(home) = dirs::home_dir() else { return };
        let targets = vec![
            target(&home.join("Library/pnpm/store").to_string_lossy(), ArtifactKind::Cache),
            target(&home.join(".cache/pnpm").to_string_lossy(), ArtifactKind::Cache),
        ];
        assert_eq!(display_names(&targets), ["pnpm store", "pnpm store (.cache)"]);
    }

    /// Two simulators can carry the same device name; the UDID separates them,
    /// cut down to the one block that fits on a row.
    #[test]
    fn two_devices_of_one_name_are_told_apart_by_their_udid() {
        let root = std::env::temp_dir().join(format!("wd40-names-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let udids = ["03203F31-0A9E-4B47-8680-1B500C8FDF81", "AB12CD34-0A9E-4B47-8680-1B500C8FDF81"];
        let mut targets = Vec::new();
        for udid in udids {
            let device = root.join("CoreSimulator/Devices").join(udid);
            let _ = std::fs::create_dir_all(device.join("data/Library/Caches"));
            let _ = std::fs::write(
                device.join("device.plist"),
                "<key>name</key><string>iPhone 15 Pro</string>",
            );
            let caches = device.join("data/Library/Caches");
            targets.push(target(&caches.to_string_lossy(), ArtifactKind::Cache));
        }
        assert_eq!(
            display_names(&targets),
            ["iPhone 15 Pro simulator (03203F31)", "iPhone 15 Pro simulator (AB12CD34)"]
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
