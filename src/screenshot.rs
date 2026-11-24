// Screenshot harness: stage each screen from a fixed fixture and render it to
// PNG through the view cache, in the popover and then in the window.
// Debug builds only; activated by WD40_SCREENSHOT_DIR. Nothing is deleted.
// Deps: objc2 AppKit/Foundation, crate::{names, popover, state, surface, window}.

use crate::names::display_names;
use crate::state::{with_state, CleanItem, CleanItemStatus, CleanProgress, DoneSummary, GroupSummary, UiScreen};
use crate::{popover, surface, window};
use objc2_app_kit::{NSBitmapImageFileType, NSBitmapImageRep};
use objc2_foundation::{MainThreadMarker, NSDictionary, NSString};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime};
use wd40::scanner::{ArtifactGroup, ArtifactKind, TargetDir};

static STARTED: AtomicBool = AtomicBool::new(false);

const MB: u64 = 1024 * 1024;
/// What the screens are drawn from: path, kind, size in MB, age in days. A
/// fixture rather than the scan, so a capture never shows a real directory.
const FIXTURE: [(&str, ArtifactKind, u64, u64); 10] = [
    ("/Users/dev/code/atlas/target", ArtifactKind::RustTarget, 21_300, 0),
    ("/Users/dev/code/relay-server/target", ArtifactKind::RustTarget, 8_400, 12),
    ("/Users/dev/code/orbit/target", ArtifactKind::RustTarget, 4_900, 40),
    ("/Users/dev/code/pixelforge/target", ArtifactKind::RustTarget, 2_300, 3),
    ("/Users/dev/code/tinyvm/target", ArtifactKind::RustTarget, 1_100, 95),
    ("/Users/dev/code/storefront/node_modules", ArtifactKind::NodeModules, 1_900, 21),
    ("/Users/dev/code/docs-site/node_modules", ArtifactKind::NodeModules, 740, 60),
    ("/Users/dev/code/dashboard/node_modules", ArtifactKind::NodeModules, 512, 2),
    ("/Users/dev/code/storefront/.next", ArtifactKind::BuildOutput, 880, 21),
    ("/Users/dev/code/docs-site/dist", ArtifactKind::BuildOutput, 96, 60),
];
/// The three points of a run the cleaning screen is captured at: how many of
/// the job's targets have really gone by then.
pub const CLEAN_FRAMES: [(&str, usize); 3] = [("start", 0), ("mid", 3), ("end", 7)];

pub fn enabled() -> bool {
    std::env::var_os("WD40_SCREENSHOT_DIR").is_some()
}

pub fn maybe_start(_mtm: MainThreadMarker) {
    if !enabled() || STARTED.swap(true, Ordering::Relaxed) {
        return;
    }
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_millis(400));
        match std::env::var_os("WD40_CAPTURE_WINDOW") {
            Some(_) => crate::capture::run(),
            None => crate::mainthread::run_on_main(sequence),
        }
    });
}

fn sequence() {
    let Some(mtm) = MainThreadMarker::new() else { return };
    let dir = PathBuf::from(std::env::var("WD40_SCREENSHOT_DIR").unwrap());
    let _ = std::fs::create_dir_all(&dir);
    let appearance = std::env::var("WD40_APPEARANCE").unwrap_or_else(|_| "light".into());
    let shot = |name: &str| save(&dir.join(format!("{name}-{appearance}.png")), mtm);

    stage_fixture();
    popover::ensure_open(mtm);
    show_screen(UiScreen::Scan, mtm);
    shot("scan");
    show_screen(UiScreen::Settings, mtm);
    shot("settings");
    for (name, gone) in CLEAN_FRAMES {
        stage_cleaning(gone);
        show_screen(UiScreen::Cleaning, mtm);
        shot(&format!("cleaning-{name}"));
    }
    stage_done();
    show_screen(UiScreen::Done, mtm);
    shot("done");

    window::open(mtm);
    show_screen(UiScreen::Scan, mtm);
    shot("window-scan");
    stage_cleaning(CLEAN_FRAMES[1].1);
    show_screen(UiScreen::Cleaning, mtm);
    shot("window-cleaning");
    show_screen(UiScreen::Done, mtm);
    shot("window-done");
    show_screen(UiScreen::Settings, mtm);
    shot("window-settings");
    window::close(mtm);

    eprintln!("wd-40: screenshots written to {}", dir.display());
    objc2_app_kit::NSApplication::sharedApplication(mtm).terminate(None);
}

