// Settings and popover action helpers invoked from MenuHandler.
// Exports: free functions that mutate AppState / spawn work.
// Deps: crate::{autostart, popover, settings_view, state, tasks}, wd40::config.

use crate::popover;
use crate::settings_view;
use crate::state::{with_state, with_state_ret, UiScreen};
use crate::tasks;
use objc2_app_kit::{NSAlert, NSAlertStyle, NSApplication, NSButton};
use objc2_foundation::{MainThreadMarker, NSString};
use wd40::scanner::ArtifactGroup;

pub fn toggle_item(tag: isize, mtm: MainThreadMarker) {
    let index = (tag - crate::scan_view::TAG_ITEM_BASE) as usize;
    // Ticking a row is the smallest state change the app has; it repaints the
    // box and the button, and leaves the list exactly where it was.
    let intent = with_state_ret(|state| {
        let was_empty = state.selected.is_empty();
        state.toggle_selected(index);
        (was_empty, state.selected.is_empty())
    }).unwrap_or((false, true));
    tasks::reclaim_for_selection_intent(intent.0, intent.1);
    if !crate::live::selection_changed(mtm) {
        popover::refresh(mtm);
    }
}

pub fn done_ack(mtm: MainThreadMarker) {
    with_state(|state| {
        state.screen = UiScreen::Scan;
        state.done = None;
    });
    popover::refresh(mtm);
    tasks::start_scan();
}

pub fn show_more(mtm: MainThreadMarker) {
    with_state(|state| state.show_all = true);
    popover::refresh(mtm);
}

/// Both settings screens scroll, and they do not share a place in a list, so a
/// screen change starts at the top rather than at the other screen's offset.
pub fn open_settings(mtm: MainThreadMarker) {
    crate::scrolling::reset_scroll();
    settings_view::collapse_disclosure();
    with_state(|state| state.screen = UiScreen::Settings);
    popover::refresh(mtm);
}

/// Leave settings for the list. Deliberately no rescan: the controls that need
/// one start it themselves, and rescanning here would clear the ticks the user
/// had already made before they came in.
pub fn close_settings(mtm: MainThreadMarker) {
    crate::scrolling::reset_scroll();
    settings_view::collapse_disclosure();
    with_state(|state| state.screen = UiScreen::Scan);
    popover::refresh(mtm);
}

pub fn interval(sender: &NSButton, mtm: MainThreadMarker) {
    if let Some(hours) = settings_view::interval_for_tag(sender.tag()) {
        with_state(|state| {
            state.config.auto_clean_hours = hours;
            state.config.save();
        });
        if hours > 0 {
            tasks::start_auto_clean(hours);
        } else {
            tasks::stop_auto_clean();
        }
        settings_view::collapse_disclosure();
    } else {
        settings_view::toggle_disclosure(settings_view::SettingsDisclosure::AutoClean);
    }
    popover::refresh(mtm);
}

pub fn set_max_age(days: u64, mtm: MainThreadMarker) {
    with_state(|state| {
        state.config.max_age_days = days.min(30);
        state.config.save();
        state.reset_selection();
    });
    popover::refresh(mtm);
}

pub fn depth(sender: &NSButton, mtm: MainThreadMarker) {
    let Some(depth) = settings_view::depth_for_tag(sender.tag()) else {
        settings_view::toggle_disclosure(settings_view::SettingsDisclosure::ScanDepth);
        popover::refresh(mtm);
        return;
    };
    with_state(|state| {
        state.config.max_depth = depth;
        state.config.save();
    });
    settings_view::collapse_disclosure();
    popover::refresh(mtm);
    tasks::start_scan();
}

