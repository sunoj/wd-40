// Labels for the caches the scan collects by path. Their own directory names
// are "Caches", "store" or "_cacache" — a dozen rows reading the same word,
// none of which says what it holds. This names them after the tool instead.
// Exports: `cache_label`. Deps: std only.

use std::path::Path;

/// Cache roots whose own directory name says nothing, matched against the tail
/// of the paths `roots::collect_dev_caches` pushes. First match wins; no tail
/// here is a suffix of another, so the order is only for reading.
const KNOWN: &[(&str, &str)] = &[
    ("Library/Developer/Xcode/DerivedData", "Xcode DerivedData"),
    ("Library/Developer/Xcode/ModuleCache.noindex", "Xcode module cache"),
    ("Library/Caches/org.swift.swiftpm", "SwiftPM cache"),
    ("Library/Caches/com.apple.dt.Xcode", "Xcode cache"),
    ("Library/Caches/Homebrew", "Homebrew downloads"),
    ("Library/Caches/pip", "pip cache"),
    (".cache/uv", "uv cache"),
    ("Library/Caches/uv", "uv cache"),
    ("Library/Caches/go-build", "Go build cache"),
    (".gradle/caches", "Gradle cache"),
    (".gradle/wrapper/dists", "Gradle distributions"),
    (".bun/install/cache", "Bun cache"),
    ("Library/Caches/CocoaPods", "CocoaPods cache"),
    ("Library/Caches/node-gyp", "node-gyp cache"),
    ("Library/Caches/Mozilla.sccache", "sccache"),
    ("Library/Caches/deno", "Deno cache"),
    (".npm/_npx", "npx cache"),
    ("Library/Developer/CoreSimulator/Caches", "CoreSimulator cache"),
    ("var/homebrew/cache", "Homebrew downloads"),
    (".local/share/pnpm/store", "pnpm store"),
    ("Library/pnpm/store", "pnpm store"),
    (".cache/pnpm", "pnpm store"),
    (".pnpm-store", "pnpm store"),
    ("Library/Caches/Yarn", "Yarn cache"),
    (".cache/yarn", "Yarn cache"),
    (".npm/_cacache", "npm cache"),
    (".cargo/registry", "Cargo registry"),
];

/// Every simulator's cache lives at
/// `…/CoreSimulator/Devices/<UDID>/data/Library/Caches`, which is why seven of
/// them arrived on screen all called "Caches".
const DEVICES: &str = "CoreSimulator/Devices";

/// What this cache is, when the path says more than the directory name does.
pub fn cache_label(path: &Path) -> Option<String> {
    if let Some(label) = simulator_label(path) {
        return Some(label);
    }
    if let Some(label) = xcode_device_support_label(path) {
        return Some(label);
    }
    let text = path.to_string_lossy();
    KNOWN
        .iter()
        .find(|(tail, _)| text.ends_with(tail))
        .map(|(_, label)| (*label).to_string())
}

fn xcode_device_support_label(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_str()?;
    if !name.ends_with(" DeviceSupport") || !path.parent()?.ends_with("Library/Developer/Xcode") {
        return None;
    }
    Some(format!("Xcode {name}"))
}

/// The device directory this cache belongs to, if it is a simulator's.
fn device_dir(path: &Path) -> Option<&Path> {
    path.ancestors()
        .find(|dir| dir.parent().is_some_and(|parent| parent.ends_with(DEVICES)))
}

fn simulator_label(path: &Path) -> Option<String> {
    let device = device_dir(path)?;
    let udid = device.file_name()?.to_str()?;
    match device_name(device) {
        Some(name) => Some(format!("{name} simulator")),
        // No name to be had — the first block of the UDID at least tells the
        // rows apart, and the full path is on the row itself.
        None => Some(format!("Simulator {}", udid.split('-').next().unwrap_or(udid))),
    }
}

