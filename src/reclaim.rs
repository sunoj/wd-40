// What a selection is actually worth, in device bytes rather than in summed
// allocated sizes. Every target goes through the extent map together: sharing
// is a relation between two directories, and one read in isolation cannot tell
// whether the blocks it holds are its own.
// Exports: `Reclaim`.
// Deps: std, crate::{extents, scanner}.

use crate::extents::Attribution;
use crate::scanner::TargetDir;

/// Device-byte accounting across the whole target list.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct Reclaim {
    /// The directory behind each owner in `attribution`, in owner order.
    ///
    /// By path, never by index. The pass takes minutes on a busy machine and
    /// the target list moves underneath it — a scan that dropped two targets
    /// used to invalidate the whole accounting, so the figure never once
    /// reached the screen and the headline stayed a sum.
    owners: Vec<std::path::PathBuf>,
    attribution: Attribution,
}

impl Reclaim {
    /// Read the extent maps of every target.
    ///
    /// Every target, not the ones a path pattern calls clone-prone: `aid`
    /// clones *from* a project's own target directory *into* a session one, so
    /// one end of the sharing is an ordinary `target/` and the other is under
    /// `.cargo-target` or in /tmp. Leaving either end out does not merely miss
    /// the saving — it makes the number wrong in the dangerous direction, by
    /// crediting a selection with blocks a copy outside the set still holds.
    ///
    /// Measured on this Mac: reading only `.cargo-target` reported 61.78 G
    /// against a true 33.52 G, because the largest clone tree of all was a
    /// 40 G directory in /tmp.
    pub fn measure(targets: &[TargetDir]) -> Option<Self> {
        if targets.is_empty() {
            return None;
        }
        let owners: Vec<std::path::PathBuf> =
            targets.iter().map(|target| target.path.clone()).collect();
        let fingerprint = fingerprint(targets);
        // Seventy seconds of syscalls is worth going a long way to avoid, and
        // the fingerprint says whether the answer would even differ.
        if let Some(kept) = crate::cache::attribution_for(&fingerprint) {
            return Some(Self { owners, attribution: kept });
        }
        // Per target: an unchanged one is answered from level one, and only
        // what a build actually touched is read again. Fresh reads are
        // independent, so they share the sizing pool's cap and background QoS.
        let per_target = read_targets(targets);
        let attribution = crate::extents::combine(&per_target);
        crate::cache::put_attribution(&fingerprint, attribution.clone());
        Some(Self { owners, attribution })
    }

    /// What the accounting depends on: which directories, and how big each is.
    pub fn fingerprint(targets: &[TargetDir]) -> Vec<(std::path::PathBuf, u64, u64)> {
        fingerprint(targets)
    }

    /// Bytes removing `selected` would return: the shared set measured against
    /// the device, plus the summed sizes of everything that shares nothing.
    ///
    /// A target the accounting has never heard of contributes its own figure,
    /// so a selection made after a rescan is under-promised rather than
    /// over-promised while the next accounting is still running.
    pub fn bytes(&self, targets: &[TargetDir], selected: &impl Fn(usize) -> bool) -> u64 {
        let mut going = vec![false; self.owners.len()];
        let mut unknown: u64 = 0;
        for (index, target) in targets.iter().enumerate() {
            if !selected(index) {
                continue;
            }
            match self.owners.iter().position(|owner| *owner == target.path) {
                Some(owner) => going[owner] = true,
                // A target found since the accounting was made. Its own figure
                // is the best that can be said about it until the next pass.
                None => unknown = unknown.saturating_add(target.size_bytes),
            }
        }
        self.attribution.union_of(&going).saturating_add(unknown)
    }

    /// What the whole list occupies on the device.
    pub fn total_of(&self, targets: &[TargetDir]) -> u64 {
        self.bytes(targets, &|_| true)
    }

