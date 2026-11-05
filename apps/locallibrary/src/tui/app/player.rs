//! Playback: starting, following and stopping tracks, the outputs, seeking and cues.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use locallibrary::anlz::{self, TrackAnalysis};
use locallibrary::{Device, Library, Track, paths};
use ratatui::widgets::ListState;

use crate::tui::audio::{self, Audio};
use crate::tui::browse::NodeId;
use crate::tui::deck::{OpenedOutput, OutputAsk};
use crate::tui::keys::Camelot;

use super::*;

impl App {
    /// The cue flash's colour and strength (1 fading to 0). None with reduced motion.
    pub fn flash(&self) -> Option<([u8; 3], f32)> {
        let (color, at) = self.flash.filter(|_| !crate::tui::theme::reduced_motion())?;
        let t = 1.0 - at.elapsed().as_secs_f32() / 0.9;
        (t > 0.0).then_some((color, t * t))
    }

    /// Seconds of track the cue lane shows: zoomed together with the deck.
    pub fn lane_secs(&self) -> f64 {
        self.deck_secs * 4.0
    }

    pub fn is_playing(&self) -> bool {
        self.audio.as_ref().is_some_and(|a| a.is_loaded() && !a.is_paused())
    }

    /// Where the playing track is (with its output gone, where it waits to play on from).
    pub fn position(&self) -> Duration {
        match &self.audio {
            Some(a) => a.position(),
            None => self.resume.filter(|_| self.now.is_some()).unwrap_or_default(),
        }
    }

    fn next_random(&mut self) -> u64 {
        // xorshift64
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.rng = x;
        x
    }

    pub(super) fn shuffle_order(&mut self, first: usize) {
        let n = self.queue.len();
        let mut order: Vec<usize> = (0..n).filter(|&i| i != first).collect();
        for i in (1..order.len()).rev() {
            let j = (self.next_random() % (i as u64 + 1)) as usize;
            order.swap(i, j);
        }
        order.insert(0, first);
        self.queue.order = order;
        self.queue.pos = 0;
    }

    /// Play `items` of `library` (all on the drive at `root`) from `start`, as the queue.
    pub(super) fn play_tracks(&mut self, library: Arc<Library>, items: Vec<usize>, root: PathBuf, start: usize) {
        if start >= items.len() {
            return;
        }
        self.resume = None;
        if self.loaded.as_ref().is_some_and(|l| l.stale && l.root() == root) {
            self.toast("the drive was unplugged", Level::Error);
            return;
        }
        let drive = self.drive_label(&root);
        self.queue = Queue { order: (0..items.len()).collect(), library, items, root, drive, pos: start };
        self.failed_in_row = 0;
        if self.shuffle {
            self.shuffle_order(start);
        }
        self.start_current();
    }

    /// Play the selected track, queueing the rest of the visible list after it.
    pub(super) fn play_selected(&mut self) {
        let (Some(l), Some(sel)) = (&self.loaded, self.track_state.selected()) else { return };
        let (library, root) = (l.library.clone(), l.root().to_path_buf());
        self.play_tracks(library, self.visible.clone(), root, sel);
    }

    /// Play the selected match in the details panel, queueing the other matches.
    pub(super) fn play_match(&mut self) {
        let (Some(d), Some(sel)) = (&self.details, self.details_state.selected()) else { return };
        if !self.details_current() {
            return;
        }
        let items = d.matches.items.iter().map(|m| m.index).collect();
        let (library, root) = (d.matches.library.clone(), d.matches.root.clone());
        self.play_tracks(library, items, root, sel);
    }

    /// Full screen: the pick among the playing track's matches back on the best one.
    pub(super) fn reset_next_pick(&mut self) {
        let any = self.now.as_ref().and_then(|n| n.matches.as_ref()).is_some_and(|m| !m.items.is_empty());
        self.next_pick = ListState::default().with_selected(any.then_some(0));
    }

