//! Lead sheets as text: chords by the bar, and melodies note by note.
//!
//! A **lead sheet** is how working musicians write a song down: the
//! melody, with chord names above it, bar by bar. The library writes the
//! same two things as short strings, so a whole song fits in a few lines
//! and can be read (and checked) by a person:
//!
//! ```text
//! chords:  "I | V | vi | IV V"
//! melody:  "E4:4 E4 F4 G4 | G4 F4 E4 D4 | ..."
//! ```
//!
//! In a chord chart, `|` separates bars, and the chords inside a bar
//! share it equally: "IV V" is half a bar each. In a melody, each note is
//! a name with its octave and, after a colon, its length in steps.

use crate::harmony::Chord;
use crate::pattern::{DEFAULT_VELOCITY, NoteEvent};
use crate::pitch::{Note, PitchClass};

use super::comping::{Bass, Comping};
use super::numeral::{self, RomanChord};
use super::voicing::voice_progression;

/// One chord of a chart, with when it starts and how long it lasts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChordSpan {
    pub chord: RomanChord,
    pub start: usize,
    pub length: usize,
}

/// Reads a chord chart: bars separated by `|`, each with one or more
/// chords that share the bar equally. A chord that repeats the one
/// before it is held on, not played again: "I | I | IV | V" is two bars
/// of I. Returns `None` if a chord cannot be read, a bar is empty, or a
/// bar of `bar` steps cannot be shared equally between its chords. An
/// empty chart has no chords.
pub fn parse_chart(text: &str, bar: usize) -> Option<Vec<ChordSpan>> {
    let mut spans: Vec<ChordSpan> = Vec::new();
    if text.trim().is_empty() {
        return Some(spans);
    }
    for (index, cell) in text.split('|').enumerate() {
        let chords = numeral::parse_progression(cell)?;
        if chords.is_empty() || !bar.is_multiple_of(chords.len()) {
            return None;
        }
        let each = bar / chords.len();
        for (i, chord) in chords.into_iter().enumerate() {
            let start = index * bar + i * each;
            match spans.last_mut() {
                Some(last) if last.chord == chord => last.length += each,
                _ => spans.push(ChordSpan {
                    chord,
                    start,
                    length: each,
                }),
            }
        }
    }
    Some(spans)
}

/// A melody read from text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Melody {
    pub notes: Vec<NoteEvent>,
    /// How long each bar was, as marked by `|`. The tests use this to
    /// check that every bar adds up.
    pub bars: Vec<usize>,
    /// How many steps the whole melody lasts, rests included.
    pub length: usize,
}

/// Reads a melody: notes separated by spaces, such as `"C4:4 E4:2 G4"`.
///
/// - A note is a letter, an optional `#` (sharp) or `b` (flat), and an
///   octave: `C4` is middle C, `F#5` the F sharp an octave and a half
///   above it.
/// - `:4` gives its length in steps (4 is a quarter note). Without it, a
///   note is as long as the one before.
/// - `-` is a rest: `-:8` is half a bar of silence in 4/4.
/// - `_` is a tie: the note before it goes on for that long, so a note
///   can be held across a bar line: `D5:12 | _:8`.
/// - `|` is a bar line. It changes nothing, but the bars it marks are
///   kept, so a test can check that each one has the right length.
///
/// Returns `None` if anything cannot be read.
pub fn parse_melody(text: &str) -> Option<Melody> {
    let mut notes: Vec<NoteEvent> = Vec::new();
    let mut bars = Vec::new();
    let mut at = 0;
    let mut bar_start = 0;
    let mut length = 4;
    for token in text.split_whitespace() {
        if token == "|" {
            bars.push(at - bar_start);
            bar_start = at;
            continue;
        }
        let name = match token.split_once(':') {
            Some((name, steps)) => {
                length = steps.parse().ok().filter(|&steps: &usize| steps > 0)?;
                name
            }
            None => token,
        };
        if name == "_" {
            // A tie: the last note goes on, across a bar line.
            notes.last_mut()?.length += length;
        } else if name != "-" {
            notes.push(NoteEvent {
                velocity: DEFAULT_VELOCITY,
                ..NoteEvent::new(note_named(name)?, at, length)
            });
        }
        at += length;
    }
    if at > bar_start {
        bars.push(at - bar_start);
    }
    Some(Melody {
        notes,
        bars,
        length: at,
    })
}

/// The note with a name such as "C4", "F#5" or "Bb3".
pub fn note_named(name: &str) -> Option<Note> {
    let mut chars = name.chars();
    let letter = match chars.next()? {
        'C' => PitchClass::C,
        'D' => PitchClass::D,
        'E' => PitchClass::E,
        'F' => PitchClass::F,
        'G' => PitchClass::G,
        'A' => PitchClass::A,
        'B' => PitchClass::B,
        _ => return None,
    };
    let rest = chars.as_str();
    let (shift, octave) = if let Some(octave) = rest.strip_prefix('#') {
        (1, octave)
    } else if let Some(octave) = rest.strip_prefix('b') {
        (-1, octave)
    } else {
        (0, rest)
    };
    let octave: i32 = octave.parse().ok()?;
    let midi = (octave + 1) * 12 + i32::from(letter.semitone()) + shift;
    u8::try_from(midi)
        .ok()
        .filter(|&midi| midi <= 127)
        .map(Note::new)
}

