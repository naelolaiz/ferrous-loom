//! The main window.
//!
//! The playlist fills the window, as in other pattern-based studios. The
//! channel rack and the piano roll are windows on top of it, opened with
//! F6 and F7. Opening and saving files lives in the [`files`] module,
//! undo and redo in [`edit`], saving the song as a WAV or MIDI file in
//! [`export`], and playing and recording on a keyboard in [`live`].
//!
//! Every change to the project goes through [`LoomApp::edit`], which
//! records it for undo and tells the audio thread what changed.

mod edit;
mod export;
mod files;
mod follow;
mod import;
mod library;
mod live;
mod view;

use std::path::PathBuf;
use std::sync::Arc;

use eframe::egui;
use loom_audio::instrument::Sampler;
use loom_audio::{
    AudioEngine, ChannelNotes, Command, Instrument, PlayMode, SampleCache, SampleData,
};
use loom_core::{
    Channel, ChannelKind, DEFAULT_VELOCITY, History, Meter, MidiMessage, Note, Pass, Pattern,
    Project, SampleSource, Tempo,
};

use crate::channel_rack::{self, RackAction};
use crate::harmony_keys::{HarmonyAction, HarmonyKeys};
use crate::library_browser::Tried;
use crate::midi::Midi;
use crate::piano_roll::{self, PianoRoll, RollAction};
use crate::playlist::{self, PlaylistAction, PlaylistView};
use crate::theme::{self, HintBar};
use crate::widgets;

/// How wide the pattern list in the toolbar is.
const PATTERN_LIST_WIDTH: f32 = 120.0;

/// The sample rate used to prepare sounds when there is no sound card.
const FALLBACK_SAMPLE_RATE: u32 = 48_000;

/// Whether we managed to open the sound card.
///
/// The app still runs without audio (for example on a machine with no
/// sound card), and shows the reason in the status bar.
enum Audio {
    /// Boxed: the engine is much bigger than a `String`, and every
    /// `Audio` would otherwise take as much room as the biggest variant.
    Running(Box<AudioEngine>),
    Unavailable(String),
}

/// The first Loom Osc channel, where the piano roll opens; or the first
/// channel if there is none.
fn first_synth(project: &Project) -> usize {
    project
        .channels()
        .iter()
        .position(|channel| channel.kind == ChannelKind::Synth)
        .unwrap_or(0)
}

/// All the state of the application window.
pub struct LoomApp {
    audio: Audio,
    project: Project,
    /// The selected pattern: the one the channel rack and the piano roll
    /// show, pattern mode plays, and new playlist clips use.
    pattern: usize,
    mode: PlayMode,
    playing: bool,
    /// Where playback starts or carries on from while stopped or paused:
    /// a step of the pattern in pattern mode, of the song in song mode.
    cursor: usize,
    /// Where Stop goes back to: the last place the playhead was moved to.
    marker: usize,
    master_volume: f32,
    rack_open: bool,
    /// The size the rack's rows needed last frame. When it changes (more
    /// steps, another channel, another pattern), the window is fitted to
    /// the rows again.
    rack_natural: egui::Vec2,
    piano_roll: PianoRoll,
    playlist: PlaylistView,
    harmony_keys: HarmonyKeys,
    /// The patterns the library wrote, to rewrite on the next click.
    auditions: Vec<library::Audition>,
    /// The style and rhythm the whole song plays: a demo's own, or the
    /// last one the library gave the song. Shown selected in the library
    /// when the selected pattern is not one the library wrote.
    song_library: Tried,
    /// The song's style and rhythm in versions of the project the library
    /// changed, so undo and redo show the right ones selected.
    song_versions: Vec<(Project, Tried)>,
    file: files::FileState,
    /// Earlier versions of the project, for undo and redo.
    history: History<Project>,
    /// Sample files already read, so undo and export do not read them again.
    samples: SampleCache,
    /// The WAV export running in the background, if any.
    export: Option<export::Export>,
    /// A recording being listened to in the background, if any.
    import: Option<import::Import>,
    /// MIDI, the typing keyboard, and recording.
    live: live::Live,
    /// The last problem to show in the status bar, such as a bad WAV file.
    message: Option<String>,
    /// The last piece of good news for the status bar ("Exported song.wav").
    notice: Option<String>,
    /// The line at the top that says what the thing under the mouse does.
    hint_bar: HintBar,
    /// A window to show in front of the others on the next frame.
    bring_to_front: Option<egui::Id>,
    /// The part of the screen the zoom keys zoom: the one clicked last.
    zoom_area: view::Area,
}

/// The id of the channel rack window.
fn rack_id() -> egui::Id {
    egui::Id::unique("channel-rack")
}

