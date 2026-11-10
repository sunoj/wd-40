// Two levels of remembered measurement, each kind kept for as long as it stays
// true. Level one is this process; level two is a file, because the expensive
// pass is the one that runs at launch and a cache that dies with the process
// never spares it.
// Exports: `ttl`, `measurement_of`, `put_measurement`, `forget`, `attribution_for`,
// `put_attribution`, `load`, `flush`. Extent maps live in `extent_cache`.
// Deps: serde, toml, dirs, crate::{extents, scanner}.

use crate::extents::Attribution;
use crate::scanner::ArtifactKind;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// How long a measurement of this kind stays believable.
///
/// Not one number: what these directories are decides how fast they go wrong.
/// A toolchain is written once, by `rustup toolchain install`. A download cache
/// grows only when something is fetched. A Rust target is written by every
/// build, which is why it is never kept at all — a stale figure for the thing
/// the app exists to watch is worse than a slow one.
pub fn ttl(kind: ArtifactKind) -> Duration {
    match kind {
        ArtifactKind::Simulator => Duration::from_secs(60 * 60),
        ArtifactKind::Toolchain => Duration::from_secs(24 * 60 * 60),
        ArtifactKind::Cache => Duration::from_secs(60 * 60),
        ArtifactKind::NodeModules => Duration::from_secs(10 * 60),
        ArtifactKind::BuildOutput => Duration::from_secs(2 * 60),
        ArtifactKind::RustTarget | ArtifactKind::TmpTarget => Duration::ZERO,
    }
}

/// The device-byte pass is expensive wherever its inputs came from, so it is
/// kept longer than any single directory — and guarded by a fingerprint of
/// every path, size, and newest descendant timestamp that went into it.
const ATTRIBUTION_TTL: Duration = Duration::from_secs(6 * 60 * 60);

/// Paths kept on disk. A developer's machine turns over build directories
/// endlessly and an unbounded file would grow for ever.
const MAX_ENTRIES: usize = 512;

#[derive(Serialize, Deserialize, Default)]
struct Store {
    #[serde(default)]
    sizes: HashMap<String, Sized_>,
    #[serde(default)]
    attribution: Option<Attributed>,
}

#[derive(Serialize, Deserialize, Clone)]
struct Sized_ {
    /// The directory's own mtime when this was measured. A clock check alone
    /// would keep a figure for a directory that changed a second later.
    modified: u64,
    /// Newest entry below the target when its contents were measured.
    content_modified: u64,
    measured: u64,
    bytes: u64,
}

#[derive(Serialize, Deserialize, Clone)]
struct Attributed {
    fingerprint: Vec<(String, u64, u64)>,
    measured: u64,
    value: Attribution,
}

static STORE: Mutex<Option<Store>> = Mutex::new(None);
/// Set when level one has moved on from what is on disk.
static DIRTY: Mutex<bool> = Mutex::new(false);

/// The size and newest content time measured for `path`, while both remain
/// within this kind's TTL and the directory has not been touched since.
pub fn measurement_of(path: &Path, kind: ArtifactKind) -> Option<(u64, SystemTime)> {
    let ttl = ttl(kind);
    if ttl.is_zero() {
        return None;
    }
    let modified = seconds(std::fs::metadata(path).ok()?.modified().ok()?)?;
    let guard = STORE.lock().ok()?;
    let entry = guard.as_ref()?.sizes.get(&key(path))?;
    let fresh = now().saturating_sub(entry.measured) <= ttl.as_secs();
    (entry.modified == modified && fresh).then(|| {
        (entry.bytes, UNIX_EPOCH + Duration::from_secs(entry.content_modified))
    })
}

pub fn put_measurement(
    path: &Path,
    kind: ArtifactKind,
    bytes: u64,
    content_modified: SystemTime,
) {
    if ttl(kind).is_zero() {
        return;
    }
    let Some(modified) = std::fs::metadata(path).ok().and_then(|m| m.modified().ok()).and_then(seconds)
    else {
        return;
    };
    let Some(content_modified) = seconds(content_modified) else { return };
    with_store(|store| {
        store.sizes.insert(
            key(path),
            Sized_ { modified, content_modified, measured: now(), bytes },
        );
    });
}

/// Drop what was kept for `path`. Called when a target is removed, so one that
/// comes back is measured rather than answered from before it went.
pub fn forget(path: &Path) {
    crate::extent_cache::forget(path);
    with_store(|store| {
        store.sizes.remove(&key(path));
        // Any accounting that counted this path is now about a disk that no
        // longer exists.
        store.attribution = None;
    });
}

/// The device-byte accounting, if it was made from exactly these paths, sizes,
/// and newest descendant timestamps and is still within its TTL. The timestamp
/// is activity evidence, not proof that physical extents stayed unchanged:
/// filesystem timestamps can be coarse and a writer can restore them.
pub fn attribution_for(fingerprint: &[(PathBuf, u64, u64)]) -> Option<Attribution> {
    let wanted = printable(fingerprint);
    let guard = STORE.lock().ok()?;
    let kept = guard.as_ref()?.attribution.as_ref()?;
    let fresh = now().saturating_sub(kept.measured) <= ATTRIBUTION_TTL.as_secs();
    (kept.fingerprint == wanted && fresh).then(|| kept.value.clone())
}

pub fn put_attribution(fingerprint: &[(PathBuf, u64, u64)], value: Attribution) {
    let printed = printable(fingerprint);
    with_store(|store| {
        store.attribution = Some(Attributed {
            fingerprint: printed.clone(),
            measured: now(),
            value: value.clone(),
        });
    });
}

