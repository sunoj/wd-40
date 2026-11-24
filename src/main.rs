// Entry point for the macOS WD-40 menu bar app.
// Owns the status item, app state, and Objective-C action handler.
// Deps: objc2 AppKit; UI in `surface` (popover or window) + views; work in `tasks`.

mod actions;
mod auto_clean;
mod autostart;
mod cache_names;
mod can;
#[cfg(debug_assertions)]
mod capture;
#[cfg(debug_assertions)]
mod capture_group;
mod checkbox;
mod choreo;
mod clean_list;
mod clean_view;
mod controls;
mod crust;
mod disk_gauge;
mod done_view;
mod group_scroll;
mod header;
mod header_legend;
mod hover_row;
mod icon;
mod layout;
mod live;
mod medal;
mod menu_rows;
mod metal;
mod motion;
mod mainthread;
mod names;
mod pace;
mod plate;
mod popover;
mod reveal;
mod scan_calm;
mod scan_footer;
mod scan_rows;
mod scan_verdict;
mod scrolling;
mod scan_view;
#[cfg(debug_assertions)]
mod screenshot;
mod selection;
mod settings_roots;
mod settings_row;
mod settings_view;
mod settings_chrome;
mod settings_frame;
#[cfg(debug_assertions)]
mod smoke;
mod startup;
mod state;
mod status;
mod status_menu;
mod style;
mod surface;
mod tasks;
mod tasks_clean;
mod theme;
mod tile;
mod trace;
mod treemap;
mod updater;
mod usage_map;
mod widgets;
mod window;
mod zone;

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{define_class, msg_send, MainThreadOnly};
use objc2_app_kit::{NSApplication, NSButton, NSSlider};
use objc2_foundation::{MainThreadMarker, NSObject};
use std::cell::RefCell;

thread_local! {
    pub(crate) static HANDLER: RefCell<Option<Retained<MenuHandler>>> = const { RefCell::new(None) };
}

