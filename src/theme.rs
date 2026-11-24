// Design tokens: the WD-40 can's label colours on cool steel neutrals, in a
// light and a dark set. Exports: `Theme`, `Theme::for_app`, `hex`, colour helpers.
// Deps: objc2_app_kit, objc2_foundation.

use objc2::rc::Retained;
use objc2_app_kit::{
    NSAppearance, NSAppearanceNameAqua, NSAppearanceNameDarkAqua, NSColor,
};
use objc2_foundation::NSArray;

/// The can's own label, laid on steel. Neutrals are cool greys; rust orange is
/// everything that can be reclaimed; the can's blue is space that is free or
/// won back; its yellow band is the one primary action; its red cap is stop.
#[derive(Clone, Copy)]
pub struct Theme {
    pub dark: bool,
    pub ink: (f64, f64, f64),
    pub ink_2: (f64, f64, f64),
    pub ink_3: (f64, f64, f64),
    pub ink_4: (f64, f64, f64),
    /// Behind the surface: the window's own backdrop.
    pub paper: (f64, f64, f64),
    pub surface: (f64, f64, f64),
    pub surface_2: (f64, f64, f64),
    pub surface_3: (f64, f64, f64),
    pub line: (f64, f64, f64),
    pub line_2: (f64, f64, f64),
    /// Rust: reclaimable bytes, ticks, anything still on disk.
    pub accent: (f64, f64, f64),
    pub accent_weak: (f64, f64, f64),
    /// Can blue: free space and space won back.
    pub pos: (f64, f64, f64),
    pub warn: (f64, f64, f64),
    /// Label yellow and the navy printed on it: the primary button.
    pub cta: (f64, f64, f64),
    pub cta_ink: (f64, f64, f64),
    /// Cap red: stopping, and what could not be removed.
    pub danger: (f64, f64, f64),
}

/// The can itself, for the stripe every surface carries along its top edge.
pub const CAN_BLUE: u32 = 0x2F4FA2;
pub const CAN_YELLOW: u32 = 0xF5C518;
pub const CAN_RED: u32 = 0xD23A2E;

impl Theme {
    pub fn light() -> Self {
        Self {
            dark: false,
            ink: hex(0x15181D),
            ink_2: hex(0x4A505A),
            ink_3: hex(0x767D88),
            ink_4: hex(0xB1B7C0),
            paper: hex(0xE6E9ED),
            surface: hex(0xF8F9FA),
            surface_2: hex(0xECEEF1),
            surface_3: hex(0xDDE1E6),
            line: hex(0xDDE1E5),
            line_2: hex(0xC6CBD2),
            accent: hex(0xC4561C),
            accent_weak: hex(0xF6E4D8),
            pos: hex(CAN_BLUE),
            warn: hex(0x8F6400),
            cta: hex(CAN_YELLOW),
            cta_ink: hex(0x14244A),
            danger: hex(0xC2342A),
        }
    }

    pub fn dark_theme() -> Self {
        Self {
            dark: true,
            ink: hex(0xECEEF1),
            ink_2: hex(0xA9AFB8),
            ink_3: hex(0x7B828C),
            ink_4: hex(0x4E545D),
            paper: hex(0x111316),
            surface: hex(0x1A1D21),
            surface_2: hex(0x23272C),
            surface_3: hex(0x2F343B),
            line: hex(0x2A2E34),
            line_2: hex(0x3B4149),
            accent: hex(0xE2702F),
            accent_weak: hex(0x3B271B),
            pos: hex(0x6F92E6),
            warn: hex(0xE0B341),
            cta: hex(CAN_YELLOW),
            cta_ink: hex(0x14244A),
            danger: hex(0xEA5A4E),
        }
    }

    /// The theme every surface is built with: the app's appearance, unless a
    /// screenshot or preview run has pinned one.
    pub fn for_app(mtm: objc2_foundation::MainThreadMarker) -> Self {
        match std::env::var("WD40_APPEARANCE") {
            Ok(value) if value.eq_ignore_ascii_case("light") => Self::light(),
            Ok(value) if value.eq_ignore_ascii_case("dark") => Self::dark_theme(),
            _ => Self::current_app(mtm),
        }
    }

    /// Resolve against the application's appearance, which is what the
    /// popover's material renders against. The status-bar button is not a
    /// substitute: the menu bar is dark under a dark wallpaper even while the
    /// system is in Light Mode, which paired light material with dark-mode ink.
    fn current_app(mtm: objc2_foundation::MainThreadMarker) -> Self {
        let app = objc2_app_kit::NSApplication::sharedApplication(mtm);
        let names = NSArray::from_slice(&[
            unsafe { NSAppearanceNameAqua },
            unsafe { NSAppearanceNameDarkAqua },
        ]);
        let dark = app
            .effectiveAppearance()
            .bestMatchFromAppearancesWithNames(&names)
            .is_some_and(|name| &*name == unsafe { NSAppearanceNameDarkAqua });
        if dark {
            Self::dark_theme()
        } else {
            Self::light()
        }
    }

    pub fn color(rgb: (f64, f64, f64)) -> Retained<NSColor> {
        NSColor::colorWithSRGBRed_green_blue_alpha(rgb.0, rgb.1, rgb.2, 1.0)
    }

    pub fn color_alpha(rgb: (f64, f64, f64), alpha: f64) -> Retained<NSColor> {
        NSColor::colorWithSRGBRed_green_blue_alpha(rgb.0, rgb.1, rgb.2, alpha)
    }
}

pub fn hex(value: u32) -> (f64, f64, f64) {
    let r = ((value >> 16) & 0xff) as f64 / 255.0;
    let g = ((value >> 8) & 0xff) as f64 / 255.0;
    let b = (value & 0xff) as f64 / 255.0;
    (r, g, b)
}

/// Force the app appearance for screenshot / preview runs.
pub fn set_app_appearance(dark: bool, mtm: objc2_foundation::MainThreadMarker) {
    use objc2_app_kit::NSApplication;
    let name = if dark {
        unsafe { NSAppearanceNameDarkAqua }
    } else {
        unsafe { NSAppearanceNameAqua }
    };
    let appearance = NSAppearance::appearanceNamed(name);
    NSApplication::sharedApplication(mtm).setAppearance(appearance.as_deref());
}
