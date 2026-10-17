//! Writing a twelve-tone row into a pattern: serialism.
//!
//! A row only says which note comes next. To make music of it, a
//! composer still decides how long each note lasts, how loud it is, in
//! which octave it sounds, and whether some notes sound together as a
//! chord. [`RowWriting`] holds those choices, and [`write_row`] turns a
//! row into the notes of a pattern.
//!
//! **Integral serialism**, the music of Olivier Messiaen, Pierre Boulez
//! and Milton Babbitt around 1950, let a series decide those things too.
//! Here each can follow the row:
//!
//! - **Durations**: the row's numbers (each note's distance in semitones
//!   above the row's first note, 0 to 11) are read as lengths, so the
//!   `k`-th note lasts as many steps as the `k`-th number, plus one.
//! - **Dynamics**: the numbers of the inversion are read as twelve
//!   levels from very soft to very loud.
//! - **Octaves**: the numbers of the retrograde, counted in threes, pick
//!   one of three octaves.
//!
//! Each choice reads a different form, so the three series do not move
//! in step with the pitches or with each other.

use crate::harmony::{RowForm, RowLabel, ToneRow};
use crate::pattern::NoteEvent;
use crate::pitch::{Note, PitchClass};

/// In which octaves the notes of the row are placed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Register {
    /// Every note in the octave above the lowest note.
    Close,
    /// Each note as near as it can be to the one before: a smooth line.
    #[default]
    Nearest,
    /// The notes spread over three octaves, as in the music of Webern,
    /// where a melody leaps from high to low.
    Wide,
}

impl Register {
    pub const ALL: [Register; 3] = [Register::Close, Register::Nearest, Register::Wide];

    pub fn name(self) -> &'static str {
        match self {
            Register::Close => "Close",
            Register::Nearest => "Smooth",
            Register::Wide => "Wide",
        }
    }

    pub fn about(self) -> &'static str {
        match self {
            Register::Close => "Every note in the same octave.",
            Register::Nearest => "Each note as close as it can be to the one before.",
            Register::Wide => "The notes leap over three octaves, as in Webern.",
        }
    }
}

/// Which forms follow the first one, until the pattern is full.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Chain {
    /// Just the form, once.
    Once,
    /// The form again and again.
    #[default]
    Repeat,
    /// The four forms in turn, all with the same number: P, R, I, RI.
    FourForms,
}

impl Chain {
    pub const ALL: [Chain; 3] = [Chain::Once, Chain::Repeat, Chain::FourForms];

    pub fn name(self) -> &'static str {
        match self {
            Chain::Once => "Once",
            Chain::Repeat => "Repeat",
            Chain::FourForms => "P, R, I, RI",
        }
    }

    /// The form to write after `count` forms have been written, starting
    /// with `first`, or `None` once there are no more.
    fn nth(self, first: RowLabel, count: usize) -> Option<RowLabel> {
        const ORDER: [RowForm; 4] = [
            RowForm::Prime,
            RowForm::Retrograde,
            RowForm::Inversion,
            RowForm::RetrogradeInversion,
        ];
        match self {
            Chain::Once => (count == 0).then_some(first),
            Chain::Repeat => Some(first),
            Chain::FourForms => {
                let start = ORDER.iter().position(|&f| f == first.form).unwrap_or(0);
                let form = ORDER[(start + count) % 4];
                Some(RowLabel::new(form, first.number))
            }
        }
    }
}

/// How to write a row into a pattern.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RowWriting {
    /// The form to start with, such as `P0`.
    pub label: RowLabel,
    pub chain: Chain,
    /// How many notes of the row sound together: 1 for a melody, 3 to
    /// play the row as four three-note chords (trichords), 6 for two
    /// hexachords.
    pub together: usize,
    /// How long each note (or chord) lasts, in steps, when the durations
    /// do not follow the row.
    pub step: usize,
    pub register: Register,
    /// The lowest note the row is written from.
    pub low: Note,
    /// Whether the durations follow the row (see the module documentation).
    pub durations: bool,
    /// Whether the velocities follow the row's inversion.
    pub dynamics: bool,
    /// Whether the octaves follow the row's retrograde.
    pub octaves: bool,
}

