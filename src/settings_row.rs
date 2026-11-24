// The settings screen's line types: a row that tints under the pointer and
// carries its click across its whole width, plus the switch, value and group
// rows the design is built from.
// Exports: row specs and builders for sections, switches, values, disclosures,
// and artifact groups.
// Deps: objc2 AppKit tracking APIs; crate::{controls, theme, widgets}.

use crate::layout::{content_w, panel_w};
use crate::controls::pill;
use crate::theme::Theme;
use crate::widgets::{
    self, add_fill, label, label_right, label_tracked, retrack, PAD_X,
};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Sel};
use objc2::{define_class, msg_send, DefinedClass, MainThreadOnly};
use objc2_app_kit::{NSBox, NSButton, NSButtonType, NSEvent, NSTrackingAreaOptions, NSView};
use objc2_foundation::{MainThreadMarker, NSPoint, NSRect, NSSize, NSString};

const ROW_H: f64 = 38.0;
/// A row carrying a second line under its label.
const HINT_ROW_H: f64 = 48.0;
const SECTION_H: f64 = 32.0;
const DIVIDER_H: f64 = 9.0;

const SWITCH_W: f64 = 38.0;
fn switch_x() -> f64 {
    panel_w() - PAD_X - SWITCH_W
}
const CHEVRON: f64 = 13.0;
fn chevron_x() -> f64 {
    panel_w() - PAD_X - CHEVRON
}
/// Right edge of a value, clear of the chevron beside it.
fn value_right() -> f64 {
    chevron_x() - 5.0
}
const CHOICE_H: f64 = 30.0;

/// What every settings row needs: what it says, and what a click on it sends.
pub struct Spec<'a> {
    pub title: &'a str,
    pub hint: Option<&'a str>,
    pub action: Sel,
}

/// An artifact group's line in "what to scan".
pub struct GroupSpec<'a> {
    pub symbol: &'a str,
    pub title: &'a str,
    /// What the group accounts for right now, or the mark for "not scanned".
    pub size: &'a str,
    pub on: bool,
    pub action: Sel,
    pub tag: isize,
}

/// One hand-drawn option nested beneath an open disclosure row.
pub struct ChoiceSpec<'a> {
    pub title: &'a str,
    pub selected: bool,
    pub action: Sel,
    pub tag: isize,
}

define_class!(
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[ivars = Retained<NSBox>]
    #[name = "WD40SettingsRow"]
    pub struct SettingsRow;

    impl SettingsRow {
        #[unsafe(method(mouseEntered:))]
        fn mouse_entered(&self, _event: &NSEvent) {
            self.ivars().setHidden(false);
        }

        #[unsafe(method(mouseExited:))]
        fn mouse_exited(&self, _event: &NSEvent) {
            self.ivars().setHidden(true);
        }

        #[unsafe(method(updateTrackingAreas))]
        fn update_tracking_areas(&self) {
            unsafe {
                let _: () = msg_send![super(self), updateTrackingAreas];
            }
            retrack(self, NSTrackingAreaOptions::empty());
        }
    }
);

/// A row of `h` points ending at `y_top`, tinted while the pointer is inside it.
/// The click target is added by `close_row` once the content is in place.
fn open_row(parent: &NSView, y_top: f64, h: f64, theme: &Theme, mtm: MainThreadMarker) -> Retained<SettingsRow> {
    let frame = NSRect::new(NSPoint::new(0.0, y_top - h), NSSize::new(panel_w(), h));
    let row = mtm.alloc().set_ivars(tint(h, theme, mtm));
    let row: Retained<SettingsRow> = unsafe { msg_send![super(row), initWithFrame: frame] };
    row.addSubview(row.ivars());
    parent.addSubview(&row);
    row
}

/// Lay one transparent button over the finished row. A 38pt pill is a small
/// thing to ask someone to hit, and the mock treats the whole line as the
/// control; on top of the content, it is the only view that answers the click.
fn close_row(row: &SettingsRow, h: f64, spec_action: Sel, tag: isize, target: &AnyObject, mtm: MainThreadMarker) {
    let button = unsafe {
        NSButton::buttonWithTitle_target_action(&NSString::from_str(""), Some(target), Some(spec_action), mtm)
    };
    button.setBordered(false);
    button.setButtonType(NSButtonType::MomentaryChange);
    button.setFrame(NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(panel_w(), h)));
    button.setTag(tag);
    row.addSubview(&button);
}

fn tint(h: f64, theme: &Theme, mtm: MainThreadMarker) -> Retained<NSBox> {
    let holder = NSView::initWithFrame(
        NSView::alloc(mtm),
        NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(panel_w(), h)),
    );
    let tint = add_fill(&holder, 0.0, 0.0, panel_w(), h, theme.surface_2, 1.0, 0.0, mtm);
    tint.removeFromSuperview();
    tint.setHidden(true);
    tint
}

/// Title, and the line under it when there is one.
fn draw_title(row: &SettingsRow, spec: &Spec<'_>, width: f64, theme: &Theme, mtm: MainThreadMarker) {
    let Some(hint) = spec.hint else {
        label(row, spec.title, PAD_X, 11.0, width, 16.0, 13.5, false, theme.ink, false, mtm);
        return;
    };
    label(row, spec.title, PAD_X, 25.0, width, 16.0, 13.5, false, theme.ink, false, mtm);
    label(row, hint, PAD_X, 9.0, width, 15.0, 11.5, false, theme.ink_3, false, mtm);
}

