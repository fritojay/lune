//! The player controller: transport, now playing, and every queue edit. Each
//! edit tells the engine (`queue_changed`) so a pre-buffered next track that
//! is no longer next is dropped, then emits `queueEdited` and saves.

#![expect(
    unsafe_code,
    reason = "the CXX-Qt bridge below generates its FFI glue as unsafe code"
)]

use std::collections::BTreeSet;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::{QList, QString, QUrl};
use lune_core::{Catalog, ListId, Queue, QueueItem, SlotId, TrackRef, View};
use lune_engine::{EntryId, PlaybackState, Player, PlayerConfig};

use crate::media_controls::{self, MediaCommand, MediaControls, NowPlaying, Playback};
use crate::property::set;
use crate::session::{self, Change, Save};
use crate::{covers, format};

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
        include!("cxx-qt-lib/qurl.h");
        type QUrl = cxx_qt_lib::QUrl;
        include!("cxx-qt-lib/qlist.h");
        type QList_i32 = cxx_qt_lib::QList<i32>;
        type QList_i64 = cxx_qt_lib::QList<i64>;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qproperty(bool, available, READ, NOTIFY)]
        #[qproperty(bool, playing, READ, NOTIFY)]
        #[qproperty(bool, has_track, cxx_name = "hasTrack", READ, NOTIFY)]
        #[qproperty(f64, position, READ, NOTIFY)]
        #[qproperty(f64, duration, READ, NOTIFY)]
        #[qproperty(QString, title, READ, NOTIFY)]
        #[qproperty(QString, artist, READ, NOTIFY)]
        #[qproperty(QString, album, READ, NOTIFY)]
        #[qproperty(QUrl, cover, READ, NOTIFY)]
        #[qproperty(i64, current_track_id, cxx_name = "currentTrackId", READ, NOTIFY)]
        #[qproperty(i32, queue_index, cxx_name = "queueIndex", READ, NOTIFY)]
        #[qproperty(bool, shuffle, READ, NOTIFY)]
        #[qproperty(f64, volume, READ, NOTIFY)]
        #[qproperty(bool, muted, READ, NOTIFY)]
        #[qproperty(QString, error, READ, NOTIFY)]
        type PlayerController = super::PlayerControllerRust;
    }

    // Each `source` is a list of tracks by its name in `Selection::new`, and
    // `rows` are some of its rows, in any order.
    unsafe extern "RustQt" {
        /// From the main pane, replaces the queue with the whole view and
        /// plays from `row`; from the queue, plays its `row`.
        #[qinvokable]
        fn play(self: Pin<&mut PlayerController>, source: &QString, row: i32);

        /// Inserts the rows' tracks after the current track.
        #[qinvokable]
        #[cxx_name = "playNext"]
        fn play_next(self: Pin<&mut PlayerController>, source: &QString, rows: &QList_i32);

        #[qinvokable]
        #[cxx_name = "addToQueue"]
        fn add_to_queue(self: Pin<&mut PlayerController>, source: &QString, rows: &QList_i32);

        /// `playlist` is its `ListId` as text.
        #[qinvokable]
        #[cxx_name = "addToPlaylist"]
        fn add_to_playlist(
            self: Pin<&mut PlayerController>,
            source: &QString,
            rows: &QList_i32,
            playlist: &QString,
        );

        /// The rows' track ids, in list order.
        #[qinvokable]
        #[cxx_name = "trackIds"]
        fn track_ids(self: &PlayerController, source: &QString, rows: &QList_i32) -> QList_i64;

        /// The track at `row`, or -1 when it isn't in the library.
        #[qinvokable]
        #[cxx_name = "trackId"]
        fn track_id(self: &PlayerController, source: &QString, row: i32) -> i64;

        #[qinvokable]
        #[cxx_name = "removeQueueRows"]
        fn remove_queue_rows(self: Pin<&mut PlayerController>, rows: &QList_i32);

        /// Moves the queue's `rows`, keeping their order, to before the row
        /// now at `to`.
        #[qinvokable]
        #[cxx_name = "moveQueueRows"]
        fn move_queue_rows(self: Pin<&mut PlayerController>, rows: &QList_i32, to: i32);

        #[qinvokable]
        #[cxx_name = "clearQueue"]
        fn clear_queue(self: Pin<&mut PlayerController>);

        #[qinvokable]
        #[cxx_name = "setShuffle"]
        fn set_shuffle_from_qml(self: Pin<&mut PlayerController>, on: bool);

        #[qinvokable]
        #[cxx_name = "togglePause"]
        fn toggle_pause(self: Pin<&mut PlayerController>);

        #[qinvokable]
        fn next(self: Pin<&mut PlayerController>);

        #[qinvokable]
        fn previous(self: Pin<&mut PlayerController>);

        #[qinvokable]
        fn seek(self: Pin<&mut PlayerController>, seconds: f64);

        /// Seeks relative to the current position.
        #[qinvokable]
        #[cxx_name = "seekBy"]
        fn seek_by(self: Pin<&mut PlayerController>, seconds: f64);

        #[qinvokable]
        #[cxx_name = "setVolume"]
        fn set_volume_from_qml(self: Pin<&mut PlayerController>, volume: f64);

        /// Silences output while keeping the volume; a volume change unmutes.
        #[qinvokable]
        #[cxx_name = "toggleMute"]
        fn toggle_mute(self: Pin<&mut PlayerController>);

        /// Reads the engine's status; call a few times a second.
        #[qinvokable]
        fn poll(self: Pin<&mut PlayerController>);

        #[qinvokable]
        #[cxx_name = "clearError"]
        fn clear_error(self: Pin<&mut PlayerController>);
    }

    impl cxx_qt::Threading for PlayerController {}
    impl cxx_qt::Initialize for PlayerController {}
}

