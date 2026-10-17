//! Opening a MIDI file as a project: File > Import MIDI file.
//!
//! [`read`](super::read) turns the file's bytes into events. This module
//! turns the events into what a project holds:
//!
//! - **Channels.** Each track of the file that plays notes becomes a
//!   channel of the rack, playing the Loom Osc. A file with a single track
//!   (format 0) keeps each instrument on a MIDI channel of its own, so
//!   there each MIDI channel becomes a rack channel instead. The drums of
//!   General MIDI's channel 10 become one channel per drum sound, like the
//!   kit of a new project.
//! - **Notes.** Every note keeps its velocity and its exact start and
//!   length, even between two steps (see [`NoteEvent::nudge`]). While the
//!   sustain pedal is down, notes ring on as they do on a piano.
//! - **Patterns.** The song is cut at bar lines into patterns of up to 16
//!   bars, holding every channel, and laid one after another on the
//!   playlist. A stretch that comes back note for note plays the same
//!   pattern again.
//! - **Tempo, time and key.** The tempo changes become tempo markers, the
//!   time signature changes time markers, and the key signature the
//!   project's key, as the file writes it. A file Ferrous Loom wrote also
//!   holds the exact key, such as E Dorian, which a key signature cannot
//!   say. A file without a key signature gets the key its notes fit best
//!   (see [`key_fits`]), and so does a file that says C major when its
//!   notes clearly are in another key: many programs write C major when
//!   nobody chose a key.

use std::collections::BTreeMap;

use super::read::{EventKind, MidiFile, Timing, Track};
use crate::harmony::{Scale, ScaleKind, key_fits};
use crate::meter::{Meter, TimeMarker, TimeSignature};
use crate::pattern::{MAX_STEPS, NoteEvent, STEP_NOTE, TICKS_PER_STEP};
use crate::playlist::{Clip, MAX_SONG_STEPS};
use crate::project::{Channel, Drum, MAX_CHANNELS, MAX_PATTERNS, Project};
use crate::tempo::{Tempo, TempoMarker};
use crate::{DRUM_CHANNEL, Note, PitchClass, STEPS_PER_BEAT};

/// Our ticks to the beat: [`TICKS_PER_STEP`] to each of the four steps.
const TICKS_PER_BEAT: u64 = (TICKS_PER_STEP * STEPS_PER_BEAT as usize) as u64;

/// The tempo of a file that does not say: the MIDI standard's 120 BPM.
const DEFAULT_BPM: f32 = 120.0;

/// The longest pattern made, in bars.
const BARS_PER_PATTERN: usize = 16;

/// The most notes kept. A long piano piece has a few thousand; a few
/// files on the internet have millions, written as a stunt, and would
/// make the piano roll too slow to use.
pub const MAX_NOTES: usize = 50_000;

/// The sustain pedal and the volume, as controller numbers.
const VOLUME: u8 = 7;
const SUSTAIN: u8 = 64;
/// The volume a MIDI channel has until the file sets one.
const DEFAULT_MIDI_VOLUME: f32 = 100.0;

/// A project made from a MIDI file, and what was found in it.
#[derive(Debug, Clone)]
pub struct Imported {
    pub project: Project,
    pub report: Report,
}

/// What the import found and did, to tell the user.
#[derive(Debug, Clone, PartialEq)]
pub struct Report {
    pub channels: usize,
    pub notes: usize,
    pub bars: usize,
    pub tempo_changes: usize,
    pub time_changes: usize,
    /// Whether the key was worked out from the notes, because the file
    /// has no key signature, or a C major the notes clearly do not fit.
    pub key_from_notes: bool,
    /// How many times the key signature changes after the first one. A
    /// project has one key, so the piano roll only shows the first.
    pub key_changes: usize,
    /// Notes that could not be kept: past the end of the longest song,
    /// beyond [`MAX_NOTES`], or a drum sound with no built-in drum.
    pub notes_left_out: usize,
    /// Whether the file was damaged, so only part of it could be read.
    pub damaged: bool,
}

impl Report {
    /// One line for the status bar, such as "Clementi: 2 channels, 812
    /// notes, 38 bars, 4/4, 120 BPM, C major".
    pub fn summary(&self, title: &str, project: &Project) -> String {
        let mut text = format!(
            "{title}: {} channel{}, {} note{}, {} bar{}, {}",
            self.channels,
            plural(self.channels),
            self.notes,
            plural(self.notes),
            self.bars,
            plural(self.bars),
            project.time_signature,
        );
        if self.time_changes > 0 {
            text += &format!(
                " (changes {} time{})",
                self.time_changes,
                plural(self.time_changes)
            );
        }
        text += &format!(", {:.0} BPM", project.tempo.bpm());
        if self.tempo_changes > 0 {
            text += &format!(
                " ({} tempo change{})",
                self.tempo_changes,
                plural(self.tempo_changes)
            );
        }
        text += &format!(", {}", project.scale.name());
        if self.key_from_notes {
            text += " (worked out from the notes)";
        }
        if self.key_changes > 0 {
            text += &format!(
                ". The key changes {} time{} later; the key shown is the first",
                self.key_changes,
                plural(self.key_changes)
            );
        }
        if self.notes_left_out > 0 {
            text += &format!(
                ". {} note{} left out",
                self.notes_left_out,
                if self.notes_left_out == 1 {
                    " was"
                } else {
                    "s were"
                }
            );
        }
        if self.damaged {
            text += ". The file is damaged: everything before the damage was read";
        }
        text
    }
}

fn plural(count: usize) -> &'static str {
    if count == 1 { "" } else { "s" }
}

/// Why a MIDI file could not become a project.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportError {
    /// The file plays no notes.
    NoNotes,
}

impl std::fmt::Display for ImportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ImportError::NoNotes => write!(f, "it has no notes"),
        }
    }
}

impl std::error::Error for ImportError {}

