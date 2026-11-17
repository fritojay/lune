use super::*;

#[test]
fn durations_are_minutes_and_seconds() {
    assert_eq!(duration(0), "0:00");
    assert_eq!(duration(59_999), "0:59");
    assert_eq!(duration(3_725_000), "62:05");
}

#[test]
fn long_durations_add_hours_only_when_needed() {
    assert_eq!(long_duration(3_599_999), "59:59");
    assert_eq!(long_duration(3_600_000), "1:00:00");
    assert_eq!(long_duration(90_061_000), "25:01:01");
}

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
fn filter_labels_depend_on_the_value() {
    let filter = |value, label: &str| Filter {
        value,
        label: label.to_owned(),
        detail: None,
        tracks: 1,
    };
    assert_eq!(filter_label(&filter(FilterValue::All, "")), "All");
    assert_eq!(
        filter_label(&filter(FilterValue::Artist(String::new()), "")),
        "Unknown Artist"
    );
    assert_eq!(
        filter_label(&filter(FilterValue::AlbumArtist("x".into()), "X")),
        "X"
    );
    assert_eq!(
        filter_label(&filter(FilterValue::Album(1), "")),
        "Unknown Album"
    );
    assert_eq!(
        filter_label(&filter(FilterValue::Year(None), "")),
        "Unknown"
    );
    assert_eq!(
        filter_label(&filter(FilterValue::Year(Some(2001)), "")),
        "2001"
    );
}