pub struct PlayerControllerRust {
    available: bool,
    playing: bool,
    has_track: bool,
    position: f64,
    duration: f64,
    title: QString,
    artist: QString,
    album: QString,
    cover: QUrl,
    current_track_id: i64,
    queue_index: i32,
    shuffle: bool,
    volume: f64,
    muted: bool,
    error: QString,
    player: Option<Player>,
    /// Whether the engine's output failure has been reported, so a
    /// dismissed error doesn't come back on the next poll.
    output_failed: bool,
    /// The entry the engine last reported playing.
    playing_entry: Option<EntryId>,
    cover_id: Option<i64>,
    /// Media keys and the OS Now Playing display.
    media: Option<Box<dyn MediaControls>>,
    /// What the OS was last told, to send only changes.
    media_now: NowPlaying,
    media_state: Option<PlaybackState>,
}

impl Default for PlayerControllerRust {
    fn default() -> Self {
        Self {
            available: false,
            playing: false,
            has_track: false,
            position: 0.0,
            duration: 0.0,
            title: QString::default(),
            artist: QString::default(),
            album: QString::default(),
            cover: QUrl::default(),
            current_track_id: -1,
            queue_index: -1,
            shuffle: false,
            volume: 1.0,
            muted: false,
            error: QString::default(),
            player: None,
            output_failed: false,
            playing_entry: None,
            cover_id: None,
            media: None,
            media_now: NowPlaying::default(),
            media_state: None,
        }
    }
}

impl cxx_qt::Initialize for qobject::PlayerController {
    fn initialize(mut self: Pin<&mut Self>) {
        match Player::new(PlayerConfig::default(), Arc::new(session::EngineQueue)) {
            Ok(player) => {
                self.as_mut().rust_mut().player = Some(player);
                set!(self, available, available_changed, true);
            }
            Err(err) => tracing::error!(%err, "no audio output; playback is disabled"),
        }
        covers::on_ready(self.qt_thread(), Self::cover_ready);
        // Created on the Qt (main) thread, as macOS requires.
        let qt_thread = self.qt_thread();
        let media = media_controls::system(move |command| {
            let _ = qt_thread.queue(move |controller| controller.media_command(command));
        });
        self.as_mut().rust_mut().media = media;
        session::on_change(self.qt_thread(), |controller, change| {
            if change == Change::Catalog {
                controller.refresh();
            }
        });
    }
}

