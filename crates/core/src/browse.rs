//! Grouping and filtering for the browser. Everything runs over the
//! in-memory track list, so switching views or filters never queries SQLite.

use std::cmp::{Ordering, Reverse};
use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::sync::LazyLock;
use std::time::SystemTime;

use serde::de::IntoDeserializer;
use serde::{Deserialize, Serialize};

use crate::normalize;
use crate::query::ListTrack;
use crate::scan::unix_ns;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Grouping {
    #[default]
    AlbumArtist,
    Artist,
    Album,
    Year,
    /// Albums by their latest added track, newest first, so an album that
    /// had singles out comes back to the top when the rest arrives.
    RecentlyAdded,
    /// Played albums by their latest played track, newest first.
    RecentlyPlayed,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub enum FilterValue {
    #[default]
    All,
    /// An album artist key.
    AlbumArtist(String),
    /// A track artist key; tracks without an artist tag have the key "".
    Artist(String),
    /// An album; also the values of the recency groupings.
    Album(i64),
    /// An album max year; `None` = Unknown.
    Year(Option<i32>),
}

/// One entry of the filter pane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Filter {
    pub value: FilterValue,
    /// The display string from the tags. Empty when the tag is missing (an
    /// unknown artist); unused for `All` and `Year`.
    pub label: String,
    /// For albums, the album artist.
    pub detail: Option<String>,
    pub tracks: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SortColumn {
    TrackNumber,
    Title,
    Artist,
    Duration,
    FileType,
    BitRate,
}

impl SortColumn {
    /// In declaration order, so `name` can index by discriminant.
    pub const ALL: [Self; 6] = [
        Self::TrackNumber,
        Self::Title,
        Self::Artist,
        Self::Duration,
        Self::FileType,
        Self::BitRate,
    ];

    /// The name it's saved under, which QML also uses for the column.
    pub fn name(self) -> &'static str {
        static NAMES: LazyLock<Vec<String>> = LazyLock::new(|| {
            SortColumn::ALL
                .iter()
                .map(|column| {
                    toml::Value::try_from(column)
                        .ok()
                        .and_then(|name| name.as_str().map(str::to_owned))
                        .unwrap_or_default()
                })
                .collect()
        });
        NAMES.get(self as usize).map_or("", String::as_str)
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::deserialize(IntoDeserializer::<serde::de::value::Error>::into_deserializer(name)).ok()
    }

    /// The order of tracks `a` and `b` of `catalog`. Track numbers sort by
    /// disc, then track. Tracks without a track number, an artist, a known
    /// length, or a known bit rate come last in both directions.
    fn compare(self, catalog: &Catalog, a: usize, b: usize, descending: bool) -> Ordering {
        let (x, y) = (&catalog.tracks[a], &catalog.tracks[b]);
        match self {
            Self::TrackNumber => missing_last(
                x.track_no.map(|no| (x.disc_no.unwrap_or(1), no)),
                y.track_no.map(|no| (y.disc_no.unwrap_or(1), no)),
                descending,
            ),
            Self::Title => missing_last(
                Some(&catalog.title_keys[a]),
                Some(&catalog.title_keys[b]),
                descending,
            ),
            Self::Artist => missing_last(
                Some(&catalog.artist_keys[a]).filter(|k| !k.is_empty()),
                Some(&catalog.artist_keys[b]).filter(|k| !k.is_empty()),
                descending,
            ),
            // lofty reports 0 when it can't read the length.
            Self::Duration => missing_last(
                (x.duration_ms > 0).then_some(x.duration_ms),
                (y.duration_ms > 0).then_some(y.duration_ms),
                descending,
            ),
            Self::FileType => missing_last(Some(&x.codec), Some(&y.codec), descending),
            Self::BitRate => missing_last(x.bitrate_kbps, y.bitrate_kbps, descending),
        }
    }
}

/// A column sort of the main pane, which replaces album order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sort {
    pub column: SortColumn,
    pub descending: bool,
}

impl Sort {
    /// The sort after clicking `column`'s heading: ascending, then
    /// descending, then back to album order (`None`).
    pub fn after_click(current: Option<Sort>, column: SortColumn) -> Option<Sort> {
        match current {
            Some(sort) if sort.column == column && sort.descending => None,
            Some(sort) if sort.column == column => Some(Sort {
                column,
                descending: true,
            }),
            _ => Some(Sort {
                column,
                descending: false,
            }),
        }
    }
}

/// The ordered track list a filter selects, as indices into the [`Catalog`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct View {
    pub tracks: Vec<usize>,
    /// Consecutive runs of one album, as ranges of `tracks`. A sorted view
    /// isn't grouped by album and has one range per track.
    pub albums: Vec<Range<usize>>,
    /// `None` in album order.
    pub sort: Option<Sort>,
}

