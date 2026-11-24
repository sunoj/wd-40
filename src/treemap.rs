// Squarified treemap shared by the scan map and the cleaning plate: one cell per
// entry, cell area proportional to its bytes, the long tail folded into one.
// The same sizes in the same region always give the same cells, which is what
// lets the scan map hand its picture to the cleaning plate unchanged.
// Exports: `Cell`, `cells`, the tile state constants, `item_state`.

use crate::state::CleanItemStatus;
use objc2_foundation::{NSPoint, NSRect, NSSize};
use wd40::disk::sum_bytes;

/// Past this the cells stop being readable, so the tail becomes a single cell.
const MAX_CELLS: usize = 40;
/// Area a cell needs on average before it can be seen and pointed at.
const CELL_AREA: f64 = 4200.0;

/// Queued for removal, or ticked on the scan map.
pub const PENDING: u8 = 0;
pub const ACTIVE: u8 = 1;
pub const DONE: u8 = 2;
pub const SKIPPED: u8 = 3;
/// Started, but only part of it left the disk.
pub const PART_GONE: u8 = 4;
/// Started, and none of it left the disk.
pub const BLOCKED: u8 = 5;
/// Not part of the job, or not ticked on the scan map.
pub const IDLE: u8 = 6;

pub struct Cell {
    pub rect: NSRect,
    /// Positions in the caller's list this cell stands for: one, or the tail.
    pub members: Vec<usize>,
    pub bytes: u64,
}

/// Lay `sizes` out in `region`, largest first. The largest entries each get a
/// cell; the rest collapse into one, so sixty node_modules do not become sixty
/// unreadable slivers.
pub fn cells(sizes: &[u64], region: NSRect) -> Vec<Cell> {
    let mut order: Vec<usize> = (0..sizes.len()).collect();
    order.sort_by(|a, b| sizes[*b].cmp(&sizes[*a]));
    let cap = ((region.size.width * region.size.height / CELL_AREA) as usize).clamp(3, MAX_CELLS);
    let head = if order.len() > cap { cap - 1 } else { order.len() };
    let mut groups: Vec<Vec<usize>> = order[..head].iter().map(|index| vec![*index]).collect();
    if head < order.len() {
        groups.push(order[head..].to_vec());
    }
    let bytes: Vec<u64> = groups.iter().map(|group| sum_bytes(group.iter().map(|i| sizes[*i]))).collect();
    let areas: Vec<f64> = bytes.iter().map(|bytes| (*bytes).max(1) as f64).collect();
    squarify(&areas, region.size.width, region.size.height)
        .into_iter()
        .zip(groups)
        .zip(bytes)
        .map(|((rect, members), bytes)| Cell {
            rect: NSRect::new(
                NSPoint::new(rect.origin.x + region.origin.x, rect.origin.y + region.origin.y),
                rect.size,
            ),
            members,
            bytes,
        })
        .collect()
}

pub fn item_state(status: CleanItemStatus) -> u8 {
    match status {
        CleanItemStatus::Pending => PENDING,
        CleanItemStatus::Active => ACTIVE,
        CleanItemStatus::Done => DONE,
        CleanItemStatus::Skipped => SKIPPED,
        CleanItemStatus::Partial => PART_GONE,
        CleanItemStatus::Failed => BLOCKED,
    }
}

pub fn contains(rect: NSRect, point: NSPoint) -> bool {
    point.x >= rect.origin.x
        && point.x < rect.origin.x + rect.size.width
        && point.y >= rect.origin.y
        && point.y < rect.origin.y + rect.size.height
}

