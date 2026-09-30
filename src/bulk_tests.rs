use super::{read_dir, read_dir_with_refills};
use crate::sizes::read_dir_standard;
use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::symlink;
use std::path::Path;

fn compare(dir: &Path, buffer_size: usize) {
    let bulk = read_dir(dir, buffer_size).expect("bulk reader must succeed without fallback");
    let standard = read_dir_standard(dir);
    assert!(!standard.broken);
    assert_eq!(bulk.entry_count, standard.entry_count);
    assert_eq!(bulk.bytes, standard.bytes);
    assert_eq!(bulk.last_modified, standard.last_modified);
    let mut actual = bulk.subdirs;
    let mut expected = standard.subdirs;
    actual.sort();
    expected.sort();
    assert_eq!(actual, expected);
}

#[test]
fn tree_matches_standard_with_default_and_tiny_buffers() {
    let root = std::env::temp_dir().join(format!("wd40-bulk-tree-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("nested/deep")).unwrap();
    std::fs::create_dir(root.join("empty")).unwrap();
    for (name, length) in [
        ("zero", 0),
        ("small", 1),
        ("medium", 4097),
        ("large", 131_072),
    ] {
        std::fs::write(root.join(name), vec![b'x'; length]).unwrap();
    }
    std::fs::write(root.join("nested/deep/child"), b"child").unwrap();
    symlink(root.join("small"), root.join("file-link")).unwrap();
    symlink(root.join("nested"), root.join("dir-link")).unwrap();
    let raw_name = root.join(OsStr::from_bytes(b"invalid-\xff-name"));
    if let Err(error) = std::fs::create_dir(raw_name) {
        // APFS commonly rejects invalid UTF-8 path components outright.
        assert_eq!(error.raw_os_error(), Some(libc::EILSEQ));
    }
    for index in 0..25 {
        std::fs::write(root.join(format!("extra-{index:03}")), b"x").unwrap();
    }
    for buffer_size in [super::BUFFER_SIZE, 256] {
        for dir in [
            &root,
            &root.join("nested"),
            &root.join("nested/deep"),
            &root.join("empty"),
        ] {
            compare(dir, buffer_size);
        }
    }
    let bulk = read_dir(&root, 256).unwrap();
    assert!(bulk.subdirs.contains(&root.join("nested")));
    assert!(!bulk.subdirs.contains(&root.join("dir-link")));
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn shorter_later_refill_never_replays_earlier_entries() {
    let root =
        std::env::temp_dir().join(format!("wd40-bulk-short-refill-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir(&root).unwrap();
    for index in 0..97 {
        let name = format!("artifact-{index:03}-{}", "x".repeat(95 - index / 2));
        std::fs::write(root.join(name), b"x").unwrap();
    }
    let mut counts = Vec::new();
    let bulk = read_dir_with_refills(&root, 512, |count| counts.push(count)).unwrap();
    assert!(counts.len() > 2, "expected multiple refills: {counts:?}");
    assert!(
        counts.windows(2).any(|pair| pair[1] < pair[0]),
        "expected a later refill with fewer entries: {counts:?}"
    );
    let standard = read_dir_standard(&root);
    assert_eq!(bulk.entry_count, standard.entry_count);
    assert_eq!(bulk.bytes, standard.bytes);
    assert_eq!(bulk.last_modified, standard.last_modified);
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn raw_name_bytes_survive_record_parsing() {
    let raw_name = b"invalid-\xff-name\0";
    let record = directory_record(raw_name, 0);
    let root = Path::new("/tmp");
    let mut found = empty_found();
    super::parse_record(&record, root, &mut found).unwrap();
    assert_eq!(found.subdirs, [root.join(OsStr::from_bytes(b"invalid-\xff-name"))]);
}

#[test]
fn mount_point_record_requires_standard_fallback() {
    let record = directory_record(b"mounted\0", libc::DIR_MNTSTATUS_MNTPOINT);
    assert!(super::parse_record(&record, Path::new("/tmp"), &mut empty_found()).is_none());
}

fn directory_record(raw_name: &[u8], mount_status: u32) -> Vec<u8> {
    let mut record = Vec::new();
    record.extend_from_slice(&(56 + raw_name.len() as u32).to_ne_bytes());
    for word in [super::COMMON, 0, super::DIRECTORY, 0, 0] {
        record.extend_from_slice(&word.to_ne_bytes());
    }
    record.extend_from_slice(&32i32.to_ne_bytes()); // relative to the name reference at 24
    record.extend_from_slice(&(raw_name.len() as u32).to_ne_bytes());
    record.extend_from_slice(&2u32.to_ne_bytes()); // VDIR
    record.extend_from_slice(&1i64.to_ne_bytes());
    record.extend_from_slice(&0i64.to_ne_bytes());
    record.extend_from_slice(&mount_status.to_ne_bytes());
    record.extend_from_slice(raw_name);
    record
}

fn empty_found() -> crate::sizes::Found {
    crate::sizes::Found {
        bytes: 0,
        entry_count: 0,
        last_modified: None,
        subdirs: Vec::new(),
        broken: false,
    }
}
