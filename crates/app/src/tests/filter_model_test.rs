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

#[test]
fn an_empty_main_pane_says_why_when_the_user_can_act_on_it() {
    let playlist = Source::Playlist {
        id: Some("4".parse().unwrap()),
    };
    let none = Source::Playlist { id: None };
    let library = Source::Library(Grouping::Genre);
    for (source, searching, tracks, said) in [
        (library, false, 3, false),
        (library, true, 3, false),
        (playlist, false, 1, false),
        (library, false, 0, false),
        (library, true, 0, true),
        (playlist, true, 0, true),
        (playlist, false, 0, true),
        (none, false, 0, true),
    ] {
        let reason = empty_reason(source, searching, tracks);
        assert_eq!(
            !reason.is_empty(),
            said,
            "{source:?} {searching} {tracks}: {reason:?}"
        );
    }
    assert_ne!(
        empty_reason(playlist, false, 0),
        empty_reason(none, false, 0)
    );
    assert_ne!(
        empty_reason(playlist, true, 0),
        empty_reason(playlist, false, 0)
    );
}
