//! Grouping and filtering for the browser. Everything runs over the
//! in-memory track list, so switching views or filters never queries SQLite.

use std::cell::RefCell;
use std::cmp::{Ordering, Reverse};
use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::sync::{Arc, LazyLock};
use std::time::SystemTime;

use serde::de::DeserializeOwned;
use serde::de::IntoDeserializer;
use serde::{Deserialize, Serialize};

use crate::lists::{ListId, Playlist};
use crate::normalize;
use crate::query::ListTrack;
use crate::scan::unix_ns;
use crate::window_state::BrowseState;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Grouping {
    #[default]
    AlbumArtist,
    Artist,
    Album,
    Year,
    Genre,
    FileType,
    /// Albums by their latest added track, newest first, so an album that
    /// had singles out comes back to the top when the rest arrives.
    RecentlyAdded,
    /// Played albums by their latest played track, newest first.
    RecentlyPlayed,
}

impl Grouping {
    /// In declaration order, so `name` can index by discriminant.
    pub const ALL: [Self; 8] = [
        Self::AlbumArtist,
        Self::Artist,
        Self::Album,
        Self::Year,
        Self::Genre,
        Self::FileType,
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
        self.def().field
    }

    /// Whether its album order lists years in a direction the user picks,
    /// which [`Catalog::newest_first`] reverses. Groupings ordered by date
    /// are always newest first.
    pub fn has_album_direction(self) -> bool {
        matches!(
            self.def().order.def().direction,
            Direction::UserPicks { .. }
        )
    }

    fn def(self) -> GroupingDef {
        let (order, field) = match self {
            Self::AlbumArtist => (Order::Catalog, Field::AlbumArtist),
            Self::Artist => (Order::Catalog, Field::Artist),
            Self::Album => (Order::Catalog, Field::Album),
            Self::Year => (Order::Year, Field::Year),
            Self::Genre => (Order::Catalog, Field::Genre),
            Self::FileType => (Order::Catalog, Field::FileType),
            Self::RecentlyAdded => (Order::Added, Field::Album),
            Self::RecentlyPlayed => (Order::Played, Field::Album),
        };
        GroupingDef { order, field }
    }
}

/// Where the main pane's tracks come from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    Library(Grouping),
    /// Its filter pane lists only "All", and its album order is its own.
    /// `None` while there is no playlist to show. A struct variant, as TOML
    /// can't write a `None` tuple field.
    Playlist {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<ListId>,
    },
}

impl Default for Source {
    fn default() -> Self {
        Self::Library(Grouping::default())
    }
}

impl Source {
    /// [`Grouping::has_album_direction`]; a playlist has none.
    pub fn has_album_direction(self) -> bool {
        match self {
            Self::Library(grouping) => grouping.has_album_direction(),
            Self::Playlist { .. } => false,
        }
    }
}

struct GroupingDef {
    order: Order,
    field: Field,
}

/// How a codec name is shown, in the filter pane and the File Type column.
pub fn file_type(codec: &str) -> String {
    codec.to_uppercase()
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
    FileType,
}

impl Field {
    /// In declaration order, so [`Catalog`] can index its keys by discriminant.
    pub const ALL: [Self; 6] = [
        Self::AlbumArtist,
        Self::Artist,
        Self::Album,
        Self::Year,
        Self::Genre,
        Self::FileType,
    ];

