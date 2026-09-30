// Menu bar app setup, kept apart from the Objective-C action handler.
use crate::{popover, state::{self, AppState, UiScreen}, tasks, theme, updater::Updater, HANDLER, MenuHandler};
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy, NSStatusBar};
use objc2_foundation::MainThreadMarker;
use wd40::config::Config;

pub(crate) fn run() {
    let mtm = MainThreadMarker::new().expect("must run on the main thread");
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);

    // Force AppKit appearance to match the requested theme so layer-backed
    // system controls (and our token pick) agree in screenshots.
    if let Ok(value) = std::env::var("WD40_APPEARANCE") {
        theme::set_app_appearance(value.eq_ignore_ascii_case("dark"), mtm);
    }

    let status_item = NSStatusBar::systemStatusBar().statusItemWithLength(-1.0);
    // Level two first: the scan that starts a moment from now is the one it
    // exists to spare.
    wd40::cache::load();
    let config = Config::load();
    let auto_hours = config.auto_clean_hours;

    HANDLER.with(|cell| *cell.borrow_mut() = Some(MenuHandler::new(mtm)));
    state::install(AppState {
        config,
        targets: Vec::new(),
        measured: Default::default(),
        selected: Default::default(),
        show_all: false,
        screen: UiScreen::Scan,
        cleaning: None,
        done: None,
        status_item,
        updater: Updater::start(),
        reclaim: None,
    });

    popover::attach(mtm);
    tasks::start_scan();
    tasks::start_auto_scan();
    if auto_hours > 0 {
        tasks::start_auto_clean(auto_hours);
    }

    app.run();
}