impl Default for RowWriting {
    fn default() -> Self {
        RowWriting {
            label: RowLabel::new(RowForm::Prime, 0),
            chain: Chain::Repeat,
            together: 1,
            step: 2,
            register: Register::Nearest,
            low: Note::new(60),
            durations: false,
            dynamics: false,
            octaves: false,
        }
    }
}

/// The twelve velocities, from very soft (ppp) to very loud (fff).
fn dynamic(level: u8) -> u8 {
    28 + 9 * level.min(11)
}

/// Writes `row` into a pattern `length` steps long, the way `writing`
/// says, starting at step 0. Notes that would end after the pattern are
/// cut short; writing stops at the end of the pattern.
pub fn write_row(row: ToneRow, writing: &RowWriting, length: usize) -> Vec<NoteEvent> {
    let first = row.semitones()[0];
    // The row's numbers: each note's distance above the first note.
    let numbers = row.semitones().map(|n| (n + 12 - first) % 12);
    let together = writing.together.clamp(1, 12);
    let step = writing.step.max(1);
    let mut notes = Vec::new();
    let mut start = 0;
    let mut previous: Option<Note> = None;
    // The place in the series of durations, dynamics and octaves, which
    // runs on from one form to the next.
    let mut place = 0;
    let mut count = 0;
    while start < length {
        let Some(label) = writing.chain.nth(writing.label, count) else {
            break;
        };
        count += 1;
        let form = row.form(label).pitch_classes();
        for group in form.chunks(together) {
            if start >= length {
                break;
            }
            let lasts = if writing.durations {
                usize::from(numbers[place % 12]) + 1
            } else {
                step
            };
            let mut below: Option<Note> = None;
            for (i, &pitch_class) in group.iter().enumerate() {
                let k = place + i;
                let octave = if writing.octaves {
                    // The retrograde's numbers, in threes.
                    Some(numbers[11 - k % 12] % 3)
                } else if writing.register == Register::Wide {
                    // Low, high, middle, and round again, against the row.
                    Some([0, 2, 1][k % 3])
                } else {
                    None
                };
                let pitch = match (octave, writing.register) {
                    (Some(octave), _) => in_octave(pitch_class, writing.low, octave),
                    (None, Register::Close) => in_octave(pitch_class, writing.low, 0),
                    // In a chord, each note goes just above the one below
                    // it; in a line, as near as can be to the last note.
                    (None, _) => match (below, previous) {
                        (Some(lower), _) => above(pitch_class, lower),
                        (None, Some(last)) => nearest(pitch_class, last, writing.low),
                        (None, None) => in_octave(pitch_class, writing.low, 0),
                    },
                };
                below = Some(pitch);
                let velocity = if writing.dynamics {
                    // The inversion's numbers.
                    dynamic((12 - numbers[k % 12]) % 12)
                } else {
                    crate::DEFAULT_VELOCITY
                };
                notes.push(NoteEvent {
                    velocity,
                    ..NoteEvent::new(pitch, start, lasts.min(length - start))
                });
                if i == 0 {
                    previous = Some(pitch);
                }
            }
            place += group.len();
            start += lasts;
        }
    }
    notes
}

/// The note of `pitch_class` in the octave `octave` octaves above the
/// one that starts at `low`.
pub(crate) fn in_octave(pitch_class: PitchClass, low: Note, octave: u8) -> Note {
    let up = low.pitch_class().semitones_up_to(pitch_class);
    let midi = u32::from(low.midi()) + u32::from(up) + 12 * u32::from(octave);
    Note::new(midi.min(127) as u8)
}

/// The note of `pitch_class` just above `lower`.
pub(crate) fn above(pitch_class: PitchClass, lower: Note) -> Note {
    let up = match lower.pitch_class().semitones_up_to(pitch_class) {
        0 => 12,
        gap => gap,
    };
    lower.transpose(i32::from(up)).unwrap_or(lower)
}

