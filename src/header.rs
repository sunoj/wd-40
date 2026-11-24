// Disk headers shared by popover screens (scan capacity and clean progress).
// Exports: `HEADER_HEIGHT`, `SCAN_HEADER_HEIGHT`, `draw_header`, `draw_scan_header`.
// Deps: crate::{disk_gauge, theme, widgets}, wd40.

use crate::layout::{content_w, panel_w};
use crate::disk_gauge::gauge_fractions;
use crate::theme::Theme;
use crate::widgets::{add_fill, add_line, label, label_right, label_tracked, PAD_X};
use objc2_app_kit::NSView;
use objc2_foundation::MainThreadMarker;
use wd40::disk::DiskSpace;
use wd40::scanner::human_size;

pub const HEADER_HEIGHT: f64 = 78.0;
pub const SCAN_HEADER_HEIGHT: f64 = 125.0;

pub struct HeaderModel {
    pub disk: Option<DiskSpace>,
    /// Total found reclaimable (not the current selection).
    pub reclaimable: u64,
    pub sizing: bool,
    pub approximate: bool,
    /// Right-side caption; defaults to "of {total}" when None.
    pub trailing: Option<String>,
    /// Sub-label left text override (e.g. cleaning "X freed so far").
    pub detail_left: Option<String>,
    /// Sub-label right text; omit ETA-style guesses — pass None to hide.
    pub detail_right: Option<String>,
}

pub fn draw_header(parent: &NSView, y_top: f64, model: &HeaderModel, theme: &Theme, mtm: MainThreadMarker) {
    let y = y_top - HEADER_HEIGHT;
    add_line(parent, 0.0, y, panel_w(), theme.line, mtm);

    let Some(disk) = model.disk else {
        label(
            parent, "Disk unavailable", PAD_X, y_top - 28.0, content_w(), 20.0, 14.0, false,
            theme.ink_2, false, mtm,
        );
        return;
    };

    let free = format!("{} free", scan_size(disk.free_bytes));
    label(parent, &free, PAD_X, y_top - 30.0, 220.0, 22.0, 20.0, true, theme.ink, false, mtm);
    let trailing = model.trailing.clone().unwrap_or_else(|| format!("of {}", human_size(disk.total_bytes)));
    label_right(
        parent, &trailing, PAD_X + 200.0, y_top - 26.0, content_w() - 200.0, 16.0, 11.5,
        theme.ink_3, true, mtm,
    );

    draw_gauge(parent, y_top - 42.0, disk, if model.sizing { 0 } else { model.reclaimable }, theme, mtm);

    draw_detail(parent, y_top, disk, model, theme, mtm);
}

fn draw_detail(
    parent: &NSView,
    y_top: f64,
    disk: DiskSpace,
    model: &HeaderModel,
    theme: &Theme,
    mtm: MainThreadMarker,
) {
    let left = model.detail_left.clone().unwrap_or_else(|| {
        if model.sizing {
            "Measuring build artifacts\u{2026}".into()
        } else {
            let bound = if model.approximate { "up to " } else { "" };
            format!("{bound}{} reclaimable", human_size(model.reclaimable))
        }
    });
    add_fill(parent, PAD_X, y_top - 62.0, 7.0, 7.0, theme.accent, 1.0, 2.0, mtm);
    label(parent, &left, PAD_X + 14.0, y_top - 66.0, 200.0, 16.0, 13.0, false, theme.ink, false, mtm);
    let right = match &model.detail_right {
        Some(right) => Some(right.clone()),
        None if !model.sizing && model.reclaimable > 0 && model.detail_left.is_none() => {
            let after = disk.free_bytes.saturating_add(model.reclaimable).min(disk.total_bytes);
            Some(format!("\u{2192} {} free", human_size(after)))
        }
        None => None,
    };
    if let Some(right) = right {
        label_right(
            parent, &right, PAD_X + 180.0, y_top - 66.0, content_w() - 180.0, 16.0, 11.5,
            theme.ink_3, true, mtm,
        );
    }
}

/// The scan header's model. While `measured` is short of `total` every figure
/// derived from it is a floor, and the header says so rather than letting a
/// number that is still climbing read as an answer.
pub struct ScanHeader {
    pub disk: Option<DiskSpace>,
    pub reclaimable: u64,
    pub measured: usize,
    pub total: usize,
    pub discovering: bool,
    /// Nothing found is past the keep threshold: the figures are recent
    /// builds, not something waiting to be cleaned.
    pub all_recent: bool,
}

impl ScanHeader {
    pub fn sizing(&self) -> bool {
        self.total > 0 && self.measured < self.total
    }

    pub fn in_progress(&self) -> bool {
        self.discovering || self.sizing()
    }
}

