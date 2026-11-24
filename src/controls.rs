// Interactive AppKit controls for the popover (buttons, checkbox, pill, slider).
// Exports: button/checkbox/switch/slider helpers used by views.
// Deps: crate::{theme, widgets}, objc2 AppKit.

use crate::style::attributed;
use crate::theme::Theme;
use crate::widgets::{add_fill, fitted_width};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Sel};
use objc2::MainThreadOnly;
use objc2_app_kit::{
    NSButton, NSColor, NSEventModifierFlags, NSFont, NSFontAttributeName,
    NSForegroundColorAttributeName, NSSlider, NSUnderlineStyleAttributeName,
};
use objc2_foundation::{
    MainThreadMarker, NSAttributedString, NSDictionary, NSNumber, NSPoint, NSRect, NSSize, NSString,
};

#[allow(clippy::too_many_arguments)]
pub fn text_button(
    parent: &objc2_app_kit::NSView,
    title: &str,
    x: f64,
    y: f64,
    w: f64,
    action: Sel,
    target: &AnyObject,
    tag: isize,
    color: (f64, f64, f64),
    mtm: MainThreadMarker,
) -> Retained<NSButton> {
    let button = unsafe {
        NSButton::buttonWithTitle_target_action(
            &NSString::from_str(title),
            Some(target),
            Some(action),
            mtm,
        )
    };
    button.setBordered(false);
    button.setFrame(NSRect::new(NSPoint::new(x, y), NSSize::new(w, 22.0)));
    button.setTag(tag);
    button.setAttributedTitle(&attributed(title, color, 12.5, false));
    parent.addSubview(&button);
    button
}

#[allow(clippy::too_many_arguments)]
pub fn text_button_underlined(
    parent: &objc2_app_kit::NSView,
    title: &str,
    x: f64,
    y: f64,
    w: f64,
    action: Sel,
    target: &AnyObject,
    tag: isize,
    color: (f64, f64, f64),
    mtm: MainThreadMarker,
) -> Retained<NSButton> {
    let button = text_button(parent, title, x, y, w, action, target, tag, color, mtm);
    let font = NSFont::systemFontOfSize(12.5);
    let tint = Theme::color(color);
    let font_obj: &AnyObject = unsafe { &*(&*font as *const NSFont as *const AnyObject) };
    let tint_obj: &AnyObject = unsafe { &*(&*tint as *const NSColor as *const AnyObject) };
    let underline = NSNumber::numberWithDouble(1.0);
    let underline_obj: &AnyObject = unsafe { &*(&*underline as *const NSNumber as *const AnyObject) };
    let attrs = NSDictionary::<NSString, AnyObject>::from_slices::<NSString>(
        &[unsafe { NSForegroundColorAttributeName }, unsafe { NSFontAttributeName }, unsafe { NSUnderlineStyleAttributeName }],
        &[tint_obj, font_obj, underline_obj],
    );
    let title = unsafe { NSAttributedString::new_with_attributes(&NSString::from_str(title), &attrs) };
    button.setAttributedTitle(&title);
    button
}

/// Text button plus a faint ⌘ shortcut hint drawn beside it.
#[allow(clippy::too_many_arguments)]
pub fn text_button_hint(
    parent: &objc2_app_kit::NSView,
    title: &str,
    hint: &str,
    x: f64,
    y: f64,
    action: Sel,
    target: &AnyObject,
    tag: isize,
    color: (f64, f64, f64),
    hint_color: (f64, f64, f64),
    mtm: MainThreadMarker,
) -> Retained<NSButton> {
    let button = text_button(parent, title, x, y, 58.0, action, target, tag, color, mtm);
    crate::widgets::label(parent, hint, x + 58.0, y + 2.0, 28.0, 16.0, 11.0, false, hint_color, true, mtm);
    button
}

#[allow(clippy::too_many_arguments)]
pub fn filled_button(
    parent: &objc2_app_kit::NSView,
    title: &str,
    x: f64,
    y: f64,
    w: f64,
    action: Sel,
    target: &AnyObject,
    tag: isize,
    fill: (f64, f64, f64),
    ink: (f64, f64, f64),
    mtm: MainThreadMarker,
) -> Retained<NSButton> {
    add_fill(parent, x, y, w, 34.0, fill, 1.0, 8.0, mtm);
    let button = unsafe {
        NSButton::buttonWithTitle_target_action(
            &NSString::from_str(title),
            Some(target),
            Some(action),
            mtm,
        )
    };
    button.setBordered(false);
    button.setFrame(NSRect::new(NSPoint::new(x, y), NSSize::new(w, 34.0)));
    button.setTag(tag);
    button.setAttributedTitle(&attributed(title, ink, 13.5, true));
    parent.addSubview(&button);
    button
}

/// What the clean button is entitled to say right now. Two of the three states
/// have nothing to delete and no figure to quote, so they say neither.
pub enum CleanCta {
    /// Sizes are still arriving; nothing may be armed against numbers that are
    /// still moving.
    Measuring,
    /// Nothing is ticked.
    Empty,
    /// Nothing is ticked because nothing is old enough to be worth ticking.
    Clear,
    Ready { count: usize, size: String },
}

