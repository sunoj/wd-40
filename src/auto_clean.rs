// Unattended clean policy and runner, independent of manual row selection.
// Exports: timer entry point, receipt text, pending-snapshot application.
// Deps: crate::{names, popover, state, tasks}, wd40 scan/clean APIs.

use crate::names::display_names;
use crate::state::{with_state, with_state_ret, UiScreen};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, SystemTime};
use wd40::cleaner::Removal;
use wd40::config::Config;
use wd40::disk::{disk_space, DiskSpace};
use wd40::scanner::{human_size, ArtifactGroup, ArtifactKind, TargetDir};

const SECONDS_PER_DAY: u64 = 86_400;

static RECEIPT: Mutex<Option<Receipt>> = Mutex::new(None);
static SNAPSHOT: Mutex<Option<Vec<TargetDir>>> = Mutex::new(None);
static RUNNING: AtomicBool = AtomicBool::new(false);

#[derive(Clone)]
struct Receipt {
    groups: Vec<(ArtifactGroup, usize)>,
    names: Vec<String>,
    freed_bytes: u64,
}

struct AutoCleanJob {
    items: Vec<(TargetDir, String)>,
    reference: Option<PathBuf>,
}

struct CleanOutcome {
    receipt: Receipt,
    remaining: Vec<(PathBuf, u64)>,
}

pub struct PopoverOpening(());

pub fn start() {
    let cleaning_screen = with_state_ret(|state| state.screen == UiScreen::Cleaning)
        .unwrap_or(false);
    if !can_start(crate::tasks::is_busy(), cleaning_screen, crate::popover::is_open()) {
        return;
    }
    let Some(config) = with_state_ret(|state| state.config.clone()) else { return };
    if config.max_age_days == 0 {
        return;
    }
    if !crate::tasks::claim_cleaning() {
        return;
    }
    crate::tasks::reset_clean_stop();
    clear_receipt();
    RUNNING.store(true, Ordering::Relaxed);
    std::thread::spawn(move || {
        let result = std::panic::catch_unwind(|| run(config, crate::tasks::clean_stop_flag()));
        if result.is_err() {
            eprintln!("wd-40: automatic clean stopped unexpectedly");
        }
        RUNNING.store(false, Ordering::Relaxed);
        crate::tasks::finish_cleaning();
    });
}

/// Opening the popover while an unattended pass is active stops that pass
/// before it can start another permanent removal.
pub fn stop_for_popover() {
    if RUNNING.load(Ordering::Relaxed) {
        crate::tasks::request_stop();
    }
}

pub fn prepare_popover_opening() -> PopoverOpening {
    stop_for_popover();
    PopoverOpening(())
}

pub fn apply_pending_snapshot() {
    let snapshot = SNAPSHOT.lock().unwrap_or_else(|error| error.into_inner()).take();
    let Some(targets) = snapshot else { return };
    with_state(|state| {
        state.targets = targets;
        state.measured = (0..state.targets.len()).collect();
        state.reclaim = None;
        state.reset_selection();
    });
}

pub fn discard_pending_snapshot() {
    SNAPSHOT.lock().unwrap_or_else(|error| error.into_inner()).take();
}

pub fn clear_receipt() {
    RECEIPT.lock().unwrap_or_else(|error| error.into_inner()).take();
}

pub fn receipt_line() -> Option<String> {
    let receipt = RECEIPT.lock().unwrap_or_else(|error| error.into_inner());
    format_receipt(receipt.as_ref())
}

fn run(config: Config, stop: &AtomicBool) {
    let mut targets = wd40::discover::scan_discover(&config);
    if stop.load(Ordering::Relaxed) {
        return;
    }
    let measured_ages = wd40::sizes::scan_sizes(&mut targets, wd40::qos::Priority::Housekeeping);
    if stop.load(Ordering::Relaxed) {
        return;
    }
    let job = gather_job(&targets, &measured_ages, &config, SystemTime::now());
    if let Some(job) = job {
        let outcome = clean(job, &config, stop);
        let receipt = apply_outcome(&mut targets, outcome);
        if !receipt.groups.is_empty() {
            *RECEIPT.lock().unwrap_or_else(|error| error.into_inner()) = Some(receipt);
        }
        targets.retain(|target| target.path.is_dir());
        wd40::cache::flush();
    }
    *SNAPSHOT.lock().unwrap_or_else(|error| error.into_inner()) = Some(targets);
}

fn gather_job(
    targets: &[TargetDir],
    measured_ages: &HashMap<PathBuf, SystemTime>,
    config: &Config,
    now: SystemTime,
) -> Option<AutoCleanJob> {
    let indices = candidate_indices(targets, measured_ages, config, now);
    if indices.is_empty() {
        return None;
    }
    let names = display_names(targets);
    let items = indices
        .into_iter()
        .map(|index| (targets[index].clone(), names[index].clone()))
        .collect();
    let reference = config
        .scan_dirs
        .iter()
        .find(|path| path.exists())
        .or_else(|| targets.first().map(|target| &target.path))
        .cloned();
    Some(AutoCleanJob { items, reference })
}

