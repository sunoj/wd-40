// The window's scan picture: every target as a tile on the plate, sized by its
// bytes and coloured by its group, solid when ticked and faint when not. A
// click ticks or unticks a tile; the line under the map names the one hovered.
// The cleaning plate lays the same tiles out, so scan to clean is one picture.
// Exports: `usage_map`, `MapView`, `entries`, `draw_key`.

use crate::crust::{group_tone, paint_steel};
use crate::header::scan_size;
use crate::names::{display_names, display_path};
use crate::state::AppState;
use crate::tile::{patches, tile_view, Entry, Patch, TileView};
use crate::treemap::{contains, IDLE, PENDING};
use crate::theme::Theme;
use crate::widgets::{add_fill, fitted_width, label, retrack};
use objc2::rc::Retained;
use objc2::{define_class, msg_send, DefinedClass, MainThreadOnly};
use objc2_app_kit::{NSEvent, NSTextField, NSTrackingAreaOptions, NSView};
use objc2_foundation::{MainThreadMarker, NSRect, NSString};
use std::cell::{Cell, OnceCell};
use std::collections::HashSet;
use wd40::scanner::ArtifactGroup;

/// Every target the scan found, in the state's own order, as the map shows it.
/// A target that has not been measured yet has no area to draw.
pub fn entries(state: &AppState) -> Vec<Entry> {
    let names = display_names(&state.targets);
    state
        .targets
        .iter()
        .enumerate()
        .map(|(index, target)| Entry {
            name: names[index].clone(),
            path: display_path(&target.path),
            bytes: if state.measured.contains(&index) { target.size_bytes } else { 0 },
            tone: group_tone(target.kind.group()),
        })
        .collect()
}

pub struct MapIvars {
    patches: Vec<Patch>,
    tiles: OnceCell<Vec<Retained<TileView>>>,
    /// The line under the map: the hovered tile, or `idle` when there is none.
    legend: Retained<NSTextField>,
    idle: String,
    hover: Cell<Option<usize>>,
    dark: bool,
}

define_class!(
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[ivars = MapIvars]
    #[name = "WD40UsageMap"]
    pub struct MapView;

    impl MapView {
        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            let footprints: Vec<NSRect> = self.ivars().patches.iter().map(|patch| patch.rect).collect();
            paint_steel(self.bounds(), self.ivars().dark, &footprints);
        }

        /// A tile standing for one target ticks it, exactly as its row does.
        /// The shared tail tile is too many things to tick with one click.
        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, event: &NSEvent) {
            let Some(patch) = self.hit(event).map(|index| &self.ivars().patches[index]) else { return };
            if let [target] = patch.members.as_slice() {
                let tag = crate::scan_view::TAG_ITEM_BASE + *target as isize;
                crate::actions::toggle_item(tag, self.mtm());
            }
        }

        #[unsafe(method(mouseMoved:))]
        fn mouse_moved(&self, event: &NSEvent) {
            self.point_at(self.hit(event));
        }

        #[unsafe(method(mouseExited:))]
        fn mouse_exited(&self, _event: &NSEvent) {
            self.point_at(None);
        }

        #[unsafe(method(acceptsFirstMouse:))]
        fn accepts_first_mouse(&self, _event: *mut NSEvent) -> bool {
            true
        }

        #[unsafe(method(updateTrackingAreas))]
        fn update_tracking_areas(&self) {
            let _: () = unsafe { msg_send![super(self), updateTrackingAreas] };
            retrack(self, NSTrackingAreaOptions::MouseMoved);
        }
    }
);

impl MapView {
    /// Show which tiles are ticked. Only the tiles that changed are touched.
    pub fn sync(&self, selected: &HashSet<usize>) {
        let Some(tiles) = self.ivars().tiles.get() else { return };
        for (patch, tile) in self.ivars().patches.iter().zip(tiles) {
            tile.show(ticked(patch, selected));
        }
    }

    fn hit(&self, event: &NSEvent) -> Option<usize> {
        let point = self.convertPoint_fromView(event.locationInWindow(), None);
        self.ivars().patches.iter().position(|patch| contains(patch.rect, point))
    }

    fn point_at(&self, index: Option<usize>) {
        if self.ivars().hover.replace(index) == index {
            return;
        }
        let text = match index.map(|index| &self.ivars().patches[index]) {
            Some(patch) => patch.tip(),
            None => self.ivars().idle.clone(),
        };
        self.ivars().legend.setStringValue(&NSString::from_str(&text));
    }
}

/// A tile is solid when everything it stands for is ticked.
fn ticked(patch: &Patch, selected: &HashSet<usize>) -> u8 {
    match patch.members.iter().all(|index| selected.contains(index)) {
        true => PENDING,
        false => IDLE,
    }
}

/// The map of `state` in `frame`, with `legend` as the line it speaks through.
pub fn usage_map(
    frame: NSRect,
    state: &AppState,
    legend: Retained<NSTextField>,
    dark: bool,
    mtm: MainThreadMarker,
) -> Retained<MapView> {
    let laid = patches(&entries(state), frame.size);
    let idle = match laid.is_empty() {
        true => "Nothing found to map".to_string(),
        false => format!(
            "{} targets \u{00b7} {} \u{00b7} click a tile to tick it",
            state.targets.len(),
            scan_size(state.total_size()),
        ),
    };
    legend.setStringValue(&NSString::from_str(&idle));
    let tiles: Vec<Retained<TileView>> = laid
        .iter()
        .enumerate()
        .map(|(index, patch)| tile_view(index, patch, ticked(patch, &state.selected), mtm))
        .collect();
    let view = mtm.alloc::<MapView>().set_ivars(MapIvars {
        patches: laid,
        tiles: OnceCell::new(),
        legend,
        idle,
        hover: Cell::new(None),
        dark,
    });
    let view: Retained<MapView> = unsafe { msg_send![super(view), initWithFrame: frame] };
    view.setWantsLayer(true);
    for tile in &tiles {
        view.addSubview(tile);
    }
    let _ = view.ivars().tiles.set(tiles);
    view
}

/// The colour key above the map: a swatch and a title for each group present.
pub fn draw_key(root: &NSView, state: &AppState, map: NSRect, theme: &Theme, mtm: MainThreadMarker) {
    let y = map.origin.y + map.size.height + 8.0;
    let mut x = map.origin.x;
    for &group in ArtifactGroup::ALL {
        if !state.targets.iter().any(|target| target.kind.group() == group) {
            continue;
        }
        let title = ArtifactGroup::title(group);
        let width = fitted_width(title, 11.5, false, mtm);
        add_fill(root, x, y + 4.0, 9.0, 9.0, group_tone(group), 1.0, 2.0, mtm);
        label(root, title, x + 14.0, y, width + 2.0, 16.0, 11.5, false, theme.ink_2, false, mtm);
        x += 14.0 + width + 18.0;
    }
}
