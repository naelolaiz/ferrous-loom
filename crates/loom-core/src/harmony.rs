//! Music theory: scales, chords, and naming what is being played.
//!
//! - [`PitchSet`] is a set of pitch classes, the basic building block.
//! - [`Scale`] is a root and a [`ScaleKind`], such as "A minor".
//! - [`Chord`] is a root and a [`ChordQuality`], such as "Am7".
//! - [`analyze`] names a group of notes: a single note, an interval or a chord.
//! - [`ChordStamp`] says what the piano roll places with one click.
//! - [`next_chords`] suggests chords that often come next in a key.
//! - [`HintLevel`] says how much the hints explain.
//! - [`keys`] are the harmony keys: the seven chords of a key, to play.
//! - [`ToneRow`] is a twelve-tone row, and [`SetClass`] names any group
//!   of notes, chord or not: music after tonality.
//! - [`Micromode`] is one of Francisco Kröpfl's twelve groups of three
//!   notes, the smallest sets, which larger sets are made of.
//! - [`key_fits`] finds the key a piece is in from how long each note
//!   sounds.

mod analysis;
mod chord;
mod hints;
mod key_finding;
pub mod keys;
mod micromode;
mod pitch_set;
mod progression;
mod row;
mod scale;
mod set_class;

pub use analysis::{Harmony, analyze, interval_name};
pub use chord::{Chord, ChordFamily, ChordQuality, ChordStamp};
pub use hints::HintLevel;
pub use key_finding::key_fits;
pub use micromode::{Micromode, MicromodeFamily, micromode_content, micromodes_in, trichords};
pub use pitch_set::PitchSet;
pub use progression::{Suggestion, next_chords};
pub use row::{ROW_PRESETS, RowForm, RowLabel, RowPreset, ToneRow};
pub use scale::{Scale, ScaleKind};
pub use set_class::{SetClass, digits, pc_digit};
