// A group's rows when there are too many to show whole: a scroll view of fixed
// height inside the list, so one large group cannot swallow it. It scrolls
// until it reaches an end and then hands the wheel to the list around it, and
// it fades the edge that has more rows beyond it.
// Exports: `GROUP_CAP`, `capped`, `forget`. Deps: crate::{theme, widgets}.

use crate::layout::panel_w;
use crate::theme::Theme;
use crate::widgets::add_fill;
use objc2::rc::Retained;
use objc2::{define_class, msg_send, DefinedClass, MainThreadOnly};
use objc2_app_kit::{NSBorderType, NSClipView, NSEvent, NSScrollElasticity, NSScrollView, NSView};
use objc2_foundation::{MainThreadMarker, NSPoint, NSRect, NSSize};
use std::cell::RefCell;
use std::collections::HashMap;

/// Rows a group shows at once before its own scrolling takes over.
pub const GROUP_CAP: usize = 7;
/// Height of the fade over an edge with more rows past it.
const CUE_H: f64 = 16.0;

thread_local! {
    /// How far each group is scrolled from its top, by group key, so a rebuild
    /// puts every group back where it was left.
    static OFFSETS: RefCell<HashMap<&'static str, f64>> = RefCell::new(HashMap::new());
    /// The capped groups on show, for the debug run that scrolls them.
    static LIVE: RefCell<Vec<Retained<GroupScroll>>> = const { RefCell::new(Vec::new()) };
}

define_class!(
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "WD40ScrollCue"]
    struct CueView;

    impl CueView {
        /// A fade is paint: the row under it keeps its clicks and its hover.
        #[unsafe(method(hitTest:))]
        fn hit_test(&self, _point: NSPoint) -> *mut NSView {
            std::ptr::null_mut()
        }
    }
);

pub struct GroupIvars {
    key: &'static str,
    top: Retained<CueView>,
    bottom: Retained<CueView>,
    /// False until the remembered offset has been put back. AppKit parks a
    /// scroll view at its document's bottom as it joins a window, and that
    /// must be neither shown nor remembered.
    placed: std::cell::Cell<bool>,
}

define_class!(
    #[unsafe(super(NSScrollView))]
    #[thread_kind = MainThreadOnly]
    #[ivars = GroupIvars]
    #[name = "WD40GroupScroll"]
    pub struct GroupScroll;

    impl GroupScroll {
        /// Scroll the group while it has somewhere to go in that direction;
        /// at an end, the wheel belongs to the list the group sits in.
        #[unsafe(method(scrollWheel:))]
        fn scroll_wheel(&self, event: &NSEvent) {
            let delta = event.scrollingDeltaY();
            let (above, below) = self.room();
            let spent = delta == 0.0 || (delta > 0.0 && above <= 0.5) || (delta < 0.0 && below <= 0.5);
            match (spent, unsafe { self.nextResponder() }) {
                (true, Some(outer)) => outer.scrollWheel(event),
                _ => unsafe { msg_send![super(self), scrollWheel: event] },
            }
        }

        #[unsafe(method(viewDidMoveToWindow))]
        fn moved_to_window(&self) {
            if self.window().is_some() {
                self.place();
            }
        }

        #[unsafe(method(reflectScrolledClipView:))]
        fn reflect(&self, clip: &NSClipView) {
            let _: () = unsafe { msg_send![super(self), reflectScrolledClipView: clip] };
            self.settle();
        }
    }
);

impl GroupScroll {
    /// Rows hidden above and below what is on show, in points.
    pub fn room(&self) -> (f64, f64) {
        let visible = self.contentView().bounds();
        let all = self.documentView().map_or(0.0, |document| document.frame().size.height);
        ((all - visible.size.height - visible.origin.y).max(0.0), visible.origin.y.max(0.0))
    }

