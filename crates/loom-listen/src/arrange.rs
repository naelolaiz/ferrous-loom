//! Turning an analysis into a project that plays along with the recording.
//!
//! The recording becomes a sampler channel whose one note, at the very
//! start of the song, plays the whole file. The chords become notes on a
//! Loom Osc channel called "Harmony", one pattern per 16 bars, laid one
//! after another in the playlist. The tempo, time signature and key are
//! set from the analysis, so the bar lines, the harmony keys and the
//! piano roll all line up with the music.
//!
//! The recording rarely starts exactly on a downbeat, so it is saved again
//! with a little silence in front (the *lead-in*): just enough for its
//! first downbeat to fall on a bar line of the song.

use std::path::{Path, PathBuf};

use loom_core::library::voicing::voice;
use loom_core::{
    Channel, ChannelKind, Chord, Clip, MAX_PATTERNS, MAX_SONG_STEPS, MAX_STEPS, Note, NoteEvent,
    Project, STEP_NOTE, STEPS_PER_BEAT, SampleSource, Tempo, TimeSignature,
};

use crate::{Analysis, Recording};

/// How many bars each harmony pattern holds.
const BARS_PER_PATTERN: usize = 16;
/// How hard the harmony chords are played: softer than the default, to
/// stay behind the recording.
const CHORD_VELOCITY: u8 = 80;

/// The playlist tracks used.
const RECORDING_TRACK: usize = 0;
const HARMONY_TRACK: usize = 1;

/// Where the bars fall: how many whole bars of lead-in come before the
/// first downbeat, and how many seconds of silence that takes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LeadIn {
    pub bars: usize,
    pub seconds: f64,
}

/// The lead-in for `analysis`: the first downbeat moves to the start of
/// the first bar that comes at or after it.
pub fn lead_in(analysis: &Analysis) -> LeadIn {
    let downbeat = analysis
        .beats
        .get(analysis.first_downbeat)
        .copied()
        .unwrap_or(0.0)
        .max(0.0);
    let bar = analysis.beat_seconds() * analysis.beats_per_bar as f64;
    let bars = (downbeat / bar).ceil() as usize;
    LeadIn {
        bars,
        seconds: bars as f64 * bar - downbeat,
    }
}

/// Saves `recording` as a 32-bit float WAV file, with `silence` seconds
/// of silence in front, for the sampler to play.
pub fn write_wav(recording: &Recording, silence: f64, path: &Path) -> Result<(), hound::Error> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: recording.sample_rate,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut writer = hound::WavWriter::create(path, spec)?;
    let silent_frames = (silence * f64::from(recording.sample_rate)).round() as usize;
    for _ in 0..silent_frames {
        writer.write_sample(0.0_f32)?;
    }
    for &sample in &recording.samples {
        writer.write_sample(sample)?;
    }
    writer.finalize()
}

/// One chord held over several beats, in song beats.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Span {
    chord: Chord,
    first_beat: usize,
    beats: usize,
}

/// The chords of `analysis` joined into spans, placed in song beats.
fn spans(analysis: &Analysis, lead_in: LeadIn) -> Vec<Span> {
    // Beat `first_downbeat` of the recording is song beat `offset`.
    let offset = lead_in.bars * analysis.beats_per_bar;
    let mut spans: Vec<Span> = Vec::new();
    for (beat, chord) in analysis.chords.iter().enumerate() {
        let Some(song_beat) = (beat + offset).checked_sub(analysis.first_downbeat) else {
            continue;
        };
        let Some(chord) = *chord else {
            continue;
        };
        match spans.last_mut() {
            Some(last) if last.chord == chord && last.first_beat + last.beats == song_beat => {
                last.beats += 1;
            }
            _ => spans.push(Span {
                chord,
                first_beat: song_beat,
                beats: 1,
            }),
        }
    }
    spans
}