impl LoomApp {
    /// Called once by eframe, before the first frame is drawn. If `path`
    /// is given (a file named on the command line), it is opened;
    /// otherwise the demo song is.
    pub fn new(cc: &eframe::CreationContext<'_>, path: Option<PathBuf>) -> Self {
        theme::apply(&cc.egui_ctx);
        // The zoom keys zoom one part of the screen at a time, not all of
        // it as egui would (see view.rs). Alt and the wheel zooms too, so
        // egui must not take it for scrolling: the scroll it smooths out
        // over the next frames would move the view after the zoom.
        cc.egui_ctx.options_mut(|options| {
            options.zoom_with_keyboard = false;
            options.input_options.zoom_modifier = egui::Modifiers::COMMAND | egui::Modifiers::ALT;
        });

        let mut audio = match AudioEngine::start() {
            Ok(engine) => Audio::Running(Box::new(engine)),
            Err(err) => Audio::Unavailable(err.to_string()),
        };
        // The engine's ways in and out for MIDI go to the MIDI threads.
        let (live_input, midi_output) = match &mut audio {
            Audio::Running(engine) => (engine.take_live_input(), engine.take_midi_output()),
            Audio::Unavailable(_) => (None, None),
        };
        let midi = Midi::new(&cc.egui_ctx, live_input, midi_output);

        let project = Project::demo();
        let piano_roll = PianoRoll::new(first_synth(&project));
        let mut app = LoomApp {
            audio,
            file: files::FileState::new(&project),
            project,
            pattern: 0,
            mode: PlayMode::Song,
            playing: false,
            cursor: 0,
            marker: 0,
            master_volume: 0.8,
            rack_open: true,
            rack_natural: egui::Vec2::ZERO,
            piano_roll,
            playlist: PlaylistView::new(),
            harmony_keys: HarmonyKeys::new(),
            auditions: Vec::new(),
            song_library: Tried::default(),
            song_versions: Vec::new(),
            history: History::default(),
            samples: SampleCache::new(),
            export: None,
            import: None,
            live: live::Live::new(midi),
            message: None,
            notice: None,
            hint_bar: HintBar::default(),
            bring_to_front: None,
            zoom_area: view::Area::Main,
        };
        app.send_whole_project();
        if let Some(path) = path {
            app.open_path(&cc.egui_ctx, path);
        }
        app
    }

    /// Sends a command to the audio thread, if audio is running.
    fn send(&mut self, command: Command) {
        if let Audio::Running(engine) = &mut self.audio {
            engine.send(command);
        }
    }

    fn sample_rate(&self) -> u32 {
        match &self.audio {
            Audio::Running(engine) => engine.info().sample_rate,
            Audio::Unavailable(_) => FALLBACK_SAMPLE_RATE,
        }
    }

    /// Builds the instrument for a channel, at `sample_rate`. If a sample
    /// file cannot be loaded, the channel gets a silent sampler and the
    /// status bar says why.
    fn build_instrument(&mut self, kind: &ChannelKind, sample_rate: u32) -> Instrument {
        match Instrument::for_channel(kind, sample_rate, &mut self.samples) {
            Ok(instrument) => instrument,
            Err(err) => {
                self.message = Some(err.to_string());
                Instrument::Sampler(Sampler::new(Arc::new(SampleData::new(Vec::new(), 1))))
            }
        }
    }

    /// Everything an empty audio thread needs to play the project, with
    /// instruments made for `sample_rate`: the tempo, the patterns, the
    /// channels and the song. Patterns go first, so each channel can bring
    /// its notes for all of them. Used at startup, after opening a file,
    /// and to export the song.
    fn project_commands(&mut self, sample_rate: u32) -> Vec<Command> {
        let mut commands = vec![
            Command::SetTempo(self.project.tempo),
            Command::SetLooseTiming(self.project.loose_timing),
        ];
        for (index, pattern) in self.project.patterns().iter().enumerate() {
            commands.push(Command::InsertPattern {
                index,
                length: pattern.length(),
            });
        }
        for index in 0..self.project.channels().len() {
            commands.push(self.channel_command(index, sample_rate));
        }
        commands.extend(self.song_commands());
        commands
    }

    /// Tells the audio thread about the whole project, at startup and
    /// after opening a file.
    fn send_whole_project(&mut self) {
        self.send(Command::Clear);
        self.send(Command::SetMasterVolume(self.master_volume));
        for command in self.project_commands(self.sample_rate()) {
            self.send(command);
        }
        self.send(Command::SelectPattern(self.pattern));
        self.send(Command::SetMode(self.mode));
        self.live.resend_channel();
        self.live.follow.forget();
        self.live.channels_changed(self.project.channels().len());
    }

    /// The command that inserts channel `index` of the project, with its
    /// instrument and its notes in every pattern.
    fn channel_command(&mut self, index: usize, sample_rate: u32) -> Command {
        let channel = self.project.channels()[index].clone();
        let notes = ChannelNotes::new(
            self.project
                .patterns()
                .iter()
                .map(|pattern| pattern.notes(index).to_vec()),
        );
        Command::InsertChannel {
            index,
            instrument: self.build_instrument(&channel.kind, sample_rate),
            notes,
            volume: channel.volume,
            muted: channel.muted,
        }
    }

    /// The song as song mode plays it: the clips on tracks that are not
    /// muted, with every repeat unrolled, cut to the loop region if one
    /// is set, and the tempo of each of its steps.
    fn song_commands(&self) -> [Command; 2] {
        let song = self.project.playback_arrangement();
        let tempos = self.project.played_tempos(&song);
        [
            Command::SetSong {
                plays: song.plays,
                length: song.length,
            },
            Command::SetSongTempos(tempos),
        ]
    }

    /// Changes the selected pattern, as one undoable step called `label`.
    fn edit_pattern(&mut self, label: &'static str, change: impl FnOnce(&mut Pattern)) {
        let index = self.pattern;
        self.edit(label, |project| {
            if let Some(pattern) = project.pattern_mut(index) {
                change(pattern);
            }
        });
    }

    /// Plays from the cursor, or stops and goes back to the marker. A
    /// cursor or marker past the end of the loop (left there by a longer
    /// pattern, or a song that got shorter) goes back to the start.
    fn set_playing(&mut self, playing: bool) {
        let length = self.loop_length();
        if self.marker >= length {
            self.marker = 0;
        }
        if self.cursor >= length {
            self.cursor = 0;
        }
        if playing {
            self.send(Command::Seek(self.cursor));
            self.send(Command::Play);
            self.playing = true;
        } else {
            self.halt();
            self.cursor = self.marker;
            self.send(Command::Seek(self.cursor));
        }
    }

    /// Stops where the playhead is, so Play carries on from there.
    fn pause(&mut self) {
        let here = self.playhead().unwrap_or(self.cursor);
        self.halt();
        self.cursor = here;
        self.send(Command::Seek(here));
    }