/// Rows of one of the lists QML shows tracks in, ascending and without
/// repeats.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Selection {
    /// Indices into the main pane's view.
    View(Vec<usize>),
    Queue(Vec<usize>),
}

impl Selection {
    /// Negative rows are left out.
    pub fn new(source: &str, rows: impl IntoIterator<Item = i32>) -> Option<Self> {
        let rows: Vec<usize> = rows
            .into_iter()
            .filter_map(index)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        match source {
            "view" => Some(Self::View(rows)),
            "queue" => Some(Self::Queue(rows)),
            _ => {
                tracing::error!(source, "no such track list");
                None
            }
        }
    }

    /// Its tracks, in order. Rows past the end of the list are left out.
    pub fn tracks(&self, catalog: &Catalog, view: &View, queue: &[QueueItem]) -> Vec<TrackRef> {
        match self {
            Self::View(rows) => rows_of(&view.tracks, rows)
                .filter_map(|&i| catalog.track(i))
                .map(|t| TrackRef {
                    id: t.id,
                    path: t.path.clone(),
                })
                .collect(),
            Self::Queue(rows) => rows_of(queue, rows)
                .map(|item| TrackRef {
                    id: item.track_id,
                    path: item.path.clone(),
                })
                .collect(),
        }
    }
}

fn rows_of<'a, T>(list: &'a [T], rows: &'a [usize]) -> impl Iterator<Item = &'a T> {
    rows.iter().filter_map(|&row| list.get(row))
}

fn index(i: i32) -> Option<usize> {
    usize::try_from(i).ok()
}

fn selection(source: &QString, rows: &QList<i32>) -> Option<Selection> {
    Selection::new(&source.to_string(), rows.iter().copied())
}

fn queue_slots(queue: &Queue, rows: &QList<i32>) -> Vec<SlotId> {
    let rows = rows.iter().filter_map(|&row| index(row));
    rows.filter_map(|row| queue.items().get(row))
        .map(|item| item.slot)
        .collect()
}

/// `None` until the library has loaded.
fn selected_tracks(selection: &Selection) -> Option<Vec<TrackRef>> {
    let catalog = session::catalog()?;
    let queue = session::lock_queue();
    Some(selection.tracks(&catalog, &session::view(), queue.items()))
}

impl qobject::PlayerController {
    fn play(self: Pin<&mut Self>, source: &QString, row: i32) {
        match Selection::new(&source.to_string(), [row]) {
            Some(Selection::View(rows)) if !rows.is_empty() => self.play_view(rows[0]),
            Some(Selection::Queue(rows)) if !rows.is_empty() => self.play_queue_row(rows[0]),
            _ => {}
        }
    }

    fn play_view(mut self: Pin<&mut Self>, start: usize) {
        let every_row = (0..session::view().len()).collect();
        let Some(tracks) = selected_tracks(&Selection::View(every_row)) else {
            return;
        };
        let entry = session::lock_queue()
            .replace(tracks, start)
            .map(session::entry);
        if let (Some(entry), Some(player)) = (entry, &self.player) {
            log_err(player.play(entry));
        }
        self.as_mut().edited();
    }

    fn play_queue_row(mut self: Pin<&mut Self>, row: usize) {
        let entry = {
            let mut queue = session::lock_queue();
            queue
                .items()
                .get(row)
                .map(|item| item.slot)
                .and_then(|slot| {
                    queue.set_current(slot);
                    queue.current().map(session::entry)
                })
        };
        if let (Some(entry), Some(player)) = (entry, &self.player) {
            log_err(player.play(entry));
        }
        session::save(Save::Cursor);
        self.as_mut().update_now_playing();
    }

    fn play_next(self: Pin<&mut Self>, source: &QString, rows: &QList<i32>) {
        self.insert(source, rows, Queue::play_next);
    }

    fn add_to_queue(self: Pin<&mut Self>, source: &QString, rows: &QList<i32>) {
        self.insert(source, rows, Queue::add);
    }

