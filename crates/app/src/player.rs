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
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::{QList, QString, QUrl};
use lune_core::{Catalog, ListId, Queue, QueueItem, SlotId, TrackRef, View};
use lune_engine::{EntryId, OutputKind, PlaybackState, Player, PlayerConfig, Status};

use crate::media_controls::{self, MediaCommand, MediaControls, NowPlaying, Playback};
use crate::preferences::with_store;
use crate::property::set;
use crate::session::{self, Change, Save};
use crate::{covers, format, ticker};

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
        #[qml_element = "Player"]
        #[qml_singleton]
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
        /// `Repeat::index`: 0 off, 1 all, 2 one.
        #[qproperty(i32, repeat, READ, NOTIFY)]
        #[qproperty(f64, volume, READ, NOTIFY)]
        #[qproperty(bool, muted, READ, NOTIFY)]
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

        /// Steps to the next repeat mode: off, all, one, and off again.
        #[qinvokable]
        #[cxx_name = "cycleRepeat"]
        fn cycle_repeat(self: Pin<&mut PlayerController>);

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

        /// `seconds` as the now-playing bar shows a time.
        #[qinvokable]
        fn clock(self: &PlayerController, seconds: f64) -> QString;

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
    repeat: i32,
    volume: f64,
    muted: bool,
    player: Option<Player>,
    opening: Opening,
    /// The entry the engine last reported playing, and its playthrough.
    playing_entry: Option<EntryId>,
    playthrough: u64,
    cover_id: Option<i64>,
    /// Media keys and the OS Now Playing display.
    media: Option<Box<dyn MediaControls>>,
    /// What the OS was last told, to send only changes.
    media_now: NowPlaying,
    media_state: Option<PlaybackState>,
    /// Polled more often while playing, so the position moves smoothly.
    poll_fast: Arc<AtomicBool>,
}

impl Default for PlayerControllerRust {
    fn default() -> Self {
        let (volume, muted) = session::volume();
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
            repeat: 0,
            volume,
            muted,
            player: None,
            opening: Opening::default(),
            playing_entry: None,
            playthrough: 0,
            cover_id: None,
            media: None,
            media_now: NowPlaying::default(),
            media_state: None,
            poll_fast: Arc::default(),
        }
    }
}

impl cxx_qt::Initialize for qobject::PlayerController {
    fn initialize(mut self: Pin<&mut Self>) {
        self.as_mut().open_output(None);
        let fast = Arc::clone(&self.poll_fast);
        let interval = move || {
            if fast.load(Ordering::Relaxed) {
                POLL_PLAYING
            } else {
                POLL_STOPPED
            }
        };
        // Also reconnects a lost output, so it runs while stopped too.
        if let Err(err) = ticker::every(self.qt_thread(), interval, Self::poll) {
            session::report_error(&format!("Playback can't be followed: {err}"));
        }
        covers::on_ready(self.qt_thread(), Self::cover_ready);
        // Created on the Qt (main) thread, as macOS requires.
        let qt_thread = self.qt_thread();
        let media = media_controls::system(move |command| {
            let _ = qt_thread.queue(move |controller| controller.media_command(command));
        });
        self.as_mut().rust_mut().media = media;
        let output = self.volume_state().output();
        if let Some(media) = &mut self.as_mut().rust_mut().media {
            media.set_volume(output);
        }
        session::on_change(self.qt_thread(), Change::Catalog, Self::refresh);
        session::on_change(self.qt_thread(), Change::OutputDevice, Self::reopen);
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
        self.as_mut().forget_resume();
        if let (Some(entry), Some(player)) = (entry, &self.player) {
            log_err(player.play(entry));
        }
        self.as_mut().edited(Save::Queue);
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
        self.as_mut().forget_resume();
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
        self.as_mut().edited(Save::Queue);
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
        self.as_mut().edited(Save::Queue);
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
        self.as_mut().edited(Save::Queue);
    }

    fn clear_queue(mut self: Pin<&mut Self>) {
        session::lock_queue().clear();
        self.as_mut().forget_resume();
        if let Some(player) = &self.player {
            log_err(player.stop());
        }
        self.as_mut().edited(Save::Queue);
    }

    fn set_shuffle_from_qml(mut self: Pin<&mut Self>, on: bool) {
        session::lock_queue().set_shuffle(on);
        // The queue is saved unshuffled, so only the shuffle flag changed.
        self.as_mut().edited(Save::Cursor);
    }

    fn cycle_repeat(mut self: Pin<&mut Self>) {
        {
            let mut queue = session::lock_queue();
            let repeat = queue.repeat().next();
            queue.set_repeat(repeat);
        }
        // What plays next may change, so the engine re-checks it.
        self.as_mut().edited(Save::Cursor);
    }