/// The device's own name, out of the plist CoreSimulator keeps beside it.
/// That file is XML, so the one key worth reading is a substring away; a plist
/// parser would be a dependency for eleven lines. A binary plist, or a file
/// that is not there, simply yields nothing.
fn device_name(device: &Path) -> Option<String> {
    let text = std::fs::read_to_string(device.join("device.plist")).ok()?;
    let after = text.split("<key>name</key>").nth(1)?;
    let start = after.find("<string>")? + "<string>".len();
    let end = start + after[start..].find("</string>")?;
    let name = after[start..end].trim();
    (!name.is_empty()).then(|| unescape(name))
}

/// The five entities an XML plist can carry inside a string.
fn unescape(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

#[cfg(test)]
mod tests {
    use super::cache_label;
    use std::path::{Path, PathBuf};

    fn scratch(name: &str) -> PathBuf {
        let path = std::env::temp_dir()
            .join(format!("wd40-cache-names-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        path
    }

    #[test]
    fn known_cache_roots_are_named_after_their_tool() {
        assert_eq!(
            cache_label(Path::new("/Users/x/Library/Developer/Xcode/DerivedData")).as_deref(),
            Some("Xcode DerivedData")
        );
        assert_eq!(cache_label(Path::new("/Users/x/.npm/_cacache")).as_deref(), Some("npm cache"));
        assert_eq!(cache_label(Path::new("/Users/x/.cache/pnpm")).as_deref(), Some("pnpm store"));
        assert_eq!(
            cache_label(Path::new("/Users/x/.cargo/registry")).as_deref(),
            Some("Cargo registry")
        );
        assert_eq!(
            cache_label(Path::new("/Users/x/Library/Caches/org.swift.swiftpm")).as_deref(),
            Some("SwiftPM cache")
        );
        assert_eq!(
            cache_label(Path::new("/Users/x/Library/Caches/com.apple.dt.Xcode")).as_deref(),
            Some("Xcode cache")
        );
        assert_eq!(
            cache_label(Path::new("/Users/x/Library/Developer/Xcode/iOS DeviceSupport")).as_deref(),
            Some("Xcode iOS DeviceSupport")
        );
        assert_eq!(cache_label(Path::new("/Users/x/Develop/thing")), None);
        for (path, label) in [
            ("Library/Caches/pip", "pip cache"), (".cache/uv", "uv cache"),
            ("Library/Caches/uv", "uv cache"), ("Library/Caches/go-build", "Go build cache"),
            (".gradle/caches", "Gradle cache"), (".gradle/wrapper/dists", "Gradle distributions"),
            (".bun/install/cache", "Bun cache"), ("Library/Caches/CocoaPods", "CocoaPods cache"),
            ("Library/Caches/node-gyp", "node-gyp cache"),
            ("Library/Caches/Mozilla.sccache", "sccache"), ("Library/Caches/deno", "Deno cache"),
            (".npm/_npx", "npx cache"),
            ("Library/Developer/CoreSimulator/Caches", "CoreSimulator cache"),
        ] {
            assert_eq!(cache_label(&Path::new("/Users/x").join(path)).as_deref(), Some(label));
        }
    }

    #[test]
    fn a_simulator_cache_is_named_after_its_device() {
        let root = scratch("device");
        let udid = "03203F31-0A9E-4B47-8680-1B500C8FDF81";
        let device = root.join("CoreSimulator/Devices").join(udid);
        let caches = device.join("data/Library/Caches");
        let _ = std::fs::create_dir_all(&caches);
        let _ = std::fs::write(
            device.join("device.plist"),
            "<dict><key>name</key><string>iPhone 15 Pro</string></dict>",
        );
        assert_eq!(cache_label(&caches).as_deref(), Some("iPhone 15 Pro simulator"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_nameless_simulator_still_tells_itself_apart() {
        let udid = "AB12CD34-0A9E-4B47-8680-1B500C8FDF81";
        let caches = scratch("nameless")
            .join("CoreSimulator/Devices")
            .join(udid)
            .join("data/Library/Caches");
        assert_eq!(cache_label(&caches).as_deref(), Some("Simulator AB12CD34"));
    }
}