    /// Stops the audio thread, finishing any recording first.
    fn halt(&mut self) {
        if self.playing {
            // Before the audio thread forgets where it was.
            self.finish_recording();
        }
        self.playing = false;
        self.send(Command::Stop);
    }

    /// How long the loop being played is: the selected pattern, or the
    /// song (one bar of 4/4 when it is empty, as the engine plays it).
    fn loop_length(&self) -> usize {
        let length = match self.mode {
            PlayMode::Pattern => self.project.patterns()[self.pattern].length(),
            PlayMode::Song => self.project.playback_arrangement().length,
        };
        if length == 0 {
            loom_core::STEPS_PER_BAR
        } else {
            length
        }
    }

    /// Moves the playhead to `step`, which also becomes the marker that
    /// Stop goes back to. While playing, playback jumps there. A step past
    /// the end of the loop goes back to the start.
    fn seek(&mut self, step: usize) {
        let step = if step >= self.loop_length() { 0 } else { step };
        self.marker = step;
        self.cursor = step;
        self.send(Command::Seek(step));
    }

    /// Moves the playhead by `bars` bars (back if negative), to the start
    /// of a bar.
    fn skip_bars(&mut self, bars: isize) {
        let meter = self.meter();
        let here = self.playhead().unwrap_or(self.cursor);
        let bar = meter.bar_at(here);
        let number = bar.number as isize;
        // Going back from inside a bar first goes to its start. While
        // playing, the playhead is never exactly on a bar line, so within
        // the first beat of a bar it counts as being at its start: two
        // quick presses go back two bars, as on a tape machine.
        let into_bar = here - bar.start;
        let at_start = into_bar == 0 || (self.playing && into_bar < bar.time.steps_per_beat());
        let target = if bars < 0 && !at_start {
            number + bars + 1
        } else {
            number + bars
        };
        if target < 0 {
            self.seek(0);
        } else {
            self.seek(meter.bar(target as usize).start);
        }
    }

    /// Where the bars are in what is playing: the song as it plays, with
    /// every change of time signature, or the selected pattern, in the
    /// time signature it is used in.
    fn meter(&self) -> Meter {
        match self.mode {
            PlayMode::Pattern => Meter::steady(self.project.pattern_time(self.pattern)),
            PlayMode::Song => self
                .project
                .played_meter(&self.project.playback_arrangement()),
        }
    }

    fn set_mode(&mut self, mode: PlayMode) {
        if mode != self.mode {
            self.finish_recording();
            self.mode = mode;
            self.cursor = 0;
            self.marker = 0;
            self.send(Command::SetMode(mode));
        }
    }

    /// Opens the editor that suits a pattern, as a double-click on one of
    /// its clips does: the piano roll when it has melodies or chords, on
    /// the channel that plays them, and the channel rack for drums.
    fn edit_clip(&mut self, pattern: usize) {
        self.select_pattern(pattern);
        let Some(notes) = self.project.pattern(pattern) else {
            return;
        };
        let current = self.piano_roll.channel();
        let melodic = |channel| notes.notes(channel).iter().any(|note| !note.is_step());
        let channel = if melodic(current) {
            Some(current)
        } else {
            piano_roll::busiest_channel(notes)
        };
        match channel {
            Some(channel) => {
                self.piano_roll.open_for(channel);
                self.bring_to_front = Some(PianoRoll::id());
            }
            None => self.show_rack(),
        }
    }

    /// Opens the channel rack and brings it in front of the other windows.
    fn show_rack(&mut self) {
        self.rack_open = true;
        self.bring_to_front = Some(rack_id());
    }

    fn select_pattern(&mut self, pattern: usize) {
        if pattern < self.project.patterns().len() {
            if pattern != self.pattern {
                self.rewind_for_new_pattern();
            }
            self.pattern = pattern;
            self.send(Command::SelectPattern(pattern));
        }
    }

    /// In pattern mode, the cursor and the marker are steps of the
    /// selected pattern. When another one is selected, they go back to its
    /// start, so Play does not begin part way through it. While playing,
    /// playback carries on where it is, and only Stop goes back to the
    /// start.
    fn rewind_for_new_pattern(&mut self) {
        if self.mode != PlayMode::Pattern {
            return;
        }
        self.marker = 0;
        if !self.playing {
            self.cursor = 0;
            self.send(Command::Seek(0));
        }
    }

    // Adding, cloning and removing patterns only change the project. The
    // edit tells the audio thread, and selects the new pattern (see
    // `sync_audio` in `edit.rs`).

    fn add_pattern(&mut self) {
        self.edit("add pattern", |project| project.add_pattern());
    }

    fn clone_pattern(&mut self) {
        let index = self.pattern;
        self.edit("clone pattern", |project| project.clone_pattern(index));
    }

    fn remove_pattern(&mut self) {
        let index = self.pattern;
        self.edit("delete pattern", |project| project.remove_pattern(index));
    }

