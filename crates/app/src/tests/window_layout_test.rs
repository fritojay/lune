use proptest::prelude::*;
use proptest::sample::select;

use super::*;
use crate::columns::COLUMNS;
use crate::test_util::arb_hidden_columns;

#[test]
fn qml_sizes_round_to_whole_pixels() {
    assert_eq!(pixels(299.4), 299);
    assert_eq!(pixels(299.5), 300);
    assert_eq!(pixels(0.0), 0);
}

#[test]
fn out_of_range_qml_sizes_saturate() {
    assert_eq!(pixels(-12.0), 0);
    assert_eq!(pixels(f64::NAN), 0);
    assert_eq!(pixels(f64::INFINITY), u32::MAX);
}

#[test]
fn saved_hidden_columns_reach_qml_by_name() {
    let state = WindowState {
        hidden_columns: [SortColumn::Artist, SortColumn::BitRate].into(),
        ..WindowState::default()
    };
    let names: Vec<String> = WindowLayoutRust::from(&state)
        .hidden_columns
        .iter()
        .map(ToString::to_string)
        .collect();
    assert_eq!(names, ["artist", "bit_rate"]);
}

#[test]
fn the_saved_album_grid_reaches_qml() {
    let state = WindowState {
        album_grid: true,
        ..WindowState::default()
    };
    assert!(WindowLayoutRust::from(&state).album_grid);
}

fn widths(available: f64, hidden: &[SortColumn]) -> Vec<f64> {
    column_widths(available, &hidden.iter().copied().collect())
}

/// Display order: #, title, artist, file type, bit rate, time.
#[test]
fn a_wide_row_gives_the_title_what_the_others_leave() {
    assert_eq!(widths(1000.0, &[]), [32.0, 494.0, 220.0, 76.0, 72.0, 56.0]);
}

/// Row widths around where columns start to squeeze, and nonsense ones.
fn arb_available() -> impl Strategy<Value = f64> {
    prop_oneof![
        4 => 0.0..1200.0,
        1 => Just(f64::NAN),
        1 => Just(f64::INFINITY),
        1 => Just(f64::NEG_INFINITY),
        1 => -100.0..0.0,
    ]
}

proptest! {
    #[test]
    fn showing_or_hiding_a_column_changes_only_that_column(
        hidden in arb_hidden_columns(),
        name in prop_oneof![select(&SortColumn::ALL[..]).prop_map(SortColumn::name), Just("nope"), Just("")],
        shown: bool,
    ) {
        let result = with_column_shown(hidden.clone(), name, shown);
        let Some(column) = SortColumn::from_name(name) else {
            prop_assert_eq!(result, None);
            return Ok(());
        };
        let mut expected = hidden.clone();
        if shown {
            expected.remove(&column);
        } else {
            expected.insert(column);
        }
        prop_assert_eq!(result, (expected != hidden).then_some(expected));
    }

    #[test]
    fn columns_squeeze_then_drop_from_the_right_before_the_title_shrinks(
        available in arb_available(),
        hidden in arb_hidden_columns(),
    ) {
        let widths = column_widths(available, &hidden);
        let row = if available.is_finite() { available.max(0.0) } else { 0.0 };
        let shown: Vec<&Column> = COLUMNS.iter().filter(|s| !hidden.contains(&s.column)).collect();
        prop_assert_eq!(widths.len(), shown.len());
        let mut dropped = false;
        let mut title = None;
        for (size, &width) in shown.iter().zip(&widths) {
            prop_assert!(width >= 0.0 && width.fract() == 0.0, "{widths:?}");
            let natural = size.max_share.map_or(size.width, |share| size.width.min(row * share));
            if size.column == SortColumn::Title {
                title = Some(width);
            } else if !size.hideable {
                prop_assert_eq!(width, natural);
            } else if width == 0.0 {
                dropped = true;
            } else {
                prop_assert!(!dropped, "only the rightmost drop: {widths:?}");
                prop_assert!((MIN_SHRUNK_WIDTH..=natural).contains(&width), "{widths:?}");
            }
        }
        let kept = widths.iter().filter(|&&w| w > 0.0).count() + usize::from(title == Some(0.0));
        let gaps = SPACING * kept.saturating_sub(1) as f64;
        let used = widths.iter().sum::<f64>() + gaps;
        let hideable_shown = shown.iter().zip(&widths).any(|(s, &w)| s.hideable && w > 0.0);
        if hideable_shown {
            prop_assert!(used <= row, "the shown hideable columns fit: {widths:?}");
        }
        let title_width = COLUMNS
            .iter()
            .find(|s| s.column == SortColumn::Title)
            .unwrap()
            .width;
        match title {
            Some(title) if title > 0.0 => {
                prop_assert!(row - 1.0 < used && used <= row, "the title fills the row: {widths:?}");
                if hideable_shown {
                    prop_assert!(title >= title_width, "{widths:?}");
                }
            }
            Some(_) => prop_assert!(!hideable_shown, "{widths:?}"),
            None => {}
        }
    }
}
