// Frame-based AppKit primitives shared by every screen (labels, fills, lines).
// Exports: layout helpers; controls live in `controls`, scrolling in `scrolling`.
// Deps: objc2, objc2_app_kit, objc2_foundation, crate::theme::Theme.

use crate::theme::{hex, Theme, CAN_BLUE, CAN_RED, CAN_YELLOW};
use objc2::rc::Retained;
use objc2::{AnyThread, MainThreadOnly};
use objc2_app_kit::{
    NSBox, NSBoxType, NSFont, NSFontAttributeName, NSForegroundColorAttributeName, NSImage,
    NSImageSymbolConfiguration, NSImageSymbolScale, NSImageView, NSKernAttributeName,
    NSLineBreakMode, NSTextAlignment, NSTextField, NSTrackingArea, NSTrackingAreaOptions, NSView,
};
use objc2_foundation::{MainThreadMarker, NSAttributedString, NSDictionary, NSPoint, NSRect, NSSize, NSNumber, NSString};

pub const PAD_X: f64 = 16.0;
/// The label stripe every screen carries along its top edge.
pub const STRIPE_H: f64 = 3.0;

/// A screen's backing: an opaque surface with the can's label along its top
/// edge. NSVisualEffectView was tried and dropped: behind-window vibrancy let
/// the desktop through and made flat mid-greys illegible, and the material had
/// not settled the first time the popover was shown. A plain fill renders the
/// same every time, in the popover and in the window.
pub fn root_view(width: f64, height: f64, theme: &Theme, mtm: MainThreadMarker) -> Retained<NSView> {
    let view = NSView::initWithFrame(
        NSView::alloc(mtm),
        NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(width, height)),
    );
    add_fill(&view, 0.0, 0.0, width, height, theme.surface, 1.0, 0.0, mtm);
    // Blue body, yellow band, red cap, in the can's own proportions.
    let mut x = 0.0;
    for (colour, share) in [(CAN_BLUE, 0.62), (CAN_YELLOW, 0.26), (CAN_RED, 0.12)] {
        add_fill(&view, x, height - STRIPE_H, width * share, STRIPE_H, hex(colour), 1.0, 0.0, mtm);
        x += width * share;
    }
    view
}

#[allow(clippy::too_many_arguments)]
pub fn add_fill(
    parent: &NSView,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    rgb: (f64, f64, f64),
    alpha: f64,
    radius: f64,
    mtm: MainThreadMarker,
) -> Retained<NSBox> {
    let box_ = NSBox::initWithFrame(
        NSBox::alloc(mtm),
        NSRect::new(NSPoint::new(x, y), NSSize::new(w.max(0.5), h.max(0.5))),
    );
    box_.setBoxType(NSBoxType::Custom);
    box_.setBorderWidth(0.0);
    box_.setFillColor(&Theme::color_alpha(rgb, alpha));
    if radius > 0.0 {
        box_.setCornerRadius(radius);
    }
    parent.addSubview(&box_);
    box_
}

pub fn add_line(
    parent: &NSView,
    x: f64,
    y: f64,
    w: f64,
    color: (f64, f64, f64),
    mtm: MainThreadMarker,
) {
    add_fill(parent, x, y, w, 1.0, color, 1.0, 0.0, mtm);
}

#[allow(deprecated)]
#[allow(clippy::too_many_arguments)]
pub fn symbol_view(
    parent: &NSView,
    symbol: &str,
    x: f64,
    y: f64,
    size: f64,
    color: (f64, f64, f64),
    mtm: MainThreadMarker,
) {
    let Some(image) = NSImage::imageWithSystemSymbolName_accessibilityDescription(
        &NSString::from_str(symbol),
        None,
    ) else {
        return;
    };
    image.setTemplate(true);
    let view = NSImageView::imageViewWithImage(&image, mtm);
    view.setFrame(NSRect::new(NSPoint::new(x, y), NSSize::new(size, size)));
    view.setContentTintColor(Some(&Theme::color(color)));
    view.setSymbolConfiguration(Some(&NSImageSymbolConfiguration::configurationWithScale(
        NSImageSymbolScale::Medium,
    )));
    parent.addSubview(&view);
}

#[allow(clippy::too_many_arguments)]
pub fn label(
    parent: &NSView,
    text: &str,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    size: f64,
    bold: bool,
    color: (f64, f64, f64),
    mono: bool,
    mtm: MainThreadMarker,
) -> Retained<NSTextField> {
    make_label(parent, text, x, y, w, h, size, bold, color, mono, false, None, mtm)
}

#[allow(clippy::too_many_arguments)]
pub fn label_tracked(
    parent: &NSView,
    text: &str,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    size: f64,
    bold: bool,
    color: (f64, f64, f64),
    mono: bool,
    tracking: f64,
    mtm: MainThreadMarker,
) -> Retained<NSTextField> {
    make_label(parent, text, x, y, w, h, size, bold, color, mono, false, Some(tracking), mtm)
}

