// One-pass macOS directory sizing via getattrlistbulk.
// Parses only the records reported by each refill, never stale buffer contents.
// Deps: libc, crate::sizes::Found.
use crate::sizes::Found;
use std::ffi::{CString, OsStr};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const COMMON: u32 = libc::ATTR_CMN_RETURNED_ATTRS
    | libc::ATTR_CMN_NAME
    | libc::ATTR_CMN_OBJTYPE
    | libc::ATTR_CMN_MODTIME;
const FILE: u32 = libc::ATTR_FILE_ALLOCSIZE;
pub(crate) const BUFFER_SIZE: usize = 256 * 1024;

pub(crate) fn read_dir(dir: &Path, buffer_size: usize) -> Option<Found> {
    read_dir_with_refills(dir, buffer_size, |_| {})
}

fn read_dir_with_refills(
    dir: &Path,
    buffer_size: usize,
    mut on_refill: impl FnMut(usize),
) -> Option<Found> {
    if buffer_size < 64 {
        return None;
    }
    let path = CString::new(dir.as_os_str().as_bytes()).ok()?;
    // SAFETY: path is NUL-terminated; a successful open transfers ownership to OwnedFd.
    let raw = unsafe {
        libc::open(
            path.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if raw < 0 {
        return None;
    }
    // SAFETY: raw is a newly opened, valid fd owned solely by this reader.
    let fd = unsafe { OwnedFd::from_raw_fd(raw) };
    let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
    // SAFETY: the fd is live and stat points to writable storage.
    if unsafe { libc::fstat(fd.as_raw_fd(), stat.as_mut_ptr()) } != 0 {
        return None;
    }
    // SAFETY: fstat initialized stat on success.
    let stat = unsafe { stat.assume_init() };
    let mut found = Found {
        bytes: 0,
        entry_count: 0,
        last_modified: Some(timestamp(stat.st_mtime, stat.st_mtime_nsec)?),
        subdirs: Vec::new(),
        broken: false,
    };
    let mut attrs = libc::attrlist {
        bitmapcount: libc::ATTR_BIT_MAP_COUNT,
        reserved: 0,
        commonattr: COMMON,
        volattr: 0,
        dirattr: 0,
        fileattr: FILE,
        forkattr: 0,
    };
    let mut buffer = vec![0u8; buffer_size];
    loop {
        // SAFETY: fd, attrlist, and the writable buffer are valid for this call.
        let count = unsafe {
            libc::getattrlistbulk(
                fd.as_raw_fd(),
                &mut attrs as *mut _ as *mut libc::c_void,
                buffer.as_mut_ptr().cast(),
                buffer.len(),
                u64::from(libc::FSOPT_PACK_INVAL_ATTRS | libc::FSOPT_NOFOLLOW),
            )
        };
        if count < 0 {
            if std::io::Error::last_os_error().raw_os_error() == Some(libc::EINTR) {
                continue;
            }
            return None;
        }
        if count == 0 {
            return Some(found);
        }
        let count = usize::try_from(count).ok()?;
        on_refill(count);
        let mut offset = 0usize;
        for _ in 0..count {
            let length = u32::from_ne_bytes(
                buffer
                    .get(offset..offset.checked_add(4)?)?
                    .try_into()
                    .ok()?,
            ) as usize;
            let end = offset.checked_add(length)?;
            let record = buffer.get(offset..end)?;
            parse_record(record, dir, &mut found)?;
            offset = end;
        }
    }
}

fn parse_record(record: &[u8], dir: &Path, found: &mut Found) -> Option<()> {
    let mut cursor = 0;
    let length = u32::from_ne_bytes(take::<4>(record, &mut cursor)?) as usize;
    if length != record.len() {
        return None;
    }
    let common = u32::from_ne_bytes(take::<4>(record, &mut cursor)?);
    let volume = u32::from_ne_bytes(take::<4>(record, &mut cursor)?);
    let directory = u32::from_ne_bytes(take::<4>(record, &mut cursor)?);
    let file = u32::from_ne_bytes(take::<4>(record, &mut cursor)?);
    let fork = u32::from_ne_bytes(take::<4>(record, &mut cursor)?);
    if common & (libc::ATTR_CMN_NAME | libc::ATTR_CMN_OBJTYPE | libc::ATTR_CMN_MODTIME)
        != (libc::ATTR_CMN_NAME | libc::ATTR_CMN_OBJTYPE | libc::ATTR_CMN_MODTIME)
        || common & !COMMON != 0
        || volume != 0
        || directory != 0
        || file & !FILE != 0
        || fork != 0
    {
        return None;
    }
    let name_ref = cursor;
    let name_offset = i32::from_ne_bytes(take::<4>(record, &mut cursor)?);
    let name_length = u32::from_ne_bytes(take::<4>(record, &mut cursor)?) as usize;
    let kind = u32::from_ne_bytes(take::<4>(record, &mut cursor)?);
    let seconds = i64::from_ne_bytes(take::<8>(record, &mut cursor)?);
    let nanos = i64::from_ne_bytes(take::<8>(record, &mut cursor)?);
    let allocation = if file & FILE != 0 {
        Some(u64::from_ne_bytes(take::<8>(record, &mut cursor)?))
    } else {
        None
    };
    let start = name_ref.checked_add_signed(name_offset as isize)?;
    let end = start.checked_add(name_length)?;
    if start < cursor {
        return None;
    }
    let name = record.get(start..end)?;
    let name = name.strip_suffix(&[0]).unwrap_or(name);
    if name.is_empty() || name.contains(&0) || name.contains(&b'/') || name == b"." || name == b".."
    {
        return None;
    }
    let modified = timestamp(seconds, nanos)?;
    found.entry_count = found.entry_count.checked_add(1)?;
    found.last_modified = found.last_modified.max(Some(modified));
    match kind {
        2 => found.subdirs.push(dir.join(OsStr::from_bytes(name))), // VDIR
        5 => {} // VLNK: count the entry, but not its allocated blocks.
        _ => found.bytes = found.bytes.saturating_add(allocation?),
    }
    Some(())
}

fn take<const N: usize>(record: &[u8], cursor: &mut usize) -> Option<[u8; N]> {
    let end = cursor.checked_add(N)?;
    let bytes = record.get(*cursor..end)?.try_into().ok()?;
    *cursor = end;
    Some(bytes)
}

fn timestamp(seconds: i64, nanos: i64) -> Option<SystemTime> {
    let nanos = u32::try_from(nanos).ok()?;
    if nanos >= 1_000_000_000 {
        return None;
    }
    if seconds >= 0 {
        UNIX_EPOCH.checked_add(Duration::new(seconds as u64, nanos))
    } else {
        UNIX_EPOCH
            .checked_sub(Duration::from_secs(seconds.unsigned_abs()))?
            .checked_add(Duration::from_nanos(u64::from(nanos)))
    }
}

#[cfg(test)]
mod tests {
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
        let mut record = Vec::new();
        record.extend_from_slice(&(52 + raw_name.len() as u32).to_ne_bytes());
        for word in [super::COMMON, 0, 0, 0, 0] {
            record.extend_from_slice(&word.to_ne_bytes());
        }
        record.extend_from_slice(&28i32.to_ne_bytes()); // relative to the name reference at 24
        record.extend_from_slice(&(raw_name.len() as u32).to_ne_bytes());
        record.extend_from_slice(&2u32.to_ne_bytes()); // VDIR
        record.extend_from_slice(&1i64.to_ne_bytes());
        record.extend_from_slice(&0i64.to_ne_bytes());
        record.extend_from_slice(raw_name);
        let root = Path::new("/tmp");
        let mut found = crate::sizes::Found {
            bytes: 0,
            entry_count: 0,
            last_modified: None,
            subdirs: Vec::new(),
            broken: false,
        };
        super::parse_record(&record, root, &mut found).unwrap();
        assert_eq!(
            found.subdirs,
            [root.join(OsStr::from_bytes(b"invalid-\xff-name"))]
        );
    }
}