pub fn switch_row(
    parent: &NSView,
    y_top: f64,
    spec: &Spec<'_>,
    on: bool,
    theme: &Theme,
    target: &AnyObject,
    mtm: MainThreadMarker,
) -> f64 {
    let h = spec.hint.map_or(ROW_H, |_| HINT_ROW_H);
    let row = open_row(parent, y_top, h, theme, mtm);
    draw_title(&row, spec, switch_x() - PAD_X - 8.0, theme, mtm);
    pill(&row, on, switch_x(), (h - 22.0) / 2.0, theme, mtm);
    close_row(&row, h, spec.action, 0, target, mtm);
    y_top - h
}

/// A row whose right side states where it stands and opens on click. `value` is
/// left off for a row that only leads somewhere.
pub fn value_row(
    parent: &NSView,
    y_top: f64,
    spec: &Spec<'_>,
    value: Option<&str>,
    theme: &Theme,
    target: &AnyObject,
    mtm: MainThreadMarker,
) -> f64 {
    let h = spec.hint.map_or(ROW_H, |_| HINT_ROW_H);
    let row = open_row(parent, y_top, h, theme, mtm);
    draw_title(&row, spec, 180.0, theme, mtm);
    if let Some(value) = value {
        let x = PAD_X + 184.0;
        label_right(&row, value, x, (h - 16.0) / 2.0, value_right() - x, 16.0, 12.5, theme.ink_2, true, mtm);
    }
    widgets::symbol_view(&row, "chevron.right", chevron_x(), (h - CHEVRON) / 2.0, CHEVRON, theme.ink_4, mtm);
    close_row(&row, h, spec.action, 0, target, mtm);
    y_top - h
}

/// A value row that reveals its choices inline instead of leading elsewhere.
#[allow(clippy::too_many_arguments)]
pub fn disclosure_row(
    parent: &NSView,
    y_top: f64,
    spec: &Spec<'_>,
    value: &str,
    expanded: bool,
    choices: &[ChoiceSpec<'_>],
    theme: &Theme,
    target: &AnyObject,
    mtm: MainThreadMarker,
) -> f64 {
    let h = spec.hint.map_or(ROW_H, |_| HINT_ROW_H);
    let row = open_row(parent, y_top, h, theme, mtm);
    draw_title(&row, spec, 180.0, theme, mtm);
    let x = PAD_X + 184.0;
    label_right(&row, value, x, (h - 16.0) / 2.0, value_right() - x, 16.0, 12.5, theme.ink_2, true, mtm);
    let chevron = if expanded { "chevron.down" } else { "chevron.right" };
    widgets::symbol_view(&row, chevron, chevron_x(), (h - CHEVRON) / 2.0, CHEVRON, theme.ink_4, mtm);
    close_row(&row, h, spec.action, 0, target, mtm);

    let mut y = y_top - h;
    if expanded {
        for choice in choices {
            y = choice_row(parent, y, choice, theme, target, mtm);
        }
    }
    y
}

fn choice_row(
    parent: &NSView,
    y_top: f64,
    choice: &ChoiceSpec<'_>,
    theme: &Theme,
    target: &AnyObject,
    mtm: MainThreadMarker,
) -> f64 {
    let row = open_row(parent, y_top, CHOICE_H, theme, mtm);
    if choice.selected {
        widgets::symbol_view(&row, "checkmark", PAD_X + 7.0, 9.0, 11.0, theme.ink_2, mtm);
    }
    label(&row, choice.title, PAD_X + 28.0, 7.0, 180.0, 16.0, 12.5, choice.selected, theme.ink_2, false, mtm);
    close_row(&row, CHOICE_H, choice.action, choice.tag, target, mtm);
    y_top - CHOICE_H
}

pub fn choice_list_height(count: usize) -> f64 {
    CHOICE_H * count as f64
}

/// Icon, name, what the group accounts for, and its switch.
pub fn group_row(
    parent: &NSView,
    y_top: f64,
    spec: &GroupSpec<'_>,
    theme: &Theme,
    target: &AnyObject,
    mtm: MainThreadMarker,
) -> f64 {
    let row = open_row(parent, y_top, ROW_H, theme, mtm);
    widgets::symbol_view(&row, spec.symbol, PAD_X, (ROW_H - 15.0) / 2.0, 15.0, theme.ink_3, mtm);
    label(&row, spec.title, PAD_X + 26.0, 11.0, 160.0, 16.0, 13.5, false, theme.ink, false, mtm);
    let x = PAD_X + 190.0;
    label_right(&row, spec.size, x, 11.5, switch_x() - 10.0 - x, 15.0, 12.0, theme.ink_3, true, mtm);
    pill(&row, spec.on, switch_x(), (ROW_H - 22.0) / 2.0, theme, mtm);
    close_row(&row, ROW_H, spec.action, spec.tag, target, mtm);
    y_top - ROW_H
}

pub fn section(parent: &NSView, y_top: f64, title: &str, theme: &Theme, mtm: MainThreadMarker) -> f64 {
    label_tracked(
        parent, title, PAD_X, y_top - 26.0, content_w(), 14.0, 11.0, false, theme.ink_3, true,
        0.66, mtm,
    );
    y_top - SECTION_H
}

pub fn divider(parent: &NSView, y_top: f64, theme: &Theme, mtm: MainThreadMarker) -> f64 {
    widgets::add_line(parent, PAD_X, y_top - 8.0, content_w(), theme.line, mtm);
    y_top - DIVIDER_H
}