/// Squarify (Bruls et al.): fill the short side with a row of tiles for as long
/// as adding one improves the worst aspect ratio, then recurse on what is left.
fn squarify(sizes: &[f64], width: f64, height: f64) -> Vec<NSRect> {
    let mut out: Vec<NSRect> = Vec::with_capacity(sizes.len());
    let (mut x, mut y, mut w, mut h) = (0.0_f64, 0.0_f64, width, height);
    let mut left: f64 = sizes.iter().sum();
    let mut i = 0;
    while i < sizes.len() && left > 0.0 && w > 0.5 && h > 0.5 {
        let vertical = w >= h;
        let side = if vertical { h } else { w };
        let scale = w * h / left;
        let mut n = 1;
        while i + n < sizes.len()
            && worst(&sizes[i..i + n + 1], side, scale) <= worst(&sizes[i..i + n], side, scale)
        {
            n += 1;
        }
        let row = &sizes[i..i + n];
        let sum: f64 = row.iter().sum();
        let thick = sum * scale / side;
        let mut off = 0.0;
        for value in row {
            let len = value * scale / thick;
            out.push(match vertical {
                true => NSRect::new(NSPoint::new(x, y + off), NSSize::new(thick, len)),
                false => NSRect::new(NSPoint::new(x + off, y), NSSize::new(len, thick)),
            });
            off += len;
        }
        if vertical {
            (x, w) = (x + thick, w - thick);
        } else {
            (y, h) = (y + thick, h - thick);
        }
        left -= sum;
        i += n;
    }
    out.resize(sizes.len(), NSRect::new(NSPoint::new(x, y), NSSize::new(0.0, 0.0)));
    out
}

fn worst(row: &[f64], side: f64, scale: f64) -> f64 {
    let sum: f64 = row.iter().sum::<f64>() * scale;
    let max = row.iter().copied().fold(f64::MIN, f64::max) * scale;
    let min = row.iter().copied().fold(f64::MAX, f64::min) * scale;
    if sum <= 0.0 || min <= 0.0 || side <= 0.0 {
        return f64::MAX;
    }
    (side * side * max / (sum * sum)).max(sum * sum / (side * side * min))
}

#[cfg(test)]
mod tests {
    use super::{cells, squarify};
    use objc2_foundation::{NSPoint, NSRect, NSSize};

    /// The scan map and the cleaning plate lay the same targets out apart from
    /// each other; the picture only carries over if they agree to the point.
    #[test]
    fn the_same_sizes_always_give_the_same_cells() {
        let region = NSRect::new(NSPoint::new(7.0, 7.0), NSSize::new(520.0, 480.0));
        let sizes = [900, 40, 400, 400, 7, 0, 120];
        let (a, b) = (cells(&sizes, region), cells(&sizes, region));
        assert_eq!(a.len(), sizes.len());
        for (one, other) in a.iter().zip(&b) {
            assert_eq!(one.members, other.members);
            assert_eq!(one.rect, other.rect);
        }
        assert_eq!(a[0].members, vec![0]);
    }

    #[test]
    fn a_long_tail_folds_into_one_cell() {
        let region = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(120.0, 105.0));
        let laid = cells(&[50, 40, 30, 20, 10, 5], region);
        assert_eq!(laid.len(), 3);
        assert_eq!(laid[2].members, vec![2, 3, 4, 5]);
        assert_eq!(laid[2].bytes, 65);
    }

    #[test]
    fn every_tile_keeps_its_share_of_the_area() {
        let sizes = [50.0, 25.0, 15.0, 6.0, 4.0];
        let rects = squarify(&sizes, 300.0, 200.0);
        let total: f64 = sizes.iter().sum();
        for (size, rect) in sizes.iter().zip(&rects) {
            let area = rect.size.width * rect.size.height;
            assert!((area - size / total * 300.0 * 200.0).abs() < 1.0, "{area}");
        }
    }

    #[test]
    fn tiles_stay_inside_the_plate() {
        let rects = squarify(&[9.0, 4.0, 2.0, 1.0], 120.0, 80.0);
        for rect in rects {
            assert!(rect.origin.x >= -0.01 && rect.origin.x + rect.size.width <= 120.01);
            assert!(rect.origin.y >= -0.01 && rect.origin.y + rect.size.height <= 80.01);
        }
    }
}
