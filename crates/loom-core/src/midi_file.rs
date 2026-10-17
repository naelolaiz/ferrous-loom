//! Standard MIDI Files (`.mid`): patterns and songs that any other studio
//! can open.
//!
//! A MIDI file holds notes, not sound: it says "C4 starts here, ends
//! there, on channel 1", and the program that opens it chooses the
//! instruments. That makes it the common language of music software, and
//! the way to take a Ferrous Loom pattern into another studio.
//!
//! The file is a header chunk (`MThd`) followed by one track chunk
//! (`MTrk`) per track. A track is a list of events, each one after a
//! *delta time*: the number of ticks since the event before it. This
//! module writes "format 1" files: the first track holds the tempo, the
//! time signature and the key, and every channel of the project gets a
//! track of its own.

use crate::arrangement::Play;
use crate::meter::{Meter, TimeMarker};
use crate::midi::{DRUM_CHANNEL, MidiMessage};
use crate::project::{Channel, ChannelKind, Drum, Project, SampleSource};
use crate::{Note, STEPS_PER_BEAT, Scale, Tempo};

pub mod import;
pub mod read;

/// How many ticks one beat (a quarter note) is divided into. 96 is a
/// common choice: it divides by 2, 3, 4, 8, 16 and 32.
pub const TICKS_PER_BEAT: u16 = 96;

/// How many ticks one step (a sixteenth note) lasts: 24, the same ticks
/// a note's `nudge` and `trim` are counted in, so a note between two
/// steps is written exactly where it plays.
pub const TICKS_PER_STEP: u32 = crate::pattern::TICKS_PER_STEP as u32;

// A check made while compiling: if someone changed one of the two
// numbers and not the other, the build would stop here.
const _: () = assert!(TICKS_PER_BEAT as u32 == TICKS_PER_STEP * STEPS_PER_BEAT);

/// The file extensions of MIDI files: `.mid` and `.midi`, `.kar` for
/// karaoke files (MIDI files with lyrics), and `.rmi` for MIDI files in a
/// RIFF container.
pub const EXTENSIONS: [&str; 4] = ["mid", "midi", "kar", "rmi"];

/// Whether `path` names a MIDI file, going by its extension.
pub fn is_midi_file(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            EXTENSIONS
                .iter()
                .any(|known| extension.eq_ignore_ascii_case(known))
        })
}

/// The whole song, as the playlist plays it: every clip on a track that
/// is not muted, every channel that is not muted, with every repeat of
/// clips and sections written out.
pub fn song_to_midi(project: &Project) -> Vec<u8> {
    let song = project.arrangement();
    let meter = project.played_meter(&song);
    let tempos = project.played_tempos(&song);
    write_file(project, &meter, &tempos, &song.plays, song.length)
}

/// One pattern, from its first step, as pattern mode loops it.
pub fn pattern_to_midi(project: &Project, pattern: usize) -> Vec<u8> {
    let length = project
        .pattern(pattern)
        .map_or(0, |pattern| pattern.length());
    let play = Play {
        pattern,
        start: 0,
        skip: 0,
        length,
    };
    let meter = Meter::steady(project.pattern_time(pattern));
    write_file(project, &meter, &[], &[play], length)
}

/// The note General MIDI drum sets play for each built-in drum.
pub fn general_midi_drum(drum: Drum) -> Note {
    Note::new(match drum {
        Drum::Kick => 36,
        Drum::Snare => 38,
        Drum::Clap => 39,
        Drum::HiHat => 42,
        Drum::OpenHat => 46,
        Drum::Rim => 37,
        Drum::LowTom => 45,
        Drum::HighTom => 50,
        Drum::Ride => 51,
        Drum::Crash => 49,
        Drum::Cowbell => 56,
        Drum::Shaker => 70,
        Drum::Conga => 63,
    })
}