    /// Applies one action from the channel rack.
    fn apply(&mut self, action: RackAction) {
        match action {
            RackAction::ToggleStep { channel, step } => {
                self.edit_pattern("change steps", |pattern| {
                    pattern.toggle(channel, step);
                });
            }
            RackAction::ClearStep { channel, step } => {
                self.edit_pattern("change steps", |pattern| {
                    pattern.set(channel, step, false);
                });
            }
            RackAction::Preview(channel) => {
                // Clicking a channel also selects it, for the piano roll
                // and the keyboards.
                self.piano_roll.select(channel);
                self.send(Command::Preview(channel));
            }
            RackAction::SetVolume { channel, volume } => {
                self.edit("change volume", |project| {
                    if let Some(state) = project.channel_mut(channel) {
                        state.volume = volume;
                    }
                });
            }
            RackAction::ToggleMute(channel) => {
                self.edit("mute channel", |project| {
                    if let Some(state) = project.channel_mut(channel) {
                        state.muted = !state.muted;
                    }
                });
            }
            RackAction::Remove(channel) => {
                self.edit("delete channel", |project| project.remove_channel(channel));
            }
            RackAction::OpenPianoRoll(channel) => self.piano_roll.open_for(channel),
            RackAction::SelectPattern(pattern) => self.select_pattern(pattern),
            RackAction::AddDrum(drum) => self.add_channel(Channel::drum(drum)),
            RackAction::AddSynth => self.add_channel(Channel::synth()),
            RackAction::AddMidiOut => self.add_channel(Channel::midi_out(0)),
            RackAction::SetMidiChannel {
                channel,
                midi_channel,
            } => {
                self.edit("change MIDI channel", |project| {
                    if let Some(state) = project.channel_mut(channel) {
                        // A name nobody changed follows the channel number.
                        if let ChannelKind::MidiOut { channel: old } = state.kind
                            && state.name == Channel::midi_out(old).name
                        {
                            state.name = Channel::midi_out(midi_channel).name;
                        }
                        state.kind = ChannelKind::MidiOut {
                            channel: midi_channel,
                        };
                    }
                });
            }
            RackAction::LoadSample { channel, path } => self.load_sample(channel, path),
        }
    }

    /// Applies one action from the piano roll.
    fn apply_roll(&mut self, action: RollAction) {
        match action {
            RollAction::AddNotes { channel, notes } => {
                self.edit_pattern("add notes", |pattern| {
                    for note in notes {
                        pattern.add_note(channel, note);
                    }
                });
            }
            RollAction::RemoveNote { channel, index } => {
                self.edit_pattern("delete notes", |pattern| {
                    pattern.remove_note(channel, index);
                });
            }
            RollAction::ReplaceNote {
                channel,
                index,
                note,
            } => {
                self.edit_pattern("change notes", |pattern| {
                    pattern.replace_note(channel, index, note);
                });
            }
            RollAction::ReplaceNotes {
                channel,
                old,
                new,
                label,
            } => {
                self.edit_pattern(label, |pattern| {
                    pattern.replace_notes(channel, &old, &new);
                });
            }
            RollAction::Preview { channel, pitches } => {
                for pitch in pitches {
                    self.send(Command::PreviewNote { channel, pitch });
                }
            }
            RollAction::SetScale(scale) => {
                self.edit("change key", |project| project.change_key(scale))
            }
            RollAction::SelectPattern(pattern) => self.select_pattern(pattern),
            RollAction::OpenRack => self.show_rack(),
        }
    }

    /// Applies one action from the harmony keys bar.
    fn apply_harmony(&mut self, action: HarmonyAction) {
        match action {
            HarmonyAction::SetScale(scale) => {
                self.edit("change key", |project| project.change_key(scale));
            }
            HarmonyAction::SetTime(time) => {
                self.edit("change time signature", |project| {
                    project.time_signature = time;
                });
            }
            // The notes go the same way as the typing keyboard's: to the
            // audio thread, which plays them on the selected channel, and
            // to the recorder.
            HarmonyAction::NoteOn(notes) => {
                for pitch in notes {
                    self.play_typed(MidiMessage::NoteOn {
                        channel: 0,
                        pitch,
                        velocity: DEFAULT_VELOCITY,
                    });
                }
            }
            HarmonyAction::NoteOff(notes) => {
                for pitch in notes {
                    self.play_typed(MidiMessage::NoteOff { channel: 0, pitch });
                }
            }
        }
    }

    /// Applies one action from the playlist.
    fn apply_playlist(&mut self, action: PlaylistAction) {
        match action {
            PlaylistAction::SelectPattern(pattern) => self.select_pattern(pattern),
            PlaylistAction::AddClip(clip) => {
                self.edit("add clip", |project| project.add_clip(clip));
            }
            PlaylistAction::MoveClip {
                index,
                track,
                start,
            } => {
                self.edit("move clip", |project| {
                    project.move_clip(index, track, start)
                });
            }
            PlaylistAction::EditClip(pattern) => self.edit_clip(pattern),
            PlaylistAction::RemoveClip(index) => {
                self.edit("delete clips", |project| project.remove_clip(index));
            }
            PlaylistAction::ToggleTrackMute(track) => {
                self.edit("mute track", |project| {
                    if let Some(track) = project.track_mut(track) {
                        track.muted = !track.muted;
                    }
                });
            }
            PlaylistAction::RenameTrack { track, name } => {
                self.edit("rename track", |project| {
                    if let Some(track) = project.track_mut(track) {
                        track.name = name;
                    }
                });
            }
            PlaylistAction::AddTrack => {
                self.edit("add track", |project| project.add_track());
            }
            PlaylistAction::InsertTrack(index) => {
                self.edit("insert track", |project| project.insert_track(index));
            }
            PlaylistAction::RemoveTrack(index) => {
                self.edit("delete track", |project| project.remove_track(index));
            }
            PlaylistAction::SwapTracks(index) => {
                self.edit("move track", |project| project.swap_tracks(index));
            }
            PlaylistAction::Seek(step) => {
                self.set_mode(PlayMode::Song);
                // The playlist's timeline counts a repeated stretch once;
                // the audio thread counts every time round.
                let song = self.project.playback_arrangement();
                let step = song
                    .passes
                    .iter()
                    .find(|pass| (pass.from..pass.from + pass.length).contains(&step))
                    .map_or(0, |pass| pass.start + (step - pass.from));
                self.seek(step);
            }
            PlaylistAction::SetClipRepeats { index, repeats } => {
                self.edit("repeat clip", |project| {
                    project.set_clip_repeats(index, repeats)
                });
            }
            PlaylistAction::AddSection(section) => {
                self.edit("add section", |project| project.add_section(section));
            }
            PlaylistAction::SetSection { index, section } => {
                self.edit("change section", |project| {
                    project.set_section(index, section)
                });
            }
            PlaylistAction::RemoveSection(index) => {
                self.edit("delete section", |project| project.remove_section(index));
            }
            PlaylistAction::DuplicateSection(index) => {
                self.edit("duplicate section", |project| {
                    project.duplicate_section(index, 1)
                });
            }
            PlaylistAction::UnrollSection(index) => {
                self.edit("write out repeats", |project| project.unroll_section(index));
            }
            PlaylistAction::SetLoop(region) => {
                self.edit("set loop", |project| project.set_loop_region(region));
            }
            PlaylistAction::SetTimeMarker(marker) => {
                self.edit("change time signature", |project| {
                    project.set_time_marker(marker)
                });
            }
            PlaylistAction::RemoveTimeMarker(start) => {
                self.edit("remove time signature change", |project| {
                    project.remove_time_marker(start)
                });
            }
            PlaylistAction::SetTempoMarker(marker) => {
                self.edit("change tempo", |project| project.set_tempo_marker(marker));
            }
            PlaylistAction::SetLooseTiming(ms) => {
                self.edit("change loose timing", |project| project.loose_timing = ms);
            }
            PlaylistAction::RemoveTempoMarker(start) => {
                self.edit("remove tempo change", |project| {
                    project.remove_tempo_marker(start)
                });
            }
            PlaylistAction::Library(action) => self.try_from_library(action),
        }
    }

