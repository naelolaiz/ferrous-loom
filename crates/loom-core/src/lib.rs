//! Core music types for Ferrous Loom.
//!
//! This crate knows about notes, tempo, time signatures, patterns,
//! harmony, the playlist and its sections, the project as a whole, how to save a
//! project to a file, what changed between two versions of a project,
//! undo and redo, the piano roll's tools for changing many notes at once,
//! MIDI note messages, MIDI files, turning keys played on a keyboard into
//! notes, twelve-tone rows and serialism, Kröpfl's micromodes, and a
//! library of styles,
//! rhythms and songs. It has no audio and no UI code, which keeps it
//! small, fast to compile and easy to test with plain `cargo test`.

pub mod arrangement;
pub mod diff;
pub mod file;
pub mod harmony;
pub mod history;
pub mod library;
pub mod meter;
pub mod micromodal;
pub mod midi;
pub mod midi_file;
pub mod pattern;
pub mod pitch;
pub mod playlist;
pub mod project;
pub mod record;
pub mod sequencer;
pub mod serial;
pub mod tempo;
pub mod tools;

// Re-export the main types so other crates can write `loom_core::Note`
// instead of `loom_core::pitch::Note`.
pub use arrangement::{Arrangement, Pass, Play};
pub use diff::{Change, diff};
pub use harmony::{Chord, ChordQuality, ChordStamp, Harmony, PitchSet, Scale, ScaleKind};
pub use history::History;
pub use meter::{Bar, Meter, TimeMarker, TimeSignature};
pub use midi::{DRUM_CHANNEL, MidiMessage};
pub use midi_file::{general_midi_drum, midi_channels, pattern_to_midi, song_to_midi};
pub use pattern::{
    DEFAULT_VELOCITY, MAX_STEPS, NoteEvent, Pattern, STEP_NOTE, TICKS_PER_STEP, tidy_notes,
};
pub use pitch::{Letter, Note, NoteName, PitchClass, Spelling};
pub use playlist::{
    Clip, LoopRegion, MAX_REPEATS, MAX_SONG_STEPS, MAX_TRACKS, Playlist, SECTION_NAMES,
    STEPS_PER_BAR, Section, Track,
};
pub use project::{
    Channel, ChannelKind, Drum, MAX_CHANNELS, MAX_LOOSE_TIMING, MAX_PATTERNS, Project, SampleSource,
};
pub use sequencer::{STEPS_PER_BEAT, Sequencer};
pub use tempo::{Tempo, TempoMap, TempoMarker};
