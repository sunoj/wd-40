// Menu bar glyph for WD-40: the can drawn as a monochrome template image.
// Exports: `rusty_icon`, `rust_text_color`.
// Deps: objc2, objc2_app_kit, objc2_foundation.

use objc2::{rc::Retained, AnyThread};
use objc2_app_kit::{NSBezierPath, NSColor, NSImage, NSLineCapStyle, NSLineJoinStyle};
use objc2_foundation::{NSPoint, NSRect, NSSize};

/// Height of the glyph in the menu bar, in points.
const GLYPH_H: f64 = 16.0;
/// The design's artboard: viewBox "116 96 252 396".
const VIEW_X: f64 = 116.0;
const VIEW_Y: f64 = 96.0;
const VIEW_W: f64 = 252.0;
const VIEW_H: f64 = 396.0;
/// Half a stroke sits outside the path, and the design's own artboard clips it
/// on the right. Pad the canvas so nothing is shaved off.
const PAD: f64 = 22.0;

/// The can from the icon design, tinted by artifact weight so the glyph and the
/// size beside it always read as one colour.
///
/// Below the first rust step the tint is `labelColor`, which only tracks the
/// menu bar correctly if AppKit resolves it at draw time — so there the glyph
/// stays a template and macOS colours it. Once there is a rust tint to show,
/// the colour is baked in and the template flag comes off. With `color` off
/// the glyph is always the template.
#[allow(deprecated)]
pub fn rusty_icon(total_bytes: u64, color: bool) -> Option<Retained<NSImage>> {
    let scale = GLYPH_H / (VIEW_H + PAD * 2.0);
    let size = NSSize::new((VIEW_W + PAD * 2.0) * scale, GLYPH_H);
    let image = NSImage::initWithSize(NSImage::alloc(), size);
    let map = |x: f64, y: f64| {
        NSPoint::new((x - VIEW_X + PAD) * scale, GLYPH_H - (y - VIEW_Y + PAD) * scale)
    };
    let rusted = color && total_bytes / (1024 * 1024 * 1024) >= 5;

    image.lockFocus();
    if rusted {
        rust_text_color(total_bytes).setStroke();
    } else {
        NSColor::blackColor().setStroke();
    }

    // The straw: across the top, then down the left side.
    let straw = NSBezierPath::new();
    straw.moveToPoint(map(336.0, 138.0));
    straw.lineToPoint(map(142.0, 138.0));
    straw.lineToPoint(map(142.0, 214.0));
    straw.setLineWidth(36.0 * scale);
    straw.setLineCapStyle(NSLineCapStyle::Round);
    straw.setLineJoinStyle(NSLineJoinStyle::Round);
    straw.stroke();

    draw_can(&map, scale);

    image.unlockFocus();
    // A template image has its colour thrown away and replaced by the menu
    // bar's, so the rust tint above only survives with the flag off. Below the
    // first rust step there is no tint to keep and template is what tracks the
    // bar correctly.
    image.setTemplate(!rusted);
    Some(image)
}

/// The can body and its two label bands, in the same stroke colour the caller
/// has already set.
fn draw_can(map: &impl Fn(f64, f64) -> NSPoint, scale: f64) {
    let top_left = map(200.0, 196.0);
    let body_rect = NSRect::new(
        NSPoint::new(top_left.x, top_left.y - 272.0 * scale),
        NSSize::new(152.0 * scale, 272.0 * scale),
    );
    let radius = 28.0 * scale;
    let body = NSBezierPath::bezierPathWithRoundedRect_xRadius_yRadius(body_rect, radius, radius);
    body.setLineWidth(36.0 * scale);
    body.stroke();

    for y in [300.0, 380.0] {
        let band = NSBezierPath::new();
        band.moveToPoint(map(200.0, y));
        band.lineToPoint(map(352.0, y));
        band.setLineWidth(26.0 * scale);
        band.setLineCapStyle(NSLineCapStyle::Round);
        band.stroke();
    }
}

/// Tint for the size shown beside the glyph — this is where artifact weight
/// still reads, now that the glyph itself is monochrome.
pub fn rust_text_color(total_bytes: u64) -> Retained<NSColor> {
    let gb = total_bytes / (1024 * 1024 * 1024);
    match gb {
        0..=4 => NSColor::labelColor(),
        5..=19 => NSColor::colorWithSRGBRed_green_blue_alpha(0.95, 0.70, 0.40, 1.0),
        20..=49 => NSColor::colorWithSRGBRed_green_blue_alpha(0.90, 0.50, 0.25, 1.0),
        _ => NSColor::colorWithSRGBRed_green_blue_alpha(0.80, 0.35, 0.15, 1.0),
    }
}
