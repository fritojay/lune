use super::*;

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
