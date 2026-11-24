// Row container: tints itself while the pointer is inside it, says which
// directory it stands for while the pointer stays, and treats the whole row as
// the hit target rather than just the 15pt tick box.
// Exports: `HoverRow`, `hover_row`, `fit_detail`.
// Deps: objc2 AppKit tracking APIs; crate::{actions, theme, widgets}.

use crate::layout::{panel_w};
use crate::theme::Theme;
use crate::widgets::retrack;
use objc2::rc::Retained;
use objc2::runtime::Sel;
use objc2::{define_class, msg_send, ClassType, DefinedClass, MainThreadOnly, Message};
use objc2_app_kit::{
    NSBox, NSBoxType, NSControl, NSEvent, NSTextField, NSTrackingAreaOptions, NSView,
};
use objc2_foundation::{MainThreadMarker, NSPoint, NSRect, NSSize, NSString};
use std::cell::{Cell, RefCell};

/// The row's second line, as two fields, so a hover does not relayout a string.
struct Detail {
    rest: Retained<NSTextField>,
    hover: Retained<NSTextField>,
}

pub struct HoverRowIvars {
    /// Sits behind the row's content, hidden until the pointer arrives.
    tint: Retained<NSBox>,
    /// Which row this is, in the same tag space the tick box uses.
    tag: Cell<isize>,
    /// Set once the row's labels exist, which is after the row itself does.
    detail: RefCell<Option<Detail>>,
}

define_class!(
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[ivars = HoverRowIvars]
    #[name = "WD40HoverRow"]
    pub struct HoverRow;

    impl HoverRow {
        #[unsafe(method(mouseEntered:))]
        fn mouse_entered(&self, _event: &NSEvent) {
            let _span = crate::trace::span("hover-enter");
            self.paint_hover(true);
        }

        #[unsafe(method(mouseExited:))]
        fn mouse_exited(&self, _event: &NSEvent) {
            let _span = crate::trace::span("hover-exit");
            self.paint_hover(false);
        }

        /// A 15pt box is a small thing to ask someone to hit for an
        /// irreversible choice; the whole row carries it.
        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, _event: &NSEvent) {
            crate::actions::toggle_item(self.ivars().tag.get(), self.mtm());
        }

        /// Anything in the row with an action of its own keeps the click, so the
        /// tick box is not toggled twice; labels, washes and badges hand it up.
        #[unsafe(method(hitTest:))]
        fn hit_test(&self, point: NSPoint) -> *mut NSView {
            let hit: *mut NSView = unsafe { msg_send![super(self), hitTest: point] };
            if hit.is_null() || keeps_click(hit) {
                return hit;
            }
            self as *const Self as *mut NSView
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

impl HoverRow {
    /// Hand the row the line under its name, what that line says at rest, and
    /// what it says under the pointer.
    pub fn set_detail(&self, field: &NSTextField, rest: &str, hover: &str) {
        field.setStringValue(&NSString::from_str(rest));
        let hover_field = twin_label(field, hover);
        hover_field.setHidden(true);
        if let Some(parent) = unsafe { field.superview() } {
            parent.addSubview(&hover_field);
        }
        *self.ivars().detail.borrow_mut() = Some(Detail {
            rest: field.retain(),
            hover: hover_field,
        });
    }

    /// A row is a hit target for an irreversible delete and says only a project
    /// name; while the pointer is on it, the second line is the directory that
    /// would actually go. The whole path is on the row's tooltip as well, since
    /// this line has one line's worth of room.
    fn show_path(&self, hovering: bool) {
        let detail = self.ivars().detail.borrow();
        let Some(detail) = detail.as_ref() else { return };
        detail.rest.setHidden(hovering);
        detail.hover.setHidden(!hovering);
    }

    pub(crate) fn paint_hover(&self, hovering: bool) {
        self.ivars().tint.setHidden(!hovering);
        self.show_path(hovering);
    }

    fn fit_detail(&self, width: f64) {
        let detail = self.ivars().detail.borrow();
        let Some(detail) = detail.as_ref() else { return };
        set_width(&detail.rest, width);
        set_width(&detail.hover, width);
    }
}

/// The age line shares its width with the hover path. A reopen that shows or
/// hides RECENT has to move both, or the path sits under the badge.
pub fn fit_detail(age: &NSTextField, width: f64) {
    if let Some(parent) = unsafe { age.superview() } {
        if let Some(row) = parent.downcast_ref::<HoverRow>() {
            row.fit_detail(width);
            return;
        }
    }
    set_width(age, width);
}

fn set_width(field: &NSTextField, width: f64) {
    let mut frame = field.frame();
    if (frame.size.width - width).abs() < 0.5 {
        return;
    }
    frame.size.width = width;
    field.setFrame(frame);
}

fn twin_label(src: &NSTextField, text: &str) -> Retained<NSTextField> {
    let field = NSTextField::initWithFrame(NSTextField::alloc(src.mtm()), src.frame());
    field.setStringValue(&NSString::from_str(text));
    field.setBezeled(false);
    field.setDrawsBackground(false);
    field.setEditable(false);
    field.setSelectable(false);
    field.setTextColor(src.textColor().as_deref());
    field.setFont(src.font().as_deref());
    field.setUsesSingleLineMode(src.usesSingleLineMode());
    field.setLineBreakMode(src.lineBreakMode());
    field
}

/// True when this view answers clicks itself.
fn keeps_click(view: *mut NSView) -> bool {
    let is_control: bool = unsafe { msg_send![view, isKindOfClass: NSControl::class()] };
    if !is_control {
        return false;
    }
    let action: Option<Sel> = unsafe { msg_send![view, action] };
    action.is_some()
}

/// A row that highlights on hover and toggles from anywhere inside it. The tint
/// is a plain filled box rather than a custom `drawRect:` — a non-opaque view
/// that paints its own background does not reliably erase what it painted last,
/// and the leftovers wiped out the group header sitting above the row.
pub fn hover_row(
    parent: &NSView,
    y: f64,
    height: f64,
    color: (f64, f64, f64),
    tag: isize,
    mtm: MainThreadMarker,
) -> Retained<HoverRow> {
    let width = panel_w();
    let frame = NSRect::new(NSPoint::new(0.0, y), NSSize::new(width, height));
    let row = mtm.alloc().set_ivars(HoverRowIvars {
        tint: make_tint(width, height, color, mtm),
        tag: Cell::new(tag),
        detail: RefCell::new(None),
    });
    let row: Retained<HoverRow> = unsafe { msg_send![super(row), initWithFrame: frame] };
    // Isolates the tint show/hide from the scroll view's backing store.
    row.setWantsLayer(true);
    // Added first so every label and control the caller adds lands on top of it.
    row.addSubview(&row.ivars().tint);
    parent.addSubview(&row);
    row
}

fn make_tint(
    width: f64,
    height: f64,
    color: (f64, f64, f64),
    mtm: MainThreadMarker,
) -> Retained<NSBox> {
    let tint = NSBox::initWithFrame(
        NSBox::alloc(mtm),
        NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(width.max(0.5), height.max(0.5))),
    );
    tint.setBoxType(NSBoxType::Custom);
    tint.setBorderWidth(0.0);
    tint.setFillColor(&Theme::color_alpha(color, 1.0));
    tint.setWantsLayer(true);
    tint.setHidden(true);
    tint
}