/// Switch one artifact group in or out of the scan. What is on screen came from
/// the old set, so the scan is redone rather than filtered.
pub fn toggle_scan_group(sender: &NSButton, mtm: MainThreadMarker) {
    let Some(group) = ArtifactGroup::from_tag(sender.tag() - settings_view::TAG_GROUP_BASE) else {
        return;
    };
    with_state(|state| {
        let on = state.config.scans(group);
        state.config.set_scans(group, !on);
        state.config.save();
    });
    popover::refresh(mtm);
    tasks::start_scan();
}

pub fn toggle_menu_bar_size(mtm: MainThreadMarker) {
    with_state(|state| {
        state.config.menu_bar_size = !state.config.menu_bar_size;
        state.config.save();
    });
    popover::refresh(mtm);
}

/// Add a folder to the scan roots. The panel is a window of its own, so the
/// popover dismisses itself while it is up; the new root is scanned straight
/// away and is there on the next open.
pub fn add_scan_root(mtm: MainThreadMarker) {
    let existing = with_state_ret(|state| state.config.scan_dirs.clone()).unwrap_or_default();
    let Some(path) = crate::settings_roots::choose_root(&existing, mtm) else { return };
    with_state(|state| {
        state.config.scan_dirs.push(path);
        state.config.save();
    });
    popover::refresh(mtm);
    tasks::start_scan();
}

pub fn remove_scan_root(sender: &NSButton, mtm: MainThreadMarker) {
    let index = (sender.tag() - settings_view::TAG_ROOT_BASE) as usize;
    with_state(|state| {
        if index >= state.config.scan_dirs.len() {
            return;
        }
        state.config.scan_dirs.remove(index);
        state.config.save();
    });
    popover::refresh(mtm);
    tasks::start_scan();
}

pub fn toggle_login(mtm: MainThreadMarker) {
    let enable = !crate::autostart::is_enabled();
    if let Err(err) = crate::autostart::set_enabled(enable) {
        show_alert(mtm, "Launch at Login", &err);
    }
    popover::refresh(mtm);
}

pub fn toggle_auto_update(mtm: MainThreadMarker) {
    with_state(|state| {
        if let Some(updater) = state.updater.as_ref() {
            updater.set_automatic_checks(!updater.automatic_checks());
        }
    });
    popover::refresh(mtm);
}

pub fn check_updates() {
    with_state(|state| {
        if let Some(updater) = state.updater.as_ref() {
            updater.check_now();
        }
    });
}

pub fn show_alert(mtm: MainThreadMarker, title: &str, body: &str) {
    let alert = NSAlert::new(mtm);
    alert.setMessageText(&NSString::from_str(title));
    alert.setInformativeText(&NSString::from_str(body));
    alert.setAlertStyle(NSAlertStyle::Informational);
    #[allow(deprecated)]
    NSApplication::sharedApplication(mtm).activateIgnoringOtherApps(true);
    alert.runModal();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disclosures_are_exclusive_and_only_choice_tags_are_accepted() {
        settings_view::collapse_disclosure();
        settings_view::toggle_disclosure(settings_view::SettingsDisclosure::AutoClean);
        assert_eq!(settings_view::active_disclosure(), Some(settings_view::SettingsDisclosure::AutoClean));
        settings_view::toggle_disclosure(settings_view::SettingsDisclosure::ScanDepth);
        assert_eq!(settings_view::active_disclosure(), Some(settings_view::SettingsDisclosure::ScanDepth));
        settings_view::toggle_disclosure(settings_view::SettingsDisclosure::ScanDepth);
        assert_eq!(settings_view::active_disclosure(), None);
        assert_eq!(settings_view::interval_for_tag(settings_view::TAG_INTERVAL_BASE + 12), Some(12));
        assert_eq!(settings_view::interval_for_tag(12), None);
        assert_eq!(settings_view::depth_for_tag(settings_view::TAG_DEPTH_BASE + 8), Some(8));
        assert_eq!(settings_view::depth_for_tag(settings_view::TAG_DEPTH_BASE + 5), None);
        assert_eq!(settings_view::depth_for_tag(5), None);
    }
}