    fn def(self) -> FieldDef {
        match self {
            Self::AlbumArtist => FieldDef {
                key: KeyKind::Stored(|t| &t.album_artist_key),
                label: |t| t.album_artist.clone(),
                missing: "Unknown Artist",
                ..FieldDef::TEXT
            },
            Self::Artist => FieldDef {
                key: KeyKind::Text(|t| t.artist.as_deref().unwrap_or_default()),
                label: |t| t.artist.clone().unwrap_or_default(),
                missing: "Unknown Artist",
                ..FieldDef::TEXT
            },
            Self::Album => FieldDef {
                key: KeyKind::Album,
                label: |t| t.album.clone(),
                values: ValueOrder::ByGrouping,
                detail: Some(|t| t.album_artist.clone()),
                reversible: false,
                missing: "Unknown Album",
            },
            Self::Year => FieldDef {
                key: KeyKind::AlbumYear,
                label: |t| t.album_year.map(|y| y.to_string()).unwrap_or_default(),
                reversible: true,
                missing: "Unknown",
                ..FieldDef::TEXT
            },
            Self::Genre => FieldDef {
                key: KeyKind::Text(|t| t.genre.as_deref().unwrap_or_default()),
                label: |t| t.genre.clone().unwrap_or_default(),
                missing: "Unknown Genre",
                ..FieldDef::TEXT
            },
            Self::FileType => FieldDef {
                // The scanner stores codecs lowercase, which is their key.
                key: KeyKind::Stored(|t| &t.codec),
                label: |t| file_type(&t.codec),
                missing: "Unknown",
                ..FieldDef::TEXT
            },
        }
    }

    /// The label shown for a value whose tag is missing.
    pub fn missing(self) -> &'static str {
        self.def().missing
    }

    /// The keys track `i` is listed under. Always one for now; everything
    /// that matches tracks to values goes through here.
    fn keys<'a>(self, catalog: &'a Catalog, i: usize) -> impl Iterator<Item = Key<'a>> {
        let track = &catalog.tracks[i];
        std::iter::once(match self.def().key {
            KeyKind::Text(_) => Key::Text(catalog.text_key(self, i)),
            KeyKind::Stored(key) => Key::Text(key(track)),
            KeyKind::Album => Key::Album(track.album_id),
            KeyKind::AlbumYear => Key::Year(track.album_year),
        })
    }

    fn label(self, track: &ListTrack) -> String {
        (self.def().label)(track)
    }
}

#[derive(Clone, Copy)]
struct FieldDef {
    key: KeyKind,
    /// Empty when the tag is missing.
    label: fn(&ListTrack) -> String,
    values: ValueOrder,
    /// Shown under the label in the filter pane.
    detail: Option<fn(&ListTrack) -> String>,
    /// Whether [`Catalog::filters_newest_first`] lists the values in reverse.
    reversible: bool,
    missing: &'static str,
}

impl FieldDef {
    /// The defaults of a tag field; `key`, `label` and `missing` are
    /// placeholders every entry replaces.
    const TEXT: Self = Self {
        key: KeyKind::Text(|_| ""),
        label: |_| String::new(),
        values: ValueOrder::ByValue,
        detail: None,
        reversible: false,
        missing: "",
    };
}

#[derive(Clone, Copy)]
enum KeyKind {
    /// [`normalize::key`] of this text, computed when the catalog is built.
    Text(fn(&ListTrack) -> &str),
    /// A normalized key the track already has.
    Stored(fn(&ListTrack) -> &str),
    Album,
    AlbumYear,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ValueOrder {
    ByValue,
    /// In the order the grouping lists their first tracks.
    ByGrouping,
}

#[derive(Clone, Copy)]
enum Order {
    Catalog,
    Year,
    Added,
    Played,
}

impl Order {
    /// In declaration order, so [`Catalog`] can index its orders by discriminant.
    const ALL: [Self; 4] = [Self::Catalog, Self::Year, Self::Added, Self::Played];