    /// Full screen: queue the picked match to play after the playing track.
    pub(super) fn play_next(&mut self) {
        if !self.playing_queue() {
            return;
        }
        let Some(m) = self.now.as_ref().and_then(|n| n.matches.as_ref()) else { return };
        let Some(mix) = self.next_pick.selected().and_then(|i| m.items.get(i)) else { return };
        let track = m.track(mix);
        // The matches index the library as it was read when the track started; the queue's
        // copy is older when the drive was read again since.
        let index = if Arc::ptr_eq(&m.library, &self.queue.library) {
            Some(mix.index)
        } else {
            self.queue.library.tracks.iter().position(|t| t.id == track.id)
        };
        let Some(index) = index else { return };
        let name = track.display_name();
        self.queue.items.push(index);
        self.queue.order.insert(self.queue.pos + 1, self.queue.items.len() - 1);
        self.drop_upcoming();
        self.toast(format!("next: {name}"), Level::Info);
    }

    /// Play the crate from the selected entry (the entries on its drive).
    pub(super) fn play_crate(&mut self) {
        let Some(sel) = self.crate_state.selected().filter(|&i| i < self.crate_list.len()) else { return };
        let root = self.crate_list.items[sel].root.clone();
        let mut start = 0;
        let mut tracks = Vec::new();
        for (i, t) in self.crate_list.items.iter().enumerate().filter(|(_, t)| t.root == root) {
            if i == sel {
                start = tracks.len();
            }
            tracks.push(t.track.clone());
        }
        if tracks.len() < self.crate_list.len() {
            let msg = format!("playing the crate's tracks on {}", self.crate_list.items[sel].drive);
            self.toast(msg, Level::Info);
        }
        let items = (0..tracks.len()).collect();
        self.play_tracks(Arc::new(Library { tracks, ..Default::default() }), items, root, start);
    }

    /// With no output open or opening (there was none, or it went away), open the one to play
    /// through: the one picked, else the default. In the background, as a slow device (Bluetooth,
    /// AirPlay) can take seconds (see `output_ready` and `no_output`).
    pub(in crate::tui) fn open_output(&mut self) {
        if self.audio.is_none() && self.deck.opening.is_none() {
            self.open_in_background(OutputAsk::Play);
        }
    }

    /// Open an output in the background, as `ask` says. The last asked for wins: what comes
    /// back for one asked for before it is dropped.
    pub(in crate::tui) fn open_in_background(&mut self, ask: OutputAsk) {
        let (id, outputs, preferred) = (self.next_id(), self.outputs, self.deck.output.clone());
        self.deck.opening = Some(id);
        let failed = ask.clone();
        self.spawn_or(
            move || {
                let opened = match &ask {
                    OutputAsk::Pick(name) => (outputs.open)(name.as_deref()).map(|a| (a, None)),
                    OutputAsk::Play => {
                        let (audio, missing) = outputs.open_preferring(preferred.as_deref());
                        audio.map(|a| (a, missing))
                    }
                };
                Msg::Output(id, ask, opened.map(|(a, missing)| (Box::new(a), missing)).map_err(|e| format!("{e:#}")))
            },
            move |e| Msg::Output(id, failed, Err(e)),
        );
    }

    /// An output opened in the background (or didn't), for the ask with this id.
    pub(super) fn output_opened(&mut self, id: u64, ask: OutputAsk, opened: Result<OpenedOutput, String>) {
        if self.deck.opening != Some(id) {
            return;
        }
        self.deck.opening = None;
        match (ask, opened) {
            (OutputAsk::Pick(name), Ok((a, _))) => crate::tui::deck::picked_output(self, name, *a),
            // (One is open already, given it meanwhile: it stays.)
            (OutputAsk::Play, Ok(_)) if self.audio.is_some() => {}
            (OutputAsk::Play, Ok((a, missing))) => self.output_ready(*a, missing),
            // (Playing on through the output it was, if any.)
            (OutputAsk::Pick(_), Err(e)) => self.toast(e, Level::Error),
            (OutputAsk::Play, Err(e)) => self.no_output(e),
        }
    }

