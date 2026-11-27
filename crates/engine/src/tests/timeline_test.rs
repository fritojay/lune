use std::path::PathBuf;

use proptest::prelude::*;

use super::*;
use crate::queue::EntryId;

fn boundary(start: u64, id: u64) -> Boundary {
    Boundary {
        start,
        entry: QueueEntry::new(EntryId(id), PathBuf::from("x")),
        offset: Duration::ZERO,
        duration: None,
        playthrough: 0,
    }
}

proptest! {
    #[test]
    fn playing_is_the_last_boundary_at_or_before_the_read_position(
        mut starts in prop::collection::vec(0..1000u64, 0..6),
        consumed in 0..1200u64,
    ) {
        starts.sort_unstable();
        let mut t = Timeline::new();
        t.boundaries.extend(starts.iter().zip(0..).map(|(&start, id)| boundary(start, id)));
        let started = starts.iter().filter(|&&start| start <= consumed).count();
        let expected = (!starts.is_empty()).then(|| started.saturating_sub(1));
        prop_assert_eq!(t.playing(consumed), expected);
    }
}