    fn add_channel(&mut self, channel: Channel) {
        self.edit("add channel", |project| project.add_channel(channel));
    }

    /// Loads a sample file into channel `channel`, or into a new channel.
    fn load_sample(&mut self, channel: Option<usize>, path: PathBuf) {
        // Read the file now, to report a bad one before changing anything.
        // It stays in the cache, so building the instrument does not
        // read it again. `reload` because the file may have changed since
        // it was last dropped.
        if let Err(err) = self.samples.reload(&path) {
            self.message = Some(format!("{}: {err}", path.display()));
            return;
        }
        let kind = ChannelKind::Sampler(SampleSource::File(path.clone()));
        let name = path
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Sample".to_string());

        self.edit("load sample", |project| {
            match channel.and_then(|index| project.channel_mut(index)) {
                Some(existing) => {
                    existing.name = name;
                    existing.kind = kind;
                }
                None => {
                    project.add_channel(Channel {
                        name,
                        kind,
                        volume: 0.8,
                        muted: false,
                        program: None,
                    });
                }
            }
        });
        self.message = None;
    }

    /// The menu bar along the very top, with the hint bar beside it.
    fn menu_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            self.file_menu(ui);
            self.edit_menu(ui);
            self.view_menu(ui);
            self.midi_menu(ui);
            ui.separator();
            self.hint_bar.show(ui);
        });
    }

    /// The toolbar under the menus: the transport, the tempo and song
    /// position displays, the pattern selector, buttons for the windows
    /// and the master volume, each in its own group. When the window is
    /// too narrow for all of it (or zoomed in), the groups wrap onto a
    /// second row instead of running off the edge.
    fn transport_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            theme::group(ui, |ui| {
                let mut mode = self.mode;
                let pattern = theme::light_button(ui, "PAT", mode == PlayMode::Pattern, None);
                if theme::hint(pattern, "Pattern mode: loop the selected pattern (L)").clicked() {
                    mode = PlayMode::Pattern;
                }
                let song =
                    theme::light_button(ui, "SONG", mode == PlayMode::Song, Some(theme::GREEN));
                if theme::hint(song, "Song mode: play the playlist (L)").clicked() {
                    mode = PlayMode::Song;
                }
                self.set_mode(mode);
            });

            theme::group(ui, |ui| {
                let start = theme::light_button(ui, "⏮", false, None);
                if theme::hint(start, "Go to the start (Home)").clicked() {
                    self.seek(0);
                }
                let back = theme::light_button(ui, "⏪", false, None);
                if theme::hint(back, "Back one bar (Page Up)").clicked() {
                    self.skip_bars(-1);
                }
                let (icon, text) = if self.playing {
                    ("❚❚", "Pause: stop here, and carry on from here next time")
                } else {
                    ("▶", "Play from the playhead (Space)")
                };
                let play = theme::light_button(ui, icon, self.playing, Some(theme::GREEN));
                if theme::hint(play, text).clicked() {
                    if self.playing {
                        self.pause();
                    } else {
                        self.set_playing(true);
                    }
                }
                let stop = theme::light_button(ui, "⏹", false, None);
                if theme::hint(
                    stop,
                    "Stop and go back to where you last put the playhead (Space)",
                )
                .clicked()
                {
                    self.set_playing(false);
                }
                let forward = theme::light_button(ui, "⏩", false, None);
                if theme::hint(forward, "Forward one bar (Page Down)").clicked() {
                    self.skip_bars(1);
                }
                self.record_button(ui);
            });

            // While a song whose tempo changes plays, the box follows the
            // tempo at the playhead, as FL Studio's does; stopped, it is
            // the tempo the song starts at, to change.
            let map = self.project.tempo_map();
            let now = if map.is_steady() || !self.playing {
                None
            } else {
                self.song_position()
                    .map(|(step, _)| map.tempo_at(step).bpm())
            };
            if let Some(now) = now {
                let lcd = theme::lcd(ui, "TEMPO NOW", &format!("{now:.1}"), 62.0);
                theme::hint(
                    lcd,
                    "The tempo playing now: this song changes tempo (see the tempo strip at \
                     the top of the playlist). Stop to change the tempo it starts at; every \
                     change moves with it",
                );
            } else {
                let mut bpm = self.project.tempo.bpm();
                let tempo = theme::lcd_frame(ui, "TEMPO", 62.0, |ui| {
                    ui.add(
                        egui::DragValue::new(&mut bpm)
                            .range(Tempo::MIN..=Tempo::MAX)
                            .speed(0.5)
                            .fixed_decimals(1),
                    )
                });
                let text = if map.is_steady() {
                    "Tempo in beats a minute. Drag up or down, or double-click to type"
                } else {
                    "The tempo the song starts at, in beats a minute. This song changes \
                     tempo (see the tempo strip at the top of the playlist), and every \
                     change moves with this one. Drag up or down, or double-click to type"
                };
                if theme::hint(tempo, text).changed() {
                    self.edit("change tempo", |project| {
                        project.scale_tempo(Tempo::new(bpm))
                    });
                }
            }
            let position = theme::lcd(ui, "BAR : BEAT : STEP", &self.position_text(), 92.0);
            theme::hint(
                position,
                "Where the playhead is: in the pattern (PAT) or the song (SONG). Click the \
             playlist's bar numbers to move it",
            );

            theme::group(ui, |ui| self.pattern_selector(ui));

            theme::group(ui, |ui| {
                let rack = theme::light_button(ui, "▦ Rack", self.rack_open, None);
                if theme::hint(rack, "Show or hide the channel rack: the steps (F6)").clicked() {
                    self.rack_open = !self.rack_open;
                }
                let roll = theme::light_button(ui, "🎹 Piano roll", self.piano_roll.open, None);
                if theme::hint(
                    roll,
                    "Show or hide the piano roll: melodies and chords (F7)",
                )
                .clicked()
                {
                    self.piano_roll.open = !self.piano_roll.open;
                }
                let harmony = theme::light_button(ui, "🎼 Harmony", self.harmony_keys.open, None);
                if theme::hint(
                    harmony,
                    "Show or hide the harmony keys: the key, the time signature and a key \
                     for each chord (F8)",
                )
                .clicked()
                {
                    self.harmony_keys.open = !self.harmony_keys.open;
                }
            });

            theme::group(ui, |ui| {
                // The label goes first, as its own widget, so the slider
                // and its name wrap to the next row together.
                ui.label(egui::RichText::new("VOL").small().color(theme::TEXT_DIM));
                let volume_slider =
                    egui::Slider::new(&mut self.master_volume, 0.0..=1.0).show_value(false);
                let volume = ui.add_sized([70.0, 22.0], volume_slider);
                let text = format!("Master volume: {:.0}%", self.master_volume * 100.0);
                if theme::hint(volume, text).changed() {
                    self.send(Command::SetMasterVolume(self.master_volume));
                }
            });
        });
    }

    /// The playhead as bar, beat and step, all counted from 1, for the
    /// song position display. Stopped, it shows the start.
    fn position_text(&self) -> String {
        let step = self.playhead().unwrap_or(self.cursor);
        let (bar, beat, sub) = self.meter().position(step);
        // A loop plays from its own start, so its bars are counted from
        // the bar it starts on, as the ruler numbers them.
        let first = match (self.mode, self.project.playlist().loop_region()) {
            (PlayMode::Song, Some(region)) => self.project.meter().bar_at(region.start).number,
            _ => 0,
        };
        let bar = bar + first;
        format!("{bar:>3}:{beat:02}:{sub:02}")
    }

    /// Choosing, adding, cloning, renaming and deleting patterns, and the
    /// selected pattern's length.
    fn pattern_selector(&mut self, ui: &mut egui::Ui) {
        let current = self.pattern;
        let mut selected = current;
        let name = self.project.patterns()[current].name.clone();
        ui.label(
            egui::RichText::new("PATTERN")
                .small()
                .color(theme::TEXT_DIM),
        );
        // A long name is cut short with "…" rather than widening the
        // toolbar onto a second row; the hint gives it whole. `width` is
        // only the narrowest the list can be, so it is given a space no
        // wider than that to fit in.
        let room = egui::vec2(PATTERN_LIST_WIDTH, ui.spacing().interact_size.y);
        let combo = ui
            .allocate_ui(room, |ui| {
                egui::ComboBox::from_id_salt("pattern")
                    .width(PATTERN_LIST_WIDTH)
                    .truncate()
                    .selected_text(
                        egui::RichText::new(&name)
                            .color(playlist::pattern_color(&self.project, current)),
                    )
                    .show_ui(ui, |ui| {
                        for (index, pattern) in self.project.patterns().iter().enumerate() {
                            let text = egui::RichText::new(&pattern.name)
                                .color(playlist::pattern_color(&self.project, index));
                            ui.selectable_value(&mut selected, index, text);
                        }
                    })
                    .response
            })
            .inner;
        theme::hint(
            combo,
            format!(
                "The selected pattern, \"{name}\": the channel rack and the piano roll show \
                 it, and new clips in the playlist use it"
            ),
        );
        if selected != current {
            self.select_pattern(selected);
        }

        if theme::hint(ui.button("➕"), "Add a new, empty pattern").clicked() {
            self.add_pattern();
        }
        // Menus normally close on any click. This one has a text field, so
        // it stays open until you click somewhere else.
        let stay_open = egui::containers::menu::MenuConfig::new()
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside);
        let menu = egui::containers::menu::MenuButton::new("⋯").config(stay_open);
        let (menu, _) = menu.ui(ui, |ui| {
            ui.label("Name");
            // The field edits a copy, so the change can go through `edit`.
            let mut name = self.project.patterns()[self.pattern].name.clone();
            if ui.text_edit_singleline(&mut name).changed() {
                self.edit_pattern("rename pattern", |pattern| pattern.name = name);
            }
            if ui.button("Clone").clicked() {
                self.clone_pattern();
                ui.close();
            }
            let can_delete = self.project.patterns().len() > 1;
            if ui
                .add_enabled(can_delete, egui::Button::new("Delete"))
                .on_hover_text("Also removes its clips from the playlist")
                .clicked()
            {
                self.remove_pattern();
                ui.close();
            }
        });

        theme::hint(menu, "Rename, clone or delete the selected pattern");

        let length = self.project.patterns()[self.pattern].length();
        let mut new_length = length;
        let time = self.project.pattern_time(self.pattern);
        let length_combo = egui::ComboBox::from_id_salt("pattern-length")
            .selected_text(time.describe_length(length))
            .show_ui(ui, |ui| {
                for option in time.pattern_lengths() {
                    ui.selectable_value(&mut new_length, option, time.describe_length(option));
                }
            })
            .response;
        theme::hint(
            length_combo,
            format!("How long the selected pattern is: {length} steps"),
        );
        if new_length != length {
            self.edit_pattern("change pattern length", |pattern| {
                pattern.set_length(new_length);
            });
        }
    }

    /// The bar along the bottom: messages on the left, zoom on the right.
    fn status_bar(&self, ui: &mut egui::Ui) {
        // The zoom control goes in first, from the right, so it always
        // has room; the messages get what is left, cut short with "…".
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            view::zoom_control(ui);
            ui.separator();
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
                self.status_messages(ui);
            });
        });
    }

    /// Which sound card is in use, the chord held, an export running, and
    /// the last piece of news or problem.
    fn status_messages(&self, ui: &mut egui::Ui) {
        match &self.audio {
            Audio::Running(engine) => {
                let info = engine.info();
                ui.label(format!(
                    "Audio: {} · {} Hz · {} channels",
                    info.device_name, info.sample_rate, info.channels
                ));
            }
            Audio::Unavailable(reason) => {
                ui.colored_label(
                    ui.visuals().warn_fg_color,
                    format!("Audio unavailable: {reason}"),
                );
            }
        }
        if let Some(chord) = self.held_chord() {
            ui.separator();
            ui.label(chord);
        }
        if let Some(export) = &self.export {
            ui.separator();
            ui.add(
                egui::ProgressBar::new(export.progress())
                    .desired_width(160.0)
                    .text(format!("Exporting {}", export.file_name())),
            );
        }
        if let Some(notice) = &self.notice {
            ui.separator();
            ui.label(notice);
        }
        if let Some(message) = &self.message {
            ui.separator();
            ui.colored_label(ui.visuals().error_fg_color, message);
        }
    }

    /// The step the sequencer is on, or `None` when stopped. In pattern
    /// mode it is a step of the selected pattern; in song mode, of the song.
    fn playhead(&self) -> Option<usize> {
        match &self.audio {
            Audio::Running(engine) if self.playing => engine.playhead(),
            _ => None,
        }
    }

    /// The step of the selected pattern being played, for the rack and the
    /// piano roll. In song mode that depends on which clip is playing.
    fn pattern_playhead(&self) -> Option<usize> {
        let step = self.playhead()?;
        match self.mode {
            PlayMode::Pattern => Some(step),
            PlayMode::Song => {
                let (step, _) = self.song_position()?;
                self.project.pattern_step_in_song(self.pattern, step)
            }
        }
    }

    /// The chord notes playing at the playhead, for the harmony keys.
    fn chord_playing(&self) -> Vec<Note> {
        match (self.playhead(), self.mode) {
            (Some(step), PlayMode::Pattern) => {
                self.project.chord_notes_in_pattern(self.pattern, step)
            }
            (Some(_), PlayMode::Song) => match self.song_position() {
                Some((step, _)) => self.project.chord_notes_in_song(step),
                None => Vec::new(),
            },
            (None, _) => Vec::new(),
        }
    }

    /// Where the song is, in song mode: the step of the playlist's
    /// timeline being played (or where Play starts, while stopped), and which time round it is if it repeats.
    /// The audio thread counts steps of the song as played, so a chorus
    /// played twice counts twice; the arrangement turns that back into a
    /// place in the playlist.
    fn song_position(&self) -> Option<(usize, Pass)> {
        if self.mode != PlayMode::Song {
            return None;
        }
        // While stopped, where Play will start.
        let step = self.playhead().unwrap_or(self.cursor);
        let song = self.project.playback_arrangement();
        let pass = *song.pass_at(step)?;
        Some((pass.from + (step - pass.start), pass))
    }

    fn keyboard_shortcuts(&mut self, ctx: &egui::Context) {
        // While a text field has the keyboard (a track or pattern name),
        // Space and L are letters, not shortcuts.
        let typing = ctx.text_edit_focused();
        // `consume_key` reports whether the key was pressed and removes the
        // press, so a focused button does not also react to Space.
        let pressed = |key| ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, key));
        if !typing && pressed(egui::Key::Space) {
            self.set_playing(!self.playing);
        }
        if !typing && pressed(egui::Key::Home) {
            self.seek(0);
        }
        if !typing && pressed(egui::Key::PageUp) {
            self.skip_bars(-1);
        }
        if !typing && pressed(egui::Key::PageDown) {
            self.skip_bars(1);
        }
        if !typing && pressed(egui::Key::L) {
            self.set_mode(match self.mode {
                PlayMode::Pattern => PlayMode::Song,
                PlayMode::Song => PlayMode::Pattern,
            });
        }
        self.view_shortcuts(ctx);
        self.zoom_shortcuts(ctx);
        self.file_shortcuts(ctx);
        if !typing {
            // While typing, Ctrl+Z belongs to the text field.
            self.edit_shortcuts(ctx);
        }
    }
}