/// Read level two. Cheap, and a file that will not parse is simply an empty
/// cache: every figure in it can be measured again.
pub fn load() {
    let store = path()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|text| toml::from_str::<Store>(&text).ok())
        .unwrap_or_default();
    if let Ok(mut guard) = STORE.lock() {
        *guard = Some(store);
    }
}

/// Write level two, if anything changed. Called when a scan is over rather than
/// on every entry: the point is to spare the next launch, not this one.
pub fn flush() {
    let Ok(mut dirty) = DIRTY.lock() else { return };
    if !*dirty {
        return;
    }
    *dirty = false;
    drop(dirty);
    let Ok(mut guard) = STORE.lock() else { return };
    let Some(store) = guard.as_mut() else { return };
    trim(store);
    let Ok(text) = toml::to_string(store) else { return };
    let Some(path) = path() else { return };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(path, text);
}

/// Keep the newest measurements and drop the rest, so a machine that turns over
/// build directories does not grow the file for ever.
fn trim(store: &mut Store) {
    if store.sizes.len() <= MAX_ENTRIES {
        return;
    }
    let mut kept: Vec<(String, Sized_)> = store.sizes.drain().collect();
    kept.sort_by_key(|(_, entry)| std::cmp::Reverse(entry.measured));
    kept.truncate(MAX_ENTRIES);
    store.sizes = kept.into_iter().collect();
}

fn with_store(edit: impl FnOnce(&mut Store)) {
    if let Ok(mut guard) = STORE.lock() {
        edit(guard.get_or_insert_with(Store::default));
        if let Ok(mut dirty) = DIRTY.lock() {
            *dirty = true;
        }
    }
}

fn printable(fingerprint: &[(PathBuf, u64, u64)]) -> Vec<(String, u64, u64)> {
    fingerprint
        .iter()
        .map(|(path, bytes, content_modified)| {
            (path.to_string_lossy().into_owned(), *bytes, *content_modified)
        })
        .collect()
}

fn key(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn seconds(at: SystemTime) -> Option<u64> {
    at.duration_since(UNIX_EPOCH).ok().map(|since| since.as_secs())
}

fn now() -> u64 {
    seconds(SystemTime::now()).unwrap_or(0)
}

fn path() -> Option<PathBuf> {
    dirs::home_dir().map(|home| home.join(".config/wd-40/cache.toml"))
}

#[cfg(test)]
mod tests {
    use super::{forget, measurement_of, put_measurement, ttl};
    use crate::scanner::ArtifactKind;
    use std::path::PathBuf;

    fn scratch(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("wd40-ttl-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        let _ = std::fs::create_dir_all(&path);
        path
    }

    /// The red line: a build directory has no TTL at all, so its figure is
    /// never answered from either level.
    #[test]
    fn a_build_directory_is_never_kept() {
        let path = scratch("target");
        put_measurement(&path, ArtifactKind::RustTarget, 4096, std::time::SystemTime::now());
        assert_eq!(measurement_of(&path, ArtifactKind::RustTarget), None);
        assert!(ttl(ArtifactKind::RustTarget).is_zero());
        assert!(ttl(ArtifactKind::TmpTarget).is_zero());
        let _ = std::fs::remove_dir_all(&path);
    }

    /// What never changes is kept longest, and what changes fastest is kept
    /// least — the whole point of having more than one number.
    #[test]
    fn the_ttls_are_ordered_by_how_fast_the_thing_moves() {
        assert!(ttl(ArtifactKind::Toolchain) > ttl(ArtifactKind::Cache));
        assert!(ttl(ArtifactKind::Cache) > ttl(ArtifactKind::NodeModules));
        assert!(ttl(ArtifactKind::NodeModules) > ttl(ArtifactKind::BuildOutput));
        assert!(ttl(ArtifactKind::BuildOutput) > ttl(ArtifactKind::RustTarget));
    }

    #[test]
    fn a_toolchain_is_answered_the_second_time() {
        let path = scratch("toolchain");
        assert_eq!(measurement_of(&path, ArtifactKind::Toolchain), None);
        let modified = std::time::SystemTime::now();
        put_measurement(&path, ArtifactKind::Toolchain, 4096, modified);
        assert_eq!(measurement_of(&path, ArtifactKind::Toolchain).map(|value| value.0), Some(4096));
        let _ = std::fs::remove_dir_all(&path);
    }

    /// The clock is not the only guard: a directory touched inside its TTL has
    /// to be measured again.
    #[test]
    fn touching_the_directory_beats_the_clock() {
        let path = scratch("touched");
        put_measurement(&path, ArtifactKind::Cache, 4096, std::time::SystemTime::now());
        assert_eq!(measurement_of(&path, ArtifactKind::Cache).map(|value| value.0), Some(4096));
        std::thread::sleep(std::time::Duration::from_millis(1100));
        let _ = std::fs::write(path.join("new"), b"x");
        assert_eq!(measurement_of(&path, ArtifactKind::Cache), None);
        let _ = std::fs::remove_dir_all(&path);
    }

    #[test]
    fn forgetting_a_removed_path_makes_it_measurable_again() {
        let path = scratch("forgotten");
        put_measurement(&path, ArtifactKind::Cache, 4096, std::time::SystemTime::now());
        forget(&path);
        assert_eq!(measurement_of(&path, ArtifactKind::Cache), None);
        let _ = std::fs::remove_dir_all(&path);
    }

    #[test]
    fn a_path_that_is_gone_is_never_answered() {
        let path = scratch("vanished");
        put_measurement(&path, ArtifactKind::Cache, 4096, std::time::SystemTime::now());
        let _ = std::fs::remove_dir_all(&path);
        assert_eq!(measurement_of(&path, ArtifactKind::Cache), None);
    }
}