/// Writes a file with the pieces of patterns `placed` for every channel,
/// lasting `length` steps, with bars counted in `meter`, and step `n` at
/// `tempos[n]` (the project's tempo throughout if `tempos` is empty).
fn write_file(
    project: &Project,
    meter: &Meter,
    tempos: &[Tempo],
    placed: &[Play],
    length: usize,
) -> Vec<u8> {
    let midi_channels = midi_channels(project.channels());
    let end = length as u32 * TICKS_PER_STEP;

    let mut tracks = vec![tempo_track(project, meter, tempos, end)];
    for (index, channel) in project.channels().iter().enumerate() {
        if channel.muted {
            continue;
        }
        let events = channel_events(project, index, midi_channels[index], placed);
        if events.is_empty() {
            continue;
        }
        let mut track = TrackWriter::default();
        track.meta(0, TRACK_NAME, channel.name.as_bytes());
        for (tick, message) in events {
            track.event(tick, &message.bytes());
        }
        tracks.push(track.finish(end));
    }

    let mut file = Vec::new();
    file.extend_from_slice(b"MThd");
    file.extend_from_slice(&6u32.to_be_bytes()); // the header's length
    file.extend_from_slice(&1u16.to_be_bytes()); // format 1
    file.extend_from_slice(&(tracks.len() as u16).to_be_bytes());
    file.extend_from_slice(&TICKS_PER_BEAT.to_be_bytes());
    for track in tracks {
        file.extend(track);
    }
    file
}

/// The MIDI channel (0 to 15) each channel of the project goes out on.
///
/// Built-in drums share channel 10, where General MIDI expects drums.
/// MIDI Out channels keep the channel they were given. Every other
/// channel gets the next free one: not 10, and not one a MIDI Out channel
/// already plays on, or the two would share one sound on the synth and
/// cut each other's notes short. When there are more channels than free
/// ones, the free ones are used again from the start (or, if MIDI Out
/// channels take all of them, every channel but 10 is).
pub fn midi_channels(channels: &[Channel]) -> Vec<u8> {
    let taken = |number: u8| {
        channels
            .iter()
            .any(|channel| channel.kind == ChannelKind::MidiOut { channel: number })
    };
    let free: Vec<u8> = (0..16u8)
        .filter(|&number| number != DRUM_CHANNEL && !taken(number))
        .collect();
    let mut melodic = if free.is_empty() {
        (0..16u8).filter(|&number| number != DRUM_CHANNEL).collect()
    } else {
        free
    }
    .into_iter()
    .cycle();
    channels
        .iter()
        .map(|channel| match channel.kind {
            ChannelKind::Sampler(SampleSource::Builtin(_)) => DRUM_CHANNEL,
            ChannelKind::MidiOut { channel } => channel,
            _ => melodic.next().unwrap_or(0),
        })
        .collect()
}

/// Every note-on and note-off of one channel, in time order.
fn channel_events(
    project: &Project,
    channel: usize,
    midi_channel: u8,
    placed: &[Play],
) -> Vec<(u32, MidiMessage)> {
    let drum = match &project.channels()[channel].kind {
        ChannelKind::Sampler(SampleSource::Builtin(drum)) => Some(*drum),
        _ => None,
    };
    let mut events = Vec::new();
    for play in placed {
        let Some(pattern) = project.pattern(play.pattern) else {
            continue;
        };
        let length = pattern.length();
        for note in pattern
            .notes(channel)
            .iter()
            .filter(|note| note.start < length)
        {
            for song_step in play.song_steps_of(note.start, length) {
                // As in the audio thread: a note that runs past the end of
                // its pattern, or of the piece being played, is cut there.
                let room = (length - note.start).min(play.end() - song_step);
                let step = song_step as u32 * TICKS_PER_STEP;
                let on = step + u32::from(note.nudge);
                let off = if note.length <= room {
                    on + note.ticks() as u32
                } else {
                    step + room as u32 * TICKS_PER_STEP
                };
                let pitch = drum.map_or(note.pitch, general_midi_drum);
                events.push((
                    on,
                    MidiMessage::NoteOn {
                        channel: midi_channel,
                        pitch,
                        velocity: note.velocity,
                    },
                ));
                events.push((
                    off,
                    MidiMessage::NoteOff {
                        channel: midi_channel,
                        pitch,
                    },
                ));
            }
        }
    }
    // At the same tick, notes end before new ones start, so a note
    // repeated straight away is heard twice, not cut off by its own end.
    events.sort_by_key(|&(tick, message)| (tick, matches!(message, MidiMessage::NoteOn { .. })));
    events
}

