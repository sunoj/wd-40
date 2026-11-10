// The sizing pool. Each target gets its own stack of directories and targets
// are opened one at a time, only as fast as the pool runs out of work: that
// keeps a walk depth-first inside a target, so sizes settle one after another
// rather than all at the end, while still opening as many targets as it takes
// to keep every worker busy.
// Exports: `Walk` (crate-internal). Deps: crate::{disk, nesting, sizes}.

use crate::disk::disk_space;
use crate::nesting::Publisher;
use crate::sizes::{dir_measurement_fallback, read_dir, Found, Measurement, SizedTarget};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Condvar, Mutex};
use std::time::SystemTime;

/// Queue plus per-target tallies, under one lock. A directory read costs orders
/// of magnitude more than the lock it takes to hand back what it found.
struct Queue {
    stacks: Vec<Vec<PathBuf>>,
    /// Targets being walked, oldest first: the oldest is served first so it
    /// settles before the ones opened after it.
    open: VecDeque<usize>,
    /// Targets not started yet.
    unopened: VecDeque<usize>,
    /// Workers holding a job right now — the queue being empty only means the
    /// walk is over once this is zero too.
    busy: usize,
    /// Workers asleep on the condvar right now.
    waiting: usize,
    /// Directories queued or in flight for each target.
    outstanding: Vec<usize>,
    bytes: Vec<u64>,
    last_modified: Vec<Option<SystemTime>>,
    newest_file: Vec<Option<SystemTime>>,
    /// Targets aged by their newest regular file rather than any entry.
    file_aged: Vec<bool>,
    /// A bulk read failed somewhere under this target, so its whole subtree is
    /// re-walked the slow way rather than reported short.
    broken: Vec<bool>,
}

impl Queue {
    /// The oldest open target with a directory left to read.
    fn pop_job(&mut self) -> Option<(usize, PathBuf)> {
        let target = *self
            .open
            .iter()
            .find(|target| !self.stacks[**target].is_empty())?;
        self.stacks[target].pop().map(|dir| (target, dir))
    }

    /// Start one more target because the pool has nothing else to do.
    fn open_next(&mut self, roots: &[PathBuf]) -> bool {
        let Some(target) = self.unopened.pop_front() else { return false };
        self.stacks[target].push(roots[target].clone());
        self.outstanding[target] = 1;
        self.open.push_back(target);
        true
    }
}

pub(crate) struct Walk {
    queue: Mutex<Queue>,
    wake: Condvar,
}

impl Walk {
    /// `skip` names targets whose size is already known, so they are never
    /// queued. Their figures reach the publisher before the walk starts, which
    /// is what keeps nesting right without walking them. `file_aged` names
    /// targets whose age is their newest regular file.
    pub fn new(paths: &[PathBuf], skip: &[bool], file_aged: &[bool]) -> Self {
        // Shallowest first. Nesting is handled by the publisher whatever the
        // order, so this is purely about the tail: the big roots (cache stores,
        // shared cargo targets) sit near the top of the tree, and opening one of
        // those last leaves every worker waiting on it.
        let mut order: Vec<usize> = (0..paths.len())
            .filter(|index| !skip.get(*index).copied().unwrap_or(false))
            .collect();
        order.sort_by_key(|index| paths[*index].components().count());
        Self {
            queue: Mutex::new(Queue {
                stacks: vec![Vec::new(); paths.len()],
                open: VecDeque::new(),
                unopened: order.into(),
                busy: 0,
                waiting: 0,
                outstanding: vec![0; paths.len()],
                bytes: vec![0; paths.len()],
                last_modified: vec![None; paths.len()],
                newest_file: vec![None; paths.len()],
                file_aged: (0..paths.len()).map(|i| file_aged.get(i).copied().unwrap_or(false)).collect(),
                broken: vec![false; paths.len()],
            }),
            wake: Condvar::new(),
        }
    }

    pub fn run(
        &self,
        paths: &[PathBuf],
        publisher: &Mutex<Publisher>,
        modified: &Mutex<Vec<Option<SystemTime>>>,
        on_size: &impl Fn(SizedTarget),
    ) {
        while let Some((target, dir)) = self.take(paths) {
            for index in self.commit(target, read_dir(&dir)) {
                let measurement = self.settle(&paths[index], index);
                modified.lock().unwrap_or_else(|error| error.into_inner())[index] =
                    measurement.last_modified;
                let settled = publisher
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .record(index, measurement.bytes);
                for (index, bytes) in settled {
                    let last_modified = modified
                        .lock()
                        .unwrap_or_else(|error| error.into_inner())[index];
                    on_size(SizedTarget { index, bytes, last_modified });
                }
            }
        }
    }

    fn take(&self, roots: &[PathBuf]) -> Option<(usize, PathBuf)> {
        let mut queue = self.queue.lock().unwrap();
        loop {
            if let Some(job) = queue.pop_job() {
                queue.busy += 1;
                return Some(job);
            }
            // Nothing left in the targets already being walked — open another
            // rather than idle, and only then.
            if queue.open_next(roots) {
                continue;
            }
            if queue.busy == 0 {
                self.wake.notify_all();
                return None;
            }
            queue.waiting += 1;
            queue = self.wake.wait(queue).unwrap();
            queue.waiting -= 1;
        }
    }

    /// Fold one directory's findings in and report any target that just ran out
    /// of outstanding directories.
    fn commit(&self, target: usize, found: Found) -> Vec<usize> {
        let mut queue = self.queue.lock().unwrap();
        queue.busy -= 1;
        queue.bytes[target] = queue.bytes[target].saturating_add(found.bytes);
        queue.last_modified[target] = queue.last_modified[target].max(found.last_modified);
        queue.newest_file[target] = queue.newest_file[target].max(found.newest_file);
        queue.broken[target] |= found.broken;
        queue.outstanding[target] += found.subdirs.len();
        queue.outstanding[target] -= 1;
        // Wake one sleeper per directory actually handed over. Waking everybody
        // on every directory is the difference between a pool and a queue of
        // threads fighting for one lock.
        let wake = found.subdirs.len().min(queue.waiting);
        queue.stacks[target].extend(found.subdirs);
        if queue.busy == 0 {
            self.wake.notify_all();
        } else {
            for _ in 0..wake {
                self.wake.notify_one();
            }
        }
        if queue.outstanding[target] > 0 {
            return Vec::new();
        }
        queue.open.retain(|open| *open != target);
        vec![target]
    }

    /// The target's raw size, held to what the volume could possibly hold. A
    /// bulk read that failed or overshot is redone with the slow walk.
    fn settle(&self, root: &Path, index: usize) -> Measurement {
        let (bytes, last_modified, broken) = {
            let queue = self.queue.lock().unwrap();
            let modified = match queue.file_aged[index] {
                true => queue.newest_file[index],
                false => queue.last_modified[index],
            };
            (queue.bytes[index], modified, queue.broken[index])
        };
        let limit = disk_space(root).map(|stats| stats.total_bytes);
        if !broken && limit.is_none_or(|max| bytes <= max) {
            return Measurement { bytes, last_modified };
        }
        let mut fallback = dir_measurement_fallback(root);
        fallback.bytes = limit.map_or(fallback.bytes, |max| fallback.bytes.min(max));
        fallback
    }
}
