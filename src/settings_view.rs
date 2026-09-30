// Settings screen inside the 380pt popover: what is scanned, what is kept, and
// where the app looks. Every control here changes something the scan does.
// Exports: `build`, disclosure state/actions, choice tag decoders, and row tags.
// Deps: crate::{controls, header, scrolling, settings_row, state, theme, widgets}.

use crate::controls::days_slider;
use crate::header::scan_size;
use crate::settings_chrome::{draw_brand, draw_footer, BRAND_H, FOOTER_H};
use crate::settings_row::{
    choice_list_height, disclosure_row, divider, group_row, section, switch_row, value_row,
    ChoiceSpec, GroupSpec, Spec,
};
use crate::state::AppState;
use crate::theme::Theme;
use crate::widgets::{
    self, label, label_right, CONTENT_WIDTH, PAD_X, POPOVER_WIDTH,
};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::sel;
use objc2_app_kit::NSView;
use objc2_foundation::{MainThreadMarker, NSPoint};
use std::cell::Cell;
use wd40::scanner::ArtifactGroup;

/// Base tags for the two kinds of row an action has to identify.
pub const TAG_GROUP_BASE: isize = 2200;
pub const TAG_ROOT_BASE: isize = 2300;
pub(crate) const TAG_INTERVAL_BASE: isize = 2400;
pub(crate) const TAG_DEPTH_BASE: isize = 2500;

const MAX_HEIGHT: f64 = 540.0;
/// Room for every section at once. What the body actually uses is measured off
/// the drawing rather than predicted by a second copy of the layout.
const SHEET_H: f64 = 1400.0;

const INTERVALS: [(u64, &str); 5] = [(0, "Off"), (1, "1h"), (6, "6h"), (12, "12h"), (24, "1d")];
const DEPTHS: [(usize, &str); 4] = [(4, "4 levels"), (6, "6 levels"), (8, "8 levels"), (10, "10 levels")];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingsDisclosure { AutoClean, ScanDepth }

thread_local! {
    static OPEN_DISCLOSURE: Cell<Option<SettingsDisclosure>> = const { Cell::new(None) };
}

pub fn build(
    state: &AppState,
    theme: &Theme,
    target: &AnyObject,
    mtm: MainThreadMarker,
) -> (Retained<NSView>, f64) {
    let sheet = widgets::root_view(SHEET_H, theme.surface, mtm);
    let used = SHEET_H - draw_body(&sheet, SHEET_H, state, theme, target, mtm);

    let body_limit = MAX_HEIGHT - BRAND_H - FOOTER_H + disclosure_height();
    let body_h = used.min(body_limit);
    let height = BRAND_H + body_h + FOOTER_H;
    let root = widgets::root_view(height, theme.surface, mtm);
    draw_brand(&root, height, theme, mtm);
    let document =
        crate::scrolling::scroll_document_view(&root, 0.0, FOOTER_H, POPOVER_WIDTH, body_h, used, mtm);
    // The sheet is taller than what it drew, so hang it from the document's top
    // edge and let the empty tail fall below the scroller.
    sheet.setFrameOrigin(NSPoint::new(0.0, document.frame().size.height - SHEET_H));
    document.addSubview(&sheet);
    draw_footer(&root, theme, target, mtm);
    (root, height)
}

fn draw_body(
    sheet: &NSView,
    mut y: f64,
    state: &AppState,
    theme: &Theme,
    target: &AnyObject,
    mtm: MainThreadMarker,
) -> f64 {
    y = draw_scanning(sheet, y, state, theme, target, mtm);
    y = divider(sheet, y, theme, mtm);
    y = draw_groups(sheet, y, state, theme, target, mtm);
    y = divider(sheet, y, theme, mtm);
    y = draw_safety(sheet, y, state, theme, target, mtm);
    y = divider(sheet, y, theme, mtm);
    y = crate::settings_roots::draw_roots(sheet, y, state, theme, target, mtm);
    y = divider(sheet, y, theme, mtm);
    y = draw_application(sheet, y, state, theme, target, mtm);
    y - 10.0
}