impl View {
    pub fn len(&self) -> usize {
        self.tracks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tracks.is_empty()
    }
}

#[derive(Clone)]
pub struct Catalog {
    /// In album artist order: album artist key, album year (Unknown last),
    /// album title key, album, then album track order.
    tracks: Vec<ListTrack>,
    artist_keys: Vec<String>,
    title_keys: Vec<String>,
    /// Albums with a track on a disc after the first.
    multi_disc: HashSet<i64>,
    /// Indices of `tracks` in year order: album year (Unknown last), then
    /// album artist order.
    by_year: Vec<usize>,
    /// Indices of `tracks` in [`Grouping::RecentlyAdded`] order, without
    /// undated albums.
    by_added: Vec<usize>,
    /// Indices of `tracks` in [`Grouping::RecentlyPlayed`] order, without
    /// albums never played.
    by_played: Vec<usize>,
    by_id: HashMap<i64, usize>,
}

impl Catalog {
    /// `tracks` must be in [`crate::Library::track_list`] order.
    pub fn new(tracks: Vec<ListTrack>) -> Self {
        let artist_keys = tracks
            .iter()
            .map(|t| normalize::key(t.artist.as_deref().unwrap_or_default()))
            .collect();
        let title_keys = tracks.iter().map(|t| normalize::key(&t.title)).collect();
        let multi_disc = tracks
            .iter()
            .filter(|t| t.disc_no.is_some_and(|d| d > 1))
            .map(|t| t.album_id)
            .collect();
        let mut by_year: Vec<usize> = (0..tracks.len()).collect();
        // Stable, so album artist order is kept within each year.
        by_year.sort_by_key(|&i| {
            let year = tracks[i].album_year;
            (year.is_none(), year)
        });
        let by_added = by_recency(&tracks, |t| t.added_ns);
        let by_played = by_recency(&tracks, |t| t.played_ns);
        let by_id = tracks.iter().enumerate().map(|(i, t)| (t.id, i)).collect();
        Self {
            tracks,
            artist_keys,
            title_keys,
            multi_disc,
            by_year,
            by_added,
            by_played,
            by_id,
        }
    }

    /// Records that playback of track `track_id` started at `at`, moving its
    /// album to the top of [`Grouping::RecentlyPlayed`].
    pub fn record_play(&mut self, track_id: i64, at: SystemTime) {
        let Some(track) = self
            .by_id
            .get(&track_id)
            .and_then(|&i| self.tracks.get_mut(i))
        else {
            return;
        };
        track.played_ns = Some(unix_ns(at));
        self.by_played = by_recency(&self.tracks, |t| t.played_ns);
    }

    pub fn len(&self) -> usize {
        self.tracks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tracks.is_empty()
    }

    pub fn track(&self, index: usize) -> Option<&ListTrack> {
        self.tracks.get(index)
    }

    pub fn by_id(&self, id: i64) -> Option<&ListTrack> {
        self.by_id.get(&id).and_then(|&i| self.tracks.get(i))
    }

    /// Whether album `album_id` has more than one disc, so its track numbers
    /// show the disc.
    pub fn is_multi_disc(&self, album_id: i64) -> bool {
        self.multi_disc.contains(&album_id)
    }

    /// The filter pane for `grouping`: `All` first, then every value that has
    /// tracks. With `matches` (search results), only tracks in it count.
    pub fn filters(&self, grouping: Grouping, matches: Option<&HashSet<i64>>) -> Vec<Filter> {
        let mut all = 0;
        let mut values: HashMap<FilterValue, (SortKey, Filter)> = HashMap::new();
        for (position, i) in self.order(grouping).enumerate() {
            let track = &self.tracks[i];
            if matches.is_some_and(|m| !m.contains(&track.id)) {
                continue;
            }
            all += 1;
            let (key, value) = self.value_of(grouping, position, i);
            values
                .entry(value)
                .or_insert_with_key(|value| {
                    let filter = Filter {
                        value: value.clone(),
                        label: match grouping {
                            Grouping::AlbumArtist => track.album_artist.clone(),
                            Grouping::Artist => track.artist.clone().unwrap_or_default(),
                            Grouping::Album
                            | Grouping::RecentlyAdded
                            | Grouping::RecentlyPlayed => track.album.clone(),
                            Grouping::Year => String::new(),
                        },
                        detail: matches!(value, FilterValue::Album(_))
                            .then(|| track.album_artist.clone()),
                        tracks: 0,
                    };
                    (key, filter)
                })
                .1
                .tracks += 1;
        }
        let mut values: Vec<(SortKey, Filter)> = values.into_values().collect();
        values.sort_unstable_by(|a, b| a.0.cmp(&b.0));
        let mut filters = Vec::with_capacity(values.len() + 1);
        filters.push(Filter {
            value: FilterValue::All,
            label: String::new(),
            detail: None,
            tracks: all,
        });
        filters.extend(values.into_iter().map(|(_, filter)| filter));
        filters
    }