/// A new project that plays `file`. `title` names its playlist track.
pub fn import(file: &MidiFile, title: &str) -> Result<Imported, ImportError> {
    let clock = Clock(file.timing);
    // A format 2 file holds separate pieces, one per track: the first
    // one is imported.
    let tracks = match file.format {
        2 => &file.tracks[..file.tracks.len().min(1)],
        _ => &file.tracks[..],
    };
    let mut left_out = 0;

    // ----- The notes, sorted into parts by track and channel -----
    let (mut parts, mut drums) = read_parts(tracks, &clock, file.format == 0);
    let total: usize = parts.iter().map(|part| part.notes.len()).sum::<usize>()
        + drums.iter().map(Vec::len).sum::<usize>();
    if total == 0 {
        return Err(ImportError::NoNotes);
    }
    left_out += keep_first_notes(&mut parts, &mut drums, MAX_NOTES);
    let drum_channels: Vec<Drum> = Drum::ALL
        .into_iter()
        .filter(|&drum| !drums[drum_index(drum)].is_empty())
        .collect();
    let room = MAX_CHANNELS - drum_channels.len();
    if parts.len() > room {
        parts = by_midi_channel(parts);
    }
    left_out += drums[UNKNOWN_DRUM].len();

    // ----- Tempo, time signature and key -----
    let mut project = Project::empty();
    let tempo_changes = set_tempos(&mut project, tracks, &clock);
    let time_changes = set_meter(&mut project, tracks, &clock);
    let (key, key_from_notes, key_changes) = choose_key(tracks, &parts);
    project.scale = key;

    // ----- Channels -----
    name_parts(&mut parts);
    for part in &parts {
        let volume = Channel::synth().volume * part.volume / DEFAULT_MIDI_VOLUME;
        project.add_channel(Channel {
            name: part.name.clone(),
            volume: volume.clamp(0.0, 1.0),
            program: part.program,
            ..Channel::synth()
        });
    }
    for &drum in &drum_channels {
        project.add_channel(Channel::drum(drum));
    }

    // ----- Patterns and the playlist -----
    // Every note as (channel, start tick, end tick, pitch, velocity),
    // in song ticks.
    let mut notes: Vec<Placed> = Vec::new();
    for (channel, part) in parts.iter().enumerate() {
        notes.extend(part.notes.iter().map(|note| Placed {
            channel,
            pitch: note.pitch,
            start: note.start,
            end: note.end,
            velocity: note.velocity,
        }));
    }
    for (offset, drum) in drum_channels.iter().enumerate() {
        let channel = parts.len() + offset;
        notes.extend(drums[drum_index(*drum)].iter().map(|hit| {
            // A drum sound plays to its end whatever the note's length,
            // so each hit is one step long, a step of the rack.
            let step_end = (hit.start / TICKS_PER_STEP + 1) * TICKS_PER_STEP;
            Placed {
                channel,
                pitch: STEP_NOTE,
                start: hit.start,
                end: step_end,
                velocity: hit.velocity,
            }
        }));
    }
    let last_tick = MAX_SONG_STEPS * TICKS_PER_STEP;
    let before = notes.len();
    notes.retain(|note| note.start < last_tick);
    left_out += before - notes.len();
    for note in &mut notes {
        note.end = note.end.min(last_tick);
    }

    let meter = project.meter();
    let end_step = notes
        .iter()
        .map(|note| note.end.div_ceil(TICKS_PER_STEP))
        .max()
        .unwrap_or(0);
    let length = meter.round_up_to_bar(end_step).min(MAX_SONG_STEPS);
    let chunks = chunks(&meter, length, &notes);
    left_out += place_patterns(&mut project, &meter, &chunks, &notes);
    if let Some(track) = project.track_mut(0) {
        track.name = title.to_string();
    }
    project.remove_unused_tracks();

    let report = Report {
        channels: project.channels().len(),
        notes: total - left_out,
        bars: meter.bar_at(length.saturating_sub(1)).number + 1,
        tempo_changes,
        time_changes,
        key_from_notes,
        key_changes,
        notes_left_out: left_out,
        damaged: file.damaged,
    };
    Ok(Imported { project, report })
}

/// Turns the file's ticks into ours, [`TICKS_PER_STEP`] to the step.
struct Clock(Timing);

impl Clock {
    /// Our tick for the file's `tick`, to the nearest one.
    fn tick(&self, tick: u64) -> usize {
        let ours = match self.0 {
            Timing::PerBeat(per_beat) => {
                let per_beat = u128::from(per_beat);
                (u128::from(tick) * u128::from(TICKS_PER_BEAT) + per_beat / 2) / per_beat
            }
            // Time in seconds, counted in beats of 120 BPM: two a second.
            Timing::PerSecond(per_second) => {
                let beats = tick as f64 / per_second * f64::from(DEFAULT_BPM) / 60.0;
                (beats * TICKS_PER_BEAT as f64).round() as u128
            }
        };
        // Far past the end of any song: kept from overflowing.
        ours.min(u128::from(u32::MAX)) as usize
    }

    /// Whether the file's tempo events count. With ticks per second they
    /// change nothing: the times are already in seconds.
    fn follows_tempo(&self) -> bool {
        matches!(self.0, Timing::PerBeat(_))
    }
}

/// A note as the file plays it, in our ticks.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Played {
    pitch: Note,
    start: usize,
    end: usize,
    velocity: u8,
}

/// The notes one MIDI channel of one track plays, and what the file says
/// about them.
#[derive(Debug, Clone, PartialEq)]
struct Part {
    track: usize,
    midi_channel: u8,
    /// The track's name, if it has one.
    track_name: Option<String>,
    /// Whether the track plays on other MIDI channels too.
    shares_track: bool,
    /// The name of the instrument, from the file's instrument name or
    /// its General MIDI program.
    instrument: Option<String>,
    /// The General MIDI program the file chooses for these notes (its
    /// first program change), from 0 (piano) to 127. It names the part
    /// and becomes its channel's [`Channel::program`].
    program: Option<u8>,
    /// The channel's volume, 0 to 127, as the file sets it first.
    volume: f32,
    name: String,
    notes: Vec<Played>,
}

/// Where in `drums` the hits of a sound with no built-in drum go.
const UNKNOWN_DRUM: usize = Drum::ALL.len();

fn drum_index(drum: Drum) -> usize {
    Drum::ALL
        .iter()
        .position(|&other| other == drum)
        .unwrap_or(UNKNOWN_DRUM)
}

/// A key that is down, or was let go while the sustain pedal holds it.
#[derive(Debug, Clone, Copy)]
struct Open {
    start: usize,
    velocity: u8,
    /// Let go, but held on by the pedal.
    held_by_pedal: bool,
}

/// What a MIDI channel is doing at one moment while its track is read.
struct ChannelState {
    /// For each key, the notes sounding on it, oldest first. There is
    /// usually one at most, but a file can strike a key again before
    /// letting it go; each note off then ends the oldest note.
    open: Vec<Vec<Open>>,
    pedal: bool,
    notes: Vec<Played>,
}

impl ChannelState {
    fn new() -> Self {
        ChannelState {
            open: vec![Vec::new(); 128],
            pedal: false,
            notes: Vec::new(),
        }
    }

    /// Ends `note`, played on `key`, at `tick`. A note let go on the tick
    /// it started on (a grace note, or a drum hit) still lasts a tick, so
    /// it is heard.
    fn end(&mut self, key: u8, note: Open, tick: usize) {
        self.notes.push(Played {
            pitch: Note::new(key),
            start: note.start,
            end: tick.max(note.start + 1),
            velocity: note.velocity.max(1),
        });
    }