/// The note of `pitch_class` nearest to `last` (up on a tritone), kept
/// within two octaves above `low` so a line cannot wander off the
/// keyboard.
pub(crate) fn nearest(pitch_class: PitchClass, last: Note, low: Note) -> Note {
    let up = i32::from(last.pitch_class().semitones_up_to(pitch_class));
    let step = if up > 6 { up - 12 } else { up };
    let note = last.transpose(step).unwrap_or(last);
    let (bottom, top) = (low.midi(), low.midi().saturating_add(24));
    if note.midi() < bottom {
        note.transpose(12).unwrap_or(note)
    } else if note.midi() >= top {
        note.transpose(-12).unwrap_or(note)
    } else {
        note
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harmony::ROW_PRESETS;
    use crate::{PitchSet, tools};

    fn suite() -> ToneRow {
        ROW_PRESETS[0].row
    }

    fn pitch_classes(notes: &[NoteEvent]) -> Vec<u8> {
        notes
            .iter()
            .map(|n| n.pitch.pitch_class().semitone())
            .collect()
    }

    #[test]
    fn a_melody_in_eighths() {
        let writing = RowWriting {
            label: suite().label(),
            chain: Chain::Once,
            ..RowWriting::default()
        };
        let notes = write_row(suite(), &writing, 64);
        assert_eq!(notes.len(), 12);
        assert_eq!(pitch_classes(&notes), suite().semitones());
        assert!(
            notes
                .iter()
                .enumerate()
                .all(|(i, n)| n.start == 2 * i && n.length == 2)
        );
        // Smooth: no leap is bigger than a tritone.
        for pair in notes.windows(2) {
            let leap = i32::from(pair[1].pitch.midi()) - i32::from(pair[0].pitch.midi());
            assert!(leap.abs() <= 6, "{leap}");
        }
    }

    #[test]
    fn the_four_forms_fill_the_pattern() {
        let writing = RowWriting {
            label: RowLabel::new(RowForm::Prime, 0),
            chain: Chain::FourForms,
            step: 1,
            ..RowWriting::default()
        };
        let notes = write_row(suite(), &writing, 48);
        assert_eq!(notes.len(), 48);
        let forms: Vec<String> = pitch_classes(&notes)
            .chunks(12)
            .map(|c| {
                let row = ToneRow::new(c.try_into().unwrap()).unwrap();
                suite().label_of(row).unwrap().to_string()
            })
            .collect();
        assert_eq!(forms, ["P0", "R0", "I0", "RI0"]);
        // The pattern ends before a fifth form could start.
        assert!(write_row(suite(), &writing, 50).len() == 50);
    }

    #[test]
    fn trichords_as_chords() {
        let writing = RowWriting {
            label: suite().label(),
            chain: Chain::Once,
            together: 3,
            step: 4,
            ..RowWriting::default()
        };
        let notes = write_row(suite(), &writing, 64);
        let chords = tools::chords(&notes);
        assert_eq!(chords.len(), 4);
        for (chord, segment) in chords.iter().zip(suite().segments(3)) {
            let set: PitchSet = chord.iter().map(|n| n.pitch.pitch_class()).collect();
            assert_eq!(set, segment);
            // Stacked upwards in row order.
            assert!(chord.windows(2).all(|w| w[0].pitch < w[1].pitch));
        }
    }

    #[test]
    fn serial_durations_dynamics_and_octaves() {
        let writing = RowWriting {
            label: suite().label(),
            chain: Chain::Once,
            durations: true,
            dynamics: true,
            octaves: true,
            low: Note::new(48),
            ..RowWriting::default()
        };
        let notes = write_row(suite(), &writing, 200);
        // E F G Db... is 0 1 3 9 2 11 4 10 7 8 5 6 above E: lengths 1 2 4 10...
        let lengths: Vec<usize> = notes.iter().map(|n| n.length).collect();
        assert_eq!(lengths, [1, 2, 4, 10, 3, 12, 5, 11, 8, 9, 6, 7]);
        // Each note starts where the last one ended.
        assert!(notes.windows(2).all(|w| w[1].start == w[0].end()));
        // Twelve different velocities, and three octaves from C3.
        let mut velocities: Vec<u8> = notes.iter().map(|n| n.velocity).collect();
        velocities.sort_unstable();
        velocities.dedup();
        assert_eq!(velocities.len(), 12);
        assert!(notes.iter().all(|n| (48..84).contains(&n.pitch.midi())));
    }

    #[test]
    fn notes_never_run_past_the_end() {
        for together in [1, 2, 3, 4, 6] {
            let writing = RowWriting {
                together,
                durations: true,
                register: Register::Wide,
                ..RowWriting::default()
            };
            let notes = write_row(suite(), &writing, 30);
            assert!(notes.iter().all(|n| n.end() <= 30));
            assert!(!notes.is_empty());
        }
    }
}
