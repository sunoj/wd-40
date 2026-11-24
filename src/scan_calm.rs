// The scan screen when nothing is past the keep threshold: the can at rest on
// clean steel, the verdict as a headline, the next target to turn old, and one
// row that unfolds the recent builds into the ordinary list.
// Exports: `LIST`, `CALM_H`, `calm`, `next_old`, `draw`.
// Deps: crate::{can, crust, hover_row, names, selection, state, widgets}.

use crate::can::{can_view, REST};
use crate::hover_row::hover_row;
use crate::layout::{content_w, panel_w};
use crate::selection::nothing_old;
use crate::state::AppState;
use crate::theme::Theme;
use crate::widgets::{add_line, label, label_right, symbol_view, PAD_X};
use objc2::rc::Retained;
use objc2::{define_class, msg_send, DefinedClass, MainThreadOnly};
use objc2_app_kit::{NSTextAlignment, NSView};
use objc2_foundation::{MainThreadMarker, NSPoint, NSRect, NSSize};
use std::collections::HashSet;
use std::time::{Duration, SystemTime};
use wd40::scanner::{ArtifactGroup, ArtifactKind, TargetDir};

/// Key in `AppState::open` for "show the list although all is clear".
pub const LIST: &str = "list";
pub const CALM_H: f64 = 262.0;
const PLATE_H: f64 = 112.0;
const ROW_H: f64 = 40.0;

/// True when the scan screen is the all-clear one: sizes are in, nothing
/// listed is old, nothing is ticked, and the list has not been asked for.
pub fn calm(state: &AppState) -> bool {
    let settled = !state.sizing() && !crate::tasks::is_scanning();
    let listed = state.targets.len() - state.empty_targets().len();
    settled
        && listed > 0
        && state.selected.is_empty()
        && !state.open.contains(LIST)
        && nothing_old(&state.targets, &state.empty_targets(), None, state.config.max_age_days)
}

/// The listed target that crosses the keep threshold first, and in how many
/// days (rounded up). Simulator devices and toolchains never join the old
/// batch, so they are not what comes next.
pub fn next_old(targets: &[TargetDir], empty: &HashSet<usize>, keep_days: u64) -> Option<(usize, u64)> {
    let limit = Duration::from_secs(keep_days.saturating_mul(86_400));
    let now = SystemTime::now();
    let left = |target: &TargetDir| limit.checked_sub(now.duration_since(target.last_modified).ok()?);
    (0..targets.len())
        .filter(|index| !empty.contains(index))
        .filter(|index| !matches!(targets[*index].kind, ArtifactKind::Toolchain | ArtifactKind::Simulator))
        .filter_map(|index| Some((index, left(&targets[index])?)))
        .min_by_key(|(_, left)| *left)
        .map(|(index, left)| (index, left.as_secs().div_ceil(86_400).max(1)))
}

pub struct SteelIvars {
    dark: bool,
}

define_class!(
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[ivars = SteelIvars]
    #[name = "WD40CalmSteel"]
    struct Steel;

    impl Steel {
        /// The cleaning plate's own steel, with nothing on it to remove.
        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            crate::crust::paint_steel(self.bounds(), self.ivars().dark, &[]);
        }
    }
);