    fn key_down(&mut self, key: u8, velocity: u8, tick: usize) {
        let open = &mut self.open[usize::from(key)];
        // Struck twice on one tick, it is one note.
        if open.iter().any(|note| note.start == tick) {
            return;
        }
        // A string the pedal holds is struck again: the old sound stops.
        let (held, sounding): (Vec<Open>, Vec<Open>) =
            open.drain(..).partition(|note| note.held_by_pedal);
        *open = sounding;
        open.push(Open {
            start: tick,
            velocity,
            held_by_pedal: false,
        });
        for note in held {
            self.end(key, note, tick);
        }
    }

    fn key_up(&mut self, key: u8, tick: usize) {
        let pedal = self.pedal;
        let open = &mut self.open[usize::from(key)];
        let Some(index) = open.iter().position(|note| !note.held_by_pedal) else {
            return;
        };
        if pedal {
            open[index].held_by_pedal = true;
        } else {
            let note = open.remove(index);
            self.end(key, note, tick);
        }
    }

    fn pedal(&mut self, down: bool, tick: usize) {
        self.pedal = down;
        if down {
            return;
        }
        for key in 0..128u8 {
            let open = &mut self.open[usize::from(key)];
            let (held, sounding): (Vec<Open>, Vec<Open>) =
                open.drain(..).partition(|note| note.held_by_pedal);
            *open = sounding;
            for note in held {
                self.end(key, note, tick);
            }
        }
    }

    /// Lets go of every key at the end of the track, `tick` (or a step
    /// after a note started, if it started on the very last tick).
    fn finish(mut self, tick: usize) -> Vec<Played> {
        for key in 0..128u8 {
            for note in std::mem::take(&mut self.open[usize::from(key)]) {
                self.end(key, note, tick.max(note.start + TICKS_PER_STEP));
            }
        }
        self.notes
    }
}

/// Reads the notes of every track: the melodic parts, and the drum hits
/// sorted by built-in drum (with one more list for sounds that have
/// none).
fn read_parts(
    tracks: &[Track],
    clock: &Clock,
    single_track: bool,
) -> (Vec<Part>, Vec<Vec<Played>>) {
    let mut parts: Vec<Part> = Vec::new();
    let mut drums: Vec<Vec<Played>> = vec![Vec::new(); UNKNOWN_DRUM + 1];
    // Programs and volumes are sometimes set on another track than the
    // notes: the first one set on each channel anywhere is the fallback.
    let mut programs: [Option<u8>; 16] = [None; 16];
    let mut volumes: [Option<u8>; 16] = [None; 16];
    for track in tracks {
        for event in &track.events {
            match event.kind {
                EventKind::Program { channel, program } => {
                    programs[usize::from(channel)].get_or_insert(program);
                }
                EventKind::Controller {
                    channel,
                    number: VOLUME,
                    value,
                } => {
                    volumes[usize::from(channel)].get_or_insert(value);
                }
                _ => {}
            }
        }
    }

    for (index, track) in tracks.iter().enumerate() {
        let mut track_name = None;
        let mut instrument_name = None;
        let mut track_programs: [Option<u8>; 16] = [None; 16];
        let mut track_volumes: [Option<u8>; 16] = [None; 16];
        let mut channels: Vec<ChannelState> = (0..16).map(|_| ChannelState::new()).collect();
        let mut last_tick = 0;
        for event in &track.events {
            let tick = clock.tick(event.tick);
            last_tick = tick;
            match &event.kind {
                EventKind::NoteOn {
                    channel,
                    key,
                    velocity,
                } => channels[usize::from(*channel)].key_down(*key, *velocity, tick),
                EventKind::NoteOff { channel, key } => {
                    channels[usize::from(*channel)].key_up(*key, tick);
                }
                EventKind::Controller {
                    channel,
                    number,
                    value,
                } => match *number {
                    SUSTAIN => channels[usize::from(*channel)].pedal(*value >= 64, tick),
                    VOLUME => {
                        track_volumes[usize::from(*channel)].get_or_insert(*value);
                    }
                    _ => {}
                },
                EventKind::Program { channel, program } => {
                    track_programs[usize::from(*channel)].get_or_insert(*program);
                }
                EventKind::TrackName(name) if track_name.is_none() && !name.is_empty() => {
                    track_name = Some(name.clone());
                }
                EventKind::InstrumentName(name)
                    if instrument_name.is_none() && !name.is_empty() =>
                {
                    instrument_name = Some(name.clone());
                }
                _ => {}
            }
        }
        // Keys still down when the track ends are let go there.
        let notes: Vec<Vec<Played>> = channels
            .into_iter()
            .map(|state| state.finish(last_tick))
            .collect();

        let playing: Vec<usize> = (0..16)
            .filter(|&channel| channel != usize::from(DRUM_CHANNEL) && !notes[channel].is_empty())
            .collect();
        for (channel, mut played) in notes.into_iter().enumerate() {
            if played.is_empty() {
                continue;
            }
            // Two notes of one pitch starting together (from two note ons
            // on one tick, written twice by mistake) are one: the longer.
            played.sort_by_key(|note| (note.start, note.pitch, std::cmp::Reverse(note.end)));
            played.dedup_by_key(|note| (note.start, note.pitch));
            if channel == usize::from(DRUM_CHANNEL) {
                for hit in played {
                    let slot =
                        general_midi_drum_sound(hit.pitch.midi()).map_or(UNKNOWN_DRUM, drum_index);
                    drums[slot].push(hit);
                }
                continue;
            }
            let program = track_programs[channel].or(programs[channel]);
            let instrument = match (&instrument_name, program) {
                (Some(name), _) if playing.len() == 1 => Some(name.clone()),
                (_, Some(program)) => {
                    Some(GENERAL_MIDI_INSTRUMENTS[usize::from(program & 0x7F)].to_string())
                }
                _ => None,
            };
            let volume = track_volumes[channel].or(volumes[channel]);
            parts.push(Part {
                track: index,
                midi_channel: channel as u8,
                // In a file of one track, the track's name is the song's.
                track_name: if single_track {
                    None
                } else {
                    track_name.clone()
                },
                shares_track: playing.len() > 1,
                instrument,
                program: program.map(|program| program & 0x7F),
                volume: volume.map_or(DEFAULT_MIDI_VOLUME, f32::from),
                name: String::new(),
                notes: played,
            });
        }
    }
    for hits in &mut drums {
        hits.sort_by_key(|hit| hit.start);
    }
    (parts, drums)
}