    /// The output to play through is open: the track waiting for it plays.
    fn output_ready(&mut self, mut a: Audio, missing: Option<String>) {
        a.set_volume(self.volume);
        self.audio = Some(a);
        self.audio_error = None;
        if let Some(why) = missing {
            self.toast(format!("{why}: playing through the default output"), Level::Info);
        }
        if let Some((id, file)) = self.waiting_file.take() {
            self.opened(id, Ok(file));
        }
    }

    /// No output opens, `why`. A track waiting for one can't play: it stops, and space carries on
    /// with it from where it was to start.
    fn no_output(&mut self, why: String) {
        if self.audio.is_some() {
            return;
        }
        self.audio_error = Some(why.clone());
        if self.now.as_ref().is_some_and(|n| n.opening) {
            let at = self.pending_seek.unwrap_or_default();
            let queued = self.playing_queue();
            self.stop();
            self.resume = queued.then_some(at);
            self.toast(format!("can't play: {why}"), Level::Error);
        }
    }

    /// Stop what's playing, to play something else. With no output open, one opens in the
    /// background meanwhile (see `open_output`).
    fn ready_to_play(&mut self) {
        self.open_output();
        if let Some(a) = &mut self.audio {
            a.stop();
        }
        self.waiting_file = None;
        self.upcoming = None;
        self.pending_seek = None;
        self.start_paused = false;
        self.deck.looped = None;
    }

    /// The output's news: a seek it couldn't make, or its stream failing. When the output is
    /// gone (unplugged, say), it's let go of, and with it the file it had open on the drive: the
    /// track waits where it was, and the next play opens whatever output there is then.
    pub(super) fn watch_output(&mut self) {
        let Some(a) = &self.audio else { return };
        let (seek_failed, error) = (a.seek_failed(), a.stream_error());
        if seek_failed {
            self.toast("can't seek in this file", Level::Error);
        }
        let Some((e, gone)) = error else { return };
        if !gone {
            self.toast(format!("audio: {e}"), Level::Error);
            return;
        }
        let (at, _) = self.playback();
        self.audio = None;
        self.upcoming = None;
        self.audio_error = Some(e.clone());
        if self.now.is_some() {
            self.resume = at;
        }
        self.toast(format!("audio: {e} Space plays on through the output there is now."), Level::Error);
    }

    /// Whether the track playing is the queue's (rather than a recorded set, played on its own).
    pub(super) fn playing_queue(&self) -> bool {
        let Some(now) = &self.now else { return false };
        (self.queue.order.get(self.queue.pos))
            .is_some_and(|&i| self.queue.track(i).file_path == now.track.file_path && self.queue.root == now.root)
    }

    /// Start the queue's current track. It shows at once; its audio file is opened in the
    /// background (see `opened`), and its analysis, artwork and matches are read in the
    /// background too, filling in when they arrive (and the output, when none is open). False
    /// when there's no track.
    fn start_current(&mut self) -> bool {
        let Some(&i) = self.queue.order.get(self.queue.pos) else { return false };
        let track = self.queue.track(i).clone();
        // Where the last run left off was for the track it was on: once another starts, it's gone.
        self.resume = None;
        self.ready_to_play();
        self.now = Some(self.open(track, true));
        true
    }

    /// Open one of the queue's tracks in the background: its audio file (see `opened`), then its
    /// analysis, artwork and matches, which fill in as they arrive. `starts`: it's the one to
    /// play now (rather than the next, opened ahead).
    pub(super) fn open(&mut self, track: Track, starts: bool) -> NowPlaying {
        let id = self.next_id();
        let root = self.queue.root.clone();
        let (t, r, auto_cue, store_root) = (track.clone(), root.clone(), self.deck.auto_cue, self.store_root.clone());
        // Matches and playlists need the drive's library, which is open unless the player
        // moved to another drive since the queue was made.
        let library = self.loaded.as_ref().filter(|l| l.root() == self.queue.root).map(|l| l.library.clone());
        let job = move |o: &Opening| {
            let file = Device::open(&r).map_err(anyhow::Error::from).and_then(|d| {
                let mut file = audio::open_file(&d.checked_path(&paths::key(&t.file_path))?)?;
                if auto_cue {
                    file.start = crate::tui::deck::auto_cue(&anlz::load_grid(&d, &t));
                }
                Ok(file)
            });
            if o.send(Msg::Opened(id, file.map_err(|e| format!("{e:#}")))) {
                o.send(Msg::TrackInfo(id, Box::new(track_info(&r, &t, library, &store_root))));
            }
        };
        self.opener.open(&self.tx, id, starts, Box::new(job));
        NowPlaying {
            id,
            key: Camelot::parse(&track.key),
            duration: Duration::from_secs(track.duration_secs as u64),
            track,
            drive: self.queue.drive.clone(),
            root,
            analysis: TrackAnalysis::default(),
            from_plugins: None,
            art: None,
            phrases: Vec::new(),
            matches: None,
            playlists: Vec::new(),
            loading: true,
            opening: true,
            advancing: false,
        }
    }

