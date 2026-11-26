use std::collections::HashSet;

use lune_core::{Sort, SortColumn};

use super::*;

#[test]
fn only_a_shown_playlist_in_its_own_order_without_a_search_is_reorderable() {
    let sources = [
        Source::Library(Grouping::Album),
        Source::Playlist { id: None },
        Source::Playlist {
            id: Some("2".parse().unwrap()),
        },
    ];
    let sorts = [
        None,
        Some(Sort {
            column: SortColumn::Title,
            descending: false,
        }),
    ];
    let searches = [
        TrackFilter::All,
        TrackFilter::Ids(Arc::new(HashSet::from([1]))),
    ];
    for source in sources {
        for sort in sorts {
            for (searched, search) in searches.iter().enumerate() {
                let state = BrowseState {
                    source,
                    sort,
                    ..BrowseState::default()
                };
                let expected = source == sources[2] && sort.is_none() && searched == 0;
                assert_eq!(
                    reorderable(&state, search),
                    expected,
                    "{source:?} {sort:?} {search:?}"
                );
            }
        }
    }
}
