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
const DIRECTORY: u32 = libc::ATTR_DIR_MOUNTSTATUS;
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
    let (fd, modified) = open_dir(dir)?;
    let mut found = Found {
        bytes: 0,
        entry_count: 0,
        last_modified: Some(modified),
        subdirs: Vec::new(),
        broken: false,
    };
    let mut attrs = libc::attrlist {
        bitmapcount: libc::ATTR_BIT_MAP_COUNT,
        reserved: 0,
        commonattr: COMMON,
        volattr: 0,
        dirattr: DIRECTORY,
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
        parse_refill(&buffer, count, dir, &mut found)?;
    }
}

fn open_dir(dir: &Path) -> Option<(OwnedFd, SystemTime)> {
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
    Some((fd, timestamp(stat.st_mtime, stat.st_mtime_nsec)?))
}

fn parse_refill(buffer: &[u8], count: usize, dir: &Path, found: &mut Found) -> Option<()> {
    let mut offset = 0usize;
    for _ in 0..count {
        let length = u32::from_ne_bytes(
            buffer.get(offset..offset.checked_add(4)?)?.try_into().ok()?,
        ) as usize;
        let end = offset.checked_add(length)?;
        parse_record(buffer.get(offset..end)?, dir, found)?;
        offset = end;
    }
    Some(())
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
        || directory & !DIRECTORY != 0
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
    // Directory attributes follow common attributes, before file attributes.
    let mount_status = if directory & DIRECTORY != 0 {
        Some(u32::from_ne_bytes(take::<4>(record, &mut cursor)?))
    } else {
        None
    };
    let allocation = if file & FILE != 0 {
        Some(u64::from_ne_bytes(take::<8>(record, &mut cursor)?))
    } else {
        None
    };
    let name = record_name(record, cursor, name_ref, name_offset, name_length)?;
    safe_mount_status(kind, mount_status)?;
    let modified = timestamp(seconds, nanos)?;
    found.entry_count = found.entry_count.checked_add(1)?;
    found.last_modified = found.last_modified.max(Some(modified));
    match kind {
        2 => found.subdirs.push(dir.join(name)), // VDIR
        5 => {} // VLNK: count the entry, but not its allocated blocks.
        _ => found.bytes = found.bytes.saturating_add(allocation?),
    }
    Some(())
}

fn record_name(
    record: &[u8], cursor: usize, reference: usize, offset: i32, length: usize,
) -> Option<&OsStr> {
    let start = reference.checked_add_signed(offset as isize)?;
    let end = start.checked_add(length)?;
    if start < cursor { return None; }
    let name = record.get(start..end)?;
    let name = name.strip_suffix(&[0]).unwrap_or(name);
    if name.is_empty() || name.contains(&0) || name.contains(&b'/') || name == b"." || name == b".." {
        return None;
    }
    Some(OsStr::from_bytes(name))
}

fn safe_mount_status(kind: u32, status: Option<u32>) -> Option<()> {
    if kind == 2 {
        if status? & libc::DIR_MNTSTATUS_MNTPOINT != 0 { return None; }
    } else if status.is_some() {
        return None;
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
#[path = "bulk_tests.rs"]
mod tests;