/// The notes a progression is accompanied with: chords and a bass line.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Backing {
    pub chords: Vec<NoteEvent>,
    pub bass: Vec<NoteEvent>,
}

/// Plays a chart in the key whose tonic is `tonic`: voices each chord
/// smoothly from the last, plays the voicings in the rhythm of
/// `comping`, and the roots in the rhythm of `bass`.
///
/// The bass line may lead into the next chord. After the last chord
/// comes the first one again, because patterns loop.
pub fn accompany(spans: &[ChordSpan], tonic: PitchClass, comping: Comping, bass: Bass) -> Backing {
    let chords: Vec<Chord> = spans.iter().map(|span| span.chord.in_key(tonic)).collect();
    let voicings = voice_progression(&chords);
    let mut backing = Backing::default();
    for (index, span) in spans.iter().enumerate() {
        let next = chords[(index + 1) % chords.len()];
        backing
            .chords
            .extend(comping.play(&voicings[index], span.start, span.length));
        backing
            .bass
            .extend(bass.play(chords[index], next, span.start, span.length));
    }
    backing
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harmony::ChordQuality;

    fn spans(text: &str, bar: usize) -> Vec<(u8, usize, usize)> {
        parse_chart(text, bar)
            .unwrap_or_else(|| panic!("{text} did not parse"))
            .iter()
            .map(|span| (span.chord.semitones, span.start, span.length))
            .collect()
    }

    #[test]
    fn chords_share_their_bar() {
        assert_eq!(
            spans("I | IV V | vi", 16),
            [(0, 0, 16), (5, 16, 8), (7, 24, 8), (9, 32, 16)]
        );
        // Three chords in a bar of 3/4: one per beat.
        assert_eq!(spans("I IV V", 12), [(0, 0, 4), (5, 4, 4), (7, 8, 4)]);
    }

    #[test]
    fn repeated_chords_are_held() {
        assert_eq!(
            spans("I | I | IV | I I", 16),
            [(0, 0, 32), (5, 32, 16), (0, 48, 16)]
        );
        // The same root with another quality is a new chord.
        assert_eq!(spans("I | I7", 16), [(0, 0, 16), (0, 16, 16)]);
    }

    #[test]
    fn bad_charts_are_refused() {
        assert_eq!(parse_chart("I | | V", 16), None, "an empty bar");
        assert_eq!(parse_chart("I IV V", 16), None, "16 steps in three");
        assert_eq!(parse_chart("I | X", 16), None);
        assert_eq!(parse_chart("  ", 16), Some(Vec::new()));
    }

    #[test]
    fn melodies_are_notes_and_lengths() {
        let melody = parse_melody("C4:4 E4:2 G4 | -:8 C5:8").unwrap();
        let notes: Vec<(u8, usize, usize)> = melody
            .notes
            .iter()
            .map(|note| (note.pitch.midi(), note.start, note.length))
            .collect();
        assert_eq!(notes, [(60, 0, 4), (64, 4, 2), (67, 6, 2), (72, 16, 8)]);
        assert_eq!(melody.bars, [8, 16]);
        assert_eq!(melody.length, 24);
    }

    #[test]
    fn ties_hold_a_note_across_the_bar() {
        let melody = parse_melody("D5:12 | _:8 B4:4").unwrap();
        let notes: Vec<(usize, usize)> = melody.notes.iter().map(|n| (n.start, n.length)).collect();
        assert_eq!(notes, [(0, 20), (20, 4)]);
        assert_eq!(melody.bars, [12, 12]);
        assert_eq!(parse_melody("_:4"), None, "nothing to tie");
    }

    #[test]
    fn note_names() {
        let midi = |name| note_named(name).map(Note::midi);
        assert_eq!(midi("C4"), Some(60));
        assert_eq!(midi("A4"), Some(69));
        assert_eq!(midi("F#5"), Some(78));
        assert_eq!(midi("Bb3"), Some(58));
        assert_eq!(midi("C-1"), Some(0));
        assert_eq!(midi("H4"), None);
        assert_eq!(midi("C"), None);
        assert_eq!(midi("G9"), Some(127));
        assert_eq!(midi("G#9"), None);
        assert_eq!(parse_melody("C4:0"), None);
        assert_eq!(parse_melody("C4:x"), None);
    }

    #[test]
    fn a_chart_is_accompanied_in_any_key() {
        let chart = parse_chart("I | V", 16).unwrap();
        let backing = accompany(&chart, PitchClass::D, Comping::Held, Bass::Held);
        let bass: Vec<u8> = backing.bass.iter().map(|note| note.pitch.midi()).collect();
        assert_eq!(bass, [38, 45], "D2 and A2");
        assert_eq!(backing.chords.len(), 6, "two triads");
        assert!(backing.chords.iter().all(|note| note.length == 16));
        assert_eq!(chart[1].chord.quality, ChordQuality::Major);
    }
}