/// The first track: the song's name, tempo, time signatures and key.
///
/// A song whose tempo changes gets a tempo event on every step where it
/// changes: one for a sudden change, and one per step of a glide, which
/// is how a MIDI file writes a ritardando.
fn tempo_track(project: &Project, meter: &Meter, tempos: &[Tempo], end: u32) -> Vec<u8> {
    let mut track = TrackWriter::default();
    track.meta(0, TRACK_NAME, b"Ferrous Loom");
    // Every event of the track, with its tick, sorted by tick at the end.
    let mut events: Vec<(u32, u8, Vec<u8>)> = Vec::new();
    // The tempo is written as microseconds per beat, in three bytes.
    let tempo_event = |tempo: Tempo| {
        let micros = (60_000_000.0 / f64::from(tempo.bpm())).round() as u32;
        micros.min(0xFF_FFFF).to_be_bytes()[1..].to_vec()
    };
    events.push((
        0,
        TEMPO,
        tempo_event(tempos.first().copied().unwrap_or(project.tempo)),
    ));
    for (step, pair) in tempos.windows(2).enumerate() {
        if pair[1] != pair[0] {
            let tick = (step as u32 + 1) * TICKS_PER_STEP;
            events.push((tick, TEMPO, tempo_event(pair[1])));
        }
    }
    // The time signature: beats to the bar, then the beat's note as a
    // power of two (2 for a quarter, 2^2 = 4; 3 for an eighth), then the
    // MIDI clocks per metronome click (24 per quarter note, 6 per step),
    // and 8 thirty-seconds per quarter. A song that changes time
    // signature gets one of these at every change.
    let first = TimeMarker::new(0, meter.first());
    for marker in std::iter::once(first).chain(meter.markers().iter().copied()) {
        let tick = marker.start as u32 * TICKS_PER_STEP;
        let signature = marker.time;
        let unit_power = signature.unit.max(1).ilog2() as u8;
        let clocks = (signature.steps_per_beat() * 6).min(255) as u8;
        events.push((
            tick,
            TIME_SIGNATURE,
            vec![signature.beats, unit_power, clocks, 8],
        ));
    }
    // The key signature: sharps (negative for flats), then 0 for major
    // or 1 for minor. A mode is written with the signature of the major
    // key it comes from; a row or a set has none.
    if let Some((sharps, minor)) = project.scale.key_signature() {
        events.push((0, KEY_SIGNATURE, vec![sharps as u8, u8::from(minor)]));
    }
    // A key signature cannot say "E Dorian" or "harmonic minor", so the
    // exact key also goes in an event of our own, which other programs
    // skip and Ferrous Loom reads back.
    events.push((0, SEQUENCER_SPECIFIC, scale_event(project.scale)));
    events.sort_by_key(|&(tick, ..)| tick);
    for (tick, kind, data) in events.into_iter().filter(|(tick, ..)| *tick <= end) {
        track.meta(tick, kind, &data);
    }
    track.finish(end)
}

/// Meta events: information for the reader, not notes.
const TRACK_NAME: u8 = 0x03;
const END_OF_TRACK: u8 = 0x2F;
const TEMPO: u8 = 0x51;
const TIME_SIGNATURE: u8 = 0x58;
const KEY_SIGNATURE: u8 = 0x59;
const SEQUENCER_SPECIFIC: u8 = 0x7F;

/// The start of our own event: 0x7D is the maker's number set aside for
/// programs that are not sold, then our name, so another program's
/// 0x7D event is never mistaken for ours.
const SCALE_EVENT_START: &[u8] = b"\x7DFerrous Loom key ";

/// The data of the sequencer-specific event that holds the exact key:
/// 0x7D, "Ferrous Loom key " and then the key as a project file writes it,
/// such as `(root:E,kind:Dorian)`.
pub fn scale_event(scale: Scale) -> Vec<u8> {
    let mut data = SCALE_EVENT_START.to_vec();
    data.extend_from_slice(ron::to_string(&scale).unwrap_or_default().as_bytes());
    data
}