fn show_screen(screen: UiScreen, mtm: MainThreadMarker) {
    with_state(|state| state.screen = screen);
    surface::refresh(mtm);
}

/// Replace whatever the scan found with the fixture, settled and sorted.
pub fn stage_fixture() {
    let now = SystemTime::now();
    with_state(|state| {
        state.targets = FIXTURE
            .iter()
            .map(|(path, kind, megabytes, days)| TargetDir {
                path: PathBuf::from(path),
                size_bytes: megabytes * MB,
                last_modified: now - Duration::from_secs(days * 86_400),
                kind: *kind,
            })
            .collect();
        state.measured = (0..state.targets.len()).collect();
        state.reclaim = None;
        state.finish_sizing();
        // Everything a week old is ticked, which leaves one group mixed.
        state.selected = (0..state.targets.len())
            .filter(|index| state.targets[*index].last_modified < now - Duration::from_secs(7 * 86_400))
            .collect();
    });
}

/// A run over the eight largest targets with the first `gone` really removed
/// and the next one coming off.
pub fn stage_cleaning(gone: usize) {
    with_state(|state| {
        let names = display_names(&state.targets);
        let items: Vec<CleanItem> = state
            .targets
            .iter()
            .enumerate()
            .take(8)
            .map(|(index, target)| CleanItem {
                index,
                name: names[index].clone(),
                size_bytes: target.size_bytes,
                freed_bytes: if index < gone { target.size_bytes } else { 0 },
                status: match index {
                    i if i < gone => CleanItemStatus::Done,
                    i if i == gone => CleanItemStatus::Active,
                    _ => CleanItemStatus::Pending,
                },
            })
            .collect();
        state.cleaning = Some(CleanProgress {
            freed_so_far: items.iter().map(|item| item.freed_bytes).sum(),
            current_path: state.targets.get(gone).map(|t| t.path.display().to_string()).unwrap_or_default(),
            done_count: gone.min(items.len()),
            total_count: items.len(),
            items,
        });
    });
}

/// The done summary for that same run finishing: every group's share of the
/// eight removed, and the two smallest targets left in place.
pub fn stage_done() {
    with_state(|state| {
        let (taken, left) = state.targets.split_at(state.targets.len().min(8));
        let removed: Vec<GroupSummary> = ArtifactGroup::ALL
            .iter()
            .filter_map(|&group| {
                let members: Vec<&TargetDir> = taken.iter().filter(|t| t.kind.group() == group).collect();
                (!members.is_empty()).then(|| GroupSummary {
                    group,
                    count: members.len(),
                    bytes: members.iter().map(|t| t.size_bytes).sum(),
                })
            })
            .collect();
        let freed: u64 = removed.iter().map(|row| row.bytes).sum();
        let disk = state.disk_stats();
        state.done = Some(DoneSummary {
            freed_bytes: freed,
            duration: Duration::from_secs(48),
            free_before: disk.map_or(0, |d| d.free_bytes),
            free_after: disk.map_or(0, |d| d.free_bytes.saturating_add(freed)),
            total_bytes: disk.map_or(0, |d| d.total_bytes),
            removed,
            skipped_count: left.len(),
            skipped_bytes: left.iter().map(|t| t.size_bytes).sum(),
            troubled_count: 0,
        });
    });
    // The run those figures describe: all eight really gone.
    stage_cleaning(8);
    with_state(|state| state.last_run = state.cleaning.take());
}

fn save(path: &Path, mtm: MainThreadMarker) {
    let Some(view) = surface::content_view(mtm) else {
        eprintln!("wd-40: no view for {}", path.display());
        return;
    };
    let bounds = view.bounds();
    let Some(rep) = view.bitmapImageRepForCachingDisplayInRect(bounds) else {
        eprintln!("wd-40: bitmap alloc failed for {}", path.display());
        return;
    };
    view.cacheDisplayInRect_toBitmapImageRep(bounds, &rep);
    write_png(&rep, path);
}

fn write_png(rep: &NSBitmapImageRep, path: &Path) {
    let props = NSDictionary::new();
    let Some(data) =
        (unsafe { rep.representationUsingType_properties(NSBitmapImageFileType::PNG, &props) })
    else {
        eprintln!("wd-40: png encode failed for {}", path.display());
        return;
    };
    if !data.writeToFile_atomically(&NSString::from_str(&path.to_string_lossy()), true) {
        eprintln!("wd-40: write failed for {}", path.display());
    }
}
