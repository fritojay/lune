use std::path::PathBuf;

use super::*;
use crate::queue::EntryId;

fn boundary(start: u64, id: u64) -> Boundary {
    Boundary {
        start,
        entry: QueueEntry::new(EntryId(id), PathBuf::from("x")),
        offset: Duration::ZERO,
        duration: None,
    }
}

#[test]
fn playing_is_the_last_boundary_at_or_before_the_read_position() {
    let mut t = Timeline::new();
    assert_eq!(t.playing(0), None);
    t.boundaries
        .extend([boundary(100, 0), boundary(500, 1), boundary(900, 2)]);
    assert_eq!(
        t.playing(0),
        Some(0),
        "before the first start: the first track"
    );
    assert_eq!(t.playing(100), Some(0));
    assert_eq!(t.playing(499), Some(0));
    assert_eq!(t.playing(500), Some(1));
    assert_eq!(t.playing(10_000), Some(2));
}
