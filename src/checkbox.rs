// The popover's tick box, kept as a handle rather than a drawing so a change of
// mind repaints three views instead of rebuilding the list under the pointer.
// Exports: `CheckBox`, `checkbox`.
// Deps: objc2 AppKit, crate::{selection, style, theme, widgets}.

use crate::selection::GroupSelection;
use crate::style::attributed;
use crate::theme::Theme;
use crate::widgets::add_fill;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Sel};
use objc2_app_kit::{NSBox, NSButton, NSButtonType, NSView};
use objc2_foundation::{MainThreadMarker, NSPoint, NSRect, NSSize, NSString};

const SIDE: f64 = 15.0;

/// A tick box and the two boxes that draw its state.
pub struct CheckBox {
    fill: Retained<NSBox>,
    /// The bar shown when some but not all of a group is ticked.
    dash: Retained<NSBox>,
    button: Retained<NSButton>,
}

impl CheckBox {
    pub fn set(&self, selection: GroupSelection, theme: &Theme) {
        let on = selection == GroupSelection::On;
        self.fill.setFillColor(&Theme::color_alpha(
            if on { theme.accent } else { theme.surface_2 },
            if on { 1.0 } else { 0.78 },
        ));
        self.fill.setBorderWidth(if on { 0.0 } else { 1.0 });
        self.fill.setBorderColor(&Theme::color(theme.line_2));
        self.dash.setHidden(selection != GroupSelection::Mixed);
        self.dash.setFillColor(&Theme::color(theme.accent));
        self.button
            .setAttributedTitle(&attributed(tick(on), crate::controls::WHITE, 10.0, true));
    }
}

fn tick(on: bool) -> &'static str {
    if on {
        "\u{2713}"
    } else {
        ""
    }
}

#[allow(clippy::too_many_arguments)]
pub fn checkbox(
    parent: &NSView,
    selection: GroupSelection,
    x: f64,
    y: f64,
    action: Sel,
    target: &AnyObject,
    tag: isize,
    theme: &Theme,
    mtm: MainThreadMarker,
) -> CheckBox {
    let fill = add_fill(parent, x, y, SIDE, SIDE, theme.surface_2, 0.78, 4.0, mtm);
    let dash = add_fill(parent, x + 3.0, y + 6.5, 9.0, 2.0, theme.accent, 1.0, 1.0, mtm);
    let button = unsafe {
        NSButton::buttonWithTitle_target_action(
            &NSString::from_str(""),
            Some(target),
            Some(action),
            mtm,
        )
    };
    button.setButtonType(NSButtonType::MomentaryChange);
    button.setBordered(false);
    button.setFrame(NSRect::new(NSPoint::new(x, y), NSSize::new(SIDE, SIDE)));
    button.setTag(tag);
    parent.addSubview(&button);
    let boxes = CheckBox { fill, dash, button };
    boxes.set(selection, theme);
    boxes
}
