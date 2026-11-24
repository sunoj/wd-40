// A slice of a screen that can be emptied and drawn again on its own, at the
// pane width it was first built for. The live patches repaint zones instead of
// rebuilding the screen. Exports: `Zone`. Deps: crate::layout, objc2 AppKit.

use crate::layout::{panel_w, with_width};
use objc2::rc::Retained;
use objc2::MainThreadOnly;
use objc2_app_kit::NSView;
use objc2_foundation::{MainThreadMarker, NSPoint, NSRect, NSSize};

/// A slice of a screen that is cheap to draw whole — a dozen views, against
/// several hundred for the screen. Its frame is its own region, so the drawing
/// code inside it keeps the coordinates it already used, and it remembers the
/// pane width it was built for so a repaint lays out exactly as the build did.
pub struct Zone {
    view: Retained<NSView>,
    top: f64,
    width: f64,
}

impl Zone {
    /// A zone `height` tall at the current pane width, with its corner at (x, y).
    pub fn new(parent: &NSView, x: f64, y: f64, height: f64, mtm: MainThreadMarker) -> Self {
        let width = panel_w();
        let view = NSView::initWithFrame(
            NSView::alloc(mtm),
            NSRect::new(NSPoint::new(x, y), NSSize::new(width, height)),
        );
        parent.addSubview(&view);
        Self { view, top: height, width }
    }

    /// Draw into the zone for the first time. `draw` is handed the zone's view
    /// and its own top edge, which is what drawing code counts down from.
    pub fn paint(&self, draw: impl FnOnce(&NSView, f64)) {
        with_width(self.width, || draw(&self.view, self.top));
    }

    /// How tall the zone was built; a repaint cannot change it.
    pub fn height(&self) -> f64 {
        self.top
    }

    /// Empty it and draw it again.
    pub fn repaint(&self, draw: impl FnOnce(&NSView, f64)) {
        for view in self.view.subviews().to_vec() {
            view.removeFromSuperview();
        }
        self.paint(draw);
    }
}