    /// Keep up with the player: when it has gone on to the queued track, that's the one playing;
    /// when the playing one ends with nothing queued, start the next; and near the end of a
    /// track, open the next one and queue it, to follow without a gap.
    pub(super) fn follow_queue(&mut self) {
        let broken = self.audio.as_ref().and_then(Audio::broken);
        if self.audio.as_mut().is_some_and(Audio::advanced) {
            match (self.upcoming.take().filter(|u| u.queued), broken) {
                (Some(_), Some(why)) => self.cut_short(why),
                (Some(up), None) => self.went_on(up),
                (None, _) => {}
            }
        }
        // Not `else`: the queued track can have played through as well since the last look (a
        // short one, or a slow tick), and keys and the screen must not act on it as playing.
        if self.now.is_some() && self.audio.as_ref().is_some_and(Audio::finished) {
            match (self.upcoming.take(), self.audio.as_ref().and_then(Audio::broken)) {
                (_, Some(why)) => self.cut_short(why),
                // Opened already: straight on with it.
                (Some(mut up), None) if up.file.is_some() => {
                    if let (Some(a), Some(file)) = (&mut self.audio, up.file.take()) {
                        a.play(file, false);
                    }
                    self.went_on(up);
                }
                _ if self.playing_queue() => {
                    self.failed_in_row = 0;
                    self.advance(true);
                }
                // A recorded set, played on its own.
                _ => self.stop(),
            }
        }
        self.open_next();
    }

    /// The playing track ended without playing through, `why` (see `Audio::broken`). Like one
    /// that won't open, the queue moves past it (with repeat one, it would come round forever).
    fn cut_short(&mut self, why: String) {
        if let Some(a) = &mut self.audio {
            a.stop();
        }
        self.wont_play(why);
    }

    /// The player went on to the next track, opened ahead.
    fn went_on(&mut self, up: Upcoming) {
        self.queue.pos = up.pos;
        self.now = Some(up.now);
        self.reset_next_pick();
        self.deck.looped = None;
        self.last_pos_ms = 0.0;
        self.failed_in_row = 0;
    }

    /// Where the track is (while it opens, where it's to start: `None` for where it starts on its
    /// own) and whether it plays (or will, once it's open).
    pub(in crate::tui) fn playback(&self) -> (Option<Duration>, bool) {
        match &self.now {
            Some(n) if n.opening => (self.pending_seek, !self.start_paused),
            _ => (Some(self.position()), self.is_playing()),
        }
    }

    /// Open the playing track again (its output changed, or went away) from `at` (`None`: where
    /// it starts on its own), with its loop, and paused unless `playing`. With no output to play
    /// it through, it waits there for the next play.
    pub(in crate::tui) fn restart(&mut self, at: Option<Duration>, playing: bool) {
        let Some(now) = &self.now else { return };
        let looped = self.deck.looped;
        let started = if self.playing_queue() {
            self.start_current()
        } else {
            // A recorded set, played on its own.
            let (path, track) = (PathBuf::from(&now.track.file_path), now.track.clone());
            self.play_file(&path, track, Duration::ZERO);
            true
        };
        if !started {
            self.resume = at.or(Some(Duration::ZERO));
            return;
        }
        self.deck.looped = looped;
        self.start_paused = !playing;
        if let Some(at) = at {
            self.seek_to(at);
        }
    }

