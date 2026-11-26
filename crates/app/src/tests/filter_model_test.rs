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

fn sources() -> Vec<Source> {
    let mut sources: Vec<Source> = Grouping::ALL.into_iter().map(Source::Library).collect();
    sources.push(Source::Playlist { id: None });
    sources.push(Source::Playlist {
        id: Some("4".parse().unwrap()),
    });
    sources
}

#[test]
fn an_empty_main_pane_says_why_or_shows_the_empty_library() {
    let bools = [false, true];
    for source in sources() {
        for loading in bools {
            for searching in bools {
                for tracks in [0, 1, 3] {
                    for library_empty in bools {
                        for has_folders in bools {
                            let state = PaneState {
                                loading,
                                source,
                                searching,
                                tracks,
                                library_empty,
                                has_folders,
                            };
                            let pane = empty_pane(&state);
                            let library = matches!(source, Source::Library(_));
                            let expected = if loading || tracks > 0 {
                                EmptyPane::Tracks
                            } else if library && !searching && library_empty {
                                EmptyPane::Library { has_folders }
                            } else {
                                EmptyPane::Hint(pane.hint())
                            };
                            assert_eq!(pane, expected, "{state:?}");
                            assert_eq!(
                                pane.hint().is_empty(),
                                !matches!(pane, EmptyPane::Hint(_)),
                                "{state:?}"
                            );
                            assert_eq!(
                                pane.library_heading().is_empty(),
                                !matches!(pane, EmptyPane::Library { .. }),
                                "{state:?}"
                            );
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn each_reason_for_an_empty_pane_reads_differently() {
    let empty = |source, searching| {
        empty_pane(&PaneState {
            loading: false,
            source,
            searching,
            tracks: 0,
            library_empty: false,
            has_folders: true,
        })
    };
    let panes = [
        empty(
            Source::Playlist {
                id: Some("4".parse().unwrap()),
            },
            false,
        ),
        empty(Source::Playlist { id: None }, false),
        empty(Source::Library(Grouping::Album), true),
        empty(Source::Library(Grouping::Album), false),
        empty(Source::Library(Grouping::RecentlyPlayed), false),
        EmptyPane::Library { has_folders: true },
        EmptyPane::Library { has_folders: false },
    ];
    let texts = panes.map(|pane| format!("{}{}", pane.hint(), pane.library_heading()));
    for (i, text) in texts.iter().enumerate() {
        assert!(!text.is_empty() && !texts[..i].contains(text), "{text:?}");
    }
}