    fn def(self) -> OrderDef {
        match self {
            Self::Catalog => OrderDef {
                tracks: TrackOrder::Catalog,
                direction: Direction::UserPicks {
                    within: |t| &t.album_artist_key,
                },
                depends_on_played: false,
            },
            Self::Year => OrderDef {
                tracks: TrackOrder::StableBy(|a, b| {
                    let year = |t: &ListTrack| (t.album_year.is_none(), t.album_year);
                    year(a).cmp(&year(b))
                }),
                direction: Direction::UserPicks { within: |_| "" },
                depends_on_played: false,
            },
            Self::Added => OrderDef {
                tracks: TrackOrder::AlbumRecency(|catalog, i| catalog.tracks[i].added_ns),
                direction: Direction::AlwaysNewestFirst,
                depends_on_played: false,
            },
            Self::Played => OrderDef {
                tracks: TrackOrder::AlbumRecency(Catalog::played_ns),
                direction: Direction::AlwaysNewestFirst,
                depends_on_played: true,
            },
        }
    }
}

#[derive(Clone, Copy)]
struct OrderDef {
    tracks: TrackOrder,
    direction: Direction,
    /// Whether [`Catalog::record_play`] makes it out of date.
    depends_on_played: bool,
}

#[derive(Clone, Copy)]
enum TrackOrder {
    Catalog,
    /// Catalog order, stably sorted by this.
    StableBy(fn(&ListTrack, &ListTrack) -> Ordering),
    /// Only albums with this date (of the catalog's track at an index), by
    /// their newest, newest first.
    AlbumRecency(fn(&Catalog, usize) -> Option<i64>),
}

impl TrackOrder {
    /// Indices of `tracks` in this order; `None` for catalog order, which
    /// needs no index.
    fn index(self, catalog: &Catalog) -> Option<Vec<usize>> {
        let tracks = &catalog.tracks;
        match self {
            Self::Catalog => None,
            Self::StableBy(compare) => {
                let mut order: Vec<usize> = (0..tracks.len()).collect();
                order.sort_by(|&a, &b| compare(&tracks[a], &tracks[b]));
                Some(order)
            }
            Self::AlbumRecency(date) => Some(by_recency(tracks, |i| date(catalog, i))),
        }
    }
}

#[derive(Clone, Copy)]
enum Direction {
    /// [`Catalog::newest_first`] lists years descending among the albums
    /// that share this key, keeping the order of the keys.
    UserPicks {
        within: fn(&ListTrack) -> &str,
    },
    AlwaysNewestFirst,
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
    /// Whether it's the value of tracks missing the tag.
    fn is_unknown(&self) -> bool {
        match self {
            Self::Text(text) => text.is_empty(),
            Self::UnknownYear => true,
            Self::All | Self::Album(_) | Self::Year(_) => false,
        }
    }

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

/// Which tracks a view or filter pane counts.
#[derive(Debug, Clone, Default)]
pub enum TrackFilter {
    #[default]
    All,
    /// Shared, since the filter pane and the view use the same search results.
    Ids(Arc<HashSet<i64>>),
    /// [`FilterValue::All`] passes every track.
    Field(Field, FilterValue),
    And(Vec<TrackFilter>),
}

impl TrackFilter {
    pub fn and(self, other: Self) -> Self {
        Self::And(vec![self, other])
    }