    /// Open the track that plays next, once the playing one is near its end.
    fn open_next(&mut self) {
        let Some(now) = self.now.as_ref().filter(|n| !n.opening) else { return };
        if self.upcoming.is_some() || self.audio.is_none() || !self.playing_queue() || self.deck.single {
            return;
        }
        if now.duration.saturating_sub(self.position()) > AHEAD {
            return;
        }
        let Some(pos) = self.next_pos(true) else { return };
        let track = self.queue.track(self.queue.order[pos]).clone();
        let mut now = self.open(track, false);
        now.advancing = true;
        self.upcoming = Some(Upcoming { now, pos, queued: false, file: None });
    }

    /// The next track was opened: queue it behind the playing one, or keep it to start as that
    /// one ends. One that won't open is left, so the queue moves on to it when the playing one
    /// ends and says why it won't play.
    fn opened_next(&mut self, file: Result<audio::OpenedFile, String>) {
        let (Some(up), Some(a)) = (&mut self.upcoming, &mut self.audio) else { return };
        up.now.opening = false;
        if let Ok(file) = file {
            up.now.duration = file_duration(&up.now.track, &file);
            match a.queue(file) {
                Ok(()) => up.queued = true,
                Err(file) => up.file = Some(file),
            }
        }
    }

    /// What comes next changed (shuffle, repeat, single play): forget the track opened for it.
    pub(in crate::tui) fn drop_upcoming(&mut self) {
        if self.upcoming.take().is_some_and(|u| u.queued)
            && let Some(a) = &mut self.audio
        {
            a.unqueue();
        }
    }

    /// A track's audio file opened in the background: play it, or say why it won't (and, moving
    /// through the queue, go on to the next).
    pub(super) fn opened(&mut self, id: u64, file: Result<audio::OpenedFile, String>) {
        if self.upcoming.as_ref().is_some_and(|u| u.now.id == id && u.now.opening) {
            self.opened_next(file);
            return;
        }
        let Some(now) = self.now.as_mut().filter(|n| n.id == id && n.opening) else {
            return; // another track was started since: this one's dropped
        };
        let output_opening = self.audio.is_none() && self.deck.opening.is_some();
        match file {
            // Opened before the output it plays through: it waits for it.
            Ok(file) if output_opening => self.waiting_file = Some((id, file)),
            Ok(file) => {
                now.opening = false;
                now.duration = file_duration(&now.track, &file);
                let paused = std::mem::take(&mut self.start_paused);
                // (The output went away while it opened: it waits for the next play.)
                let Some(a) = &mut self.audio else { return };
                a.play(file, paused);
                crate::tui::deck::apply_loop(self);
                if let Some(at) = self.pending_seek.take() {
                    self.seek_to(at);
                }
            }
            Err(e) => {
                now.opening = false;
                self.wont_play(e);
            }
        }
    }

    /// The playing track won't play, `why`: say so, and moving through the queue, go on to the
    /// next (but not round and round when none of them plays).
    fn wont_play(&mut self, why: String) {
        let Some(now) = self.now.take() else { return };
        self.toast(format!("{}: {why}", now.track.display_name()), Level::Error);
        self.failed_in_row += 1;
        if now.advancing && self.failed_in_row < self.queue.len() {
            self.advance(false);
        } else {
            self.failed_in_row = 0;
        }
    }

    /// Where the queue goes next: on, round to the start unless repeat is off, or at the end of
    /// a track (`auto`) with repeat one, the same track again.
    pub(super) fn next_pos(&self, auto: bool) -> Option<usize> {
        let (pos, n) = (self.queue.pos, self.queue.order.len());
        if auto && self.repeat == Repeat::One && pos < n {
            Some(pos)
        } else if pos + 1 < n {
            Some(pos + 1)
        } else if self.repeat != Repeat::Off && n > 0 {
            Some(0)
        } else {
            None
        }
    }

