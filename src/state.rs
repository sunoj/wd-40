// Shared application state for the WD-40 menu bar popover.
// Exports: `AppState`, `UiScreen`, clean/done summaries, selection accessors.
// Deps: objc2_app_kit, crate::{selection, updater}, wd40 lib.

use crate::selection::{default_selection, empty_measured, selected_bytes, settled_order};
use crate::updater::Updater;
use objc2::rc::Retained;
use objc2_app_kit::NSStatusItem;
use std::cell::RefCell;
use std::collections::HashSet;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};
use wd40::config::Config;
use wd40::disk::{disk_space, sum_bytes, DiskSpace};
use wd40::reclaim::Reclaim;
use wd40::scanner::{ArtifactGroup, TargetDir};

thread_local! {
    static APP_STATE: RefCell<Option<AppState>> = const { RefCell::new(None) };
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum UiScreen {
    Scan,
    Cleaning,
    Done,
    Settings,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CleanItemStatus {
    Pending,
    Active,
    Done,
    /// Removal stopped part way: some of it went, the rest is still on disk.
    Partial,
    /// Nothing was removed.
    Failed,
    /// Never started, because the run was stopped first.
    Skipped,
}

impl CleanItemStatus {
    /// True once this item will not change again.
    pub fn settled(self) -> bool {
        !matches!(self, Self::Pending | Self::Active)
    }
}

#[derive(Clone)]
pub struct CleanItem {
    /// Position in `AppState::targets` of the target this item removes.
    pub index: usize,
    pub name: String,
    pub size_bytes: u64,
    /// What this item actually returned — below `size_bytes` when it went part way.
    pub freed_bytes: u64,
    pub status: CleanItemStatus,
}

#[derive(Clone, Default)]
pub struct CleanProgress {
    pub items: Vec<CleanItem>,
    pub freed_so_far: u64,
    /// The target most recently picked up; several run at once.
    pub current_path: String,
    pub done_count: usize,
    pub total_count: usize,
}

impl CleanProgress {
    pub fn active_count(&self) -> usize {
        self.items.iter().filter(|item| item.status == CleanItemStatus::Active).count()
    }

    /// True while any target is still queued or being removed. The cleaning
    /// plate sprays itself for exactly this long.
    pub fn working(&self) -> bool {
        self.items.iter().any(|item| !item.status.settled())
    }

    /// Items that were touched but did not come off cleanly.
    pub fn troubled_count(&self) -> usize {
        self.items
            .iter()
            .filter(|item| matches!(item.status, CleanItemStatus::Partial | CleanItemStatus::Failed))
            .count()
    }
}

#[derive(Clone)]
pub struct GroupSummary {
    pub group: ArtifactGroup,
    pub count: usize,
    pub bytes: u64,
}

#[derive(Clone)]
pub struct DoneSummary {
    pub freed_bytes: u64,
    pub duration: Duration,
    pub free_before: u64,
    pub free_after: u64,
    pub total_bytes: u64,
    pub removed: Vec<GroupSummary>,
    pub skipped_count: usize,
    pub skipped_bytes: u64,
    /// Targets that were started but did not come off cleanly.
    pub troubled_count: usize,
}

pub(crate) struct AppState {
    pub config: Config,
    pub targets: Vec<TargetDir>,
    /// Positions in `targets` whose size has settled. Anything not in here has
    /// no figure yet and must never be drawn as though it had one.
    pub measured: HashSet<usize>,
    pub selected: HashSet<usize>,
    /// What the user has unfolded: group keys, and `scan_calm::LIST`.
    pub open: HashSet<&'static str>,
    pub screen: UiScreen,
    pub cleaning: Option<CleanProgress>,
    pub done: Option<DoneSummary>,
    /// How the last clean ended, item by item: what the done screen's map shows.
    pub last_run: Option<CleanProgress>,
    pub status_item: Retained<NSStatusItem>,
    pub updater: Option<Updater>,
    /// Device-byte accounting for the targets that can share blocks, once it
    /// has been worked out. `None` until then, and the totals fall back to
    /// summed sizes — which overstate, so the promise is only ever made once
    /// it can be kept.
    pub reclaim: Option<Reclaim>,
}

fn apply_size_to_target(target: &mut TargetDir, bytes: u64, last_modified: Option<SystemTime>) {
    target.size_bytes = bytes;
    if let Some(last_modified) = last_modified {
        target.last_modified = last_modified;
    }
}

impl AppState {
    /// True while sizes are still arriving, so every total on screen is a floor
    /// rather than an answer.
    pub fn sizing(&self) -> bool {
        !self.targets.is_empty() && self.measured.len() < self.targets.len()
    }

    /// What everything on the list is worth. Summed allocated sizes until the
    /// device accounting lands, and the device figure after — on a disk seeded
    /// by `cp -Rc` those differ fourfold, and only the second is what deleting
    /// returns. See docs/apfs-clone-overcount.md.
    pub fn total_size(&self) -> u64 {
        match self.exact() {
            Some(exact) => exact.bytes(&self.targets, &|_| true),
            None => sum_bytes(self.targets.iter().map(|t| t.size_bytes)),
        }
    }

    /// What the ticked rows are worth. The same accounting, asked about a
    /// subset: removing one clone of a pair frees nothing its twin still holds.
    pub fn selected_size(&self) -> u64 {
        match self.exact() {
            Some(exact) => exact.bytes(&self.targets, &|index| self.selected.contains(&index)),
            None => selected_bytes(&self.targets, &self.selected),
        }
    }

    /// The accounting, if it still describes the targets on screen. A rescan
    /// that moved them leaves it stale, and a stale promise is worse than a
    /// conservative one.
    fn exact(&self) -> Option<&Reclaim> {
        self.reclaim.as_ref().filter(|found| found.covers(&self.targets))
    }

    pub fn group_size(&self, group: ArtifactGroup) -> u64 {
        sum_bytes(
            self.targets
                .iter()
                .filter(|t| t.kind.group() == group)
                .map(|t| t.size_bytes),
        )
    }

    /// Targets the list must not draw: measured, and holding nothing. Until a
    /// target's own figure lands it stays in, so nothing pops into the list one
    /// row at a time while the pass runs.
    pub fn empty_targets(&self) -> HashSet<usize> {
        empty_measured(&self.targets, &self.measured)
    }

    /// True when every target in this group has a settled size.
    pub fn group_settled(&self, group: ArtifactGroup) -> bool {
        self.targets
            .iter()
            .enumerate()
            .filter(|(_, t)| t.kind.group() == group)
            .all(|(index, _)| self.measured.contains(&index))
    }

    /// Record one settled size. Row order is deliberately left alone until the
    /// pass is over: re-sorting on every arrival would shuffle the list under
    /// the pointer while the user is reading it.
    pub fn apply_size(&mut self, index: usize, bytes: u64, last_modified: Option<SystemTime>) {
        if let Some(target) = self.targets.get_mut(index) {
            apply_size_to_target(target, bytes, last_modified);
            self.measured.insert(index);
        }
    }

    /// Sort largest first now that every figure is in, carrying the ticks with
    /// their targets — a selection is a promise about paths, not positions.
    ///
    /// The targets that measured empty are dropped here, and for good: they
    /// were rows that could free nothing. Their bytes were zero, so no total on
    /// screen moves; the group counts drop to what the list actually shows. A
    /// tick on one of them goes with it, which is the only way the button can
    /// keep counting exactly the rows the user can see.
    pub fn finish_sizing(&mut self) {
        let order = settled_order(&self.targets, &self.measured);
        let selected: HashSet<usize> = order
            .iter()
            .enumerate()
            .filter(|(_, old)| self.selected.contains(old))
            .map(|(new, _)| new)
            .collect();
        self.targets = order.iter().map(|index| self.targets[*index].clone()).collect();
        self.measured = (0..self.targets.len()).collect();
        self.selected = selected;
    }

    pub fn reference_path(&self) -> Option<PathBuf> {
        self.config
            .scan_dirs
            .iter()
            .find(|path| path.exists())
            .or_else(|| self.targets.first().map(|target| &target.path))
            .cloned()
    }

    pub fn disk_stats(&self) -> Option<DiskSpace> {
        self.reference_path().as_deref().and_then(disk_space)
    }

    pub fn reset_selection(&mut self) {
        self.selected = default_selection(&self.targets, self.config.max_age_days);
        self.open.clear();
    }

    pub fn toggle_selected(&mut self, index: usize) {
        if !self.selected.insert(index) {
            self.selected.remove(&index);
        }
    }
}

pub(crate) fn install(state: AppState) {
    APP_STATE.with(|cell| *cell.borrow_mut() = Some(state));
}

pub(crate) fn with_state<F: FnOnce(&mut AppState)>(f: F) {
    APP_STATE.with(|cell| {
        if let Some(state) = cell.borrow_mut().as_mut() {
            f(state);
        }
    });
}

pub(crate) fn with_state_ret<F: FnOnce(&mut AppState) -> R, R>(f: F) -> Option<R> {
    APP_STATE.with(|cell| cell.borrow_mut().as_mut().map(f))
}

#[cfg(test)]
mod tests {
    use super::apply_size_to_target;
    use std::path::PathBuf;
    use std::time::{Duration, SystemTime};
    use wd40::scanner::{ArtifactKind, TargetDir};

    #[test]
    fn applying_a_settled_size_keeps_the_descendant_timestamp() {
        let original = SystemTime::UNIX_EPOCH + Duration::from_secs(3);
        let descendant = SystemTime::UNIX_EPOCH + Duration::from_secs(9);
        let mut target = TargetDir {
            path: PathBuf::from("target"),
            size_bytes: 0,
            last_modified: original,
            kind: ArtifactKind::RustTarget,
        };
        apply_size_to_target(&mut target, 11, Some(descendant));
        assert_eq!(target.size_bytes, 11);
        assert_eq!(target.last_modified, descendant);
    }
}