    fn toggle_pause(mut self: Pin<&mut Self>) {
        let Some(player) = &self.player else {
            if let Some(playing) = self.as_mut().rust_mut().opening.toggle_resume() {
                set!(self, playing, playing_changed, playing);
            }
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
        if !seconds.is_finite() {
            return;
        }
        let target = Duration::from_secs_f64(seconds.max(0.0));
        let Some(player) = &self.player else {
            self.as_mut().rust_mut().opening.seek_resume(target);
            return;
        };
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
        session::set_volume(state.level, state.muted);
        let output = state.output();
        if let Some(player) = &self.player {
            player.set_volume(output as f32);
        }
        if let Some(media) = &mut self.as_mut().rust_mut().media {
            media.set_volume(output);
        }
    }

    fn media_command(mut self: Pin<&mut Self>, command: MediaCommand) {
        let state = match &self.player {
            Some(player) => Some(player.status().state),
            None => self.opening.resumes_playing().map(|playing| {
                if playing {
                    PlaybackState::Playing
                } else {
                    PlaybackState::Paused
                }
            }),
        };
        match command {
            MediaCommand::Play if state == Some(PlaybackState::Playing) => {}
            MediaCommand::Pause if state != Some(PlaybackState::Playing) => {}
            MediaCommand::Play | MediaCommand::Pause | MediaCommand::Toggle => self.toggle_pause(),
            MediaCommand::Next => self.next(),
            MediaCommand::Previous => self.previous(),
            MediaCommand::Stop => {
                self.as_mut().forget_resume();
                if let Some(player) = &self.player {
                    log_err(player.stop());
                }
            }
            MediaCommand::Seek(position) => self.seek(position.as_secs_f64()),
            MediaCommand::SeekBy(seconds) => self.seek_by(seconds),
            MediaCommand::SetVolume(volume) => self.set_volume_from_qml(volume),
        }
    }

    fn clock(&self, seconds: f64) -> QString {
        QString::from(&format::clock(seconds))
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
        let status = self.player.as_ref().map(Player::status);
        if self.opening.due(status.as_ref(), Instant::now()) {
            self.reopen();
            return;
        }
        let Some(status) = status else {
            return;
        };
        // A new playthrough of the same entry is it repeated.
        if status.entry != self.playing_entry || status.playthrough != self.playthrough {
            if let (Some(stopped), None) = (self.playing_entry, status.entry) {
                session::lock_queue().set_resume_position(SlotId(stopped.0), Duration::ZERO);
            }
            self.as_mut().rust_mut().playing_entry = status.entry;
            self.as_mut().rust_mut().playthrough = status.playthrough;
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
        let playing = status.state == PlaybackState::Playing;
        set!(self, playing, playing_changed, playing);
        self.poll_fast.store(playing, Ordering::Relaxed);
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

    /// Opens the output the settings choose, or the default in its place,
    /// on another thread: finding devices can take a while. The new one goes
    /// on from `resume`, or from where an earlier failed open would have.
    fn open_output(mut self: Pin<&mut Self>, resume: Option<Resume>) {
        if !self
            .as_mut()
            .rust_mut()
            .opening
            .start(resume, Instant::now())
        {
            return;
        }
        let preferred = with_store(|store| store.settings().output_device.clone());
        let config = PlayerConfig {
            output: OutputKind::Device { preferred },
            ..PlayerConfig::default()
        };
        let qt_thread = self.qt_thread();
        let spawned = std::thread::Builder::new()
            .name("lune-output-open".into())
            .spawn(move || {
                let opened = Player::new(config, Arc::new(session::EngineQueue));
                // An error means the object is gone (the app is quitting).
                let _ = qt_thread.queue(move |controller| controller.opened(opened));
            });
        if let Err(err) = spawned {
            self.opened(Err(err.into()));
        }
    }

    fn opened(mut self: Pin<&mut Self>, opened: lune_engine::Result<Player>) {
        match self.as_mut().rust_mut().opening.finish(opened) {
            Finished::Again => self.open_output(None),
            Finished::Opened(player, resume) => {
                player.set_volume(self.volume_state().output() as f32);
                self.as_mut().rust_mut().player = Some(player);
                set!(self, available, available_changed, true);
                if let Some(resume) = resume {
                    self.go_on_from(resume);
                }
            }
            Finished::Failed(err, first) => {
                if first {
                    session::report_error(&format!(
                        "No sound output could be opened, so playback waits for one: {err}"
                    ));
                }
                set!(self, available, available_changed, false);
            }
        }
    }

    /// Replaces the output, after it failed or to switch devices, and goes
    /// on from the same place in the same track.
    fn reopen(mut self: Pin<&mut Self>) {
        let resume = self
            .player
            .as_ref()
            .and_then(|player| resume_from(&player.status()));
        // Some devices take one stream at a time, so the old one goes first.
        drop(self.as_mut().rust_mut().player.take());
        // The new output starts with nothing heard, which isn't the end of
        // the track that was playing.
        self.as_mut().rust_mut().playing_entry = None;
        self.as_mut().rust_mut().playthrough = 0;
        set!(self, available, available_changed, false);
        self.open_output(resume);
    }

    /// See [`Opening::forget_resume`].
    fn forget_resume(mut self: Pin<&mut Self>) {
        if self.as_mut().rust_mut().opening.forget_resume() {
            set!(self, playing, playing_changed, false);
        }
    }

    fn go_on_from(self: Pin<&mut Self>, (entry, position, playing): Resume) {
        let slot = SlotId(entry.0);
        let item = {
            let mut queue = session::lock_queue();
            queue.set_resume_position(slot, position);
            queue
                .items()
                .iter()
                .find(|item| item.slot == slot)
                .map(session::entry)
        };
        if playing && let (Some(item), Some(player)) = (item, &self.player) {
            log_err(player.play_from(item, position));
        }
    }

    /// Re-reads the queue and now playing (after the library loads or
    /// changes).
    fn refresh(mut self: Pin<&mut Self>) {
        self.as_mut().show_modes();
        self.update_now_playing();
    }

    fn show_modes(mut self: Pin<&mut Self>) {
        let (shuffle, repeat) = {
            let queue = session::lock_queue();
            (queue.shuffle(), queue.repeat())
        };
        set!(self, shuffle, shuffle_changed, shuffle);
        let repeat = i32::try_from(repeat.index()).unwrap_or(0);
        set!(self, repeat, repeat_changed, repeat);
    }

    /// After a queue edit: the engine re-checks what's next, and `save` is saved.
    fn edited(mut self: Pin<&mut Self>, save: Save) {
        if let Some(player) = &self.player {
            log_err(player.queue_changed());
        }
        session::save(save);
        self.as_mut().show_modes();
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
        session::set_now_playing(track_id);
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

const POLL_PLAYING: Duration = Duration::from_millis(100);
const POLL_STOPPED: Duration = Duration::from_millis(250);

const REOPEN_EVERY: Duration = Duration::from_secs(2);
/// Longest wait between tries while no output opens, e.g. on a machine
/// without sound.
const REOPEN_AT_MOST_EVERY: Duration = Duration::from_secs(30);

/// Where a new output goes on from: the entry heard, its position, and
/// whether it was playing (a paused one waits to be resumed).
type Resume = (EntryId, Duration, bool);

/// When to open the output, and what the next one goes on from.
#[derive(Debug, Default)]
struct Opening {
    running: bool,
    /// Asked for while one was running, which then opens with out of date
    /// settings, so it is opened again.
    again: bool,
    last_start: Option<Instant>,
    /// Opens failed in a row; each one waits twice as long.
    failures: u32,
    /// Kept until an output opens, so one that fails to open doesn't lose it.
    resume: Option<Resume>,
}

#[derive(Debug, PartialEq)]
enum Finished<T, E> {
    Again,
    /// With what to go on from.
    Opened(T, Option<Resume>),
    /// And whether it's the first failure in a row.
    Failed(E, bool),
}

impl Opening {
    /// Whether to start an open now; else one is running, and another
    /// follows it.
    fn start(&mut self, resume: Option<Resume>, now: Instant) -> bool {
        if resume.is_some() {
            self.resume = resume;
        }
        if self.running {
            self.again = true;
            return false;
        }
        self.running = true;
        self.last_start = Some(now);
        true
    }

    fn finish<T, E>(&mut self, opened: Result<T, E>) -> Finished<T, E> {
        self.running = false;
        if std::mem::take(&mut self.again) {
            return Finished::Again;
        }
        match opened {
            Ok(opened) => {
                self.failures = 0;
                Finished::Opened(opened, self.resume.take())
            }
            Err(err) => {
                self.failures = self.failures.saturating_add(1);
                Finished::Failed(err, self.failures == 1)
            }
        }
    }

    /// Whether what the next output goes on from plays, if there is one.
    fn resumes_playing(&self) -> Option<bool> {
        self.resume.map(|(_, _, playing)| playing)
    }

    /// Pauses or unpauses what the next output goes on from, so a pause
    /// while there is no output isn't undone when one opens.
    fn toggle_resume(&mut self) -> Option<bool> {
        let (_, _, playing) = self.resume.as_mut()?;
        *playing = !*playing;
        Some(*playing)
    }

    fn seek_resume(&mut self, position: Duration) {
        if let Some((_, at, _)) = &mut self.resume {
            *at = position;
        }
    }

    /// The user chose what plays while there was no output, so the next
    /// one doesn't go back to where the old one was. Returns whether there
    /// was somewhere to go back to.
    fn forget_resume(&mut self) -> bool {
        self.resume.take().is_some()
    }

    /// Whether to open the output again: there is none, it failed, or it
    /// plays in place of the preferred device, which is back. At most once
    /// per [`REOPEN_EVERY`], doubled for each open that failed in a row.
    fn due(&self, status: Option<&Status>, now: Instant) -> bool {
        let wanted = status.is_none_or(|status| status.output_failed || status.preferred_back);
        let wait = REOPEN_EVERY
            .saturating_mul(2u32.saturating_pow(self.failures))
            .min(REOPEN_AT_MOST_EVERY);
        !self.running
            && wanted
            && self
                .last_start
                .is_none_or(|at| now.duration_since(at) >= wait)
    }
}

fn resume_from(status: &Status) -> Option<Resume> {
    let entry = status.entry?;
    Some((
        entry,
        status.position,
        status.state == PlaybackState::Playing,
    ))
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