    /// Move through the queue. `auto` is the end of a track (repeat-one applies).
    pub(super) fn advance(&mut self, auto: bool) {
        let Some(pos) = self.next_pos(auto) else {
            self.stop();
            return;
        };
        self.queue.pos = pos;
        if auto && self.deck.single {
            // Single play: stop, with the next track ready for space.
            self.stop();
            self.resume = Some(Duration::ZERO);
            return;
        }
        if self.start_current() {
            if let Some(now) = &mut self.now {
                now.advancing = true;
            }
        } else {
            self.stop();
        }
    }

    pub(super) fn previous(&mut self) {
        if self.position() > Duration::from_secs(3) || self.queue.pos == 0 {
            self.seek_to(Duration::ZERO);
        } else {
            self.queue.pos -= 1;
            self.start_current();
        }
    }

    pub(super) fn stop(&mut self) {
        if let Some(a) = &mut self.audio {
            a.stop();
        }
        self.now = None;
        self.upcoming = None;
        self.waiting_file = None;
        self.opener.drop_all();
    }

    /// Before ejecting the drive at `root`: stop playing from it (the track, and the next one
    /// opened ahead), or its open files keep it from being unmounted. Files opened for them and
    /// on their way here are closed now rather than at the next tick; the eject waits for one
    /// being opened (see `opener_idle`).
    pub(in crate::tui) fn let_go_of(&mut self, root: &Path) {
        let on = |p: &Path| p.starts_with(root);
        if self.now.as_ref().is_some_and(|n| on(&n.root)) || self.upcoming.as_ref().is_some_and(|u| on(&u.now.root)) {
            self.stop();
            self.resume = None;
            while let Ok(msg) = self.rx.try_recv() {
                self.handle_msg(msg);
            }
        }
    }

    /// Held while a track is being opened: an eject waits for it.
    pub(in crate::tui) fn opener_idle(&self) -> Arc<Mutex<()>> {
        self.opener.idle.clone()
    }

    pub(super) fn toggle_pause(&mut self) {
        let output_opening = self.audio.is_none() && self.deck.opening.is_some();
        match (&self.audio, &self.now) {
            // Still opening (a slow drive, or a slow output): it starts paused or playing as last
            // asked.
            (Some(_), Some(n)) if n.opening => self.start_paused = !self.start_paused,
            (None, Some(n)) if n.opening && output_opening => self.start_paused = !self.start_paused,
            (Some(a), Some(_)) => a.toggle_pause(),
            // The output went away: play on from where it was, through what there is now.
            (None, Some(_)) => {
                let at = self.resume.take();
                self.restart(at, true);
            }
            _ if self.resumable().is_some() => {
                // Where the last run left off.
                let at = self.resume.take().unwrap_or_default();
                if self.start_current() && at > Duration::ZERO {
                    self.seek_to(at);
                }
            }
            _ => self.play_selected(),
        }
    }

    pub(in crate::tui) fn seek_to(&mut self, pos: Duration) {
        self.seek(pos, false);
    }

    /// Seek, `keep_time` for a target worked out from the playhead (see `Audio::seek`).
    fn seek(&mut self, pos: Duration, keep_time: bool) {
        let Some(now) = &self.now else { return };
        // Short of the end, so a seek doesn't end the track; a length that isn't known doesn't
        // hold one back.
        let end = now.duration.checked_sub(Duration::from_millis(250)).filter(|_| !now.duration.is_zero());
        let (pos, opening) = (end.map_or(pos, |end| pos.min(end)), now.opening);
        crate::tui::deck::seeking(self, pos);
        if opening {
            self.pending_seek = Some(pos);
            return;
        }
        match &self.audio {
            Some(a) => a.seek(pos, keep_time),
            // The output went away: where it plays on from.
            None => self.resume = Some(pos),
        }
        self.last_seek = Instant::now();
    }

    pub(super) fn seek_by(&mut self, secs: f64) {
        let target = (self.position().as_secs_f64() + secs).max(0.0);
        self.seek_to(Duration::from_secs_f64(target));
    }