impl eframe::App for LoomApp {
    /// Called by eframe every time the window needs to be redrawn.
    /// egui is "immediate mode": we describe the whole UI on every call.
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if let Audio::Running(engine) = &mut self.audio {
            engine.collect_garbage();
        }
        let ctx = ui.ctx().clone();
        // Before anything is drawn: whether a menu is open as the mouse
        // button goes down.
        widgets::track_dismissing(&ctx);
        self.start_frame_for_undo(&ctx);
        self.keyboard_shortcuts(&ctx);
        self.live_input(&ctx);
        self.handle_dropped_files(&ctx);
        self.check_export(&ctx);
        self.check_import(&ctx);
        if self.playing {
            // Keep redrawing so the playhead moves.
            ctx.request_repaint();
        }

        self.hint_bar.start_frame(&ctx);
        let bar = |fill| {
            egui::Frame::new()
                .fill(fill)
                .inner_margin(egui::Margin::symmetric(6, 3))
                .stroke(egui::Stroke::new(1.0, theme::OUTLINE))
        };
        egui::Panel::top("menu-bar")
            .frame(bar(theme::BACKGROUND))
            .show(ui, |ui| self.menu_bar(ui));
        egui::Panel::top("transport")
            .frame(bar(theme::PANEL).inner_margin(egui::Margin::symmetric(6, 5)))
            .show(ui, |ui| self.transport_bar(ui));
        egui::Panel::bottom("status")
            .frame(bar(theme::BACKGROUND))
            .show(ui, |ui| self.status_bar(ui));
        // Added after the status bar, so it sits just above it.
        let harmony_actions = if self.harmony_keys.open {
            let held = self.live.held_pitches();
            let channel = self.piano_roll.channel();
            let channel_name = self
                .project
                .channels()
                .get(channel)
                .map_or("", |channel| channel.name.as_str());
            let heard = self.chord_playing();
            let digits = !self.live.typing() && !ctx.text_edit_focused();
            egui::Panel::bottom("harmony-keys")
                .frame(bar(theme::PANEL).inner_margin(egui::Margin::symmetric(8, 4)))
                .show(ui, |ui| {
                    self.harmony_keys
                        .show(ui, &self.project, channel_name, &held, &heard, digits)
                })
                .inner
        } else {
            self.harmony_keys.release()
        };
        for action in harmony_actions {
            self.apply_harmony(action);
        }