/// The key a sequencer-specific event holds, if [`scale_event`] wrote it.
pub fn scale_from_event(data: &[u8]) -> Option<Scale> {
    let text = std::str::from_utf8(data.strip_prefix(SCALE_EVENT_START)?).ok()?;
    ron::from_str(text).ok()
}

/// Builds one track chunk, keeping the time of the last event so each
/// new one can be written as a delta.
#[derive(Default)]
struct TrackWriter {
    data: Vec<u8>,
    last_tick: u32,
}

impl TrackWriter {
    /// Writes the time since the last event. Events must come in order.
    fn delta(&mut self, tick: u32) {
        let delta = tick.saturating_sub(self.last_tick);
        write_variable_length(&mut self.data, delta);
        self.last_tick = self.last_tick.max(tick);
    }

    fn event(&mut self, tick: u32, bytes: &[u8]) {
        self.delta(tick);
        self.data.extend_from_slice(bytes);
    }

    fn meta(&mut self, tick: u32, kind: u8, bytes: &[u8]) {
        self.delta(tick);
        self.data.extend_from_slice(&[0xFF, kind]);
        write_variable_length(&mut self.data, bytes.len() as u32);
        self.data.extend_from_slice(bytes);
    }

    /// Ends the track at `end` (or at its last event, if that is later)
    /// and wraps it in its chunk header.
    fn finish(mut self, end: u32) -> Vec<u8> {
        self.meta(end.max(self.last_tick), END_OF_TRACK, &[]);
        let mut chunk = Vec::with_capacity(8 + self.data.len());
        chunk.extend_from_slice(b"MTrk");
        chunk.extend_from_slice(&(self.data.len() as u32).to_be_bytes());
        chunk.extend(self.data);
        chunk
    }
}