    /// True while this accounting still says something about what is on screen.
    /// Being keyed by path, it survives a rescan that added or dropped targets
    /// — only one that replaced every last one of them is worthless.
    pub fn covers(&self, targets: &[TargetDir]) -> bool {
        targets
            .iter()
            .any(|target| self.owners.iter().any(|owner| *owner == target.path))
    }
}

fn read_targets(targets: &[TargetDir]) -> Vec<crate::extents::TargetExtents> {
    let next = std::sync::Mutex::new(0_usize);
    let finished = std::sync::Mutex::new(Vec::with_capacity(targets.len()));
    let worker_count = crate::qos::workers().min(targets.len());

    std::thread::scope(|scope| {
        for _ in 0..worker_count {
            scope.spawn(|| {
                crate::qos::background();
                loop {
                    let index = {
                        let mut next = next.lock().unwrap_or_else(|error| error.into_inner());
                        if *next == targets.len() {
                            None
                        } else {
                            let index = *next;
                            *next += 1;
                            Some(index)
                        }
                    };
                    let Some(index) = index else { break };
                    let target = &targets[index];
                    let read = match crate::extent_cache::extents_of(
                        &target.path,
                        target.size_bytes,
                        target.last_modified,
                    ) {
                        Some(kept) => kept,
                        None => {
                            let read = crate::extents::read_target(&target.path);
                            crate::extent_cache::put_extents(
                                &target.path,
                                target.size_bytes,
                                target.last_modified,
                                read.clone(),
                            );
                            read
                        }
                    };
                    finished
                        .lock()
                        .unwrap_or_else(|error| error.into_inner())
                        .push((index, read));
                }
            });
        }
    });

    let mut finished = finished.into_inner().unwrap_or_default();
    finished.sort_unstable_by_key(|(index, _)| *index);
    finished.into_iter().map(|(_, target)| target).collect()
}

#[cfg(test)]
mod tests {
    use super::Reclaim;
    use crate::scanner::{ArtifactKind, TargetDir};
    use std::collections::HashSet;
    use std::path::PathBuf;
    use std::sync::Mutex;
    use std::time::SystemTime;

    fn target(path: PathBuf, size_bytes: u64) -> TargetDir {
        TargetDir { path, size_bytes, last_modified: SystemTime::UNIX_EPOCH, kind: ArtifactKind::RustTarget }
    }

