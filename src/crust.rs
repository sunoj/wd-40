// The rust crust: where on the plate it lies, what share of the volume it
// stands for, and how the plate's two layers are painted — the steel underneath, once, and a patch
// of rust per target, each in its own view.
// Exports: `crust_region`, `share_line`, `group_tone`, `paint_steel`,
// `paint_tile`, `Tone`, `PLATE_H`.
// Deps: crate::{metal, treemap}, objc2 AppKit.

use crate::metal::{brushed, grey, inset, outline, rings, rnd};
use crate::theme::hex;
use crate::treemap::{ACTIVE, IDLE, PENDING};
use objc2_app_kit::{NSBezierPath, NSColor};
use objc2_foundation::{NSPoint, NSRect, NSSize};
use wd40::disk::DiskSpace;
use wd40::scanner::ArtifactGroup;

pub type Tone = (f64, f64, f64);

/// Height of the plate in the popover. The window gives it what room it has.
pub const PLATE_H: f64 = 190.0;
/// Largest radius of the turning marks at the plate's centre.
const RINGS: f64 = 96.0;
/// Steel left showing round the crust, so the rust reads as lying on a plate.
const RIM: f64 = 7.0;

/// The part of the plate the crust covers: all of it but the rim. The job's
/// targets share that area by their bytes, so every target that comes off is a
/// patch of steel you can see, and a finished job is a clean plate.
pub fn crust_region(plate: NSSize) -> NSRect {
    inset(NSRect::new(NSPoint::new(0.0, 0.0), plate), RIM)
}

/// How much of the whole volume this job is. The plate is not drawn to that
/// scale — a job is rarely more than a few percent of a disk, and a plate that
/// was nearly all steel before it started would have nothing to show — so the
/// true figure is said in words instead. With no volume size there is none.
pub fn share_line(job_bytes: u64, disk: Option<DiskSpace>) -> String {
    let Some(total) = disk.map(|d| d.total_bytes).filter(|total| *total > 0) else {
        return "volume size unknown".to_string();
    };
    let fraction = (job_bytes as f64 / total as f64).clamp(0.0, 1.0);
    format!("{} of the volume is crust", share_text(fraction))
}

fn share_text(fraction: f64) -> String {
    match fraction * 100.0 {
        pct if pct >= 10.0 => format!("{pct:.0}%"),
        pct if pct >= 1.0 => format!("{pct:.1}%"),
        pct if pct >= 0.01 => format!("{pct:.2}%"),
        _ => "under 0.01%".to_string(),
    }
}

/// One colour per group, so the map reads by kind as well as by size. Rust
/// targets are rust; the rest take the can's own colours and their neighbours.
pub fn group_tone(group: ArtifactGroup) -> Tone {
    hex(match group {
        ArtifactGroup::Rust => 0xB5521F,
        ArtifactGroup::NodeModules => 0xC99A1E,
        ArtifactGroup::BuildOutput => 0x3F5FB0,
        ArtifactGroup::Caches => 0x3F8C86,
        ArtifactGroup::Simulators => 0x7B5BA6,
        ArtifactGroup::Toolchains => 0x9A3B34,
    })
}

/// The plate itself: brushed steel, its turning marks, and the footprint of
/// every tile scribed into it. The tiles sit over the footprints, so a scribe
/// line only shows once its target has really gone. Painted once.
pub fn paint_steel(bounds: NSRect, dark: bool, footprints: &[NSRect]) {
    NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(bounds, 10.0, 10.0).addClip();
    brushed(bounds, dark);
    let centre = NSPoint::new(bounds.size.width / 2.0, bounds.size.height / 2.0);
    // The turning marks keep the size they have in the popover: on the
    // window's plate they would otherwise swallow it.
    rings(centre, (bounds.size.height * 0.44).min(RINGS), dark);
    grey(if dark { 0.9 } else { 0.2 }, 0.22).setStroke();
    for rect in footprints {
        outline(inset(*rect, 1.5), 1.0);
    }
    NSColor::colorWithSRGBRed_green_blue_alpha(0.11, 0.1, 0.09, 0.16).setStroke();
    let edge = NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(inset(bounds, 0.5), 9.5, 9.5);
    edge.setLineWidth(1.0);
    edge.stroke();
}