define_class!(
    #[unsafe(super = NSObject)]
    #[thread_kind = MainThreadOnly]
    #[name = "MenuHandler"]
    pub struct MenuHandler;

    impl MenuHandler {
        #[unsafe(method(togglePopover:))]
        fn toggle_popover(&self, _sender: &AnyObject) {
            if status_menu::requested(self.mtm()) {
                return status_menu::show(self.mtm());
            }
            popover::toggle(self.mtm());
        }

        #[unsafe(method(openWindow:))]
        fn open_window(&self, _sender: &AnyObject) {
            window::open(self.mtm());
        }

        #[unsafe(method(handleToggleItem:))]
        fn handle_toggle_item(&self, sender: &NSButton) {
            actions::toggle_item(sender.tag(), self.mtm());
        }

        #[unsafe(method(handleToggleGroup:))]
        fn handle_toggle_group(&self, sender: &NSButton) {
            actions::toggle_group(sender.tag(), self.mtm());
        }

        #[unsafe(method(handleTickOld:))]
        fn handle_tick_old(&self, _sender: &AnyObject) {
            actions::tick_old(self.mtm());
        }

        #[unsafe(method(handleRevealItem:))]
        fn handle_reveal_item(&self, sender: &NSButton) {
            reveal::reveal_item(sender.tag());
        }

        #[unsafe(method(handleCleanSelected:))]
        fn handle_clean_selected(&self, _sender: &AnyObject) {
            tasks::spawn_clean_selected();
        }

        #[unsafe(method(handleStopClean:))]
        fn handle_stop_clean(&self, _sender: &AnyObject) {
            tasks::request_stop();
        }

        /// The run is over; the screen is only still up to be read.
        #[unsafe(method(handleShowResult:))]
        fn handle_show_result(&self, _sender: &AnyObject) {
            tasks::show_result_now(self.mtm());
        }

        #[unsafe(method(cleanHoldTick:))]
        fn clean_hold_tick(&self, _sender: *mut AnyObject) {
            tasks::show_result(self.mtm());
        }

        #[unsafe(method(handleDoneAck:))]
        fn handle_done_ack(&self, _sender: &AnyObject) {
            actions::done_ack(self.mtm());
        }

        #[unsafe(method(handleToggleList:))]
        fn handle_toggle_list(&self, _sender: &AnyObject) {
            actions::toggle_fold(usize::MAX, self.mtm());
        }

        #[unsafe(method(handleRescan:))]
        fn handle_rescan(&self, _sender: &AnyObject) {
            // start_scan must still see Done so it can leave that screen now.
            tasks::start_scan();
        }

        #[unsafe(method(openSettings:))]
        fn open_settings(&self, _sender: &AnyObject) {
            actions::open_settings(self.mtm());
        }

        #[unsafe(method(settingsInterval:))]
        fn settings_interval(&self, sender: &NSButton) {
            actions::interval(sender, self.mtm());
        }

        #[unsafe(method(settingsSetMaxAge:))]
        fn settings_set_max_age(&self, sender: &NSSlider) {
            let days = sender.doubleValue().round().clamp(0.0, 30.0) as u64;
            actions::set_max_age(days, self.mtm());
        }

        #[unsafe(method(settingsDepth:))]
        fn settings_depth(&self, sender: &NSButton) {
            actions::depth(sender, self.mtm());
        }

        #[unsafe(method(settingsToggleGroup:))]
        fn settings_toggle_group(&self, sender: &NSButton) {
            actions::toggle_scan_group(sender, self.mtm());
        }

        #[unsafe(method(settingsToggleMenuBarSize:))]
        fn settings_toggle_menu_bar_size(&self, _sender: &AnyObject) {
            actions::toggle_menu_bar_size(self.mtm());
        }

        #[unsafe(method(settingsToggleMenuBarColor:))]
        fn settings_toggle_menu_bar_color(&self, _sender: &AnyObject) {
            actions::toggle_menu_bar_color(self.mtm());
        }

        #[unsafe(method(settingsAddRoot:))]
        fn settings_add_root(&self, _sender: &AnyObject) {
            actions::add_scan_root(self.mtm());
        }

        #[unsafe(method(settingsRemoveRoot:))]
        fn settings_remove_root(&self, sender: &NSButton) {
            actions::remove_scan_root(sender, self.mtm());
        }

        #[unsafe(method(settingsBack:))]
        fn settings_back(&self, _sender: &AnyObject) {
            actions::close_settings(self.mtm());
        }

        #[unsafe(method(settingsToggleLoginItem:))]
        fn settings_toggle_login_item(&self, _sender: &AnyObject) {
            actions::toggle_login(self.mtm());
        }

        #[unsafe(method(settingsToggleAutoUpdate:))]
        fn settings_toggle_auto_update(&self, _sender: &AnyObject) {
            actions::toggle_auto_update(self.mtm());
        }

        #[unsafe(method(settingsCheckForUpdates:))]
        fn settings_check_for_updates(&self, _sender: &AnyObject) {
            actions::check_updates();
        }

        #[unsafe(method(autoCleanTick:))]
        fn auto_clean_tick(&self, _sender: *mut AnyObject) {
            auto_clean::start();
        }

        #[unsafe(method(autoScanTick:))]
        fn auto_scan_tick(&self, _sender: *mut AnyObject) {
            if !tasks::is_busy() {
                tasks::start_scan();
            }
        }

        #[unsafe(method(scanDone:))]
        fn scan_done(&self, _sender: *mut AnyObject) {
            tasks::on_scan_done(self.mtm());
        }

        #[unsafe(method(sizesTick:))]
        fn sizes_tick(&self, _sender: *mut AnyObject) {
            tasks::on_sizes_tick(self.mtm());
        }

        #[unsafe(method(reclaimDone:))]
        fn reclaim_done(&self, _sender: *mut AnyObject) {
            tasks::on_reclaim_done(self.mtm());
        }

        #[unsafe(method(sizesDone:))]
        fn sizes_done(&self, _sender: *mut AnyObject) {
            tasks::on_sizes_done(self.mtm());
        }

        #[unsafe(method(discoverySweepTick:))]
        fn discovery_sweep_tick(&self, _sender: *mut AnyObject) {
            tasks::on_discovery_sweep_tick(self.mtm());
        }

        #[unsafe(method(cleanDone:))]
        fn clean_done(&self, _sender: *mut AnyObject) {
            tasks::on_clean_done(self.mtm());
        }

        #[unsafe(method(cleanProgress:))]
        fn clean_progress(&self, _sender: *mut AnyObject) {
            tasks::on_progress(self.mtm());
        }

        #[unsafe(method(quit:))]
        fn quit(&self, _sender: &AnyObject) {
            NSApplication::sharedApplication(self.mtm()).terminate(None);
        }
    }
);

impl MenuHandler {
    pub(crate) fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(());
        unsafe { msg_send![super(this), init] }
    }
}

fn main() {
    startup::run();
}