    fn scratch(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("wd40-reclaim-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        path
    }

    /// The bug this replaced: the accounting was keyed by index and thrown
    /// away whenever a scan changed the target count. On a machine where a
    /// build finishes every few minutes that was always, so the exact figure
    /// never once reached the screen. Keyed by path, what it knows survives.
    #[test]
    fn a_rescan_that_moved_the_targets_does_not_throw_the_accounting_away() {
        let root = scratch("moved");
        let kept = root.join("kept");
        let _ = std::fs::create_dir_all(&kept);
        let _ = std::fs::write(kept.join("payload.bin"), vec![b'x'; 1 << 20]);

        let before = vec![target(kept.clone(), 1 << 20)];
        let found = Reclaim::measure(&before).expect("one target");

        // A later scan dropped one target and found two new ones; `kept` moved
        // to the end of the list.
        let after = vec![
            target(root.join("fresh-a"), 4096),
            target(root.join("fresh-b"), 8192),
            target(kept, 1 << 20),
        ];
        assert!(found.covers(&after), "it still describes one of these rows");

        // The two it never saw contribute their own figures; the one it did is
        // answered from the device.
        let all = found.bytes(&after, &|_| true);
        assert!(all >= 4096 + 8192, "the new targets still count: {all}");
        let new_only = found.bytes(&after, &|index| index < 2);
        assert_eq!(new_only, 4096 + 8192, "targets it never saw count in full");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn an_empty_list_needs_no_accounting() {
        assert!(Reclaim::measure(&[]).is_none());
    }

    /// The red line, and the bug this replaced: a clone in /tmp is as much a
    /// clone as one under `.cargo-target`. Nothing about where a directory sits
    /// may decide whether its blocks are counted.
    #[test]
    fn sharing_is_found_wherever_the_directories_sit() {
        let root = scratch("anywhere");
        let source = root.join("Develop/proj/target");
        let session = root.join("tmp/cc-target-session");
        let _ = std::fs::create_dir_all(&source);
        let _ = std::fs::create_dir_all(&session);
        let payload = source.join("payload.bin");
        let _ = std::fs::write(&payload, vec![b'x'; 4 << 20]);
        let cloned = std::process::Command::new("cp")
            .arg("-c").arg(&payload).arg(session.join("payload.bin")).status();
        if !cloned.is_ok_and(|status| status.success()) {
            eprintln!("skipped: cp -c unavailable");
            return;
        }

        let targets = vec![target(source, 4 << 20), target(session, 4 << 20)];
        let found = Reclaim::measure(&targets).expect("two targets");

        let four_mb = (4 << 20) as u64;
        assert!(found.total_of(&targets) < four_mb * 2, "the clone is not a second copy");

        // Neither one alone frees anything the other still holds.
        let only_session: HashSet<usize> = [1].into_iter().collect();
        assert_eq!(found.bytes(&targets, &|index| only_session.contains(&index)), 0);
        // Both together free what they share.
        assert!(found.bytes(&targets, &|_| true) >= four_mb);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_same_size_rewrite_invalidates_physical_cache_gates() {
        let root = scratch("same-size-rewrite");
        let source = root.join("source");
        let rewritten = root.join("rewritten");
        let _ = std::fs::create_dir_all(&source);
        let _ = std::fs::create_dir_all(&rewritten);
        let payload = source.join("payload.bin");
        let _ = std::fs::write(&payload, vec![b'x'; 4 << 20]);
        let cloned = std::process::Command::new("cp")
            .arg("-c").arg(&payload).arg(rewritten.join("payload.bin")).status();
        if !cloned.is_ok_and(|status| status.success()) {
            eprintln!("skipped: cp -c unavailable");
            return;
        }

        let measure = |path: PathBuf| {
            let target = target(path, 0);
            let result = Mutex::new(None);
            crate::sizes::size_targets(std::slice::from_ref(&target), crate::qos::Priority::Waited, |sized| {
                *result.lock().unwrap() = Some((sized.bytes, sized.last_modified));
            });
            let (size_bytes, last_modified) = result.into_inner().unwrap().expect("one target");
            TargetDir { path: target.path, size_bytes, last_modified: last_modified.unwrap_or(SystemTime::UNIX_EPOCH), kind: target.kind }
        };
        crate::cache::forget(&source);
        crate::cache::forget(&rewritten);
        let before = vec![measure(source.clone()), measure(rewritten.clone())];
        let cached = Reclaim::measure(&before).expect("two targets");
        let four_mb = (4 << 20) as u64;
        assert!(cached.total_of(&before) < four_mb * 2, "baseline is not a clone");

        std::thread::sleep(std::time::Duration::from_millis(1100));
        let _ = std::fs::write(rewritten.join("payload.bin"), vec![b'y'; 4 << 20]);
        let after = vec![measure(source), measure(rewritten)];
        assert_eq!(after[0].size_bytes, before[0].size_bytes);
        assert_eq!(after[1].size_bytes, before[1].size_bytes);
        assert!(after[1].last_modified > before[1].last_modified);

        let fresh = Reclaim::measure(&after).expect("two targets");
        assert!(fresh.total_of(&after) >= four_mb * 2, "rewrite stayed physically shared");
        let _ = std::fs::remove_dir_all(&root);
    }
}

fn fingerprint(targets: &[TargetDir]) -> Vec<(std::path::PathBuf, u64, u64)> {
    targets
        .iter()
        .map(|target| {
            let content_modified = target
                .last_modified
                .duration_since(std::time::UNIX_EPOCH)
                .ok()
                .map_or(0, |since| since.as_nanos().min(u64::MAX as u128) as u64);
            (target.path.clone(), target.size_bytes, content_modified)
        })
        .collect()
}