/// The disk figures, gauge and legend, in a zone whose lower edge is the
/// header's rule. The verdict line under the legend is `scan_verdict`'s.
pub fn draw_scan_header(parent: &NSView, y_top: f64, model: &ScanHeader, theme: &Theme, mtm: MainThreadMarker) {
    add_line(parent, 0.0, 0.0, panel_w(), theme.line, mtm);
    let Some(disk) = model.disk else {
        label(parent, "Disk unavailable", PAD_X, y_top - 38.0, content_w(), 24.0, 14.0, false, theme.ink_2, false, mtm);
        return;
    };

    let free = format!("{} free", scan_size(disk.free_bytes));
    label_tracked(parent, &free, PAD_X, y_top - 38.0, 220.0, 24.0, 20.0, true, theme.ink, false, -0.28, mtm);
    label_right(parent, &format!("of {}", scan_size(disk.total_bytes)), PAD_X + 200.0, y_top - 35.0, content_w() - 200.0, 16.0, 11.5, theme.ink_3, true, mtm);

    let used = disk.total_bytes.saturating_sub(disk.free_bytes).min(disk.total_bytes);
    let reclaim_landed = crate::tasks::take_reclaim_landed();
    draw_scan_gauge(parent, y_top - 59.0, disk, model, used, reclaim_landed, theme, mtm);
    if model.discovering {
        crate::motion::reset_scan_gauge();
    }
    let reclaimable = if model.discovering { 0 } else { model.reclaimable.min(used) };
    // Everything found, by name: how much of it is old is the verdict's to say.
    let tone = if model.all_recent { theme.ink_3 } else { theme.accent };
    crate::header_legend::draw_legend_row(
        parent,
        y_top - 86.0,
        [
            (if model.all_recent { "Recent builds" } else { "Artifacts" }, human_size(reclaimable), tone),
            ("In use", human_size(used.saturating_sub(reclaimable)), theme.ink_4),
            ("Free", human_size(disk.free_bytes), theme.pos),
        ],
        theme,
        mtm,
    );

}

fn draw_scan_gauge(
    parent: &NSView,
    y: f64,
    disk: DiskSpace,
    model: &ScanHeader,
    used: u64,
    reclaim_landed: bool,
    theme: &Theme,
    mtm: MainThreadMarker,
) {
    let w = content_w();
    add_fill(parent, PAD_X, y, w, 11.0, theme.pos, 0.26, 3.0, mtm);
    if model.discovering {
        let sweep = add_fill(parent, 0.0, y, 38.0, 11.0, theme.surface, 0.65, 3.0, mtm);
        crate::motion::install_discovery_sweep(sweep, PAD_X, w);
        return;
    }

    let reclaimable = model.reclaimable.min(used);
    let parts = gauge_fractions(disk, reclaimable);
    let used_w = w * parts.used;
    add_fill(parent, PAD_X, y, used_w, 11.0, theme.ink_4, 1.0, 0.0, mtm);
    let target_w = w * parts.artifacts;
    let animation = crate::motion::scan_gauge_animation(
        target_w, model.sizing(), model.in_progress(), reclaim_landed,
    );
    let artifact = add_fill(parent, PAD_X + used_w, y, animation.from, 11.0, if model.all_recent { theme.ink_3 } else { theme.accent }, 1.0, 0.0, mtm);
    crate::motion::glide_for_gauge(&artifact, PAD_X + used_w, y, animation.to, animation.duration);
    let marker = add_fill(parent, PAD_X + used_w + animation.from, y, 1.0, 11.0, theme.pos, 0.55, 0.0, mtm);
    crate::motion::glide_gauge_marker(&marker, PAD_X + used_w + animation.to, y, animation.duration);
}

pub fn scan_size(bytes: u64) -> String {
    wd40::scanner::human_size(bytes)
        .replace('T', " TB")
        .replace('G', " GB")
        .replace('M', " MB")
        .replace('K', " KB")
}

fn draw_gauge(
    parent: &NSView,
    y: f64,
    disk: DiskSpace,
    reclaimable: u64,
    theme: &Theme,
    mtm: MainThreadMarker,
) {
    let parts = gauge_fractions(disk, reclaimable);
    let w = content_w();
    add_fill(parent, PAD_X, y, w, 7.0, theme.surface_2, 1.0, 3.5, mtm);
    let used_w = w * parts.used;
    let art_w = w * parts.artifacts;
    add_fill(parent, PAD_X, y, used_w, 7.0, theme.ink_4, 1.0, 3.5, mtm);
    add_fill(parent, PAD_X + used_w, y, art_w.max(0.0), 7.0, theme.accent, 1.0, 3.5, mtm);
}