/// A new project that plays the recording saved at `wav` (written by
/// [`write_wav`] with `lead_in`) with a harmony track following it.
pub fn follow(analysis: &Analysis, lead_in: LeadIn, wav: PathBuf, title: &str) -> Project {
    let mut project = Project::empty();
    project.tempo = Tempo::new(analysis.bpm as f32);
    project.time_signature = TimeSignature::new(analysis.beats_per_bar as u8, 4);
    if let Some(scale) = analysis.scale {
        project.scale = scale;
    }
    let bar = project.time_signature.steps_per_bar();
    let beat = STEPS_PER_BEAT as usize;

    let recording = project
        .add_channel(Channel {
            name: title.to_string(),
            kind: ChannelKind::Sampler(SampleSource::File(wav)),
            volume: 0.8,
            muted: false,
            program: None,
        })
        .unwrap_or(0);
    let harmony = project
        .add_channel(Channel {
            name: "Harmony".to_string(),
            ..Channel::synth()
        })
        .unwrap_or(1);

    // Pattern 1 starts the recording, once, at the start of the song.
    if let Some(pattern) = project.pattern_mut(0) {
        pattern.name = "Recording".to_string();
        pattern.set_length(bar);
        pattern.set_notes(recording, [NoteEvent::new(STEP_NOTE, 0, 1)]);
    }
    project.add_clip(Clip::new(0, RECORDING_TRACK, 0));

    // The chords, in patterns of 16 bars.
    let pattern_steps = (BARS_PER_PATTERN * bar).min(MAX_STEPS / bar * bar);
    let mut notes_by_pattern: Vec<Vec<NoteEvent>> = Vec::new();
    let mut previous: Option<Vec<Note>> = None;
    for span in spans(analysis, lead_in) {
        let voicing = voice(span.chord, previous.as_deref());
        let mut start = span.first_beat * beat;
        let end = ((span.first_beat + span.beats) * beat).min(MAX_SONG_STEPS);
        // A span that crosses into the next pattern is cut in two.
        while start < end {
            let index = start / pattern_steps;
            let pattern_end = (index + 1) * pattern_steps;
            let length = end.min(pattern_end) - start;
            if notes_by_pattern.len() <= index {
                notes_by_pattern.resize(index + 1, Vec::new());
            }
            for &pitch in &voicing {
                notes_by_pattern[index].push(NoteEvent {
                    velocity: CHORD_VELOCITY,
                    ..NoteEvent::new(pitch, start - index * pattern_steps, length)
                });
            }
            start += length;
        }
        previous = Some(voicing);
    }
    for (index, notes) in notes_by_pattern.into_iter().enumerate() {
        if notes.is_empty() || project.patterns().len() >= MAX_PATTERNS {
            continue;
        }
        let Some(pattern_index) = project.add_pattern() else {
            break;
        };
        if let Some(pattern) = project.pattern_mut(pattern_index) {
            pattern.name = format!("Chords {}", index + 1);
            pattern.set_length(pattern_steps);
            pattern.set_notes(harmony, notes);
        }
        project.add_clip(Clip::new(
            pattern_index,
            HARMONY_TRACK,
            index * pattern_steps,
        ));
    }
    if let Some(track) = project.track_mut(RECORDING_TRACK) {
        track.name = "Recording".to_string();
    }
    if let Some(track) = project.track_mut(HARMONY_TRACK) {
        track.name = "Harmony".to_string();
    }
    project.remove_unused_tracks();
    project
}

#[cfg(test)]
mod tests {
    use super::*;
    use loom_core::{ChordQuality, PitchClass};

    fn analysis() -> Analysis {
        let c = Some(Chord::new(PitchClass::C, ChordQuality::Major));
        let g = Some(Chord::new(PitchClass::G, ChordQuality::Major));
        Analysis {
            // A beat every half second (120 BPM), the first at 0.3 s.
            beats: (0..12).map(|i| 0.3 + 0.5 * i as f64).collect(),
            bpm: 120.0,
            beats_per_bar: 4,
            // Bar 1 starts on the third beat, at 1.3 s.
            first_downbeat: 2,
            scale: None,
            chords: vec![g, g, c, c, c, c, g, g, g, g, None, c],
        }
    }

    #[test]
    fn the_first_downbeat_moves_to_a_bar_line() {
        let lead_in = lead_in(&analysis());
        // A bar lasts 2 s, so the downbeat at 1.3 s moves to 2 s.
        assert_eq!(lead_in.bars, 1);
        assert!((lead_in.seconds - 0.7).abs() < 1e-9);
    }

    #[test]
    fn chords_join_into_spans_in_song_beats() {
        let analysis = analysis();
        let spans = spans(&analysis, lead_in(&analysis));
        let firsts: Vec<_> = spans.iter().map(|s| (s.first_beat, s.beats)).collect();
        // The two beats before the downbeat land in the lead-in bar.
        assert_eq!(firsts, [(2, 2), (4, 4), (8, 4), (13, 1)]);
    }

    #[test]
    fn the_project_plays_the_recording_and_the_chords() {
        let analysis = analysis();
        let lead_in = lead_in(&analysis);
        let project = follow(&analysis, lead_in, PathBuf::from("song.wav"), "Song");
        assert_eq!(project.channels().len(), 2);
        assert_eq!(project.patterns().len(), 2);
        assert_eq!(project.patterns()[0].notes(0).len(), 1);
        // Four spans of three-note chords.
        assert_eq!(project.patterns()[1].notes(1).len(), 12);
        assert_eq!(project.playlist().clips().len(), 2);
    }
}