/// Writes a number in MIDI's "variable-length quantity" format: seven
/// bits per byte, the most significant first, with the top bit set on
/// every byte except the last. Small numbers, the most common delta
/// times, take a single byte.
fn write_variable_length(out: &mut Vec<u8>, value: u32) {
    let mut groups = [0u8; 5];
    let mut count = 0;
    let mut rest = value;
    loop {
        groups[count] = (rest & 0x7F) as u8;
        count += 1;
        rest >>= 7;
        if rest == 0 {
            break;
        }
    }
    for index in (0..count).rev() {
        let more = if index > 0 { 0x80 } else { 0 };
        out.push(groups[index] | more);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::playlist::{Clip, Section};
    use crate::{NoteEvent, Tempo, TempoMarker};

    fn variable_length(value: u32) -> Vec<u8> {
        let mut out = Vec::new();
        write_variable_length(&mut out, value);
        out
    }

    #[test]
    fn variable_length_numbers() {
        // The examples from the MIDI file specification.
        assert_eq!(variable_length(0), [0x00]);
        assert_eq!(variable_length(0x40), [0x40]);
        assert_eq!(variable_length(0x7F), [0x7F]);
        assert_eq!(variable_length(0x80), [0x81, 0x00]);
        assert_eq!(variable_length(0x2000), [0xC0, 0x00]);
        assert_eq!(variable_length(0x3FFF), [0xFF, 0x7F]);
        assert_eq!(variable_length(0x4000), [0x81, 0x80, 0x00]);
        assert_eq!(variable_length(0x0FFF_FFFF), [0xFF, 0xFF, 0xFF, 0x7F]);
    }

    /// Reads a file back into tracks of (tick, bytes) events, the way any
    /// MIDI reader would, to check what was written.
    fn read(file: &[u8]) -> Vec<Vec<(u32, Vec<u8>)>> {
        assert_eq!(&file[..4], b"MThd");
        assert_eq!(&file[8..10], [0, 1], "format 1");
        let count = u16::from_be_bytes([file[10], file[11]]) as usize;
        assert_eq!(u16::from_be_bytes([file[12], file[13]]), TICKS_PER_BEAT);
        let mut at = 14;
        let mut tracks = Vec::new();
        for _ in 0..count {
            assert_eq!(&file[at..at + 4], b"MTrk");
            let length = u32::from_be_bytes(file[at + 4..at + 8].try_into().unwrap()) as usize;
            let data = &file[at + 8..at + 8 + length];
            at += 8 + length;
            let mut events = Vec::new();
            let (mut i, mut tick) = (0, 0);
            while i < data.len() {
                let mut delta = 0;
                loop {
                    let byte = data[i];
                    i += 1;
                    delta = (delta << 7) | u32::from(byte & 0x7F);
                    if byte & 0x80 == 0 {
                        break;
                    }
                }
                tick += delta;
                let size = if data[i] == 0xFF {
                    3 + data[i + 2] as usize // FF, kind, length (one byte here), data
                } else {
                    3
                };
                events.push((tick, data[i..i + size].to_vec()));
                i += size;
            }
            tracks.push(events);
        }
        assert_eq!(at, file.len(), "nothing after the last track");
        tracks
    }

    #[test]
    fn tempo_changes_are_written_where_they_happen() {
        let mut project = project();
        project.add_clip(Clip::new(0, 0, 0));
        project.add_clip(Clip::new(0, 0, 16));
        // A sudden change to 60 BPM on bar 2, then a glide back up to
        // 100 over its last four steps.
        project.set_tempo_marker(TempoMarker::new(16, 60.0));
        project.set_tempo_marker(TempoMarker::gliding(28, 100.0, 4));
        let tracks = read(&song_to_midi(&project));
        let tempos: Vec<(u32, Vec<u8>)> = tracks[0]
            .iter()
            .filter(|(_, event)| event[..2] == [0xFF, 0x51])
            .cloned()
            .collect();
        let micros = |bpm: f64| {
            let m = (60_000_000.0 / bpm).round() as u32;
            vec![0xFF, 0x51, 3, (m >> 16) as u8, (m >> 8) as u8, m as u8]
        };
        let step = TICKS_PER_STEP;
        assert_eq!(
            tempos,
            vec![
                (0, micros(120.0)),
                (16 * step, micros(60.0)),
                (28 * step, micros(70.0)),
                (29 * step, micros(80.0)),
                (30 * step, micros(90.0)),
                (31 * step, micros(100.0)),
            ]
        );
    }

    /// A project with a Loom Osc and a kick, at 120 BPM, and one bar-long
    /// pattern.
    fn project() -> Project {
        let mut project = Project::empty();
        project.tempo = Tempo::new(120.0);
        project.add_channel(Channel::synth());
        project.add_channel(Channel::drum(Drum::Kick));
        let pattern = project.pattern_mut(0).unwrap();
        pattern.add_note(0, NoteEvent::new(Note::C4, 0, 4));
        pattern.add_note(0, NoteEvent::new(Note::A4, 14, 8)); // runs past the end
        pattern.add_note(1, NoteEvent::step(0));
        pattern.add_note(1, NoteEvent::step(8));
        project
    }

    #[test]
    fn a_pattern_becomes_three_tracks() {
        let tracks = read(&pattern_to_midi(&project(), 0));
        assert_eq!(tracks.len(), 3, "tempo, synth, kick");

        // 120 BPM is 500,000 microseconds per beat: 07 A1 20.
        assert!(tracks[0].contains(&(0, vec![0xFF, 0x51, 3, 0x07, 0xA1, 0x20])));
        // Every track ends where the pattern does: 16 steps of 24 ticks.
        for track in &tracks {
            assert_eq!(track.last(), Some(&(16 * 24, vec![0xFF, 0x2F, 0])));
        }

        let synth: Vec<_> = tracks[1].iter().filter(|(_, e)| e[0] != 0xFF).collect();
        assert_eq!(
            synth,
            [
                &(0, vec![0x90, 60, 100]),
                &(96, vec![0x80, 60, 64]),
                &(14 * 24, vec![0x90, 69, 100]),
                // Cut at the end of the pattern, as it is played.
                &(16 * 24, vec![0x80, 69, 64]),
            ]
        );
    }

    #[test]
    fn the_time_signature_is_written() {
        let mut project = project();
        let tracks = read(&pattern_to_midi(&project, 0));
        assert!(
            tracks[0].contains(&(0, vec![0xFF, 0x58, 4, 4, 2, 24, 8])),
            "4/4"
        );
        // 6/8: six eighths (2^3), a click every dotted quarter (36 clocks).
        project.time_signature = crate::TimeSignature::new(6, 8);
        let tracks = read(&pattern_to_midi(&project, 0));
        assert!(
            tracks[0].contains(&(0, vec![0xFF, 0x58, 4, 6, 3, 36, 8])),
            "6/8"
        );
    }

    #[test]
    fn the_key_signature_is_written() {
        let mut project = project();
        let signature = |project: &Project| -> Vec<Vec<u8>> {
            read(&pattern_to_midi(project, 0))[0]
                .iter()
                .filter(|(_, event)| event[..2] == [0xFF, 0x59])
                .map(|(_, event)| event.clone())
                .collect()
        };
        assert_eq!(signature(&project), [vec![0xFF, 0x59, 2, 0, 0]], "C major");
        project.scale = crate::Scale::new(crate::PitchClass::G, crate::ScaleKind::NaturalMinor);
        assert_eq!(
            signature(&project),
            [vec![0xFF, 0x59, 2, 0xFE, 1]],
            "G minor: two flats"
        );
        project.scale = crate::Scale::new(crate::PitchClass::E, crate::ScaleKind::Dorian);
        assert_eq!(
            signature(&project),
            [vec![0xFF, 0x59, 2, 2, 0]],
            "E Dorian: as D major"
        );
        project.scale = crate::Scale::row(crate::harmony::ROW_PRESETS[0].row);
        assert!(signature(&project).is_empty(), "a row has no key signature");
    }

    #[test]
    fn the_exact_key_is_written_too() {
        let mut project = project();
        let scales = [
            crate::Scale::new(crate::PitchClass::E, crate::ScaleKind::Dorian),
            crate::Scale::new(crate::PitchClass::CSharp, crate::ScaleKind::HarmonicMinor),
            crate::Scale::row(crate::harmony::ROW_PRESETS[0].row),
        ];
        for scale in scales {
            project.scale = scale;
            let written: Vec<Option<crate::Scale>> = read(&pattern_to_midi(&project, 0))[0]
                .iter()
                .filter(|(_, event)| event[..2] == [0xFF, 0x7F])
                .map(|(_, event)| scale_from_event(&event[3..]))
                .collect();
            assert_eq!(written, [Some(scale)], "{scale:?}");
        }
        assert_eq!(scale_from_event(b"\x7DSomeone else"), None);
        assert_eq!(scale_from_event(b"\x7DFerrous Loom key (root:"), None);
    }

    #[test]
    fn every_change_of_time_signature_is_written() {
        let mut project = project();
        project.add_clip(crate::Clip::new(0, 0, 0).repeated(4));
        project.set_time_marker(TimeMarker::new(32, crate::TimeSignature::new(3, 4)));
        let tracks = read(&song_to_midi(&project));
        let signatures: Vec<&(u32, Vec<u8>)> = tracks[0]
            .iter()
            .filter(|(_, event)| event[..2] == [0xFF, 0x58])
            .collect();
        assert_eq!(
            signatures,
            [
                &(0, vec![0xFF, 0x58, 4, 4, 2, 24, 8]),
                &(32 * 24, vec![0xFF, 0x58, 4, 3, 2, 24, 8])
            ]
        );
    }

    #[test]
    fn drums_use_channel_ten_and_general_midi_notes() {
        let tracks = read(&pattern_to_midi(&project(), 0));
        let kicks: Vec<_> = tracks[2]
            .iter()
            .filter(|(_, e)| e[0] == 0x99)
            .map(|(tick, e)| (*tick, e[1]))
            .collect();
        assert_eq!(kicks, [(0, 36), (8 * 24, 36)]);
    }

    #[test]
    fn a_song_places_each_clip() {
        let mut project = project();
        project.add_clip(Clip::new(0, 0, 0));
        project.add_clip(Clip::new(0, 0, 32));
        let tracks = read(&song_to_midi(&project));
        let starts: Vec<u32> = tracks[1]
            .iter()
            .filter(|(_, e)| e[0] == 0x90 && e[1] == 60)
            .map(|(tick, _)| *tick)
            .collect();
        assert_eq!(starts, [0, 32 * 24]);

        // A muted channel is left out, as it is when the song plays.
        project.channel_mut(1).unwrap().muted = true;
        assert_eq!(read(&song_to_midi(&project)).len(), 2);
    }

    #[test]
    fn repeats_are_written_out() {
        let mut project = project();
        project.add_clip(Clip::new(0, 0, 0).repeated(2));
        let mut twice = Section::new("Twice", 0, 32);
        twice.repeats = 2;
        project.add_section(twice);
        let tracks = read(&song_to_midi(&project));
        let starts: Vec<u32> = tracks[1]
            .iter()
            .filter(|(_, e)| e[0] == 0x90 && e[1] == 60)
            .map(|(tick, _)| *tick)
            .collect();
        // The clip plays its pattern twice, and the section plays twice.
        assert_eq!(starts, [0, 16 * 24, 32 * 24, 48 * 24]);
    }

    #[test]
    fn a_repeated_note_ends_before_it_starts_again() {
        let mut project = Project::empty();
        project.add_channel(Channel::synth());
        let pattern = project.pattern_mut(0).unwrap();
        pattern.add_note(0, NoteEvent::new(Note::C4, 0, 2));
        pattern.add_note(0, NoteEvent::new(Note::C4, 2, 2));
        let tracks = read(&pattern_to_midi(&project, 0));
        let at_two_steps: Vec<u8> = tracks[1]
            .iter()
            .filter(|(tick, _)| *tick == 48)
            .map(|(_, e)| e[0])
            .collect();
        assert_eq!(at_two_steps, [0x80, 0x90]);
    }

    #[test]
    fn a_section_played_once_does_not_shorten_a_note() {
        // A note from step 28 to step 36 of a four-bar pattern, and a
        // section, played once, over the first two bars: the note still
        // ends on step 36, as without the section.
        let mut project = Project::empty();
        project.add_channel(Channel::synth());
        let pattern = project.pattern_mut(0).unwrap();
        pattern.set_length(64);
        pattern.add_note(0, NoteEvent::new(Note::new(48), 28, 8));
        project.add_clip(Clip::new(0, 0, 0));
        project.add_section(Section::new("Intro", 0, 32));
        let tracks = read(&song_to_midi(&project));
        let notes: Vec<_> = tracks[1].iter().filter(|(_, e)| e[0] != 0xFF).collect();
        assert_eq!(
            notes,
            [
                &(28 * 24, vec![0x90, 48, 100]),
                &(36 * 24, vec![0x80, 48, 64])
            ]
        );
    }

    #[test]
    fn other_channels_stay_off_the_midi_out_channels() {
        let channels = [Channel::midi_out(0), Channel::synth(), Channel::midi_out(2)];
        assert_eq!(midi_channels(&channels), [0, 1, 2]);
        let channels = [Channel::synth(), Channel::synth(), Channel::midi_out(1)];
        assert_eq!(midi_channels(&channels), [0, 2, 1]);
        // MIDI Out channels on all fifteen: the others share them.
        let mut channels: Vec<Channel> = (0..16)
            .filter(|&number| number != DRUM_CHANNEL)
            .map(Channel::midi_out)
            .collect();
        channels.push(Channel::synth());
        assert_eq!(midi_channels(&channels).last(), Some(&0));
    }

    #[test]
    fn midi_out_channels_keep_their_channel() {
        let channels = [
            Channel::synth(),
            Channel::drum(Drum::Snare),
            Channel::midi_out(4),
            Channel::synth(),
        ];
        assert_eq!(midi_channels(&channels), [0, DRUM_CHANNEL, 4, 1]);
    }
}
