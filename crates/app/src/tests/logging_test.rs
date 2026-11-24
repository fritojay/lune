use std::io::Write;

use super::*;
use crate::test_util::TempDir;

fn contents(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap()
}

#[test]
fn the_log_is_emptied_at_each_launch() {
    let dir = TempDir::new();
    let log = dir.write("lune.log", b"the last run");
    let mut file = open_first(std::slice::from_ref(&log)).unwrap();
    assert_eq!(contents(&log), "");
    write!(file, "this run").unwrap();
    assert_eq!(contents(&log), "this run");
}

#[test]
fn a_log_that_cannot_be_created_falls_back_to_the_next() {
    let dir = TempDir::new();
    let not_a_dir = dir.write("data", b"");
    let fallback = dir.path().join("temp").join("lune.log");
    let mut file = open_first(&[not_a_dir.join("lune.log"), fallback.clone()]).unwrap();
    write!(file, "logged").unwrap();
    assert_eq!(contents(&fallback), "logged");
}

#[test]
fn another_instances_log_is_left_alone() {
    let dir = TempDir::new();
    let own = dir.path().join("lune.log");
    let fallback = dir.path().join("temp.log");
    let mut first = open_first(std::slice::from_ref(&own)).unwrap();
    write!(first, "first").unwrap();

    let mut second = open_first(&[own.clone(), fallback.clone()]).unwrap();
    write!(second, "second").unwrap();
    assert_eq!(contents(&own), "first");
    assert_eq!(contents(&fallback), "second");
}

#[test]
fn no_usable_log_is_none() {
    let dir = TempDir::new();
    let not_a_dir = dir.write("data", b"");
    assert!(open_first(&[not_a_dir.join("lune.log")]).is_none());
    assert!(open_first(&[]).is_none());
}