/// One tile of crust, filling its own view, in its group's colour. The one
/// being removed right now carries a bright rim; one the run left behind is
/// hatched; one that is not ticked, or not in the job, is flat and unpitted.
pub fn paint_tile(bounds: NSRect, tone: Tone, index: usize, state: u8) {
    // Pits near an edge would otherwise spill onto the steel beside the tile.
    NSBezierPath::bezierPathWithRect(bounds).addClip();
    // Neighbours of one group differ a little, so their seams can be seen.
    let shade = 0.9 + 0.1 * ((index % 4) as f64 / 3.0);
    NSColor::colorWithSRGBRed_green_blue_alpha(tone.0 * shade, tone.1 * shade, tone.2 * shade, 1.0).setFill();
    NSBezierPath::fillRect(bounds);
    if state != IDLE {
        pits(bounds, index);
    }
    grey(0.05, 0.5).setStroke();
    outline(inset(bounds, 0.5), 1.0);
    match state {
        ACTIVE => {
            NSColor::colorWithSRGBRed_green_blue_alpha(1.0, 0.96, 0.9, 0.9).setStroke();
            outline(inset(bounds, 1.5), 2.0);
        }
        PENDING | IDLE => {}
        _ => hatch(bounds),
    }
}

/// The crust's texture: light and dark pits, the same every time it is drawn.
fn pits(bounds: NSRect, index: usize) {
    let mut seed = 0x9e37_79b9_u32 ^ (index as u32).wrapping_mul(2_654_435_761);
    let count = ((bounds.size.width * bounds.size.height / 220.0) as usize).clamp(6, 90);
    for _ in 0..count {
        let cx = rnd(&mut seed) * bounds.size.width;
        let cy = rnd(&mut seed) * bounds.size.height;
        let light = rnd(&mut seed) > 0.5;
        grey(if light { 1.0 } else { 0.0 }, 0.06 + rnd(&mut seed) * 0.16).setFill();
        crate::metal::circle_path(NSPoint::new(cx, cy), 1.5 + rnd(&mut seed) * 5.5).fill();
    }
}

/// Diagonal strokes over a tile that is staying on the plate.
fn hatch(bounds: NSRect) {
    grey(0.08, 0.3).setStroke();
    let mut x = -bounds.size.height;
    while x < bounds.size.width {
        let line = NSBezierPath::new();
        line.moveToPoint(NSPoint::new(x, 0.0));
        line.lineToPoint(NSPoint::new(x + bounds.size.height, bounds.size.height));
        line.setLineWidth(1.5);
        line.stroke();
        x += 7.0;
    }
}

#[cfg(test)]
mod tests {
    use super::{crust_region, share_line, share_text, PLATE_H, RIM};
    use objc2_foundation::NSSize;
    use wd40::disk::DiskSpace;
use wd40::scanner::ArtifactGroup;

pub type Tone = (f64, f64, f64);

    fn disk(total: u64) -> Option<DiskSpace> {
        Some(DiskSpace { free_bytes: total / 2, total_bytes: total })
    }

    #[test]
    fn the_crust_covers_the_plate_inside_its_rim_at_any_size() {
        for plate in [NSSize::new(348.0, PLATE_H), NSSize::new(900.0, 520.0)] {
            let crust = crust_region(plate);
            assert_eq!(crust.origin.x, RIM);
            assert_eq!(crust.size.width, plate.width - RIM * 2.0);
            assert_eq!(crust.size.height, plate.height - RIM * 2.0);
        }
    }

    #[test]
    fn the_legend_carries_the_true_share_of_the_volume() {
        assert_eq!(share_line(250, disk(1000)), "25% of the volume is crust");
        assert_eq!(share_line(1, disk(10_000_000)), "under 0.01% of the volume is crust");
    }

    #[test]
    fn without_a_volume_size_no_share_is_invented() {
        assert_eq!(share_line(500, None), "volume size unknown");
    }

    #[test]
    fn share_text_keeps_small_fractions_readable() {
        assert_eq!(share_text(0.061), "6.1%");
        assert_eq!(share_text(0.0004), "0.04%");
    }
}
