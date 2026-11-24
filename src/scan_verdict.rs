// The scan header's last line: the screen's one verdict on what can safely be
// cleaned, the action that ticks it, and the note on what a size means. It is
// a zone of its own, so a tick repaints this line and not the gauge above it.
// Exports: `VERDICT_H`, `Verdict`, `verdict`, `draw`.
// Deps: crate::{controls, header, selection, state, tasks, widgets}.

use crate::controls::text_button_underlined;
use crate::header::scan_size;
use crate::layout::content_w;
use crate::selection::{nothing_old, old_targets};
use crate::state::AppState;
use crate::theme::Theme;
use crate::widgets::{fitted_width, label, label_right, PAD_X};
use objc2::runtime::AnyObject;
use objc2::sel;
use objc2_app_kit::{NSImage, NSImageView, NSView};
use objc2_foundation::{MainThreadMarker, NSPoint, NSRect, NSSize, NSString};
use wd40::disk::sum_bytes;

/// The strip along the bottom of the scan header that the verdict is drawn in.
pub const VERDICT_H: f64 = 30.0;
const TAG_TICK_OLD: isize = 2008;
const TAG_HIDE_LIST: isize = 2006;
const TEXT_Y: f64 = 13.0;
const INFO: f64 = 12.0;

#[derive(Debug, PartialEq, Eq)]
pub enum Verdict {
    Finding,
    Measuring { measured: usize, total: usize },
    /// Nothing is listed; the list says so itself.
    Nothing,
    /// Nothing listed is past the keep threshold. `listed` is true when the
    /// list is on show only because it was asked for, and can be put away.
    Clear { days: u64, listed: bool },
    /// The batch tick-old stands for. `ceiling`: the size is a scanned sum.
    Old { count: usize, bytes: u64, ticked: bool, ceiling: bool, days: u64 },
    /// Old targets, but only of the kinds the batch leaves alone.
    ByHand { count: usize, days: u64 },
}

/// The verdict for this state. The count is `selection::old_targets`, the same
/// batch the action ticks and auto-clean takes; "clear" is `nothing_old`.
pub fn verdict(state: &AppState) -> Verdict {
    let (days, empty) = (state.config.max_age_days, state.empty_targets());
    if crate::tasks::is_discovering() {
        return Verdict::Finding;
    }
    if state.sizing() {
        return Verdict::Measuring { measured: state.measured.len(), total: state.targets.len() };
    }
    let old = old_targets(&state.targets, &empty, days);
    if !old.is_empty() {
        let bytes = sum_bytes(old.iter().map(|index| state.targets[*index].size_bytes));
        let ceiling = !state.reclaim.as_ref().is_some_and(|found| found.covers(&state.targets));
        return Verdict::Old { count: old.len(), bytes, ticked: old.is_subset(&state.selected), ceiling, days };
    }
    if state.targets.len() == empty.len() {
        return Verdict::Nothing;
    }
    if nothing_old(&state.targets, &empty, None, days) {
        let listed = state.selected.is_empty() && state.open.contains(crate::scan_calm::LIST);
        return Verdict::Clear { days, listed };
    }
    let by_hand = (0..state.targets.len()).filter(|index| !empty.contains(index));
    let count = by_hand.filter(|index| !crate::selection::is_recent(&state.targets[*index], days)).count();
    Verdict::ByHand { count, days }
}

impl Verdict {
    pub fn text(&self) -> String {
        match self {
            Self::Finding => "Finding build artifacts\u{2026}".into(),
            Self::Measuring { .. } => "Measuring build artifacts\u{2026}".into(),
            Self::Nothing => String::new(),
            Self::Clear { days, .. } => format!("\u{2713} All clear \u{00b7} nothing older than {days}d"),
            // Ticked, the size is the clean button's to state: it has the exact one.
            Self::Old { count, ticked: true, .. } => format!("All {count} old ticked"),
            Self::Old { count, bytes, ceiling, .. } => {
                let bound = if *ceiling { "up to " } else { "" };
                format!("{count} old \u{00b7} {bound}{} can go", scan_size(*bytes))
            }
            Self::ByHand { count, .. } => format!("{count} old devices/toolchains \u{00b7} tick by hand"),
        }
    }