    /// Play a file that isn't one of a drive's tracks (a recorded set) on its own, from `start`
    /// (the queue stays, for afterwards). `track` is what the player shows for it. The file is
    /// opened in the background.
    pub(in crate::tui) fn play_file(&mut self, path: &Path, track: Track, start: Duration) {
        self.ready_to_play();
        self.resume = None;
        self.pending_seek = (start > Duration::ZERO).then_some(start);
        let id = self.next_id();
        let file = path.to_path_buf();
        let job = move |o: &Opening| {
            o.send(Msg::Opened(id, audio::open_file(&file).map_err(|e| format!("{e:#}"))));
        };
        self.opener.open(&self.tx, id, true, Box::new(job));
        self.now = Some(NowPlaying {
            id,
            key: None,
            drive: "recorded set".into(),
            root: path.parent().map(Path::to_path_buf).unwrap_or_default(),
            duration: Duration::from_secs(track.duration_secs as u64),
            track,
            analysis: TrackAnalysis::default(),
            from_plugins: None,
            art: None,
            phrases: Vec::new(),
            matches: None,
            playlists: Vec::new(),
            loading: false,
            opening: true,
            advancing: false,
        });
    }

    pub(super) fn jump_cue(&mut self, forward: bool) {
        let Some(now) = &self.now else { return };
        let marks: Vec<f64> = now.cue_times().into_iter().map(f64::from).collect();
        let Some((t, at)) =
            crate::tui::deck::mark_jump(&now.analysis.beats, self.position_ms(), &marks, forward, self.snap())
        else {
            self.toast("no more cues", Level::Info);
            return;
        };
        let color = (now.analysis.cues.iter())
            .filter(|c| f64::from(c.start_ms) == t)
            .max_by_key(|c| c.hot_cue.is_some())
            .map(|c| crate::tui::theme::palette().cue_color(c));
        self.jump(at, color);
    }

    /// Jump to `ms` into the playing track, flashing the visualiser in a cue's colour.
    pub(in crate::tui) fn jump(&mut self, ms: f64, color: Option<[u8; 3]>) {
        let keep_time = self.snap() == crate::tui::deck::Snap::Phase;
        self.seek(Duration::from_micros((ms.max(0.0) * 1000.0).round() as u64), keep_time);
        self.last_pos_ms = ms;
        if let Some(color) = color {
            self.flash = Some((color, Instant::now()));
        }
    }

    pub(super) fn change_volume(&mut self, delta: f32) {
        self.set_volume(((self.volume + delta) * 20.0).round() / 20.0);
    }

    pub(super) fn set_volume(&mut self, v: f32) {
        self.volume = v.clamp(0.0, 1.5);
        if let Some(a) = &mut self.audio {
            a.set_volume(self.volume);
        }
    }

    /// Select the playing track in the list, switching to all tracks if needed. A recorded set
    /// (played from the Mixes view, on its own) is shown there instead.
    pub(super) fn locate_playing(&mut self) {
        let Some(now) = &self.now else { return };
        if !self.playing_queue() {
            self.set_view(View::Mixes);
            return;
        }
        let (root, path) = (now.root.clone(), now.track.file_path.clone());
        self.locate(&root, &path);
    }

    /// Select a track of the drive at `root` in the library, switching to all tracks if needed.
    pub(in crate::tui) fn locate(&mut self, root: &Path, path: &str) {
        let find = |app: &App| {
            let l = app.loaded.as_ref()?;
            if l.root() != root {
                return None;
            }
            app.visible.iter().position(|&i| l.library.tracks[i].file_path == path)
        };
        if let Some(i) = find(self) {
            self.track_state.select(Some(i));
        } else if self.loaded.as_ref().is_some_and(|l| l.root() == root) {
            // Not in the current view: show all tracks.
            self.query.clear();
            if let Some(l) = &mut self.loaded {
                l.node = NodeId::All;
            }
            self.playlist_state.select(Some(0));
            self.refresh_visible();
            if let Some(i) = find(self) {
                self.track_state.select(Some(i));
            }
        } else {
            self.toast("the track is on another drive", Level::Info);
            return;
        }
        self.view = View::Library;
        self.focus = Focus::Tracks;
    }
}
