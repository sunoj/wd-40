// The scan header's legend: three swatch-title-value items laid out at their
// natural widths with the slack spread between them.
// Exports: `draw_legend_row`. Deps: crate::{layout, theme, widgets}.

use crate::layout::content_w;
use crate::theme::Theme;
use crate::widgets::{self, add_fill, label, PAD_X};
use objc2_app_kit::NSView;
use objc2_foundation::MainThreadMarker;

/// Lay the three legend items out at their natural widths and spread the slack
/// between them, as the mock's space-between does. The upper-bound qualifier
/// stays in the detail line below so this tight row keeps all three values.
#[derive(Clone, Copy)]
struct LegendItemLayout {
    title_x: f64,
    title_width: f64,
    value_x: f64,
    value_width: f64,
}

fn legend_layout(widths: [(f64, f64); 3]) -> [LegendItemLayout; 3] {
    const SWATCH: f64 = 8.0;
    const AFTER_SWATCH: f64 = 6.0;
    const BEFORE_VALUE: f64 = 6.0;
    const DISPLAY_EXTRA: f64 = 2.0;
    let natural: f64 = widths
        .iter()
        .map(|(title, value)| {
            SWATCH + AFTER_SWATCH + title + DISPLAY_EXTRA + BEFORE_VALUE + value + DISPLAY_EXTRA
        })
        .sum();
    let gap = ((content_w() - natural) / 2.0).max(4.0);
    let mut x = PAD_X;
    std::array::from_fn(|index| {
        let (title_width, value_width) = widths[index];
        let title_x = x + SWATCH + AFTER_SWATCH;
        let value_x = title_x + title_width + DISPLAY_EXTRA + BEFORE_VALUE;
        x = value_x + value_width + DISPLAY_EXTRA + gap;
        LegendItemLayout { title_x, title_width, value_x, value_width }
    })
}

pub fn draw_legend_row(
    parent: &NSView,
    y: f64,
    items: [(&str, String, (f64, f64, f64)); 3],
    theme: &Theme,
    mtm: MainThreadMarker,
) {
    const SWATCH: f64 = 8.0;
    const AFTER_SWATCH: f64 = 6.0;
    let widths = std::array::from_fn(|index| {
        let (title, value, _) = &items[index];
        (
            widgets::fitted_width(title, 12.0, false, mtm),
            widgets::fitted_width(&value, 11.5, true, mtm),
        )
    });
    for ((title, value, swatch), layout) in items.iter().zip(legend_layout(widths)) {
        add_fill(parent, layout.title_x - AFTER_SWATCH - SWATCH, y + 4.0, SWATCH, SWATCH, *swatch, 1.0, 2.0, mtm);
        label(parent, title, layout.title_x, y, layout.title_width + 2.0, 17.0, 12.0, false, theme.ink, false, mtm);
        label(parent, value, layout.value_x, y, layout.value_width + 2.0, 17.0, 11.5, false, theme.ink_2, true, mtm);
    }
}

#[cfg(test)]
mod tests {
    use super::legend_layout;
    use crate::layout::content_w;
    use crate::widgets::PAD_X;

    #[test]
    fn widest_legend_values_fit_without_overlap_or_clipping() {
        // Worst realistic settled values: three 999.9G figures at the row's
        // measured title/value widths. The qualifier is intentionally below.
        let items = legend_layout([(65.0, 43.0), (38.0, 43.0), (25.0, 43.0)]);
        let right_edge = PAD_X + content_w();
        for item in items {
            assert!(item.title_x + item.title_width + 2.0 + 6.0 <= item.value_x);
            assert!(item.value_x + item.value_width + 2.0 <= right_edge);
        }
        for pair in items.windows(2) {
            assert!(pair[0].value_x + pair[0].value_width + 2.0 <= pair[1].title_x);
        }
    }
}