        let song_position = self.song_position();
        let tried = self.tried();
        let playlist_zoom = egui::vec2(
            view::Area::Main.zoom(&ctx, view::Axis::Horizontal),
            view::Area::Main.zoom(&ctx, view::Axis::Vertical),
        );
        let playlist = egui::CentralPanel::default_margins().show(ui, |ui| {
            self.playlist.show(
                ui,
                &self.project,
                self.pattern,
                song_position,
                tried,
                playlist_zoom,
            )
        });
        // The windows stay over the playlist, so they never hide the
        // toolbar, the harmony keys or the status bar.
        let area = playlist.response.rect;
        let playlist_actions = playlist.inner;
        for action in playlist_actions {
            self.apply_playlist(action);
        }

        // The channel rack and the piano roll float on top of the playlist.
        let playhead = self.pattern_playhead();
        let mut rack_open = self.rack_open;
        let pattern_name = &self.project.patterns()[self.pattern].name;
        if let Some(id) = self.bring_to_front.take() {
            ctx.move_to_top(egui::LayerId::new(egui::Order::Middle, id));
        }
        // The rack opens as big as its rows, up to the size of the
        // playlist; it can be resized, and scrolls when made smaller.
        // When the rows grow or shrink (a longer pattern, a new channel),
        // it is fitted to them again, so no step hides past its edge.
        let rack_scale = Self::rack_scale(&ctx);
        let natural =
            channel_rack::natural_size(&ctx.global_style(), &self.project, self.pattern, 1.0)
                * rack_scale;
        // A window's default size is its outside: the contents, the frame
        // around them and the title bar.
        let style = ctx.global_style();
        let frame = style.spacing.window_margin.sum();
        let title = style.text_styles[&egui::TextStyle::Heading].size + 2.0 * frame.y;
        let rack_size =
            (natural + frame + egui::vec2(0.0, title)).min(area.size() - egui::vec2(40.0, 80.0));
        let mut rack = egui::Window::new(format!("Channel rack · {pattern_name}"))
            .id(rack_id())
            .open(&mut rack_open)
            .default_pos([240.0, 380.0])
            .default_size(rack_size)
            .constrain_to(area)
            .resizable(true);
        if natural != self.rack_natural {
            if self.rack_natural != egui::Vec2::ZERO {
                rack = rack.fixed_size(rack_size);
            }
            self.rack_natural = natural;
        }
        let rack_actions = rack
            .show(&ctx, |ui| {
                widgets::zoom_contents(ui, rack_scale);
                let selected = self.piano_roll.channel();
                channel_rack::show(ui, &self.project, self.pattern, selected, playhead)
            })
            .and_then(|window| window.inner)
            .unwrap_or_default();
        self.rack_open = rack_open;
        for action in rack_actions {
            self.apply(action);
        }

        let held = self.live.held_pitches();
        self.piano_roll.hints = self.harmony_keys.hints;
        let roll_zoom = egui::vec2(
            view::Area::PianoRoll.zoom(&ctx, view::Axis::Horizontal),
            view::Area::PianoRoll.zoom(&ctx, view::Axis::Vertical),
        );
        let roll_actions = self.piano_roll.show(
            &ctx,
            &self.project,
            self.pattern,
            playhead,
            &held,
            area,
            roll_zoom,
        );
        for action in roll_actions {
            self.apply_roll(action);
        }

        self.unsaved_changes_dialog(&ctx);
        self.update_title(&ctx);
        self.end_frame_for_undo(&ctx);
    }
}