fn draw_scanning(
    parent: &NSView, mut y: f64, state: &AppState,
    theme: &Theme, target: &AnyObject, mtm: MainThreadMarker,
) -> f64 {
    y = section(parent, y, "SCANNING", theme, mtm);
    let open = open_disclosure();
    let interval_choices = INTERVALS.map(|(hours, title)| ChoiceSpec {
        title,
        selected: state.config.auto_clean_hours == hours,
        action: sel!(settingsInterval:),
        tag: TAG_INTERVAL_BASE + hours as isize,
    });
    y = disclosure_row(
        parent, y,
        &Spec {
            title: "Auto clean",
            hint: auto_clean_hint(state.config.auto_clean_hours, state.config.max_age_days),
            action: sel!(settingsInterval:),
        },
        interval_label(state.config.auto_clean_hours), open == Some(SettingsDisclosure::AutoClean),
        &interval_choices, theme, target, mtm,
    );
    let depth = format!("{} levels", state.config.max_depth);
    let depth_choices = DEPTHS.map(|(depth, title)| ChoiceSpec {
        title,
        selected: state.config.max_depth == depth,
        action: sel!(settingsDepth:),
        tag: TAG_DEPTH_BASE + depth as isize,
    });
    y = disclosure_row(
        parent, y,
        &Spec {
            title: "Scan depth",
            hint: Some("How far below each root to look"),
            action: sel!(settingsDepth:),
        },
        &depth, open == Some(SettingsDisclosure::ScanDepth), &depth_choices, theme, target, mtm,
    );
    switch_row(
        parent, y,
        &Spec {
            title: "Show size in menu bar",
            hint: Some("The reclaimable total beside the glyph"),
            action: sel!(settingsToggleMenuBarSize:),
        },
        state.config.menu_bar_size, theme, target, mtm,
    )
}

fn draw_groups(
    parent: &NSView,
    mut y: f64,
    state: &AppState,
    theme: &Theme,
    target: &AnyObject,
    mtm: MainThreadMarker,
) -> f64 {
    y = section(parent, y, "WHAT TO SCAN", theme, mtm);
    for &group in ArtifactGroup::ALL {
        let size = group_total(state, group);
        y = group_row(
            parent, y,
            &GroupSpec {
                symbol: group.symbol(),
                title: group.title(),
                size: &size,
                on: state.config.scans(group),
                action: sel!(settingsToggleGroup:),
                tag: TAG_GROUP_BASE + group.tag(),
            },
            theme, target, mtm,
        );
    }
    y
}

/// What a group accounts for, from the scan that is running or has finished.
///
/// A group that is switched off was never looked for, and says so rather than
/// reporting the nothing it found. A group switched back on has no figure until
/// the rescan reaches it, and a total still being added to is drawn as a floor —
/// the same three states the scan list's group headers use.
fn group_total(state: &AppState, group: ArtifactGroup) -> String {
    if !state.config.scans(group) {
        return "\u{2014}".to_string();
    }
    let bytes = state.group_size(group);
    let settled = state.group_settled(group) && !crate::tasks::is_busy();
    if !settled && bytes == 0 {
        return "\u{2026}".to_string();
    }
    let floor = if settled { "" } else { "\u{2265} " };
    format!("{floor}{}", scan_size(bytes))
}

