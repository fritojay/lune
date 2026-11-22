//! Grouping and filtering for the browser. Everything runs over the
//! in-memory track list, so switching views or filters never queries SQLite.

use std::cmp::{Ordering, Reverse};
use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::sync::LazyLock;
use std::time::SystemTime;

use serde::de::DeserializeOwned;
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
    Genre,
    /// Albums by their latest added track, newest first, so an album that
    /// had singles out comes back to the top when the rest arrives.
    RecentlyAdded,
    /// Played albums by their latest played track, newest first.
    RecentlyPlayed,
}

impl Grouping {
    /// In declaration order, so `name` can index by discriminant.
    pub const ALL: [Self; 7] = [
        Self::AlbumArtist,
        Self::Artist,
        Self::Album,
        Self::Year,
        Self::Genre,
        Self::RecentlyAdded,
        Self::RecentlyPlayed,
    ];

    /// The name it's saved under, which QML also uses for the grouping.
    pub fn name(self) -> &'static str {
        static NAMES: LazyLock<Vec<String>> = LazyLock::new(|| serde_names(&Grouping::ALL));
        NAMES.get(self as usize).map_or("", String::as_str)
    }

    pub fn from_name(name: &str) -> Option<Self> {
        from_serde_name(name)
    }

    pub fn field(self) -> Field {
        self.def().1
    }

    /// The order its values are listed in, and what they are.
    fn def(self) -> (Order, Field) {
        match self {
            Self::AlbumArtist => (Order::Catalog, Field::AlbumArtist),
            Self::Artist => (Order::Catalog, Field::Artist),
            Self::Album => (Order::Catalog, Field::Album),
            Self::Year => (Order::Year, Field::Year),
            Self::Genre => (Order::Catalog, Field::Genre),
            Self::RecentlyAdded => (Order::Added, Field::Album),
            Self::RecentlyPlayed => (Order::Played, Field::Album),
        }
    }
}

/// What a grouping's values are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    AlbumArtist,
    /// The track artist.
    Artist,
    Album,
    /// The album's max year.
    Year,
    /// The track genre.
    Genre,
}

impl Field {
    fn key<'a>(self, catalog: &'a Catalog, i: usize) -> Key<'a> {
        let track = &catalog.tracks[i];
        match self {
            Self::AlbumArtist => Key::Text(&track.album_artist_key),
            Self::Artist => Key::Text(&catalog.artist_keys[i]),
            Self::Album => Key::Album(track.album_id),
            Self::Year => Key::Year(track.album_year),
            Self::Genre => Key::Text(&catalog.genre_keys[i]),
        }
    }

    fn label(self, track: &ListTrack) -> String {
        match self {
            Self::AlbumArtist => track.album_artist.clone(),
            Self::Artist => track.artist.clone().unwrap_or_default(),
            Self::Album => track.album.clone(),
            Self::Year => track.album_year.map(|y| y.to_string()).unwrap_or_default(),
            Self::Genre => track.genre.clone().unwrap_or_default(),
        }
    }
}

#[derive(Clone, Copy)]
enum Order {
    Catalog,
    Year,
    Added,
    Played,
}

/// A value of a grouping; the grouping's [`Field`] says what it is.
#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FilterValue {
    #[default]
    All,
    /// A tag key; tracks without the tag have the key "". The aliases read
    /// window state saved when each field had its own variant.
    #[serde(alias = "album_artist", alias = "artist", alias = "genre")]
    Text(String),
    Album(i64),
    Year(i32),
    /// TOML has no null, so this isn't `Year(None)`. After `Year` so it
    /// sorts last.
    UnknownYear,
}

impl FilterValue {
    /// `None` for `All`.
    fn key(&self) -> Option<Key<'_>> {
        match self {
            Self::All => None,
            Self::Text(text) => Some(Key::Text(text)),
            Self::Album(id) => Some(Key::Album(*id)),
            Self::Year(year) => Some(Key::Year(Some(*year))),
            Self::UnknownYear => Some(Key::Year(None)),
        }
    }
}

/// A [`FilterValue`] borrowed from the catalog, so matching tracks against
/// one doesn't allocate per track.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Key<'a> {
    Text(&'a str),
    Album(i64),
    Year(Option<i32>),
}

impl Key<'_> {
    fn to_value(self) -> FilterValue {
        match self {
            Self::Text(text) => FilterValue::Text(text.to_owned()),
            Self::Album(id) => FilterValue::Album(id),
            Self::Year(Some(year)) => FilterValue::Year(year),
            Self::Year(None) => FilterValue::UnknownYear,
        }
    }
}

/// One entry of the filter pane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Filter {
    pub value: FilterValue,
    pub field: Field,
    /// The display string from the tags. Empty when the tag is missing and
    /// for `All`.
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
        static NAMES: LazyLock<Vec<String>> = LazyLock::new(|| serde_names(&SortColumn::ALL));
        NAMES.get(self as usize).map_or("", String::as_str)
    }

    pub fn from_name(name: &str) -> Option<Self> {
        from_serde_name(name)
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
    genre_keys: Vec<String>,
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
        let genre_keys = tracks
            .iter()
            .map(|t| normalize::key(t.genre.as_deref().unwrap_or_default()))
            .collect();
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
            genre_keys,
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
        let field = grouping.field();
        let mut all = 0;
        // Each value with the position of its first track.
        let mut values: HashMap<Key<'_>, (usize, Filter)> = HashMap::new();
        for (position, i) in self.order(grouping).enumerate() {
            let track = &self.tracks[i];
            if matches.is_some_and(|m| !m.contains(&track.id)) {
                continue;
            }
            all += 1;
            values
                .entry(field.key(self, i))
                .or_insert_with_key(|key| {
                    let filter = Filter {
                        value: key.to_value(),
                        field,
                        label: field.label(track),
                        detail: (field == Field::Album).then(|| track.album_artist.clone()),
                        tracks: 0,
                    };
                    (position, filter)
                })
                .1
                .tracks += 1;
        }
        let mut values: Vec<(usize, Filter)> = values.into_values().collect();
        // Albums keep the grouping's order; other values sort by themselves.
        if field == Field::Album {
            values.sort_unstable_by_key(|(position, _)| *position);
        } else {
            values.sort_unstable_by(|a, b| a.1.value.cmp(&b.1.value));
        }
        let mut filters = Vec::with_capacity(values.len() + 1);
        filters.push(Filter {
            value: FilterValue::All,
            field,
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
        let field = grouping.field();
        let wanted = value.key();
        let mut view = View::default();
        for i in self.order(grouping) {
            let track = &self.tracks[i];
            if matches.is_some_and(|m| !m.contains(&track.id)) {
                continue;
            }
            if wanted.is_some_and(|wanted| field.key(self, i) != wanted) {
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
        match grouping.def().0 {
            Order::Catalog => Box::new(0..self.tracks.len()),
            Order::Year => Box::new(self.by_year.iter().copied()),
            Order::Added => Box::new(self.by_added.iter().copied()),
            Order::Played => Box::new(self.by_played.iter().copied()),
        }
    }
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

/// The names unit variants serialize as.
fn serde_names<T: Serialize>(all: &[T]) -> Vec<String> {
    all.iter()
        .map(|value| {
            toml::Value::try_from(value)
                .ok()
                .and_then(|name| name.as_str().map(str::to_owned))
                .unwrap_or_default()
        })
        .collect()
}

fn from_serde_name<T: DeserializeOwned>(name: &str) -> Option<T> {
    T::deserialize(IntoDeserializer::<serde::de::value::Error>::into_deserializer(name)).ok()
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