    fn insert(
        mut self: Pin<&mut Self>,
        source: &QString,
        rows: &QList<i32>,
        insert: fn(&mut Queue, Vec<TrackRef>),
    ) {
        let Some(tracks) = selection(source, rows).and_then(|s| selected_tracks(&s)) else {
            return;
        };
        insert(&mut session::lock_queue(), tracks);
        self.as_mut().edited();
    }

    fn add_to_playlist(
        self: Pin<&mut Self>,
        source: &QString,
        rows: &QList<i32>,
        playlist: &QString,
    ) {
        let (Some(tracks), Ok(id)) = (
            selection(source, rows).and_then(|s| selected_tracks(&s)),
            playlist.to_string().parse::<ListId>(),
        ) else {
            return;
        };
        let track_ids: Vec<i64> = tracks.into_iter().map(|t| t.id).collect();
        if !track_ids.is_empty() {
            session::edit_playlists(|playlists| {
                if let Some(playlist) = playlists.iter_mut().find(|p| p.id == id) {
                    playlist.track_ids.extend(track_ids);
                }
            });
        }
    }

    fn track_ids(&self, source: &QString, rows: &QList<i32>) -> QList<i64> {
        let mut ids = QList::<i64>::default();
        for track in selection(source, rows)
            .and_then(|s| selected_tracks(&s))
            .unwrap_or_default()
        {
            ids.append(track.id);
        }
        ids
    }

    fn track_id(&self, source: &QString, row: i32) -> i64 {
        let catalog = session::catalog();
        Selection::new(&source.to_string(), [row])
            .and_then(|selection| selected_tracks(&selection)?.first().map(|t| t.id))
            .filter(|&id| catalog.is_some_and(|c| c.by_id(id).is_some()))
            .unwrap_or(-1)
    }

    fn remove_queue_rows(mut self: Pin<&mut Self>, rows: &QList<i32>) {
        {
            let mut queue = session::lock_queue();
            let slots = queue_slots(&queue, rows);
            queue.remove(&slots);
        }
        self.as_mut().edited();
    }

    fn move_queue_rows(mut self: Pin<&mut Self>, rows: &QList<i32>, to: i32) {
        let Some(to) = index(to) else {
            return;
        };
        {
            let mut queue = session::lock_queue();
            let slots = queue_slots(&queue, rows);
            queue.move_to(&slots, to);
        }
        self.as_mut().edited();
    }

    fn clear_queue(mut self: Pin<&mut Self>) {
        session::lock_queue().clear();
        if let Some(player) = &self.player {
            log_err(player.stop());
        }
        self.as_mut().edited();
    }

    fn set_shuffle_from_qml(mut self: Pin<&mut Self>, on: bool) {
        session::lock_queue().set_shuffle(on);
        self.as_mut().edited();
    }

    fn toggle_pause(mut self: Pin<&mut Self>) {
        let Some(player) = &self.player else {
            return;
        };
        if player.status().state != PlaybackState::Stopped {
            player.toggle_pause();
            return;
        }
        // Stopped: continue the current track, or start the first one.
        let (entry, resume) = {
            let mut queue = session::lock_queue();
            if queue.current().is_none()
                && let Some(slot) = queue.items().first().map(|i| i.slot)
            {
                queue.set_current(slot);
            }
            (queue.current().map(session::entry), queue.resume_position())
        };
        if let Some(entry) = entry {
            log_err(player.play_from(entry, resume));
        }
        self.as_mut().update_now_playing();
    }

    fn next(self: Pin<&mut Self>) {
        if let Some(player) = &self.player {
            log_err(player.next());
        }
    }

    fn previous(self: Pin<&mut Self>) {
        if let Some(player) = &self.player {
            log_err(player.previous());
        }
    }

    fn seek(mut self: Pin<&mut Self>, seconds: f64) {
        let Some(player) = &self.player else {
            return;
        };
        if !seconds.is_finite() {
            return;
        }
        let target = Duration::from_secs_f64(seconds.max(0.0));
        log_err(player.seek(target));
        // Tell the OS now: the engine reports the new position a moment later.
        let state = player.status().state;
        self.as_mut().report_playback(state, target);
    }