/// Keeps the `limit` earliest notes of all, and returns how many were
/// left out.
fn keep_first_notes(parts: &mut [Part], drums: &mut [Vec<Played>], limit: usize) -> usize {
    let mut starts: Vec<usize> = parts
        .iter()
        .flat_map(|part| part.notes.iter())
        .chain(drums.iter().flatten())
        .map(|note| note.start)
        .collect();
    if starts.len() <= limit {
        return 0;
    }
    starts.sort_unstable();
    // Notes starting before `cut` stay, and as many on `cut` as fit.
    let cut = starts[limit - 1];
    let mut room_at_cut = limit - starts.partition_point(|&start| start < cut);
    let mut left_out = 0;
    let lists = parts
        .iter_mut()
        .map(|part| &mut part.notes)
        .chain(drums.iter_mut());
    for notes in lists {
        let before = notes.len();
        notes.retain(|note| {
            if note.start < cut {
                true
            } else if note.start == cut && room_at_cut > 0 {
                room_at_cut -= 1;
                true
            } else {
                false
            }
        });
        left_out += before - notes.len();
    }
    left_out
}

/// The parts joined by MIDI channel, for a file with more tracks than a
/// rack has room for.
fn by_midi_channel(parts: Vec<Part>) -> Vec<Part> {
    let mut joined: Vec<Part> = Vec::new();
    for part in parts {
        match joined
            .iter_mut()
            .find(|other| other.midi_channel == part.midi_channel)
        {
            Some(other) => {
                other.notes.extend(part.notes);
                other.notes.sort_by_key(|note| (note.start, note.pitch));
            }
            None => joined.push(part),
        }
    }
    joined
}

/// Gives each part its name: the track's name, unless the track plays
/// on several MIDI channels; then the instrument's name; then the MIDI
/// channel. Two parts with the same name are numbered.
fn name_parts(parts: &mut [Part]) {
    for part in parts.iter_mut() {
        let channel = format!("Channel {}", part.midi_channel + 1);
        part.name = match (&part.track_name, &part.instrument) {
            (Some(track), _) if !part.shares_track => track.clone(),
            (Some(track), Some(instrument)) => format!("{track}: {instrument}"),
            (Some(track), None) => format!("{track}: {channel}"),
            (None, Some(instrument)) => instrument.clone(),
            (None, None) => channel,
        };
    }
    let names: Vec<String> = parts.iter().map(|part| part.name.clone()).collect();
    for (index, part) in parts.iter_mut().enumerate() {
        let same = names[..index]
            .iter()
            .filter(|&other| *other == part.name)
            .count();
        if same > 0 {
            part.name = format!("{} {}", part.name, same + 1);
        }
    }
}

/// Sets the project's tempo and adds a tempo marker for every change.
/// Returns how many changes there are.
fn set_tempos(project: &mut Project, tracks: &[Track], clock: &Clock) -> usize {
    project.tempo = Tempo::new(DEFAULT_BPM);
    if !clock.follows_tempo() {
        return 0;
    }
    // By step, the last tempo set on it: a file can set several on one
    // step, and only the last one is heard.
    let mut tempos: BTreeMap<usize, f32> = BTreeMap::new();
    for (tick, micros) in song_events(tracks, |kind| match kind {
        EventKind::Tempo(micros) => Some(*micros),
        _ => None,
    }) {
        let step = (clock.tick(tick) + TICKS_PER_STEP / 2) / TICKS_PER_STEP;
        tempos.insert(step, (60_000_000.0 / f64::from(micros)) as f32);
    }
    let mut changes = 0;
    let mut now = Tempo::new(DEFAULT_BPM);
    for (step, bpm) in tempos {
        let tempo = Tempo::new(bpm);
        if step > 0 && tempo == now {
            continue;
        }
        if project.set_tempo_marker(TempoMarker::new(step, bpm)) && step > 0 {
            changes += 1;
        }
        now = tempo;
    }
    changes
}

/// Sets the project's time signature and adds a time marker for every
/// change. Returns how many changes there are.
fn set_meter(project: &mut Project, tracks: &[Track], clock: &Clock) -> usize {
    let mut times: BTreeMap<usize, TimeSignature> = BTreeMap::new();
    for (tick, time) in song_events(tracks, |kind| match kind {
        EventKind::TimeSignature { beats, unit } => time_signature(*beats, *unit),
        _ => None,
    }) {
        let step = (clock.tick(tick) + TICKS_PER_STEP / 2) / TICKS_PER_STEP;
        times.insert(step, time);
    }
    let first = times.get(&0).copied().unwrap_or(TimeSignature::COMMON);
    let markers = times
        .into_iter()
        .filter(|&(step, _)| step > 0)
        .map(|(step, time)| TimeMarker::new(step, time));
    // `tidy` leaves out the markers that change nothing: files often
    // repeat the time signature at the start of every section.
    let meter = Meter::new(first, markers).tidy();
    project.time_signature = meter.first();
    for &marker in meter.markers() {
        project.set_time_marker(marker);
    }
    project.playlist().time_markers().len()
}

/// A MIDI time signature as one Ferrous Loom can count: bars must be a
/// whole number of sixteenth-note steps. 6/32 is written 3/16; 3/32 has
/// no such form and is left out.
fn time_signature(beats: u8, unit: u8) -> Option<TimeSignature> {
    let (mut beats, mut unit) = (beats, unit);
    while unit > 16 && beats.is_multiple_of(2) {
        beats /= 2;
        unit /= 2;
    }
    let time = TimeSignature::new(beats, unit);
    (beats > 0 && unit <= 16 && time.steps_per_bar() <= MAX_STEPS).then_some(time)
}

/// The events of one kind from every track, as (tick, value), in time
/// order. Tempo and time signature apply to the whole song, whichever
/// track holds them.
fn song_events<T>(tracks: &[Track], pick: impl Fn(&EventKind) -> Option<T>) -> Vec<(u64, T)> {
    let mut events: Vec<(u64, T)> = tracks
        .iter()
        .flat_map(|track| &track.events)
        .filter_map(|event| pick(&event.kind).map(|value| (event.tick, value)))
        .collect();
    // A stable sort keeps events on the same tick in the order they were
    // read, so the last one written wins.
    events.sort_by_key(|&(tick, _)| tick);
    events
}