fn draw_safety(
    parent: &NSView,
    mut y: f64,
    state: &AppState,
    theme: &Theme,
    target: &AnyObject,
    mtm: MainThreadMarker,
) -> f64 {
    y = section(parent, y, "SAFETY", theme, mtm);
    let days = state.config.max_age_days;
    label(parent, "Keep builds newer than", PAD_X, y - 20.0, 220.0, 16.0, 13.5, false, theme.ink, false, mtm);
    let age = if days == 1 { "1 day".to_string() } else { format!("{days} days") };
    label_right(parent, &age, PAD_X + 200.0, y - 20.0, CONTENT_WIDTH - 200.0, 16.0, 12.5, theme.ink, true, mtm);
    days_slider(parent, days, PAD_X, y - 48.0, CONTENT_WIDTH, sel!(settingsSetMaxAge:), target, 0, mtm);
    label(parent, "0d", PAD_X, y - 66.0, 40.0, 14.0, 10.5, false, theme.ink_4, true, mtm);
    label_right(parent, "30d", PAD_X + CONTENT_WIDTH - 40.0, y - 66.0, 40.0, 14.0, 10.5, theme.ink_4, true, mtm);
    y - 78.0
}

fn draw_application(
    parent: &NSView,
    mut y: f64,
    state: &AppState,
    theme: &Theme,
    target: &AnyObject,
    mtm: MainThreadMarker,
) -> f64 {
    y = section(parent, y, "APPLICATION", theme, mtm);
    y = switch_row(
        parent, y,
        &Spec { title: "Launch at login", hint: None, action: sel!(settingsToggleLoginItem:) },
        crate::autostart::is_enabled(), theme, target, mtm,
    );
    let Some(updater) = state.updater.as_ref() else { return y };
    y = switch_row(
        parent, y,
        &Spec {
            title: "Automatic updates",
            hint: Some("Check for a new version in the background"),
            action: sel!(settingsToggleAutoUpdate:),
        },
        updater.automatic_checks(), theme, target, mtm,
    );
    value_row(
        parent, y,
        &Spec { title: "Check for updates\u{2026}", hint: None, action: sel!(settingsCheckForUpdates:) },
        None, theme, target, mtm,
    )
}

fn interval_label(hours: u64) -> &'static str {
    INTERVALS.iter().find(|(h, _)| *h == hours).map(|(_, l)| *l).unwrap_or("custom")
}

fn auto_clean_hint(hours: u64, max_age_days: u64) -> Option<&'static str> {
    (hours > 0 && max_age_days == 0).then_some("Paused: set keep-days above 0")
}

pub fn toggle_disclosure(disclosure: SettingsDisclosure) {
    OPEN_DISCLOSURE.with(|open| {
        let next = (open.get() != Some(disclosure)).then_some(disclosure);
        open.set(next);
    });
}

pub fn collapse_disclosure() { OPEN_DISCLOSURE.set(None); }

pub fn interval_for_tag(tag: isize) -> Option<u64> {
    INTERVALS.iter().find(|(hours, _)| TAG_INTERVAL_BASE + *hours as isize == tag).map(|&(hours, _)| hours)
}

pub fn depth_for_tag(tag: isize) -> Option<usize> {
    DEPTHS.iter().find(|(depth, _)| TAG_DEPTH_BASE + *depth as isize == tag).map(|&(depth, _)| depth)
}

fn open_disclosure() -> Option<SettingsDisclosure> { OPEN_DISCLOSURE.get() }

fn disclosure_height() -> f64 {
    match open_disclosure() {
        Some(SettingsDisclosure::AutoClean) => choice_list_height(INTERVALS.len()),
        Some(SettingsDisclosure::ScanDepth) => choice_list_height(DEPTHS.len()),
        None => 0.0,
    }
}

#[cfg(test)]
pub fn active_disclosure() -> Option<SettingsDisclosure> { open_disclosure() }

#[cfg(test)]
mod tests {
    use super::auto_clean_hint;

    #[test]
    fn auto_clean_pause_hint_only_appears_when_enabled_without_a_threshold() {
        assert_eq!(auto_clean_hint(1, 0), Some("Paused: set keep-days above 0"));
        assert_eq!(auto_clean_hint(0, 0), None);
        assert_eq!(auto_clean_hint(1, 7), None);
    }
}