    fn seek_by(self: Pin<&mut Self>, seconds: f64) {
        let target = (self.position + seconds).clamp(0.0, self.duration.max(0.0));
        self.seek(target);
    }

    fn set_volume_from_qml(self: Pin<&mut Self>, volume: f64) {
        let mut state = self.volume_state();
        state.set_level(volume);
        self.apply_volume(state);
    }

    fn toggle_mute(self: Pin<&mut Self>) {
        let mut state = self.volume_state();
        state.toggle_mute();
        self.apply_volume(state);
    }

    fn volume_state(&self) -> Volume {
        Volume {
            level: self.volume,
            muted: self.muted,
        }
    }

    fn apply_volume(mut self: Pin<&mut Self>, state: Volume) {
        set!(self, volume, volume_changed, state.level);
        set!(self, muted, muted_changed, state.muted);
        let output = state.output();
        if let Some(player) = &self.player {
            player.set_volume(output as f32);
        }
        if let Some(media) = &mut self.as_mut().rust_mut().media {
            media.set_volume(output);
        }
    }

    fn media_command(self: Pin<&mut Self>, command: MediaCommand) {
        let state = self.player.as_ref().map(|p| p.status().state);
        match command {
            MediaCommand::Play if state == Some(PlaybackState::Playing) => {}
            MediaCommand::Pause if state != Some(PlaybackState::Playing) => {}
            MediaCommand::Play | MediaCommand::Pause | MediaCommand::Toggle => self.toggle_pause(),
            MediaCommand::Next => self.next(),
            MediaCommand::Previous => self.previous(),
            MediaCommand::Stop => {
                if let Some(player) = &self.player {
                    log_err(player.stop());
                }
            }
            MediaCommand::Seek(position) => self.seek(position.as_secs_f64()),
            MediaCommand::SeekBy(seconds) => self.seek_by(seconds),
            MediaCommand::SetVolume(volume) => self.set_volume_from_qml(volume),
        }
    }

    fn report_playback(mut self: Pin<&mut Self>, state: PlaybackState, position: Duration) {
        self.as_mut().rust_mut().media_state = Some(state);
        if let Some(media) = &mut self.as_mut().rust_mut().media {
            media.set_playback(match state {
                PlaybackState::Stopped => Playback::Stopped,
                PlaybackState::Paused => Playback::Paused(position),
                PlaybackState::Playing => Playback::Playing(position),
            });
        }
    }

    fn poll(mut self: Pin<&mut Self>) {
        let Some(status) = self.player.as_ref().map(Player::status) else {
            return;
        };
        if status.output_failed && !self.output_failed {
            self.as_mut().rust_mut().output_failed = true;
            self.as_mut().rust_mut().error = QString::from(
                "The audio output stopped working, e.g. the device was disconnected. \
                 Restart Lune to hear sound again.",
            );
            self.as_mut().error_changed();
        }
        if status.entry != self.playing_entry {
            if let (Some(stopped), None) = (self.playing_entry, status.entry) {
                session::lock_queue().set_resume_position(SlotId(stopped.0), Duration::ZERO);
            }
            self.as_mut().rust_mut().playing_entry = status.entry;
            if let Some(entry) = status.entry {
                let track_id = {
                    let mut queue = session::lock_queue();
                    queue.set_current(SlotId(entry.0));
                    queue.current().map(|item| item.track_id)
                };
                session::save(Save::Cursor);
                if let Some(track_id) = track_id {
                    session::record_play(track_id);
                }
            }
            self.as_mut().update_now_playing();
        }
        set!(
            self,
            playing,
            playing_changed,
            status.state == PlaybackState::Playing
        );
        let position = {
            let mut queue = session::lock_queue();
            match status.entry {
                Some(entry) => {
                    queue.set_resume_position(SlotId(entry.0), status.position);
                    status.position
                }
                None => queue.resume_position(),
            }
        };
        set!(self, position, position_changed, position.as_secs_f64());
        if let Some(duration) = status.duration {
            set!(self, duration, duration_changed, duration.as_secs_f64());
        }
        // Only state changes: the OS extrapolates the position while playing.
        if self.media_state != Some(status.state) {
            self.report_playback(status.state, status.position);
        }
    }