/// The key of the song, whether it was worked out from the notes, and
/// how many times the key signature changes after the first.
fn choose_key(tracks: &[Track], parts: &[Part]) -> (Scale, bool, usize) {
    let signatures = song_events(tracks, |kind| match kind {
        EventKind::KeySignature { sharps, minor } => Some((*sharps, *minor)),
        _ => None,
    });
    let mut changes = 0;
    for pair in signatures.windows(2) {
        if pair[1].1 != pair[0].1 {
            changes += 1;
        }
    }
    // A file Ferrous Loom wrote holds the exact key, mode and all.
    let exact = song_events(tracks, |kind| match kind {
        EventKind::ExactKey(scale) => Some(*scale),
        _ => None,
    });
    if let Some(&(_, scale)) = exact.first() {
        return (scale, false, changes);
    }
    let mut durations = [0.0; 12];
    for note in parts.iter().flat_map(|part| &part.notes) {
        durations[usize::from(note.pitch.pitch_class().semitone())] +=
            (note.end - note.start) as f64;
    }
    let fits = key_fits(&durations);
    let best = fits.first().copied().filter(|&(_, fit)| fit > 0.0);
    // Otherwise the key signature says it, as it is written: whoever
    // wrote the file knew the key better than a guess. With one
    // exception: many programs write "C major" into every file when
    // nobody chose a key, so when another key fits the notes clearly
    // better than C major, it is that one.
    if let Some(&(_, (sharps, minor))) = signatures.first() {
        let major = major_tonic(sharps);
        let scale = if minor {
            Scale::new(major.transpose(9), ScaleKind::NaturalMinor)
        } else {
            Scale::new(major, ScaleKind::Major)
        };
        let fit = |scale: Scale| {
            fits.iter()
                .find(|(other, _)| *other == scale)
                .map_or(0.0, |(_, fit)| *fit)
        };
        if (sharps, minor) == (0, false)
            && let Some((best, best_fit)) = best
            && best_fit - fit(scale) > CLEARLY_BETTER
        {
            return (best, true, changes);
        }
        return (scale, false, changes);
    }
    // A file without a key signature: the key the notes fit best.
    match best {
        Some((best, _)) => (best, true, changes),
        None => (Scale::default(), false, changes),
    }
}

/// How much better another key must fit the notes than a file's C major
/// for the C major to be taken as a default nobody chose.
const CLEARLY_BETTER: f64 = 0.1;

/// The tonic of the major key with `sharps` sharps (flats if negative):
/// each sharp moves it a fifth up (seven semitones), each flat a fifth
/// down. One sharp is G major, two flats Bb major.
fn major_tonic(sharps: i8) -> PitchClass {
    PitchClass::C.transpose(i32::from(sharps) * 7)
}

/// A note of the song, in song ticks, on a channel of the project.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Placed {
    channel: usize,
    pitch: Note,
    start: usize,
    end: usize,
    velocity: u8,
}

/// Where the song is cut into patterns: (first step, end step) of each
/// piece, at bar lines, each at most [`BARS_PER_PATTERN`] bars and
/// [`MAX_STEPS`] steps long.
///
/// A note that is still sounding where a pattern ends has to be cut in
/// two and struck again in the next one. So each piece ends, if it can,
/// at a bar line no note crosses: of the last half of the bars it could
/// end on, the one fewest notes cross, the latest of those.
fn chunks(meter: &Meter, length: usize, notes: &[Placed]) -> Vec<(usize, usize)> {
    // How many notes cross each step's start: a difference array, +1
    // where a note starts crossing and -1 after it stops.
    let mut change = vec![0i64; length + 2];
    for note in notes {
        let first = note.start / TICKS_PER_STEP + 1;
        let last = note.end.div_ceil(TICKS_PER_STEP).saturating_sub(1);
        if first <= last && first <= length {
            change[first] += 1;
            change[(last + 1).min(length + 1)] -= 1;
        }
    }
    let mut crossing = Vec::with_capacity(length + 1);
    let mut count = 0;
    for &delta in change.iter().take(length + 1) {
        count += delta;
        crossing.push(count);
    }

    let bar_starts: Vec<usize> = meter
        .bars()
        .map(|bar| bar.start)
        .take_while(|&start| start < length)
        .chain(std::iter::once(length))
        .collect();
    let mut chunks = Vec::new();
    let mut first = 0;
    while first + 1 < bar_starts.len() {
        let start = bar_starts[first];
        let mut last = first + 1;
        while last + 1 < bar_starts.len()
            && last - first < BARS_PER_PATTERN
            && bar_starts[last + 1] - start <= MAX_STEPS
        {
            last += 1;
        }
        if last + 1 < bar_starts.len() {
            let earliest = first + (last - first).div_ceil(2);
            last = (earliest..=last)
                .rev()
                .min_by_key(|&bar| crossing[bar_starts[bar]])
                .unwrap_or(last);
        }
        chunks.push((start, bar_starts[last]));
        first = last;
    }
    chunks
}

/// Fills the project's patterns with the notes of each chunk, and places
/// them on the playlist. Returns how many notes were left out because
/// the project ran out of patterns.
fn place_patterns(
    project: &mut Project,
    meter: &Meter,
    chunks: &[(usize, usize)],
    notes: &[Placed],
) -> usize {
    let channels = project.channels().len();
    // The notes of each chunk, by channel. A note that runs past the end
    // of its chunk goes on in the next one.
    let mut rows: Vec<Vec<Vec<NoteEvent>>> = vec![vec![Vec::new(); channels]; chunks.len()];
    let chunk_of = |tick: usize| chunks.partition_point(|&(_, end)| end * TICKS_PER_STEP <= tick);
    for note in notes {
        let mut start = note.start;
        let mut index = chunk_of(start);
        while index < chunks.len() && start < note.end {
            let (first, end) = chunks[index];
            let until = note.end.min(end * TICKS_PER_STEP);
            rows[index][note.channel].push(NoteEvent {
                velocity: note.velocity,
                ..NoteEvent::exact(note.pitch, start - first * TICKS_PER_STEP, until - start)
            });
            start = until;
            index += 1;
        }
    }

    let mut made: Vec<usize> = Vec::new();
    let mut left_out = 0;
    for (index, &(first, end)) in chunks.iter().enumerate() {
        let notes = &rows[index];
        if notes.iter().all(Vec::is_empty) {
            continue;
        }
        let length = end - first;
        // A stretch that comes back note for note plays the same pattern.
        let same = made.iter().copied().find(|&pattern| {
            project.pattern(pattern).is_some_and(|pattern| {
                pattern.length() == length
                    && (0..channels)
                        .all(|channel| pattern.notes(channel) == notes[channel].as_slice())
            })
        });
        let pattern = match same {
            Some(pattern) => pattern,
            None => {
                let new = if made.is_empty() {
                    Some(0)
                } else if project.patterns().len() < MAX_PATTERNS {
                    project.add_pattern()
                } else {
                    None
                };
                let Some(new) = new else {
                    left_out += notes.iter().map(Vec::len).sum::<usize>();
                    continue;
                };
                if let Some(pattern) = project.pattern_mut(new) {
                    pattern.name = bars_name(meter, first, end);
                    pattern.set_length(length);
                    for (channel, notes) in notes.iter().enumerate() {
                        pattern.set_notes(channel, notes.iter().copied());
                    }
                }
                made.push(new);
                new
            }
        };
        project.add_clip(Clip::new(pattern, 0, first));
    }
    left_out
}

