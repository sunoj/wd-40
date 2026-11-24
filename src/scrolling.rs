// The list's scroll position, and the scroll view it belongs to. Ticking a row
// used to rebuild the whole tree and send the list back to the top, so you had
// to find your place again before ticking the next one; this remembers it.
// Exports: `remember_scroll`, `reset_scroll`, `scroll_document_view`.
// Deps: objc2 AppKit, crate::widgets.

use objc2::rc::Retained;
use objc2::MainThreadOnly;
use objc2_app_kit::{NSBorderType, NSScrollView, NSView};
use objc2_foundation::{MainThreadMarker, NSPoint, NSRect, NSSize};

thread_local! {
    /// The live scroll view, so its position can be read before a rebuild.
    static SCROLL: std::cell::RefCell<Option<Retained<NSScrollView>>> =
        const { std::cell::RefCell::new(None) };
    /// How far the list is scrolled from the top, in points.
    static SCROLL_TOP: std::cell::Cell<f64> = const { std::cell::Cell::new(0.0) };
}

/// Record where the list is scrolled to. Ticking a row rebuilds the whole view
/// tree, and without this the list jumped back to the top every time — you had
/// to find your place and scroll down again before ticking the next row.
pub fn remember_scroll() {
    SCROLL.with(|cell| {
        let Some(scroll) = cell.borrow().clone() else { return };
        let clip = scroll.contentView();
        let doc_h = scroll.documentView().map_or(0.0, |d| d.frame().size.height);
        let visible_h = clip.bounds().size.height;
        SCROLL_TOP.set(((doc_h - visible_h) - clip.bounds().origin.y).max(0.0));
    });
}

/// Send the next list back to the top — after a rescan the old offset would
/// point into rows that no longer exist.
pub fn reset_scroll() {
    SCROLL_TOP.set(0.0);
    crate::group_scroll::forget();
}

#[allow(clippy::too_many_arguments)]
pub fn scroll_document_view(
    parent: &NSView,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    document_h: f64,
    mtm: MainThreadMarker,
) -> Retained<NSView> {
    let scroll = NSScrollView::initWithFrame(
        NSScrollView::alloc(mtm),
        NSRect::new(NSPoint::new(x, y), NSSize::new(w, h)),
    );
    scroll.setBorderType(NSBorderType::NoBorder);
    scroll.setHasHorizontalScroller(false);
    scroll.setHasVerticalScroller(true);
    scroll.setAutohidesScrollers(true);
    scroll.setDrawsBackground(false);
    let document = NSView::initWithFrame(
        NSView::alloc(mtm),
        NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(w, document_h.max(h))),
    );
    scroll.setDocumentView(Some(&document));
    parent.addSubview(&scroll);
    // AppKit's origin is bottom-left, so an untouched scroll view opens showing
    // the end of the document. Park it where the list was last left instead —
    // at the top on a fresh list, since SCROLL_TOP starts at zero.
    let top_of_document = document_h.max(h) - h;
    let origin_y = (top_of_document - SCROLL_TOP.get()).clamp(0.0, top_of_document);
    let content = scroll.contentView();
    content.setBoundsOrigin(NSPoint::new(0.0, origin_y));
    scroll.reflectScrolledClipView(&content);
    SCROLL.with(|cell| *cell.borrow_mut() = Some(scroll));
    document
}
