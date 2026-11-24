// The settings screen's "roots" block: the folders the walk starts from, what
// each one is currently holding, and the two ways to change the list.
// Exports: `draw_roots`, `add_root`, `remove_root`.
// Deps: objc2 AppKit (NSOpenPanel); crate::{controls, names, state, widgets}.

use crate::layout::{content_w};
use crate::controls::text_button;
use crate::state::AppState;
use crate::theme::Theme;
use crate::widgets::{self, add_fill, fitted_width, label, label_right, PAD_X};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::sel;
use objc2_app_kit::{NSApplication, NSModalResponse, NSOpenPanel, NSView};
use objc2_foundation::{MainThreadMarker, NSString};
use std::path::{Path, PathBuf};

const BOX_H: f64 = 32.0;
const ROOT_ROW_H: f64 = 38.0;
const ADD_ROW_H: f64 = 30.0;
/// NSModalResponseOK, which objc2-app-kit does not export as a constant.
const MODAL_OK: NSModalResponse = 1;

pub fn draw_roots(
    parent: &NSView,
    mut y: f64,
    state: &AppState,
    theme: &Theme,
    target: &AnyObject,
    mtm: MainThreadMarker,
) -> f64 {
    y = crate::settings_row::section(parent, y, "ROOTS", theme, mtm);
    for (index, dir) in state.config.scan_dirs.iter().enumerate() {
        draw_root(parent, y, index, dir, state, theme, target, mtm);
        y -= ROOT_ROW_H;
    }
    widgets::symbol_view(parent, "plus", PAD_X + 1.0, y - 18.0, 12.0, theme.ink_2, mtm);
    let add = fitted_width("Add folder\u{2026}", 12.5, false, mtm) + 4.0;
    text_button(
        parent, "Add folder\u{2026}", PAD_X + 18.0, y - 22.0, add, sel!(settingsAddRoot:), target, 0,
        theme.ink_2, mtm,
    );
    y - ADD_ROW_H
}

#[allow(clippy::too_many_arguments)]
fn draw_root(
    parent: &NSView,
    y_top: f64,
    index: usize,
    dir: &Path,
    state: &AppState,
    theme: &Theme,
    target: &AnyObject,
    mtm: MainThreadMarker,
) {
    let y = y_top - BOX_H;
    let frame = add_fill(parent, PAD_X, y, content_w(), BOX_H, theme.surface, 1.0, 7.0, mtm);
    frame.setBorderWidth(1.0);
    frame.setBorderColor(&Theme::color(theme.line));
    let path = crate::names::display_path(dir);
    label(parent, &path, PAD_X + 10.0, y + 8.0, 180.0, 16.0, 12.0, false, theme.ink, true, mtm);
    let found = holding(state, dir);
    label_right(parent, &found, PAD_X + 190.0, y + 9.0, content_w() - 190.0 - 28.0, 15.0, 11.0, theme.ink_3, true, mtm);
    text_button(
        parent, "\u{2715}", PAD_X + content_w() - 26.0, y + 5.0, 20.0, sel!(settingsRemoveRoot:),
        target, crate::settings_view::TAG_ROOT_BASE + index as isize, theme.ink_4, mtm,
    );
}

/// How many of the targets on screen came from under this root. Roots collected
/// by name — /tmp, the cargo target root, the dev caches — belong to no scan
/// root and are counted under none of them.
fn holding(state: &AppState, dir: &Path) -> String {
    let count = state.targets.iter().filter(|target| target.path.starts_with(dir)).count();
    match count {
        1 => "1 target".to_string(),
        count => format!("{count} targets"),
    }
}

/// Ask for a folder and add it to the scan roots. Returns the chosen path, or
/// `None` when the panel was cancelled or the folder is already a root.
pub fn choose_root(existing: &[PathBuf], mtm: MainThreadMarker) -> Option<PathBuf> {
    let panel = NSOpenPanel::openPanel(mtm);
    panel.setCanChooseDirectories(true);
    panel.setCanChooseFiles(false);
    panel.setAllowsMultipleSelection(false);
    panel.setMessage(Some(&NSString::from_str("Choose a folder for WD-40 to scan")));
    panel.setPrompt(Some(&NSString::from_str("Add")));
    #[allow(deprecated)]
    NSApplication::sharedApplication(mtm).activateIgnoringOtherApps(true);
    if panel.runModal() != MODAL_OK {
        return None;
    }
    let url: Retained<objc2_foundation::NSURL> = panel.URLs().firstObject()?;
    let path = PathBuf::from(url.path()?.to_string());
    (!existing.iter().any(|root| root == &path)).then_some(path)
}