/// "Bars 1–16", or "Bar 5" for one bar.
fn bars_name(meter: &Meter, first: usize, end: usize) -> String {
    let from = meter.bar_at(first).number + 1;
    let to = meter.bar_at(end.saturating_sub(1)).number + 1;
    if from == to {
        format!("Bar {from}")
    } else {
        format!("Bars {from}–{to}")
    }
}

/// The built-in drum closest to a General MIDI drum sound (notes 35 to
/// 81 on channel 10, and a few more from General MIDI 2), or `None` for
/// sounds no built-in drum is anything like, such as a whistle.
pub fn general_midi_drum_sound(key: u8) -> Option<Drum> {
    use Drum::*;
    Some(match key {
        35 | 36 | 86 => Kick,               // bass drums, mute surdo
        37 | 31 | 75 | 76 | 77 | 85 => Rim, // side stick, sticks, claves, wood blocks, castanets
        38 | 40 => Snare,                   // acoustic and electric snare
        39 => Clap,
        42 | 44 => HiHat, // closed and pedal hi-hat
        46 => OpenHat,
        41 | 43 | 45 | 47 | 66 | 87 => LowTom, // floor and low toms, low timbale, open surdo
        48 | 50 | 65 => HighTom,               // high toms, high timbale
        49 | 52 | 55 | 57 => Crash,            // crash, Chinese and splash cymbals
        51 | 53 | 59 | 80 | 81 => Ride,        // rides, ride bell, triangles
        56 | 67 | 68 => Cowbell,               // cowbell, agogos
        54 | 58 | 69 | 70 | 73 | 74 | 82 | 83 => Shaker, // tambourine, vibraslap, cabasa, maracas, guiros, shaker, jingle bell
        60..=64 => Conga,                                // bongos and congas
        _ => return None,
    })
}

