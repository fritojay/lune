//! Display strings for tag values. Tag text is shown as is; only missing
//! values get a placeholder.

use lune_core::{Filter, FilterValue};

pub fn duration(ms: u64) -> String {
    let secs = ms / 1000;
    format!("{}:{:02}", secs / 60, secs % 60)
}

/// Long durations, such as a queue's total: hours appear once there are any.
pub fn long_duration(ms: u64) -> String {
    let secs = ms / 1000;
    if secs < 3600 {
        return duration(ms);
    }
    format!("{}:{:02}:{:02}", secs / 3600, secs / 60 % 60, secs % 60)
}

/// The disc is shown only for albums with more than one disc.
pub fn track_number(disc: Option<u32>, track: Option<u32>, multi_disc: bool) -> String {
    match (track, multi_disc) {
        (None, _) => String::new(),
        (Some(no), true) => format!("{}.{no}", disc.unwrap_or(1)),
        (Some(no), false) => no.to_string(),
    }
}

pub fn year(year: Option<i32>) -> String {
    year.map_or_else(|| "Unknown".to_owned(), |y| y.to_string())
}

pub fn artist(name: &str) -> String {
    or(name, "Unknown Artist")
}

pub fn album(title: &str) -> String {
    or(title, "Unknown Album")
}

pub fn genre(name: &str) -> String {
    or(name, "Unknown Genre")
}

pub fn file_type(codec: &str) -> String {
    codec.to_uppercase()
}

/// Empty when unknown.
pub fn bit_rate(kbps: Option<u32>) -> String {
    kbps.map_or_else(String::new, |kbps| format!("{kbps} kbps"))
}

pub fn filter_label(filter: &Filter) -> String {
    match &filter.value {
        FilterValue::All => "All".to_owned(),
        FilterValue::AlbumArtist(_) | FilterValue::Artist(_) => artist(&filter.label),
        FilterValue::Album(_) => album(&filter.label),
        FilterValue::Year(y) => year(*y),
        FilterValue::Genre(_) => genre(&filter.label),
    }
}

/// Nanoseconds since the Unix epoch, as milliseconds.
pub fn epoch_ms(ns: i64) -> f64 {
    (ns / 1_000_000) as f64
}

/// Capped for QML, whose ints are 32-bit.
pub fn qml_int(value: u32) -> i32 {
    i32::try_from(value).unwrap_or(i32::MAX)
}

fn or(text: &str, placeholder: &str) -> String {
    if text.trim().is_empty() {
        placeholder.to_owned()
    } else {
        text.to_owned()
    }
}

#[cfg(test)]
#[path = "tests/format_test.rs"]
mod tests;