fn clean(job: AutoCleanJob, config: &Config, stop: &AtomicBool) -> CleanOutcome {
    let before = job.reference.as_deref().and_then(disk_space);
    let mut receipt = Receipt { groups: Vec::new(), names: Vec::new(), freed_bytes: 0 };
    let mut remaining = Vec::new();
    for (target, name) in job.items {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        let measured = wd40::sizes::measure_dir_with_modified(&target.path);
        if !is_candidate(&target, config, SystemTime::now(), measured.last_modified) {
            continue;
        }
        let mut measured_target = target;
        measured_target.size_bytes = measured.bytes;
        let removal = wd40::cleaner::remove_target(&measured_target);
        if let Some(left_bytes) = remaining_bytes(&removal, measured.bytes) {
            remaining.push((measured_target.path.clone(), left_bytes));
        }
        record_removal(&mut receipt, &measured_target, name, removal);
    }
    receipt.freed_bytes = freed_bytes(before, job.reference.as_deref().and_then(disk_space), receipt.freed_bytes);
    CleanOutcome { receipt, remaining }
}

fn can_start(busy: bool, cleaning_screen: bool, popover_open: bool) -> bool {
    !busy && !cleaning_screen && !popover_open
}

fn candidate_indices(
    targets: &[TargetDir],
    measured_ages: &HashMap<PathBuf, SystemTime>,
    config: &Config,
    now: SystemTime,
) -> Vec<usize> {
    if config.max_age_days == 0 {
        return Vec::new();
    }
    targets
        .iter()
        .enumerate()
        .filter(|(_, target)| {
            is_candidate(target, config, now, measured_ages.get(&target.path).copied())
        })
        .map(|(index, _)| index)
        .collect()
}

fn is_candidate(
    target: &TargetDir,
    config: &Config,
    now: SystemTime,
    last_modified: Option<SystemTime>,
) -> bool {
    if config.max_age_days == 0
        || matches!(target.kind, ArtifactKind::Toolchain | ArtifactKind::Simulator)
        || !config.scans(target.kind.group())
    {
        return false;
    }
    let Some(last_modified) = last_modified else { return false };
    let threshold = Duration::from_secs(config.max_age_days.saturating_mul(SECONDS_PER_DAY));
    now.duration_since(last_modified).is_ok_and(|age| age >= threshold)
}

fn remaining_bytes(removal: &Removal, measured: u64) -> Option<u64> {
    match removal {
        Removal::Gone => None,
        Removal::Partial { left_bytes, .. } => Some(*left_bytes),
        Removal::Refused(_) => Some(measured),
    }
}

fn apply_outcome(targets: &mut [TargetDir], outcome: CleanOutcome) -> Receipt {
    for (path, left_bytes) in outcome.remaining {
        if let Some(target) = targets.iter_mut().find(|target| target.path == path) {
            target.size_bytes = left_bytes;
        }
    }
    outcome.receipt
}

fn record_removal(receipt: &mut Receipt, target: &TargetDir, name: String, removal: Removal) {
    let freed = removal.freed_bytes(target.size_bytes);
    if freed == 0 {
        return;
    }
    if let Some((_, count)) = receipt.groups.iter_mut().find(|(group, _)| *group == target.kind.group()) {
        *count += 1;
    } else {
        receipt.groups.push((target.kind.group(), 1));
    }
    receipt.names.push(name);
    receipt.freed_bytes = receipt.freed_bytes.saturating_add(freed);
}

fn freed_bytes(before: Option<DiskSpace>, after: Option<DiskSpace>, measured: u64) -> u64 {
    match (before, after) {
        (Some(before), Some(after)) => after.free_bytes.saturating_sub(before.free_bytes),
        _ => measured,
    }
}

fn format_receipt(receipt: Option<&Receipt>) -> Option<String> {
    let receipt = receipt?;
    let count: usize = receipt.groups.iter().map(|(_, count)| count).sum();
    let kinds = receipt
        .groups
        .iter()
        .map(|(group, _)| receipt_group_label(*group))
        .collect::<Vec<_>>()
        .join("/");
    Some(format!(
        "Auto-cleaned {count} targets ({}) \u{00b7} {kinds} \u{00b7} {}",
        receipt.names.join(", "),
        human_size(receipt.freed_bytes)
    ))
}

fn receipt_group_label(group: ArtifactGroup) -> &'static str {
    match group {
        ArtifactGroup::Rust => "Rust",
        ArtifactGroup::NodeModules => "Node",
        ArtifactGroup::BuildOutput => "Builds",
        ArtifactGroup::Caches => "Caches",
        ArtifactGroup::Simulators => "Simulators",
        ArtifactGroup::Toolchains => "Toolchains",
    }
}

#[cfg(test)]
#[path = "auto_clean_tests.rs"]
mod tests;
