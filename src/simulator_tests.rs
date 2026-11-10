// Injected simctl regression tests: no real erase/delete commands are run.
// Covers state, availability, malformed responses, nesting, and file-only age.
use super::*;
const UDID: &str = "12345678-1234-1234-1234-123456789ABC";

fn json(state: &str, available: bool) -> String {
    format!(
        r#"{{"devices":{{"com.apple.CoreSimulator.SimRuntime.iOS-27-0":[{{
        "udid":"{UDID}","name":"iPhone 17","state":"{state}","isAvailable":{available}
    }}]}}}}"#
    )
}

#[test]
fn cleanup_rechecks_state_and_selects_tool_action() {
    let root = Path::new("/fake/Devices");
    for (state, available, action) in [
        ("Shutdown", true, Some("erase")),
        ("Shutdown", false, Some("delete")),
        ("Booted", true, None),
        ("Creating", false, None),
    ] {
        let mut calls = Vec::new();
        let result = clean_with(&root.join(UDID), root, &mut |args| {
            calls.push(args.iter().map(|s| s.to_string()).collect::<Vec<_>>());
            Ok(if calls.len() == 1 {
                json(state, available)
            } else {
                String::new()
            })
        });
        assert_eq!(calls[0], ["simctl", "list", "devices", "-j"]);
        assert_eq!(result.is_ok(), action.is_some());
        assert_eq!(calls.len(), if action.is_some() { 2 } else { 1 });
        if let Some(action) = action {
            assert_eq!(calls[1], ["simctl", action, UDID]);
        }
    }
}

#[test]
fn invalid_listing_and_tool_failure_refuse_cleanup() {
    let root = Path::new("/fake/Devices");
    for response in [
        Ok("bad json".into()),
        Ok("{}".into()),
        Err("tool failed".into()),
    ] {
        assert!(clean_with(&root.join(UDID), root, &mut |_| response.clone()).is_err());
    }
    let mut calls = 0;
    let error = clean_with(&root.join(UDID), root, &mut |_| {
        calls += 1;
        if calls == 1 {
            Ok(json("Shutdown", true))
        } else {
            Err("erase failed".into())
        }
    })
    .unwrap_err();
    assert_eq!(error, "erase failed");
}

#[test]
fn only_shutdown_devices_replace_their_nested_cache() {
    let root = std::env::temp_dir().join(format!("wd40-simulator-{}", std::process::id()));
    let path = root.join(UDID);
    std::fs::create_dir_all(path.join("data/Library/Caches")).unwrap();
    for state in ["Booted", "Shutdown"] {
        let mut found = vec![TargetDir {
            path: path.join("data/Library/Caches"),
            kind: ArtifactKind::Cache,
            size_bytes: 0,
            last_modified: SystemTime::UNIX_EPOCH,
        }];
        collect_with(&mut found, &root, &mut |_| Ok(json(state, true)));
        assert_eq!(found.len(), 1);
        assert_eq!(
            matches!(found[0].kind, ArtifactKind::Simulator),
            state == "Shutdown"
        );
        if state == "Shutdown" {
            assert_eq!(label(&path), "iPhone 17 · iOS 27.0");
        }
    }
    let mut found = Vec::new();
    collect_with(&mut found, &root, &mut |_| Ok("invalid".into()));
    assert!(found.is_empty());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn age_ignores_directory_and_files_outside_data() {
    let root = std::env::temp_dir().join(format!("wd40-simulator-age-{}", std::process::id()));
    std::fs::create_dir_all(root.join("data")).unwrap();
    let file = std::fs::File::create(root.join("data/old")).unwrap();
    let old = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1000);
    file.set_modified(old).unwrap();
    std::fs::write(root.join("device.plist"), "new").unwrap();
    std::fs::create_dir(root.join("data/new-directory")).unwrap();
    assert_eq!(file_age(&root), Some(old));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn mirror_and_descendants_are_excluded() {
    let mirror = dirs::home_dir()
        .unwrap()
        .join("Library/Developer/CoreDevice/DeviceFS");
    assert!(is_device_fs(&mirror));
    assert!(is_device_fs(&mirror.join("device/target")));
}