/// The all-clear screen, `CALM_H` tall, hanging from `y_top`.
pub fn draw(parent: &NSView, y_top: f64, state: &AppState, theme: &Theme, mtm: MainThreadMarker) {
    let plate_y = y_top - 14.0 - PLATE_H;
    let frame = NSRect::new(NSPoint::new(PAD_X, plate_y), NSSize::new(content_w(), PLATE_H));
    let steel = mtm.alloc::<Steel>().set_ivars(SteelIvars { dark: theme.dark });
    let steel: Retained<Steel> = unsafe { msg_send![super(steel), initWithFrame: frame] };
    let at = NSPoint::new((content_w() - REST.size.0) / 2.0, (PLATE_H - REST.size.1) / 2.0);
    steel.addSubview(&can_view(REST, at, mtm));
    parent.addSubview(&steel);

    let days = state.config.max_age_days;
    let centred = |text: &str, y: f64, height: f64, size: f64, bold: bool, ink| {
        let field = label(parent, text, PAD_X, y, content_w(), height, size, bold, ink, false, mtm);
        field.setAlignment(NSTextAlignment::Center);
    };
    centred("All clear", plate_y - 42.0, 26.0, 20.0, true, theme.ink);
    centred(&format!("Nothing older than {}.", in_days(days)), plate_y - 62.0, 18.0, 13.0, false, theme.ink_2);
    let empty = state.empty_targets();
    if let Some((index, wait)) = next_old(&state.targets, &empty, days) {
        let name = &crate::names::display_names(&state.targets)[index];
        centred(&format!("Next: {name} turns old in {}", in_days(wait)), plate_y - 82.0, 16.0, 11.5, false, theme.ink_3);
    }
    draw_summary(parent, y_top - CALM_H + 4.0, state, &empty, theme, mtm);
}

fn in_days(days: u64) -> String {
    format!("{days} day{}", if days == 1 { "" } else { "s" })
}

/// One row for everything recent, which unfolds into the grouped list for
/// anyone who wants to tick a recent build by hand.
fn draw_summary(parent: &NSView, y: f64, state: &AppState, empty: &HashSet<usize>, theme: &Theme, mtm: MainThreadMarker) {
    add_line(parent, PAD_X, y + ROW_H, content_w(), theme.line, mtm);
    let listed = (0..state.targets.len()).filter(|index| !empty.contains(index));
    let groups: Vec<ArtifactGroup> = listed.clone().map(|index| state.targets[index].kind.group()).collect();
    let kinds = ArtifactGroup::ALL.iter().filter(|group| groups.contains(group)).count();
    let tag = crate::scan_view::TAG_FOLD_BASE + ArtifactGroup::ALL.len() as isize;
    let row = hover_row(parent, y, ROW_H, theme.surface_2, tag, mtm);
    let what = format!("{} recent builds in {kinds} groups", listed.count());
    label(&row, &what, PAD_X, 11.0, content_w() - 90.0, 18.0, 13.0, false, theme.ink, false, mtm);
    label_right(&row, "Show", panel_w() - PAD_X - 74.0, 12.0, 56.0, 16.0, 12.0, theme.ink_2, false, mtm);
    symbol_view(&row, "chevron.right", panel_w() - PAD_X - 13.0, 14.0, 11.0, theme.ink_3, mtm);
}

#[cfg(test)]
mod tests {
    use super::next_old;
    use std::collections::HashSet;
    use std::time::{Duration, SystemTime};
    use wd40::scanner::{ArtifactKind, TargetDir};

    fn aged(kind: ArtifactKind, hours: u64) -> TargetDir {
        let last_modified = SystemTime::now() - Duration::from_secs(hours * 3_600);
        TargetDir { path: "/w/a/target".into(), size_bytes: 1, last_modified, kind }
    }

    /// The oldest recent target is next, in whole days rounded up; a device is
    /// never next, and neither is a target that is old already or left out.
    #[test]
    fn the_next_target_to_turn_old_is_the_oldest_recent_one() {
        use ArtifactKind::{NodeModules, RustTarget, Simulator};
        let targets = [aged(RustTarget, 24), aged(NodeModules, 100), aged(Simulator, 160), aged(RustTarget, 400), aged(RustTarget, 150)];
        assert_eq!(next_old(&targets, &HashSet::from([4]), 7), Some((1, 3)));
        assert_eq!(next_old(&targets, &HashSet::new(), 7), Some((4, 1)));
        assert_eq!(next_old(&targets[2..4], &HashSet::new(), 7), None);
    }
}