    fn clear_error(mut self: Pin<&mut Self>) {
        set!(self, error, error_changed, QString::default());
    }

    /// Re-reads the queue and now playing (after the library loads or
    /// changes).
    fn refresh(mut self: Pin<&mut Self>) {
        let shuffle = session::lock_queue().shuffle();
        set!(self, shuffle, shuffle_changed, shuffle);
        self.update_now_playing();
    }

    /// After a queue edit: the engine re-checks what's next, and it's saved.
    fn edited(mut self: Pin<&mut Self>) {
        if let Some(player) = &self.player {
            log_err(player.queue_changed());
        }
        session::save(Save::Queue);
        let shuffle = session::lock_queue().shuffle();
        set!(self, shuffle, shuffle_changed, shuffle);
        self.as_mut().update_now_playing();
        session::queue_edited();
    }

    /// Shows the queue's current track, whether or not it is playing.
    fn update_now_playing(mut self: Pin<&mut Self>) {
        let (track_id, queue_index) = {
            let queue = session::lock_queue();
            (
                queue.current().map(|item| item.track_id),
                queue.current_index(),
            )
        };
        let track = track_id.and_then(|id| Some(session::catalog()?.by_id(id)?.clone()));
        set!(
            self,
            queue_index,
            queue_index_changed,
            queue_index
                .and_then(|i| i32::try_from(i).ok())
                .unwrap_or(-1)
        );
        set!(
            self,
            current_track_id,
            current_track_id_changed,
            track_id.unwrap_or(-1)
        );
        set!(self, has_track, has_track_changed, track.is_some());
        let (title, artist, album) = match &track {
            Some(t) => (
                t.title.clone(),
                format::artist(t.artist.as_deref().unwrap_or_default()),
                format::album(&t.album),
            ),
            None => Default::default(),
        };
        set!(self, title, title_changed, QString::from(&title));
        set!(self, artist, artist_changed, QString::from(&artist));
        set!(self, album, album_changed, QString::from(&album));
        if let Some(t) = &track {
            set!(
                self,
                duration,
                duration_changed,
                t.duration_ms as f64 / 1000.0
            );
        }
        let cover_id = track.and_then(|t| t.art_id);
        self.as_mut().rust_mut().cover_id = cover_id;
        self.show_cover();
    }

    fn cover_ready(self: Pin<&mut Self>, art_id: i64) {
        if self.cover_id == Some(art_id) {
            self.show_cover();
        }
    }

    fn show_cover(mut self: Pin<&mut Self>) {
        let path = self.cover_id.and_then(|id| covers::covers().path(id));
        let url = path
            .as_deref()
            .and_then(covers::file_url)
            .unwrap_or_default();
        set!(self, cover, cover_changed, url);
        let now = NowPlaying {
            title: self.title.to_string(),
            artist: self.artist.to_string(),
            album: self.album.to_string(),
            duration: self
                .has_track
                .then(|| Duration::from_secs_f64(self.duration.max(0.0))),
            cover: path.as_deref().and_then(media_controls::cover_url),
        };
        if now != self.media_now {
            if let Some(media) = &mut self.as_mut().rust_mut().media {
                media.set_now_playing(&now);
            }
            self.as_mut().rust_mut().media_now = now;
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Volume {
    level: f64,
    muted: bool,
}

impl Volume {
    /// Choosing a level unmutes, so the change is heard.
    fn set_level(&mut self, level: f64) {
        self.level = if level.is_finite() {
            level.clamp(0.0, 1.0)
        } else {
            1.0
        };
        self.muted = false;
    }

    fn toggle_mute(&mut self) {
        self.muted = !self.muted;
    }

    fn output(self) -> f64 {
        if self.muted { 0.0 } else { self.level }
    }
}

fn log_err(result: lune_engine::Result<()>) {
    if let Err(err) = result {
        tracing::error!(%err, "playback command failed");
    }
}

#[cfg(test)]
#[path = "tests/player_test.rs"]
mod tests;