    /// What the info mark beside the verdict says: what "old" means here, and
    /// why a sum of sizes is a ceiling. None while there is no figure to note.
    pub fn note(&self) -> Option<String> {
        let days = match self {
            Self::Clear { days, .. } | Self::Old { days, .. } | Self::ByHand { days, .. } => days,
            _ => return None,
        };
        Some(format!(
            "Old means untouched for {days} days or more. Simulator devices and toolchains are never ticked for you.\n\
             Sizes are as scanned: builds that share APFS clones overlap, so cleaning can free less than the sum."
        ))
    }
}

pub fn draw(parent: &NSView, verdict: &Verdict, theme: &Theme, target: &AnyObject, mtm: MainThreadMarker) {
    let text = verdict.text();
    let ink = if matches!(verdict, Verdict::Clear { .. }) { theme.ok } else { theme.ink_2 };
    let width = fitted_width(&text, 11.5, true, mtm) + 4.0;
    label(parent, &text, PAD_X, TEXT_Y, width, 16.0, 11.5, false, ink, true, mtm);
    if let Some(note) = verdict.note() {
        info_mark(parent, PAD_X + width + 2.0, TEXT_Y + 2.0, &note, theme, mtm);
    }
    let right = |title: &str, action: objc2::runtime::Sel, tag: isize| {
        let w = fitted_width(title, 12.5, false, mtm) + 8.0;
        text_button_underlined(parent, title, PAD_X + content_w() - w + 3.0, TEXT_Y - 4.0, w, action, target, tag, theme.accent, mtm);
    };
    match verdict {
        Verdict::Measuring { measured, total } => {
            let counted = format!("{measured} of {total} measured");
            label_right(parent, &counted, PAD_X + 230.0, TEXT_Y, content_w() - 230.0, 16.0, 11.5, theme.ink_3, true, mtm);
        }
        Verdict::Old { ticked: false, .. } => right("Tick them", sel!(handleTickOld:), TAG_TICK_OLD),
        Verdict::Clear { listed: true, .. } => right("Hide list", sel!(handleToggleList:), TAG_HIDE_LIST),
        _ => {}
    }
}

/// A small "i" whose tooltip carries the caveat: quiet until it is asked.
#[allow(deprecated)]
fn info_mark(parent: &NSView, x: f64, y: f64, note: &str, theme: &Theme, mtm: MainThreadMarker) {
    let name = NSString::from_str("info.circle");
    let Some(image) = NSImage::imageWithSystemSymbolName_accessibilityDescription(&name, Some(&NSString::from_str(note))) else {
        return;
    };
    image.setTemplate(true);
    let view = NSImageView::imageViewWithImage(&image, mtm);
    view.setFrame(NSRect::new(NSPoint::new(x, y), NSSize::new(INFO, INFO)));
    view.setContentTintColor(Some(&Theme::color(theme.ink_3)));
    view.setToolTip(Some(&NSString::from_str(note)));
    parent.addSubview(&view);
}

#[cfg(test)]
mod tests {
    use super::Verdict;

    #[test]
    fn each_state_says_one_thing() {
        let old = |ticked, ceiling| Verdict::Old { count: 230, bytes: 3 << 30, ticked, ceiling, days: 7 };
        assert_eq!(old(false, true).text(), "230 old \u{00b7} up to 3.0 GB can go");
        assert_eq!(old(false, false).text(), "230 old \u{00b7} 3.0 GB can go");
        assert_eq!(old(true, true).text(), "All 230 old ticked");
        assert_eq!(Verdict::Clear { days: 7, listed: false }.text(), "\u{2713} All clear \u{00b7} nothing older than 7d");
        assert_eq!(Verdict::ByHand { count: 3, days: 7 }.text(), "3 old devices/toolchains \u{00b7} tick by hand");
        assert_eq!(Verdict::Measuring { measured: 1, total: 9 }.text(), "Measuring build artifacts\u{2026}");
        assert_eq!(Verdict::Nothing.text(), "");
    }

    #[test]
    fn the_caveat_waits_for_a_figure() {
        assert!(Verdict::Finding.note().is_none());
        assert!(Verdict::Measuring { measured: 1, total: 9 }.note().is_none());
        let note = Verdict::ByHand { count: 1, days: 14 }.note().expect("a note");
        assert!(note.contains("14 days") && note.contains("APFS clones"));
    }
}