/// The 128 instruments of General MIDI, by program number. A file that
/// asks for program 40 on a channel wants a violin there.
pub const GENERAL_MIDI_INSTRUMENTS: [&str; 128] = [
    // Pianos
    "Acoustic Grand Piano",
    "Bright Acoustic Piano",
    "Electric Grand Piano",
    "Honky-tonk Piano",
    "Electric Piano 1",
    "Electric Piano 2",
    "Harpsichord",
    "Clavinet",
    // Chromatic percussion
    "Celesta",
    "Glockenspiel",
    "Music Box",
    "Vibraphone",
    "Marimba",
    "Xylophone",
    "Tubular Bells",
    "Dulcimer",
    // Organs
    "Drawbar Organ",
    "Percussive Organ",
    "Rock Organ",
    "Church Organ",
    "Reed Organ",
    "Accordion",
    "Harmonica",
    "Tango Accordion",
    // Guitars
    "Acoustic Guitar (nylon)",
    "Acoustic Guitar (steel)",
    "Electric Guitar (jazz)",
    "Electric Guitar (clean)",
    "Electric Guitar (muted)",
    "Overdriven Guitar",
    "Distortion Guitar",
    "Guitar Harmonics",
    // Basses
    "Acoustic Bass",
    "Electric Bass (finger)",
    "Electric Bass (pick)",
    "Fretless Bass",
    "Slap Bass 1",
    "Slap Bass 2",
    "Synth Bass 1",
    "Synth Bass 2",
    // Strings
    "Violin",
    "Viola",
    "Cello",
    "Contrabass",
    "Tremolo Strings",
    "Pizzicato Strings",
    "Orchestral Harp",
    "Timpani",
    // Ensembles
    "String Ensemble 1",
    "String Ensemble 2",
    "Synth Strings 1",
    "Synth Strings 2",
    "Choir Aahs",
    "Voice Oohs",
    "Synth Voice",
    "Orchestra Hit",
    // Brass
    "Trumpet",
    "Trombone",
    "Tuba",
    "Muted Trumpet",
    "French Horn",
    "Brass Section",
    "Synth Brass 1",
    "Synth Brass 2",
    // Reeds
    "Soprano Sax",
    "Alto Sax",
    "Tenor Sax",
    "Baritone Sax",
    "Oboe",
    "English Horn",
    "Bassoon",
    "Clarinet",
    // Pipes
    "Piccolo",
    "Flute",
    "Recorder",
    "Pan Flute",
    "Blown Bottle",
    "Shakuhachi",
    "Whistle",
    "Ocarina",
    // Synth leads
    "Lead 1 (square)",
    "Lead 2 (sawtooth)",
    "Lead 3 (calliope)",
    "Lead 4 (chiff)",
    "Lead 5 (charang)",
    "Lead 6 (voice)",
    "Lead 7 (fifths)",
    "Lead 8 (bass + lead)",
    // Synth pads
    "Pad 1 (new age)",
    "Pad 2 (warm)",
    "Pad 3 (polysynth)",
    "Pad 4 (choir)",
    "Pad 5 (bowed)",
    "Pad 6 (metallic)",
    "Pad 7 (halo)",
    "Pad 8 (sweep)",
    // Synth effects
    "FX 1 (rain)",
    "FX 2 (soundtrack)",
    "FX 3 (crystal)",
    "FX 4 (atmosphere)",
    "FX 5 (brightness)",
    "FX 6 (goblins)",
    "FX 7 (echoes)",
    "FX 8 (sci-fi)",
    // Ethnic
    "Sitar",
    "Banjo",
    "Shamisen",
    "Koto",
    "Kalimba",
    "Bagpipe",
    "Fiddle",
    "Shanai",
    // Percussive
    "Tinkle Bell",
    "Agogo",
    "Steel Drums",
    "Woodblock",
    "Taiko Drum",
    "Melodic Tom",
    "Synth Drum",
    "Reverse Cymbal",
    // Sound effects
    "Guitar Fret Noise",
    "Breath Noise",
    "Seashore",
    "Bird Tweet",
    "Telephone Ring",
    "Helicopter",
    "Applause",
    "Gunshot",
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::midi_file::read::Event;

    /// A file of `tracks`, each a list of (tick, event), at 480 ticks to
    /// the beat, as many programs write.
    fn file(format: u16, tracks: Vec<Vec<(u64, EventKind)>>) -> MidiFile {
        MidiFile {
            format,
            timing: Timing::PerBeat(480),
            tracks: tracks
                .into_iter()
                .map(|events| Track {
                    events: events
                        .into_iter()
                        .map(|(tick, kind)| Event { tick, kind })
                        .collect(),
                })
                .collect(),
            damaged: false,
        }
    }

    fn on(channel: u8, key: u8, velocity: u8) -> EventKind {
        EventKind::NoteOn {
            channel,
            key,
            velocity,
        }
    }

    fn off(channel: u8, key: u8) -> EventKind {
        EventKind::NoteOff { channel, key }
    }

    /// A note from `start` for `ticks`, file ticks.
    fn note(channel: u8, key: u8, start: u64, ticks: u64) -> [(u64, EventKind); 2] {
        [
            (start, on(channel, key, 90)),
            (start + ticks, off(channel, key)),
        ]
    }

    /// Every note of channel `channel` in the song, as (start tick, end
    /// tick, MIDI note, velocity), in our ticks.
    fn song_notes(project: &Project, channel: usize) -> Vec<(usize, usize, u8, u8)> {
        let mut notes = Vec::new();
        for clip in project.playlist().clips() {
            let pattern = project.pattern(clip.pattern).unwrap();
            for note in pattern.notes(channel) {
                let offset = clip.start * TICKS_PER_STEP;
                notes.push((
                    offset + note.start_tick(),
                    offset + note.end_tick(),
                    note.pitch.midi(),
                    note.velocity,
                ));
            }
        }
        notes.sort_unstable();
        notes
    }

    fn import(file: &MidiFile) -> Imported {
        super::import(file, "Song").unwrap()
    }

    #[test]
    fn triplets_keep_their_exact_time_and_velocity() {
        // Eighth-note triplets at 480 ticks a beat: every 160 ticks,
        // which is 32 of ours.
        let mut events = vec![(0, EventKind::TrackName("Flute".to_string()))];
        for (n, velocity) in [70u8, 90, 110].into_iter().enumerate() {
            let start = n as u64 * 160;
            events.push((start, on(0, 72, velocity)));
            events.push((start + 150, off(0, 72)));
        }
        let imported = import(&file(1, vec![events]));
        let project = &imported.project;
        assert_eq!(project.channels()[0].name, "Flute");
        assert_eq!(
            song_notes(project, 0),
            [(0, 30, 72, 70), (32, 62, 72, 90), (64, 94, 72, 110)]
        );
        // The second starts 8 ticks into step 1.
        let second = project.patterns()[0].notes(0)[1];
        assert_eq!((second.start, second.nudge), (1, 8));
        assert_eq!(imported.report.notes, 3);
    }

    #[test]
    fn tempo_time_and_key() {
        let tempo_track = vec![
            (0, EventKind::Tempo(500_000)),                      // 120 BPM
            (0, EventKind::TimeSignature { beats: 3, unit: 4 }), // 3/4
            (
                0,
                EventKind::KeySignature {
                    sharps: -3,
                    minor: true,
                },
            ), // C minor
            (480 * 6, EventKind::Tempo(750_000)),                // 80 BPM, bar 3
            (480 * 6, EventKind::TimeSignature { beats: 3, unit: 4 }), // the same again
            (480 * 9, EventKind::TimeSignature { beats: 6, unit: 8 }),
        ];
        let notes: Vec<(u64, EventKind)> = (0..12)
            .flat_map(|beat| note(0, 60, beat * 480, 480))
            .collect();
        let imported = import(&file(1, vec![tempo_track, notes]));
        let project = &imported.project;
        assert_eq!(project.tempo.bpm(), 120.0);
        assert_eq!(
            project.playlist().tempo_markers(),
            [TempoMarker::new(24, 80.0)]
        );
        assert_eq!(project.time_signature, TimeSignature::new(3, 4));
        assert_eq!(
            project.playlist().time_markers(),
            [TimeMarker::new(36, TimeSignature::new(6, 8))],
            "the repeated 3/4 changes nothing"
        );
        assert_eq!(project.scale.name(), "C minor");
        assert!(!imported.report.key_from_notes);
        assert_eq!(
            (imported.report.tempo_changes, imported.report.time_changes),
            (1, 1)
        );
    }

    /// The major scale on `tonic` and its tonic chord.
    fn major_scale_and_chord(tonic: u8) -> Vec<(u64, EventKind)> {
        let mut events = Vec::new();
        for (n, step) in [0, 2, 4, 5, 7, 9, 11, 12].into_iter().enumerate() {
            events.extend(note(0, tonic + step, n as u64 * 480, 480));
        }
        for step in [0, 4, 7] {
            events.extend(note(0, tonic + step, 8 * 480, 4 * 480));
        }
        events
    }

    fn in_d_major() -> Vec<(u64, EventKind)> {
        major_scale_and_chord(62)
    }

    #[test]
    fn without_a_key_signature_the_notes_choose_the_key() {
        let imported = import(&file(0, vec![in_d_major()]));
        assert_eq!(imported.project.scale.name(), "D major");
        assert!(imported.report.key_from_notes);
    }

    #[test]
    fn a_default_c_major_gives_way_to_the_notes() {
        let mut events = vec![(
            0,
            EventKind::KeySignature {
                sharps: 0,
                minor: false,
            },
        )];
        events.extend(in_d_major());
        let imported = import(&file(0, vec![events]));
        assert_eq!(imported.project.scale.name(), "D major");
        assert!(imported.report.key_from_notes);
        // A C major the notes fit stays.
        let mut events = vec![(
            0,
            EventKind::KeySignature {
                sharps: 0,
                minor: false,
            },
        )];
        events.extend(major_scale_and_chord(60));
        let imported = import(&file(0, vec![events]));
        assert_eq!(imported.project.scale.name(), "C major");
        assert!(!imported.report.key_from_notes);
    }

    #[test]
    fn any_other_key_signature_is_taken_as_written() {
        // Whoever wrote the file chose the key; the notes do not overrule it.
        for (sharps, minor, name) in [
            (0, true, "A minor"),
            (3, false, "A major"),
            (-2, false, "Bb major"),
        ] {
            let mut events = vec![(0, EventKind::KeySignature { sharps, minor })];
            events.extend(in_d_major());
            let imported = import(&file(0, vec![events]));
            assert_eq!(imported.project.scale.name(), name);
            assert!(!imported.report.key_from_notes);
        }
    }

    #[test]
    fn a_file_from_ferrous_loom_keeps_its_exact_key() {
        // E Dorian is written with the signature of D major, and exactly
        // in an event of our own.
        let dorian = Scale::new(PitchClass::E, ScaleKind::Dorian);
        let mut events = vec![
            (
                0,
                EventKind::KeySignature {
                    sharps: 2,
                    minor: false,
                },
            ),
            (0, EventKind::ExactKey(dorian)),
        ];
        events.extend(in_d_major());
        let imported = import(&file(0, vec![events]));
        assert_eq!(imported.project.scale, dorian);
    }

    #[test]
    fn drums_become_a_channel_per_sound() {
        let mut events = vec![(
            0,
            EventKind::Program {
                channel: 0,
                program: 33,
            },
        )];
        events.extend(note(0, 40, 0, 1920)); // a bass note
        for beat in 0..4 {
            events.extend(note(9, 36, beat * 480, 10)); // kick
            events.extend(note(9, 42, beat * 480 + 240, 10)); // closed hat
        }
        events.extend(note(9, 44, 0, 10)); // pedal hat: a hat too
        events.extend(note(9, 71, 0, 10)); // a whistle: no such drum
        let imported = import(&file(0, vec![events]));
        let project = &imported.project;
        let names: Vec<&str> = project.channels().iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["Electric Bass (finger)", "Kick", "Hat"]);
        let pattern = &project.patterns()[0];
        assert!(pattern.is_step_row(1) && pattern.is_step_row(2));
        let kicks: Vec<usize> = pattern.notes(1).iter().map(|n| n.start).collect();
        assert_eq!(kicks, [0, 4, 8, 12]);
        let hats: Vec<usize> = pattern.notes(2).iter().map(|n| n.start).collect();
        assert_eq!(hats, [0, 2, 6, 10, 14], "the pedal hat is a hat too");
        assert_eq!(imported.report.notes_left_out, 1, "the whistle");
    }

    #[test]
    fn a_single_track_file_splits_by_channel() {
        let mut events = vec![
            (0, EventKind::TrackName("My Song".to_string())),
            (
                0,
                EventKind::Program {
                    channel: 0,
                    program: 0,
                },
            ),
            (
                0,
                EventKind::Program {
                    channel: 1,
                    program: 40,
                },
            ),
            (
                0,
                EventKind::Program {
                    channel: 2,
                    program: 40,
                },
            ),
        ];
        events.extend(note(0, 60, 0, 480));
        events.extend(note(1, 72, 0, 480));
        events.extend(note(2, 76, 0, 480));
        let mut events: Vec<_> = events.into_iter().collect();
        events.sort_by_key(|(tick, _)| *tick);
        let project = import(&file(0, vec![events])).project;
        let names: Vec<&str> = project.channels().iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["Acoustic Grand Piano", "Violin", "Violin 2"]);
        let programs: Vec<Option<u8>> = project.channels().iter().map(|c| c.program).collect();
        assert_eq!(programs, [Some(0), Some(40), Some(40)]);
    }

    #[test]
    fn the_sustain_pedal_holds_notes_on() {
        let events = vec![
            (
                0,
                EventKind::Controller {
                    channel: 0,
                    number: SUSTAIN,
                    value: 127,
                },
            ),
            (0, on(0, 48, 80)),
            (240, off(0, 48)),
            (480, on(0, 52, 80)),
            (720, off(0, 52)),
            (
                1440,
                EventKind::Controller {
                    channel: 0,
                    number: SUSTAIN,
                    value: 0,
                },
            ),
            (1440, on(0, 55, 80)),
            (1680, off(0, 55)),
        ];
        let project = import(&file(0, vec![events])).project;
        let ends: Vec<usize> = song_notes(&project, 0).iter().map(|n| n.1).collect();
        // C and E ring on until the pedal comes up on beat 4 (tick 288
        // of ours); G was struck after it.
        assert_eq!(ends, [288, 288, 336]);
    }

    #[test]
    fn a_long_song_is_cut_at_bar_lines_where_no_note_sounds() {
        // Forty bars of 4/4: a note on every beat, and a low C held over
        // the bar line between bars 16 and 17, from the middle of one to
        // the middle of the other.
        let mut events = Vec::new();
        for beat in 0..160u64 {
            events.extend(note(0, 60 + (beat % 4) as u8, beat * 480, 480));
        }
        events.extend(note(0, 48, 15 * 1920 + 960, 1920));
        events.sort_by_key(|(tick, _)| *tick);
        let project = import(&file(0, vec![events])).project;
        let pieces: Vec<(usize, usize)> = project
            .playlist()
            .clips()
            .iter()
            .map(|clip| (clip.start, project.pattern(clip.pattern).unwrap().length()))
            .collect();
        // The first piece ends before bar 16, so the held note is not cut.
        assert_eq!(
            pieces,
            [(0, 15 * 16), (15 * 16, 16 * 16), (31 * 16, 9 * 16)]
        );
        assert!(song_notes(&project, 0).contains(&(15 * 384 + 192, 16 * 384 + 192, 48, 90)));
        // The same four notes every bar: the second and third pieces hold
        // different numbers of bars, so they are different patterns.
        assert_eq!(project.patterns().len(), 3);
    }

    #[test]
    fn a_stretch_that_comes_back_plays_the_same_pattern() {
        let mut events = Vec::new();
        for beat in 0..(64 * 4u64) {
            events.extend(note(0, 60 + (beat % 4) as u8, beat * 480, 480));
        }
        let project = import(&file(0, vec![events])).project;
        assert_eq!(project.playlist().clips().len(), 4);
        assert_eq!(
            project.patterns().len(),
            1,
            "four times sixteen bars, the same"
        );
        assert_eq!(project.patterns()[0].name, "Bars 1–16");
    }

    #[test]
    fn a_file_without_notes_is_refused() {
        let tempo_only = file(1, vec![vec![(0, EventKind::Tempo(500_000))]]);
        assert_eq!(
            super::import(&tempo_only, "x").unwrap_err(),
            ImportError::NoNotes
        );
    }

    #[test]
    fn every_demo_comes_back_note_for_note() {
        // Export every demo song as a MIDI file, import it, and export it
        // again: the two files must hold the same notes, velocities,
        // tempo changes, time signatures and key, at the same ticks.
        fn contents(bytes: &[u8]) -> Vec<(u64, String)> {
            let file = super::super::read::read(bytes).unwrap();
            let mut events: Vec<(u64, String)> = file
                .tracks
                .iter()
                .flat_map(|track| &track.events)
                .filter_map(|event| {
                    match &event.kind {
                        EventKind::NoteOn { key, velocity, .. } => {
                            Some(format!("on {key} {velocity}"))
                        }
                        EventKind::NoteOff { key, .. } => Some(format!("off {key}")),
                        EventKind::Tempo(micros) => Some(format!("tempo {micros}")),
                        EventKind::TimeSignature { beats, unit } => {
                            Some(format!("time {beats}/{unit}"))
                        }
                        EventKind::KeySignature { sharps, minor } => {
                            Some(format!("key {sharps} {minor}"))
                        }
                        _ => None,
                    }
                    .map(|text| (event.tick, text))
                })
                .collect();
            events.sort();
            events
        }
        for song in crate::library::song::SONGS {
            let project = crate::library::demo::song_demo(song);
            let exported = crate::song_to_midi(&project);
            let file = super::super::read::read(&exported).unwrap();
            let imported = super::import(&file, song.name).unwrap().project;
            assert_eq!(imported.scale, project.scale, "{}", song.name);
            assert_eq!(
                contents(&crate::song_to_midi(&imported)),
                contents(&exported),
                "{}",
                song.name
            );
        }
    }
}
