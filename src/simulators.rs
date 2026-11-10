// CoreSimulator discovery and tool-owned cleanup; never unlink device directories.
// Exports: collect, label, clean, is_device_fs. Deps: serde_json, std.
use crate::scanner::{ArtifactKind, TargetDir};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, OnceLock};
use std::time::SystemTime;

#[derive(Deserialize)]
struct Listing {
    devices: BTreeMap<String, Vec<Device>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Device {
    udid: String,
    name: String,
    state: String,
    is_available: bool,
}

type Runner<'a> = dyn FnMut(&[&str]) -> Result<String, String> + 'a;
static LABELS: OnceLock<Mutex<BTreeMap<PathBuf, String>>> = OnceLock::new();

fn run(args: &[&str]) -> Result<String, String> {
    let output = Command::new("/usr/bin/xcrun")
        .args(args)
        .output()
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr)
            .lines()
            .next()
            .filter(|line| !line.is_empty())
            .unwrap_or("simctl failed")
            .to_string());
    }
    String::from_utf8(output.stdout).map_err(|e| e.to_string())
}

fn listing(runner: &mut Runner<'_>) -> Result<Listing, String> {
    serde_json::from_str(&runner(&["simctl", "list", "devices", "-j"])?).map_err(|e| e.to_string())
}

fn root() -> Option<PathBuf> {
    dirs::home_dir().map(|home| home.join("Library/Developer/CoreSimulator/Devices"))
}

pub fn collect(found: &mut Vec<TargetDir>) {
    let Some(root) = root() else { return };
    collect_with(found, &root, &mut run);
}

fn collect_with(found: &mut Vec<TargetDir>, root: &Path, runner: &mut Runner<'_>) {
    let Ok(list) = listing(runner) else { return };
    let mut labels = LABELS.get_or_init(Mutex::default).lock().unwrap();
    for (runtime, devices) in list.devices {
        for device in devices
            .into_iter()
            .filter(|device| device.state == "Shutdown")
        {
            if !valid_udid(&device.udid) {
                continue;
            }
            let path = root.join(&device.udid);
            if path.is_symlink() || !path.is_dir() {
                continue;
            }
            found.retain(|target| target.path != path.join("data/Library/Caches"));
            labels.insert(
                path.clone(),
                format!("{} · {}", device.name, runtime_label(&runtime)),
            );
            found.push(TargetDir {
                path,
                kind: ArtifactKind::Simulator,
                size_bytes: 0,
                last_modified: SystemTime::UNIX_EPOCH,
            });
        }
    }
}

fn runtime_label(runtime: &str) -> String {
    let name = runtime.rsplit('.').next().unwrap_or(runtime);
    // `iOS-27-2` is iOS 27.2; a device-specific runtime adds `-iPhone19-4`.
    let mut parts = name.split('-');
    let platform = parts.next().unwrap_or(name);
    let rest: Vec<&str> = parts.collect();
    let numeric = rest.iter().take_while(|part| part.parse::<u32>().is_ok()).count();
    let version = rest[..numeric].join(".");
    let suffix = rest[numeric..].join("-");
    [platform, &version, &suffix].iter().filter(|part| !part.is_empty())
        .copied().collect::<Vec<_>>().join(" ")
}

fn valid_udid(udid: &str) -> bool {
    udid.len() == 36
        && udid.bytes().enumerate().all(|(i, byte)| {
            if [8, 13, 18, 23].contains(&i) {
                byte == b'-'
            } else {
                byte.is_ascii_hexdigit()
            }
        })
}

pub fn label(path: &Path) -> String {
    LABELS
        .get_or_init(Mutex::default)
        .lock()
        .unwrap()
        .get(path)
        .cloned()
        .unwrap_or_else(|| {
            path.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned()
        })
}

pub fn clean(path: &Path) -> Result<(), String> {
    let root = root().ok_or("simulator home unavailable")?;
    clean_with(path, &root, &mut run)
}

fn clean_with(path: &Path, root: &Path, runner: &mut Runner<'_>) -> Result<(), String> {
    let udid = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("missing simulator UDID")?;
    if path.parent() != Some(root) || !valid_udid(udid) || root.is_symlink() || path.is_symlink() {
        return Err("invalid simulator path".into());
    }
    let list = listing(runner)?;
    let device = list
        .devices
        .values()
        .flatten()
        .find(|device| device.udid == udid)
        .ok_or("simulator no longer listed")?;
    if device.state != "Shutdown" {
        return Err("simulator is no longer Shutdown".into());
    }
    let action = if device.is_available {
        "erase"
    } else {
        "delete"
    };
    runner(&["simctl", action, udid]).map(|_| ())
}

/// The virtual mirror of booted devices (`CoreDevice/DeviceFS`), or anything
/// inside it. Decided from the path alone: discovery asks this of every
/// directory it meets, so it must not cost a syscall.
pub fn is_device_fs(path: &Path) -> bool {
    let mut previous: Option<&std::ffi::OsStr> = None;
    path.components().any(|part| {
        let part = part.as_os_str();
        let hit = previous == Some(std::ffi::OsStr::new("CoreDevice")) && part == "DeviceFS";
        previous = Some(part);
        hit
    })
}

#[cfg(test)]
#[path = "simulator_tests.rs"]
mod tests;