    /// Show a fade at each edge that has more past it, and note the offset.
    fn settle(&self) {
        if !self.ivars().placed.get() {
            return;
        }
        let (above, below) = self.room();
        self.ivars().top.setHidden(above <= 0.5);
        self.ivars().bottom.setHidden(below <= 0.5);
        OFFSETS.with(|cell| cell.borrow_mut().insert(self.ivars().key, above));
    }
}

impl GroupScroll {
    /// Put the group where it was last left: at its top, the first time.
    fn place(&self) {
        let above = OFFSETS.with(|cell| cell.borrow().get(self.ivars().key).copied().unwrap_or(0.0));
        let (hidden_above, hidden_below) = self.room();
        let most = hidden_above + hidden_below;
        let clip = self.contentView();
        clip.setBoundsOrigin(NSPoint::new(0.0, (most - above).clamp(0.0, most)));
        self.ivars().placed.set(true);
        self.reflectScrolledClipView(&clip);
    }
}

/// Send every group back to its top: after a rescan the old offsets point at
/// rows that are no longer there.
pub fn forget() {
    OFFSETS.with(|cell| cell.borrow_mut().clear());
}

/// The capped groups now on show, in list order.
#[cfg(debug_assertions)]
pub fn live() -> Vec<Retained<GroupScroll>> {
    LIVE.with(|cell| cell.borrow().iter().filter(|group| group.window().is_some()).cloned().collect())
}

/// A scroll view `visible_h` tall with its top at `y_top` in `parent`, over a
/// document `document_h` tall, which is returned for the rows to be drawn in.
pub fn capped(
    parent: &NSView,
    y_top: f64,
    visible_h: f64,
    document_h: f64,
    key: &'static str,
    theme: &Theme,
    mtm: MainThreadMarker,
) -> Retained<NSView> {
    let (width, y) = (panel_w(), y_top - visible_h);
    let cue = |edge_y: f64, rising: bool| -> Retained<CueView> {
        let frame = NSRect::new(NSPoint::new(0.0, edge_y), NSSize::new(width, CUE_H));
        let view: Retained<CueView> = unsafe { msg_send![CueView::alloc(mtm), initWithFrame: frame] };
        // Four strips stand in for a gradient: opaque at the edge, clear inside.
        for (strip, alpha) in [0.85, 0.6, 0.35, 0.15].into_iter().enumerate() {
            let at = if rising { strip as f64 } else { 3.0 - strip as f64 };
            add_fill(&view, 0.0, at * 4.0, width, 4.0, theme.surface, alpha, 0.0, mtm);
        }
        view
    };
    let (top, bottom) = (cue(y + visible_h - CUE_H, false), cue(y, true));
    let scroll = mtm.alloc::<GroupScroll>().set_ivars(GroupIvars { key, top: top.clone(), bottom: bottom.clone(), placed: std::cell::Cell::new(false) });
    let frame = NSRect::new(NSPoint::new(0.0, y), NSSize::new(width, visible_h));
    let scroll: Retained<GroupScroll> = unsafe { msg_send![super(scroll), initWithFrame: frame] };
    scroll.setBorderType(NSBorderType::NoBorder);
    scroll.setHasVerticalScroller(true);
    scroll.setAutohidesScrollers(true);
    scroll.setDrawsBackground(false);
    // No rubber band: at an end the wheel goes to the outer list instead.
    scroll.setVerticalScrollElasticity(NSScrollElasticity::None);
    let document = NSView::initWithFrame(
        NSView::alloc(mtm),
        NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(width, document_h)),
    );
    scroll.setDocumentView(Some(&document));
    parent.addSubview(&scroll);
    parent.addSubview(&top);
    parent.addSubview(&bottom);
    scroll.place();
    LIVE.with(|cell| {
        let mut live = cell.borrow_mut();
        // Keep this build's groups; drop the ones a rebuild has left behind.
        live.retain(|group| unsafe { group.superview() }.is_some_and(|holder| std::ptr::eq(&*holder, parent)));
        live.push(scroll);
    });
    document
}
