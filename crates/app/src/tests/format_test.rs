use proptest::prelude::*;

use super::*;

#[test]
fn track_numbers_show_the_disc_only_on_multi_disc_albums() {
    assert_eq!(track_number(None, None, false), "");
    assert_eq!(track_number(Some(2), None, true), "");
    assert_eq!(track_number(Some(1), Some(7), false), "7");
    assert_eq!(track_number(Some(2), Some(7), true), "2.7");
    assert_eq!(track_number(None, Some(7), true), "1.7");
}

#[test]
fn missing_values_get_placeholders() {
    assert_eq!(year(None), "Unknown");
    assert_eq!(year(Some(1999)), "1999");
    assert_eq!(artist(""), "Unknown Artist");
    assert_eq!(artist("  "), "Unknown Artist");
    assert_eq!(artist("Björk"), "Björk");
    assert_eq!(album(""), "Unknown Album");
}

#[test]
fn filter_labels_depend_on_the_field() {
    let filter = |value, field, label: &str| Filter {
        value,
        field,
        label: label.to_owned(),
        detail: None,
        tracks: 1,
    };
    let text = || FilterValue::Text("x".into());
    assert_eq!(
        filter_label(&filter(FilterValue::All, Field::Genre, "")),
        "All"
    );
    assert_eq!(
        filter_label(&filter(text(), Field::Artist, "")),
        "Unknown Artist"
    );
    assert_eq!(filter_label(&filter(text(), Field::AlbumArtist, "X")), "X");
    assert_eq!(
        filter_label(&filter(FilterValue::Album(1), Field::Album, "")),
        "Unknown Album"
    );
    assert_eq!(
        filter_label(&filter(FilterValue::UnknownYear, Field::Year, "")),
        "Unknown"
    );
    assert_eq!(
        filter_label(&filter(FilterValue::Year(2001), Field::Year, "2001")),
        "2001"
    );
    assert_eq!(
        filter_label(&filter(text(), Field::Genre, "")),
        "Unknown Genre"
    );
    assert_eq!(filter_label(&filter(text(), Field::Genre, "Jazz")), "Jazz");
}

#[test]
fn epoch_times_are_in_whole_milliseconds() {
    assert_eq!(epoch_ms(1_700_000_000_123_456_789), 1_700_000_000_123.0);
    assert_eq!(epoch_ms(0), 0.0);
}

#[test]
fn a_time_before_the_epoch_stays_negative() {
    assert_eq!(epoch_ms(-5_000_000), -5.0);
}

#[test]
fn numbers_too_big_for_qml_are_capped() {
    assert_eq!(qml_int(1280), 1280);
    assert_eq!(qml_int(u32::MAX), i32::MAX);
}

#[test]
fn bit_rates_are_in_kbps_and_empty_when_unknown() {
    assert_eq!(bit_rate(Some(320)), "320 kbps");
    assert_eq!(bit_rate(None), "");
}

/// The whole seconds `text` shows, if the first field has no leading zero and
/// each field after it is two digits under 60.
fn shown_seconds(text: &str) -> Option<u64> {
    let mut fields = text.split(':');
    let first = fields.next()?;
    if first.len() > 1 && first.starts_with('0') {
        return None;
    }
    let mut secs: u64 = first.parse().ok()?;
    for field in fields {
        let value: u64 = field.parse().ok()?;
        if field.len() != 2 || value >= 60 {
            return None;
        }
        secs = secs * 60 + value;
    }
    Some(secs)
}

proptest! {
    #[test]
    fn a_filter_label_is_the_tag_or_its_fields_placeholder_when_missing(
        field in proptest::sample::select(&Field::ALL[..]),
        label in prop_oneof![Just(String::new()), "[ \t]{1,3}", any::<String>()],
        all: bool,
    ) {
        let filter = Filter {
            value: if all { FilterValue::All } else { FilterValue::Text(label.clone()) },
            field,
            label: label.clone(),
            detail: None,
            tracks: 1,
        };
        let expected = if all {
            "All"
        } else if label.trim().is_empty() {
            field.missing()
        } else {
            &label
        };
        prop_assert_eq!(filter_label(&filter), expected);
        prop_assert!(!field.missing().is_empty());
    }

    #[test]
    fn durations_show_whole_minutes_and_seconds_and_long_ones_add_hours(
        ms in prop_oneof![0..10_000_000u64, 3_599_000..3_601_000u64, any::<u64>()],
    ) {
        let short = duration(ms);
        prop_assert_eq!(short.matches(':').count(), 1);
        prop_assert_eq!(shown_seconds(&short), Some(ms / 1000));
        let long = long_duration(ms);
        prop_assert_eq!(long.matches(':').count(), if ms < 3_600_000 { 1 } else { 2 });
        prop_assert_eq!(shown_seconds(&long), Some(ms / 1000));
    }
}