    /// The tracks `value` selects in `grouping`, grouped by album. With
    /// `matches` (search results), only tracks in it are included.
    pub fn view(
        &self,
        grouping: Grouping,
        value: &FilterValue,
        matches: Option<&HashSet<i64>>,
    ) -> View {
        let mut view = View::default();
        for i in self.order(grouping) {
            let track = &self.tracks[i];
            if matches.is_some_and(|m| !m.contains(&track.id)) {
                continue;
            }
            let selected = match value {
                FilterValue::All => true,
                FilterValue::AlbumArtist(key) => track.album_artist_key == *key,
                FilterValue::Artist(key) => self.artist_keys[i] == *key,
                FilterValue::Album(id) => track.album_id == *id,
                FilterValue::Year(year) => track.album_year == *year,
            };
            if !selected {
                continue;
            }
            let at = view.tracks.len();
            let same_album = view
                .tracks
                .last()
                .is_some_and(|&prev| self.tracks[prev].album_id == track.album_id);
            match view.albums.last_mut() {
                Some(album) if same_album => album.end = at + 1,
                _ => view.albums.push(at..at + 1),
            }
            view.tracks.push(i);
        }
        view
    }

    /// `view` reordered by `sort`. Ties keep `view`'s order.
    pub fn sorted(&self, mut view: View, sort: Sort) -> View {
        view.tracks
            .sort_by(|&a, &b| sort.column.compare(self, a, b, sort.descending));
        view.albums = (0..view.tracks.len()).map(|i| i..i + 1).collect();
        view.sort = Some(sort);
        view
    }

    fn order(&self, grouping: Grouping) -> Box<dyn Iterator<Item = usize> + '_> {
        match grouping {
            Grouping::Year => Box::new(self.by_year.iter().copied()),
            Grouping::RecentlyAdded => Box::new(self.by_added.iter().copied()),
            Grouping::RecentlyPlayed => Box::new(self.by_played.iter().copied()),
            Grouping::AlbumArtist | Grouping::Artist | Grouping::Album => {
                Box::new(0..self.tracks.len())
            }
        }
    }

    /// The value track `i`, at `position` in `grouping`'s order, has in
    /// `grouping`, and where that value sorts.
    fn value_of(&self, grouping: Grouping, position: usize, i: usize) -> (SortKey, FilterValue) {
        let track = &self.tracks[i];
        match grouping {
            Grouping::AlbumArtist => (
                SortKey::Text(track.album_artist_key.clone()),
                FilterValue::AlbumArtist(track.album_artist_key.clone()),
            ),
            Grouping::Artist => (
                SortKey::Text(self.artist_keys[i].clone()),
                FilterValue::Artist(self.artist_keys[i].clone()),
            ),
            // Only the first track of each album is kept as the key, and it
            // is visited first, so albums keep the grouping's order.
            Grouping::Album | Grouping::RecentlyAdded | Grouping::RecentlyPlayed => (
                SortKey::Position(position),
                FilterValue::Album(track.album_id),
            ),
            Grouping::Year => (
                SortKey::Year(track.album_year.is_none(), track.album_year),
                FilterValue::Year(track.album_year),
            ),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum SortKey {
    Text(String),
    Position(usize),
    Year(bool, Option<i32>),
}

/// Indices of the tracks of albums with a `date`, newest album first. An
/// album's date is its newest track's, like its year is its max year. Ties
/// keep catalog order.
fn by_recency(tracks: &[ListTrack], date: impl Fn(&ListTrack) -> Option<i64>) -> Vec<usize> {
    let mut latest: HashMap<i64, i64> = HashMap::new();
    for track in tracks {
        if let Some(date) = date(track) {
            latest
                .entry(track.album_id)
                .and_modify(|l| *l = (*l).max(date))
                .or_insert(date);
        }
    }
    let mut order: Vec<(Reverse<i64>, usize)> = tracks
        .iter()
        .enumerate()
        .filter_map(|(i, t)| Some((Reverse(*latest.get(&t.album_id)?), i)))
        .collect();
    // Albums are contiguous in catalog order, so they stay contiguous.
    order.sort_unstable();
    order.into_iter().map(|(_, i)| i).collect()
}

fn missing_last<T: Ord>(a: Option<T>, b: Option<T>, descending: bool) -> Ordering {
    match (a, b) {
        (Some(x), Some(y)) if descending => y.cmp(&x),
        (Some(x), Some(y)) => x.cmp(&y),
        (x, y) => x.is_none().cmp(&y.is_none()),
    }
}

#[cfg(test)]
#[path = "tests/browse_test.rs"]
mod tests;