/// Multiline label that wraps at word boundaries (footer copy, notes).
#[allow(clippy::too_many_arguments)]
pub fn label_wrap(
    parent: &NSView,
    text: &str,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    size: f64,
    color: (f64, f64, f64),
    mtm: MainThreadMarker,
) -> Retained<NSTextField> {
    make_label(parent, text, x, y, w, h, size, false, color, false, true, None, mtm)
}

#[allow(clippy::too_many_arguments)]
fn make_label(
    parent: &NSView,
    text: &str,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    size: f64,
    bold: bool,
    color: (f64, f64, f64),
    mono: bool,
    wrap: bool,
    tracking: Option<f64>,
    mtm: MainThreadMarker,
) -> Retained<NSTextField> {
    let field = NSTextField::initWithFrame(
        NSTextField::alloc(mtm),
        NSRect::new(NSPoint::new(x, y), NSSize::new(w, h)),
    );
    field.setStringValue(&NSString::from_str(text));
    field.setBezeled(false);
    field.setDrawsBackground(false);
    field.setEditable(false);
    field.setSelectable(false);
    field.setTextColor(Some(&Theme::color(color)));
    let font = if mono {
        NSFont::monospacedSystemFontOfSize_weight(size, 0.23)
    } else if bold {
        NSFont::boldSystemFontOfSize(size)
    } else {
        NSFont::systemFontOfSize(size)
    };
    field.setFont(Some(&font));
    if let Some(tracking) = tracking {
        field.setAttributedStringValue(&tracked_string(text, &font, color, tracking));
    }
    if wrap {
        field.setUsesSingleLineMode(false);
        field.setMaximumNumberOfLines(0);
        field.setLineBreakMode(NSLineBreakMode::ByWordWrapping);
        field.setPreferredMaxLayoutWidth(w);
        if let Some(cell) = field.cell() {
            cell.setWraps(true);
        }
    }
    parent.addSubview(&field);
    field
}

/// Letter-spaced text. AppKit only applies kerning through an attributed
/// string, so a tracked label cannot go through `setStringValue`.
fn tracked_string(
    text: &str,
    font: &NSFont,
    color: (f64, f64, f64),
    tracking: f64,
) -> Retained<NSAttributedString> {
    let font_obj: &objc2::runtime::AnyObject =
        unsafe { &*(font as *const NSFont as *const objc2::runtime::AnyObject) };
    let color = Theme::color(color);
    let color_obj: &objc2::runtime::AnyObject =
        unsafe { &*(&*color as *const _ as *const objc2::runtime::AnyObject) };
    let kern = NSNumber::numberWithDouble(tracking);
    let kern_obj: &objc2::runtime::AnyObject =
        unsafe { &*(&*kern as *const NSNumber as *const objc2::runtime::AnyObject) };
    let attrs = NSDictionary::<NSString, objc2::runtime::AnyObject>::from_slices::<NSString>(
        &[
            unsafe { NSForegroundColorAttributeName },
            unsafe { NSFontAttributeName },
            unsafe { NSKernAttributeName },
        ],
        &[color_obj, font_obj, kern_obj],
    );
    unsafe { NSAttributedString::new_with_attributes(&NSString::from_str(text), &attrs) }
}

#[allow(clippy::too_many_arguments)]
pub fn label_right(
    parent: &NSView,
    text: &str,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    size: f64,
    color: (f64, f64, f64),
    mono: bool,
    mtm: MainThreadMarker,
) -> Retained<NSTextField> {
    let field = label(parent, text, x, y, w, h, size, false, color, mono, mtm);
    field.setAlignment(NSTextAlignment::Right);
    field
}

/// Replace a view's tracking areas with one covering its whole visible rect.
/// Every hovering view here wants the same base set — enter/exit, active
/// whatever the app is doing, sized from the view — plus whatever `extra` it
/// needs. Rebuilt on each `updateTrackingAreas`, or it stops firing on scroll.
pub fn retrack(view: &NSView, extra: NSTrackingAreaOptions) {
    unsafe {
        for area in view.trackingAreas() {
            view.removeTrackingArea(&area);
        }
        let area = NSTrackingArea::initWithRect_options_owner_userInfo(
            NSTrackingArea::alloc(),
            NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(0.0, 0.0)),
            extra
                | NSTrackingAreaOptions::MouseEnteredAndExited
                | NSTrackingAreaOptions::ActiveAlways
                | NSTrackingAreaOptions::InVisibleRect,
            Some(view),
            None,
        );
        view.addTrackingArea(&area);
    }
}

/// Width this text needs at this size, measured with a throwaway field so a
/// caller can lay out by content instead of guessing a column width.
pub fn fitted_width(text: &str, size: f64, mono: bool, mtm: MainThreadMarker) -> f64 {
    let field = NSTextField::initWithFrame(
        NSTextField::alloc(mtm),
        NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(0.0, 0.0)),
    );
    field.setStringValue(&NSString::from_str(text));
    field.setBezeled(false);
    field.setDrawsBackground(false);
    field.setEditable(false);
    let font = if mono {
        NSFont::monospacedSystemFontOfSize_weight(size, 0.23)
    } else {
        NSFont::systemFontOfSize(size)
    };
    field.setFont(Some(&font));
    field.sizeToFit();
    field.frame().size.width
}