    fn passes(&self, catalog: &Catalog, i: usize) -> bool {
        match self {
            Self::All => true,
            Self::Ids(ids) => ids.contains(&catalog.tracks[i].id),
            Self::Field(field, value) => value
                .key()
                .is_none_or(|wanted| field.keys(catalog, i).any(|key| key == wanted)),
            Self::And(filters) => filters.iter().all(|filter| filter.passes(catalog, i)),
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

impl Filter {
    /// Every value of `field`, counting `tracks`.
    pub fn all(field: Field, tracks: usize) -> Self {
        Self {
            value: FilterValue::All,
            field,
            label: String::new(),
            detail: None,
            tracks,
        }
    }
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
                Some(catalog.text_key(Field::Artist, a)).filter(|k| !k.is_empty()),
                Some(catalog.text_key(Field::Artist, b)).filter(|k| !k.is_empty()),
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

/// The sort that sort menu column `column` and `descending` describe. ""
/// is album order.
fn sort_of(column: &str, descending: bool) -> Option<Sort> {
    let column = SortColumn::from_name(column)?;
    Some(Sort { column, descending })
}

/// The sort menu's column and direction for `sort`, or for album order in
/// direction `album_descending`.
pub fn sort_state(sort: Option<Sort>, album_descending: bool) -> (&'static str, bool) {
    sort.map_or(("", album_descending), |sort| {
        (sort.column.name(), sort.descending)
    })
}

/// The sort after a click on heading `clicked`, or `None` for a heading
/// that doesn't exist.
pub fn after_heading_click(current: Option<Sort>, clicked: &str) -> Option<Option<Sort>> {
    Some(Sort::after_click(current, SortColumn::from_name(clicked)?))
}

/// The sort picked from the menu, or `None` for a column that doesn't exist
/// or the sort already in effect.
fn chosen(current: Option<Sort>, column: &str, descending: bool) -> Option<Option<Sort>> {
    let sort = if column.is_empty() {
        None
    } else {
        Some(sort_of(column, descending)?)
    };
    (sort != current).then_some(sort)
}

/// What a pick from the sort menu changes.
#[derive(Debug, PartialEq, Eq)]
pub enum MenuChoice {
    Sort(Option<Sort>),
    /// Album order's direction, where descending puts the newest albums first.
    AlbumDescending(bool),
}

/// What picking `column` and `descending` from the sort menu changes, or
/// `None` for a column that doesn't exist or what's already in effect. ""
/// is album order: from a column sort it comes back in the direction it
/// had, and in album order it sets that direction if it is `directional`.
pub fn menu_choice(
    current: Option<Sort>,
    album_descending: bool,
    directional: bool,
    column: &str,
    descending: bool,
) -> Option<MenuChoice> {
    if !column.is_empty() || current.is_some() {
        return chosen(current, column, descending).map(MenuChoice::Sort);
    }
    (directional && descending != album_descending)
        .then_some(MenuChoice::AlbumDescending(descending))
}

/// The ordered track list a filter selects, as indices into the [`Catalog`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct View {
    pub tracks: Vec<usize>,
    /// Consecutive runs of one album, as ranges of `tracks`. A flat view
    /// has one range per track.
    pub albums: Vec<Range<usize>>,
    /// `None` in album order.
    pub sort: Option<Sort>,
    /// Not grouped by album: a sorted view, or a playlist.
    pub flat: bool,
}

impl View {
    pub fn len(&self) -> usize {
        self.tracks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tracks.is_empty()
    }

    fn flatten(&mut self) {
        self.albums = (0..self.tracks.len()).map(|i| i..i + 1).collect();
        self.flat = true;
    }
}

/// What the panes show for a [`BrowseState`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Browsed {
    pub filters: Vec<Filter>,
    /// Of `filters`: the selected value, or `All` when it isn't listed.
    pub selected_row: usize,
    pub view: View,
}

#[derive(Clone)]
pub struct Catalog {
    /// In album artist order: album artist key, album year (Unknown last),
    /// album title key, album, then album track order.
    tracks: Vec<ListTrack>,
    /// Per field, by discriminant: the keys of each track for
    /// [`KeyKind::Text`] fields, empty for the rest.
    text_keys: [Vec<String>; Field::ALL.len()],
    /// Title is a sort column but not a field.
    title_keys: Vec<String>,
    /// Albums with a track on a disc after the first.
    multi_disc: HashSet<i64>,
    /// Per order, by discriminant: [`TrackOrder::index`], built when first
    /// used. Shared, not `&mut`, so recording a play never copies the
    /// catalog that list models hold.
    orders: [RefCell<Option<Arc<Vec<usize>>>>; Order::ALL.len()],
    /// Plays recorded since the catalog was built, by track index. They win
    /// over the tracks' own `played_ns`, which keep the dates they were
    /// loaded with.
    played: RefCell<HashMap<usize, i64>>,
    by_id: HashMap<i64, usize>,
}

impl Catalog {
    /// `tracks` must be in [`crate::Library::track_list`] order.
    pub fn new(tracks: Vec<ListTrack>) -> Self {
        let text_keys = Field::ALL.map(|field| match field.def().key {
            KeyKind::Text(text) => keys_of(tracks.iter().map(text)),
            KeyKind::Stored(_) | KeyKind::Album | KeyKind::AlbumYear => Vec::new(),
        });
        let title_keys = tracks.iter().map(|t| normalize::key(&t.title)).collect();
        let multi_disc = tracks
            .iter()
            .filter(|t| t.disc_no.is_some_and(|d| d > 1))
            .map(|t| t.album_id)
            .collect();
        let by_id = tracks.iter().enumerate().map(|(i, t)| (t.id, i)).collect();
        Self {
            tracks,
            text_keys,
            title_keys,
            multi_disc,
            orders: Default::default(),
            played: RefCell::default(),
            by_id,
        }
    }

    /// Records that playback of track `track_id` started at `at`, moving its
    /// album to the top of [`Grouping::RecentlyPlayed`].
    pub fn record_play(&self, track_id: i64, at: SystemTime) {
        let Some(&i) = self.by_id.get(&track_id) else {
            return;
        };
        self.played.borrow_mut().insert(i, unix_ns(at));
        for order in Order::ALL {
            if order.def().depends_on_played {
                self.orders[order as usize].take();
            }
        }
    }

    fn played_ns(&self, i: usize) -> Option<i64> {
        let recorded = self.played.borrow().get(&i).copied();
        recorded.or(self.tracks[i].played_ns)
    }

    pub fn len(&self) -> usize {
        self.tracks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tracks.is_empty()
    }

    /// The key of track `i` for `field`, which must be a [`KeyKind::Text`] field.
    fn text_key(&self, field: Field, i: usize) -> &str {
        &self.text_keys[field as usize][i]
    }

    pub fn track(&self, index: usize) -> Option<&ListTrack> {
        self.tracks.get(index)
    }

    pub fn by_id(&self, id: i64) -> Option<&ListTrack> {
        self.by_id.get(&id).and_then(|&i| self.tracks.get(i))
    }

    /// The [`Grouping::Album`] value that lists track `track_id`.
    pub fn album_of(&self, track_id: i64) -> Option<FilterValue> {
        self.by_id(track_id).map(|t| FilterValue::Album(t.album_id))
    }

    /// Whether album `album_id` has more than one disc, so its track numbers
    /// show the disc.
    pub fn is_multi_disc(&self, album_id: i64) -> bool {
        self.multi_disc.contains(&album_id)
    }

    /// The filter pane for `grouping`: `All` first, then every value that has
    /// tracks. Only tracks that pass `filter` count.
    pub fn filters(&self, grouping: Grouping, filter: &TrackFilter) -> Vec<Filter> {
        self.filters_in(self.order(grouping), grouping.field(), filter)
    }

    /// [`Catalog::filters`] to go with [`Catalog::newest_first`]: albums in
    /// the order it shows them, and years newest first. `All` stays first and
    /// Unknown last.
    pub fn filters_newest_first(&self, grouping: Grouping, filter: &TrackFilter) -> Vec<Filter> {
        let field = grouping.field();
        let def = field.def();
        // Only values in grouping order take their order from the tracks;
        // other values would only take their labels from a different first
        // track.
        let mut filters = if def.values == ValueOrder::ByGrouping {
            let view = self.view(grouping, filter);
            let order = self.newest_first(view, grouping).tracks.into_iter();
            self.filters_in(order, field, filter)
        } else {
            self.filters(grouping, filter)
        };
        if def.reversible
            && let Some(values) = filters.get_mut(1..)
        {
            values.sort_by(|a, b| {
                let unknown = a.value.is_unknown().cmp(&b.value.is_unknown());
                unknown.then_with(|| b.value.cmp(&a.value))
            });
        }
        filters
    }

    /// The filter pane for `field`'s values, taking albums in `order`.
    fn filters_in(
        &self,
        order: impl Iterator<Item = usize>,
        field: Field,
        filter: &TrackFilter,
    ) -> Vec<Filter> {
        let def = field.def();
        let mut all = 0;
        // Each value with the position of its first track.
        let mut values: HashMap<Key<'_>, (usize, Filter)> = HashMap::new();
        for (position, i) in order.enumerate() {
            if !filter.passes(self, i) {
                continue;
            }
            let track = &self.tracks[i];
            all += 1;
            for key in field.keys(self, i) {
                values
                    .entry(key)
                    .or_insert_with_key(|key| {
                        let filter = Filter {
                            value: key.to_value(),
                            field,
                            label: field.label(track),
                            detail: def.detail.map(|detail| detail(track)),
                            tracks: 0,
                        };
                        (position, filter)
                    })
                    .1
                    .tracks += 1;
            }
        }
        let mut values: Vec<(usize, Filter)> = values.into_values().collect();
        match def.values {
            ValueOrder::ByGrouping => values.sort_unstable_by_key(|(position, _)| *position),
            ValueOrder::ByValue => values.sort_unstable_by(|a, b| a.1.value.cmp(&b.1.value)),
        }
        let mut filters = Vec::with_capacity(values.len() + 1);
        filters.push(Filter::all(field, all));
        filters.extend(values.into_iter().map(|(_, filter)| filter));
        filters
    }

    /// The tracks that pass `filter`, in `grouping`'s order, grouped by album.
    pub fn view(&self, grouping: Grouping, filter: &TrackFilter) -> View {
        self.view_in(self.order(grouping), filter)
    }

    /// The tracks of `track_ids` that pass `filter`, in that order and with
    /// repeats, grouped into runs of one album. Ids not in the catalog are
    /// left out.
    pub fn playlist_view(&self, track_ids: &[i64], filter: &TrackFilter) -> View {
        let order = track_ids
            .iter()
            .filter_map(|id| self.by_id.get(id).copied());
        let mut view = self.view_in(order, filter);
        view.flatten();
        view
    }

    /// For each row of `view`, a [`Catalog::playlist_view`] of `track_ids`
    /// (sorted or not), the position in `track_ids` it shows. `None` only
    /// when `view` is of another list.
    pub fn playlist_positions(&self, view: &View, track_ids: &[i64]) -> Vec<Option<usize>> {
        // A playlist view, even sorted (the sort is stable), keeps a track's
        // repeats in playlist order, and shows all of them or none.
        let mut positions: HashMap<i64, Vec<usize>> = HashMap::new();
        for (position, &id) in track_ids.iter().enumerate() {
            positions.entry(id).or_default().push(position);
        }
        let mut seen: HashMap<i64, usize> = HashMap::new();
        view.tracks
            .iter()
            .map(|&i| {
                let id = self.tracks.get(i)?.id;
                let repeat = seen.entry(id).or_default();
                let position = positions.get(&id)?.get(*repeat).copied();
                *repeat += 1;
                position
            })
            .collect()
    }

    fn view_in(&self, order: impl Iterator<Item = usize>, filter: &TrackFilter) -> View {
        let mut view = View::default();
        for i in order {
            if !filter.passes(self, i) {
                continue;
            }
            let track = &self.tracks[i];
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
        view.flatten();
        view.sort = Some(sort);
        view
    }

    /// `view`, in album order for `grouping`, with years descending: each
    /// album artist's albums newest first, or for [`Grouping::Year`] the
    /// newest year first. Unknown years stay last. Groupings ordered by date
    /// are returned unchanged.
    pub fn newest_first(&self, view: View, grouping: Grouping) -> View {
        let first = |album: &Range<usize>| &self.tracks[view.tracks[album.start]];
        let year = |album: &Range<usize>| {
            let year = first(album).album_year;
            (year.is_none(), Reverse(year))
        };
        let mut albums = view.albums.clone();
        match grouping.def().order.def().direction {
            // Stable, so the order within each year is kept.
            Direction::UserPicks { within } => {
                albums.sort_by_key(|album| (within(first(album)), year(album)));
            }
            Direction::AlwaysNewestFirst => return view,
        }
        let mut newest = View::default();
        for album in albums {
            let at = newest.tracks.len();
            newest.tracks.extend_from_slice(&view.tracks[album]);
            newest.albums.push(at..newest.tracks.len());
        }
        newest
    }

    /// The filter pane and main pane for `state`, counting only tracks that
    /// pass `search`. A playlist not in `playlists` shows no tracks.
    pub fn browse(
        &self,
        state: &BrowseState,
        playlists: &[Playlist],
        search: &TrackFilter,
    ) -> Browsed {
        let grouping = match state.source {
            Source::Library(grouping) => grouping,
            Source::Playlist { .. } => {
                let state = BrowseState {
                    selected: FilterValue::All,
                    ..*state
                };
                let view = self.browsed_view(&state, playlists, search);
                // The field is unused: `All` has no value to label.
                let all = Filter::all(Grouping::default().field(), view.len());
                return Browsed {
                    filters: vec![all],
                    selected_row: 0,
                    view,
                };
            }
        };
        let filters = if state.album_descending {
            self.filters_newest_first(grouping, search)
        } else {
            self.filters(grouping, search)
        };
        let selected_row = filters
            .iter()
            .position(|f| f.value == state.selected)
            .unwrap_or(0);
        let selected = filters[selected_row].value.clone();
        let view = self.browsed_view(&BrowseState { selected, ..*state }, playlists, search);
        Browsed {
            filters,
            selected_row,
            view,
        }
    }

    /// [`Catalog::browse`]'s view, without the filter pane. Empty when
    /// `state.selected` no longer exists.
    pub fn browsed_view(
        &self,
        state: &BrowseState,
        playlists: &[Playlist],
        search: &TrackFilter,
    ) -> View {
        let view = match state.source {
            Source::Library(grouping) => {
                let selected = TrackFilter::Field(grouping.field(), state.selected.clone());
                self.view(grouping, &search.clone().and(selected))
            }
            Source::Playlist { id } => {
                let track_ids = playlists.iter().find(|p| Some(p.id) == id);
                let track_ids = track_ids.map_or(&[][..], |p| &p.track_ids);
                self.playlist_view(track_ids, search)
            }
        };
        match (state.sort, state.source) {
            (Some(sort), _) => self.sorted(view, sort),
            (None, Source::Library(grouping)) if state.album_descending => {
                self.newest_first(view, grouping)
            }
            (None, _) => view,
        }
    }

    fn order(&self, grouping: Grouping) -> Box<dyn Iterator<Item = usize> + '_> {
        let order = grouping.def().order;
        let index = {
            let mut cached = self.orders[order as usize].borrow_mut();
            if cached.is_none() {
                *cached = order.def().tracks.index(self).map(Arc::new);
            }
            cached.clone()
        };
        match index {
            None => Box::new(0..self.tracks.len()),
            Some(index) => Box::new((0..index.len()).map(move |k| index[k])),
        }
    }
}

/// [`normalize::key`] of each of `texts`, computed once per distinct text:
/// tags such as artists repeat across many tracks.
fn keys_of<'a>(texts: impl Iterator<Item = &'a str>) -> Vec<String> {
    let mut keys: HashMap<&str, String> = HashMap::new();
    texts
        .map(|text| {
            keys.entry(text)
                .or_insert_with(|| normalize::key(text))
                .clone()
        })
        .collect()
}

/// Indices of the tracks of albums with a `date`, newest album first. An
/// album's date is its newest track's, like its year is its max year. Ties
/// keep catalog order.
fn by_recency(tracks: &[ListTrack], date: impl Fn(usize) -> Option<i64>) -> Vec<usize> {
    let mut latest: HashMap<i64, i64> = HashMap::new();
    for (i, track) in tracks.iter().enumerate() {
        if let Some(date) = date(i) {
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
