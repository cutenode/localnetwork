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

    /// Play `items` of `library` (all on the drive at `root`) from `start`, as the list the queue
    /// plays on with after the crate's picks ("next from"). The picks stay.
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
        let order = (0..items.len()).collect();
        self.queue = Queue { order, library, items, root, drive, pos: start, name: String::new() };
        self.crate_list.playing = None;
        self.failed_in_row = 0;
        if self.shuffle {
            self.shuffle_order(start);
        }
        self.start_current();
    }

    /// Play the selected track, queueing the rest of the visible list after it.
    pub(super) fn play_selected(&mut self) {
        let (Some(l), Some(sel)) = (&self.loaded, self.track_state.selected()) else { return };
        let (library, root, name) = (l.library.clone(), l.root().to_path_buf(), l.playlist_name());
        self.play_tracks(library, self.visible.clone(), root, sel);
        self.queue.name = name;
    }

    /// Play the selected match in the details panel, queueing the other matches.
    pub(super) fn play_match(&mut self) {
        let (Some(d), Some(sel)) = (&self.details, self.details_state.selected()) else { return };
        if !self.details_current() {
            return;
        }
        let items = d.matches.items.iter().map(|m| m.index).collect();
        let (library, root) = (d.matches.library.clone(), d.matches.root.clone());
        let of = self.selected_track().and_then(|i| Some(self.loaded.as_ref()?.library.tracks[i].title.clone()));
        self.play_tracks(library, items, root, sel);
        self.queue.name = format!("mixes into {}", of.unwrap_or_default());
    }

    /// Full screen: the pick among the playing track's matches back on the best one.
    pub(super) fn reset_next_pick(&mut self) {
        let any = self.now.as_ref().and_then(|n| n.matches.as_ref()).is_some_and(|m| !m.items.is_empty());
        self.next_pick = ListState::default().with_selected(any.then_some(0));
    }

    /// The tracks `q n` and `q a` queue, and the drive they're on: in player mode the pick among
    /// what mixes into the playing track; in the library the marked tracks (in the list's order)
    /// or the selected one, the selected match, or the queue's selected entry.
    pub(super) fn picked_tracks(&self) -> Option<(PathBuf, Vec<Track>)> {
        if self.deck_showing() {
            let m = self.now.as_ref()?.matches.as_ref()?;
            let mix = m.items.get(self.next_pick.selected()?)?;
            return Some((m.root.clone(), vec![m.track(mix).clone()]));
        }
        if self.view != View::Library {
            return None;
        }
        match self.focus {
            Focus::Crate => {
                let t = self.crate_list.items.get(self.crate_state.selected()?)?;
                Some((t.root.clone(), vec![t.track.clone()]))
            }
            Focus::Details => {
                let d = self.details.as_ref().filter(|_| self.details_current())?;
                let mix = d.matches.items.get(self.details_state.selected()?)?;
                Some((d.matches.root.clone(), vec![d.matches.track(mix).clone()]))
            }
            Focus::Tracks | Focus::Playlists | Focus::Drives => {
                let l = self.loaded.as_ref()?;
                let tracks: Vec<Track> = self.targets().into_iter().map(|i| l.library.tracks[i].clone()).collect();
                (!tracks.is_empty()).then(|| (l.root().to_path_buf(), tracks))
            }
        }
    }

    /// `q n` (`next`) or `q a`: queue the tracks picked (see `picked_tracks`) to play next, before
    /// the other picks, or after them.
    pub(super) fn queue_picked(&mut self, next: bool) {
        match self.picked_tracks() {
            Some((root, tracks)) => self.queue_tracks(&root, tracks, next),
            None => self.toast("select a track to queue", Level::Info),
        }
    }

    /// Put `tracks` of the drive at `root` in the queue: to play `next`, or at the end.
    pub(in crate::tui) fn queue_tracks(&mut self, root: &Path, tracks: Vec<Track>, next: bool) {
        let (n, drive) = (tracks.len(), self.drive_label(root));
        let name = tracks.first().filter(|_| n == 1).map(Track::display_name);
        let added = match next {
            true => self.crate_list.play_next(root, &drive, tracks),
            false => self.crate_list.add(root, &drive, tracks),
        };
        self.queue_changed();
        let up = self.crate_list.up_next().count();
        let tracks = |a| crate::output::plural(a, "track");
        let msg = match (next, name, added, n - added) {
            (true, Some(name), ..) => format!("next: {name}"),
            (true, None, a, _) => format!("next: {}", tracks(a)),
            (false, _, 0, _) => "already up next".to_string(),
            (false, Some(name), ..) => format!("queued: {name} ({up} up next)"),
            (false, None, a, 0) => format!("queued {} ({up} up next)", tracks(a)),
            (false, None, a, d) => format!("queued {} ({d} already up next)", tracks(a)),
        };
        self.toast(msg, Level::Info);
        if self.crate_state.selected().is_none() && !self.crate_list.is_empty() {
            self.crate_state.select(Some(0));
        }
    }

    /// Enter in the queue: play the selected entry now. The picks before it stay to play.
    pub(super) fn play_pick(&mut self) {
        let Some(t) = self.crate_state.selected().and_then(|i| self.crate_list.items.get(i)) else { return };
        let next = Next::Pick(t.root.clone(), t.track.file_path.clone());
        if self.loaded.as_ref().is_some_and(|l| l.stale && l.root() == t.root) {
            self.toast("the drive was unplugged", Level::Error);
            return;
        }
        self.failed_in_row = 0;
        self.go_to(&next);
        self.start_current();
    }

    /// What plays after the playing track: the first pick still to play, else the list's next (see
    /// `next_pos`).
    pub(in crate::tui) fn next_up(&self, auto: bool) -> Option<Next> {
        if let Some(t) = self.crate_list.up_next().next() {
            return Some(Next::Pick(t.root.clone(), t.track.file_path.clone()));
        }
        let pos = self.next_pos(auto)?;
        Some(Next::From { pos, item: self.queue.order[pos] })
    }

    /// The track `next` is, and the drive it's on.
    pub(in crate::tui) fn track_of(&self, next: &Next) -> Option<(&Track, &Path)> {
        match next {
            Next::Pick(root, path) => {
                let t = &self.crate_list.items[self.crate_list.position(root, path)?];
                Some((&t.track, &t.root))
            }
            Next::From { item, .. } => Some((self.queue.track(*item), &self.queue.root)),
        }
    }

    /// The queue's track to play now, and the drive it's on: the pick playing, else the list's at
    /// its place.
    pub(in crate::tui) fn current(&self) -> Option<(&Track, &Path)> {
        if let Some(p) = &self.crate_list.playing {
            return Some((&p.track, &p.root));
        }
        let &i = self.queue.order.get(self.queue.pos)?;
        Some((self.queue.track(i), &self.queue.root))
    }

    /// Make `next` the queue's current track: a pick is played from then on.
    fn go_to(&mut self, next: &Next) {
        match next {
            Next::Pick(root, path) => {
                if let Some(i) = self.crate_list.position(root, path) {
                    self.crate_list.items[i].played = true;
                    self.crate_list.playing = Some(self.crate_list.items[i].clone());
                }
            }
            Next::From { pos, .. } => {
                self.queue.pos = *pos;
                self.crate_list.playing = None;
            }
        }
    }

    /// What's queued changed (a pick, shuffle, repeat): a track opened ahead that no longer plays
    /// next is let go of, and the one that does is opened in its place.
    pub(in crate::tui) fn queue_changed(&mut self) {
        let next = self.next_up(true);
        if self.upcoming.as_ref().is_some_and(|u| Some(&u.next) != next.as_ref()) {
            self.drop_upcoming();
        }
    }

    /// Fill, when it's on and nothing's queued after the playing track: the best match for it
    /// (as "mixes into" ranks them) that hasn't played and isn't queued goes on the end of the
    /// queue, marked as filled.
    fn fill_queue(&mut self, auto: bool) {
        if !self.fill || self.next_up(auto).is_some() {
            return;
        }
        let Some(now) = self.now.as_ref().filter(|_| self.playing_queue()) else { return };
        let Some(m) = now.matches.as_ref() else { return };
        let fresh = |t: &&Track| {
            t.file_path != now.track.file_path
                && !self.played_tracks.contains(&t.file_path)
                && !self.crate_list.contains(&m.root, &t.file_path)
        };
        let Some(t) = m.items.iter().map(|x| m.track(x)).find(fresh).cloned() else { return };
        let (root, drive) = (m.root.clone(), self.drive_label(&m.root));
        let name = t.display_name();
        self.crate_list.add(&root, &drive, [t]);
        if let Some(last) = self.crate_list.items.last_mut() {
            last.filled = true;
        }
        self.toast(format!("filled from matches: {name}"), Level::Info);
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

    /// Whether the track playing is the queue's, a pick or the list's (rather than a recorded set,
    /// played on its own).
    pub(super) fn playing_queue(&self) -> bool {
        let Some(now) = &self.now else { return false };
        self.current().is_some_and(|(t, root)| t.file_path == now.track.file_path && root == now.root)
    }

    /// Start the queue's current track. It shows at once; its audio file is opened in the
    /// background (see `opened`), and its analysis, artwork and matches are read in the
    /// background too, filling in when they arrive (and the output, when none is open). False
    /// when there's no track.
    fn start_current(&mut self) -> bool {
        let Some((track, root)) = self.current().map(|(t, r)| (t.clone(), r.to_path_buf())) else { return false };
        // Where the last run left off was for the track it was on: once another starts, it's gone.
        self.resume = None;
        self.ready_to_play();
        self.now = Some(self.open(track, root, true));
        true
    }

    /// Open one of the queue's tracks, on the drive at `root`, in the background: its audio file
    /// (see `opened`), then its analysis, artwork and matches, which fill in as they arrive.
    /// `starts`: it's the one to play now (rather than the next, opened ahead).
    pub(super) fn open(&mut self, track: Track, root: PathBuf, starts: bool) -> NowPlaying {
        let id = self.next_id();
        let (t, r, auto_cue, store_root) = (track.clone(), root.clone(), self.deck.auto_cue, self.store_root.clone());
        // Matches and playlists need the drive's library, which is open unless the player
        // moved to another drive since the track was queued.
        let library = self.loaded.as_ref().filter(|l| l.root() == root).map(|l| l.library.clone());
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
            root,
            analysis: TrackAnalysis::default(),
            waves: Default::default(),
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
        self.go_to(&up.next);
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
        self.fill_queue(true);
        let Some(next) = self.next_up(true) else { return };
        let Some((track, root)) = self.track_of(&next).map(|(t, r)| (t.clone(), r.to_path_buf())) else { return };
        let mut now = self.open(track, root, false);
        now.advancing = true;
        self.upcoming = Some(Upcoming { now, next, queued: false, file: None });
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
        if now.advancing && self.failed_in_row < self.queue.len() + self.crate_list.len() {
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

    /// Move through the queue: the next pick, else the list's next. `auto` is the end of a track
    /// (repeat-one applies to the list).
    pub(super) fn advance(&mut self, auto: bool) {
        self.fill_queue(auto);
        let Some(next) = self.next_up(auto) else {
            self.stop();
            return;
        };
        self.go_to(&next);
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

    /// Back: to the start of the track, or (in its first seconds) the one before. From a pick, that's
    /// the pick played before it, else the list's track, and the pick goes back to play next.
    pub(super) fn previous(&mut self) {
        if self.position() > Duration::from_secs(3) {
            return self.seek_to(Duration::ZERO);
        }
        if let Some(p) = self.crate_list.playing.take() {
            let at = self.crate_list.position(&p.root, &p.track.file_path);
            if let Some(i) = at {
                self.crate_list.items[i].played = false;
            }
            let before = self.crate_list.items[..at.unwrap_or(0)].iter().rposition(|t| t.played);
            if let Some(b) = before {
                self.crate_list.playing = Some(self.crate_list.items[b].clone());
            }
            if before.is_some() || self.current().is_some() {
                self.start_current();
            } else {
                self.crate_list.playing = Some(p);
                if let Some(i) = at {
                    self.crate_list.items[i].played = true;
                }
                self.seek_to(Duration::ZERO);
            }
        } else if self.queue.pos == 0 {
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
            root: path.parent().map(Path::to_path_buf).unwrap_or_default(),
            duration: Duration::from_secs(track.duration_secs as u64),
            track,
            analysis: TrackAnalysis::default(),
            waves: Default::default(),
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
        self.set_view(View::Library);
        self.focus = Focus::Tracks;
    }
}