impl CleanCta {
    fn title(&self) -> String {
        match self {
            Self::Measuring => "Measuring\u{2026}".into(),
            Self::Empty => "Tick what to clean".into(),
            Self::Clear => "Tick a recent build to clean it anyway".into(),
            Self::Ready { count, .. } => format!("Clean {count} selected"),
        }
    }

    fn size(&self) -> Option<&str> {
        match self {
            Self::Ready { size, .. } => Some(size),
            _ => None,
        }
    }

    fn armed(&self) -> bool {
        matches!(self, Self::Ready { .. })
    }
}

const CTA_H: f64 = 34.0;
/// Knobs and ticks sit on a coloured ground in both appearances.
pub const WHITE: (f64, f64, f64) = (1.0, 1.0, 1.0);
const GLYPH: f64 = 13.0;
const AFTER_GLYPH: f64 = 8.0;
const BEFORE_SIZE: f64 = 10.0;

/// Trash glyph, title and total, centred as one cluster. Fixed offsets only
/// looked centred for one title length, and every state here has a different one.
#[allow(clippy::too_many_arguments)]
pub fn clean_button(
    parent: &objc2_app_kit::NSView,
    cta: &CleanCta,
    x: f64,
    y: f64,
    w: f64,
    action: Sel,
    target: &AnyObject,
    tag: isize,
    theme: &Theme,
    mtm: MainThreadMarker,
) {
    let armed = cta.armed();
    // The label's yellow band with its navy print — and nothing but grey until
    // there is something it would really remove.
    let (fill, ink, alpha) = match cta {
        CleanCta::Ready { .. } => (theme.cta, theme.cta_ink, 1.0),
        // Quiet: the header carries the verdict, and this is only the way on.
        CleanCta::Clear => (theme.surface_2, theme.ink_3, 1.0),
        _ => (theme.surface_3, theme.ink_3, 1.0),
    };
    add_fill(parent, x, y, w, CTA_H, fill, alpha, 8.0, mtm);
    let title = cta.title();
    let title_w = fitted_width(&title, 13.5, false, mtm);
    let size_w = cta.size().map_or(0.0, |size| fitted_width(size, 12.5, true, mtm));
    let lead = if armed { GLYPH + AFTER_GLYPH } else { 0.0 };
    let trail = cta.size().map_or(0.0, |_| BEFORE_SIZE + size_w);
    let mut cursor = x + ((w - (lead + title_w + trail)) / 2.0).max(0.0);
    if armed {
        crate::widgets::symbol_view(parent, "trash", cursor, y + 10.0, GLYPH, ink, mtm);
        cursor += lead;
    }
    crate::widgets::label(parent, &title, cursor, y + 8.0, title_w + 2.0, 18.0, 13.5, false, ink, false, mtm);
    if let Some(size) = cta.size() {
        cursor += title_w + BEFORE_SIZE;
        let amount = crate::widgets::label(parent, size, cursor, y + 9.0, size_w + 2.0, 16.0, 12.5, false, ink, true, mtm);
        amount.setAlphaValue(0.7);
    }
    let button = unsafe {
        NSButton::buttonWithTitle_target_action(&NSString::from_str(""), Some(target), Some(action), mtm)
    };
    button.setBordered(false);
    button.setFrame(NSRect::new(NSPoint::new(x, y), NSSize::new(w, CTA_H)));
    button.setTag(tag);
    button.setEnabled(armed);
    parent.addSubview(&button);
}

/// Pill-shaped toggle track + knob (mock: 38×22, radius 99). Paint only: the
/// row it sits in is the click target, so a second button under the pill would
/// only be a smaller way to hit the same thing.
pub fn pill(parent: &objc2_app_kit::NSView, on: bool, x: f64, y: f64, theme: &Theme, mtm: MainThreadMarker) {
    let track = if on { theme.accent } else { theme.surface_3 };
    add_fill(parent, x, y, 38.0, 22.0, track, 1.0, 11.0, mtm);
    let knob_x = if on { x + 18.0 } else { x + 2.0 };
    add_fill(parent, knob_x, y + 2.0, 18.0, 18.0, WHITE, 1.0, 9.0, mtm);
}

/// Keep-days NSSlider (0–30) matching the mock safety control.
#[allow(clippy::too_many_arguments)]
pub fn days_slider(
    parent: &objc2_app_kit::NSView,
    days: u64,
    x: f64,
    y: f64,
    w: f64,
    action: Sel,
    target: &AnyObject,
    tag: isize,
    mtm: MainThreadMarker,
) -> Retained<NSSlider> {
    let slider = NSSlider::initWithFrame(
        NSSlider::alloc(mtm),
        NSRect::new(NSPoint::new(x, y), NSSize::new(w, 22.0)),
    );
    slider.setMinValue(0.0);
    slider.setMaxValue(30.0);
    slider.setDoubleValue(days.min(30) as f64);
    slider.setContinuous(false);
    slider.setTag(tag);
    unsafe {
        slider.setTarget(Some(target));
        slider.setAction(Some(action));
    }
    parent.addSubview(&slider);
    slider
}

/// Attach ⌘ + key as the button's key equivalent.
pub fn set_cmd_key(button: &NSButton, key: &str) {
    button.setKeyEquivalent(&NSString::from_str(key));
    button.setKeyEquivalentModifierMask(NSEventModifierFlags::Command);
}
