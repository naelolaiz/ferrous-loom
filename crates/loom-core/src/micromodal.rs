//! Writing music from one micromode, the way Kröpfl taught it.
//!
//! A passage built on one micromode uses nothing but that group of three
//! notes, moved and turned upside down: Menor 3 on C, then on D, then
//! upside down on F#... Every chord and every step of the melody has the
//! same intervals, so the passage has one colour, however far it goes.
//!
//! Two choices make music of it:
//!
//! - The **walk**: which form of the micromode comes next ([`Walk`]).
//!   Keeping two notes and moving one, so that each group grows out of
//!   the last, gives smooth harmony that still reaches all twelve notes
//!   in time. Moving every note the same distance gives a sequence.
//! - The **texture**: how an instrument plays each form ([`Texture`]): as
//!   a chord, a melody, an arpeggio or a bass line.
//!
//! [`walk`] makes the list of forms, and [`write_forms`] writes them into
//! a pattern. All the instruments of a style play the same walk, each in
//! its own texture, so they always agree.

use crate::harmony::{Micromode, PitchSet};
use crate::pattern::NoteEvent;
use crate::pitch::{Note, PitchClass};
use crate::serial::{Register, above, in_octave, nearest};

/// Which form of the micromode comes after each one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Walk {
    /// Keep two notes and move one: the next form shares two notes with
    /// this one. Of the forms that do, it takes the one whose new note
    /// has not been heard for longest, so the walk goes round all twelve
    /// notes, then the one whose new note moves least. For the major and
    /// minor triad these moves are the three of neo-Riemannian theory:
    /// C major to C minor, to E minor, or to A minor.
    ///
    /// Three micromodes cannot get out of a smaller collection this way:
    /// Mayor 1 and Mayor 3 stay in one whole-tone scale, and the
    /// diminished triad in one diminished seventh chord, because every
    /// form that shares two notes with them is inside it too. The
    /// augmented triad shares no two notes with another, so it moves a
    /// semitone instead.
    CommonTones,
    /// Move every note up this many semitones: a sequence.
    Transpose(u8),
}

impl Walk {
    pub fn name(self) -> String {
        match self {
            Walk::CommonTones => "Keep two notes, move one".to_string(),
            Walk::Transpose(1) => "Up a semitone each time".to_string(),
            Walk::Transpose(11) => "Down a semitone each time".to_string(),
            Walk::Transpose(n) => format!("Up {n} semitones each time"),
        }
    }
}

/// `count` forms of `micromode`, starting with `first` (which should be
/// one of its forms), each following the one before as `walk` says.
pub fn walk(micromode: &Micromode, first: PitchSet, walk: Walk, count: usize) -> Vec<PitchSet> {
    let members = micromode.members();
    let mut forms = Vec::with_capacity(count);
    // When each pitch class was last heard, as the number of the form.
    let mut heard: [Option<usize>; 12] = [None; 12];
    let mut current = first;
    for index in 0..count {
        forms.push(current);
        if forms.len() == count {
            break;
        }
        for pitch_class in current.iter() {
            heard[usize::from(pitch_class.semitone())] = Some(index);
        }
        current = match walk {
            Walk::Transpose(n) => current.transpose(n % 12),
            Walk::CommonTones => next_by_common_tones(&members, current, &heard, &forms),
        };
    }
    forms
}

/// The form after `current` in a [`Walk::CommonTones`]: see there.
fn next_by_common_tones(
    members: &[PitchSet],
    current: PitchSet,
    heard: &[Option<usize>; 12],
    before: &[PitchSet],
) -> PitchSet {
    let shared = |set: PitchSet| PitchSet::from_bits(set.bits() & current.bits()).len();
    let most = members
        .iter()
        .filter(|&&set| set != current)
        .map(|&set| shared(set))
        .max()
        .unwrap_or(0);
    if most == 0 {
        return current.transpose(1);
    }
    let gone: Vec<PitchClass> = current.iter().collect();
    members
        .iter()
        .copied()
        .filter(|&set| set != current && shared(set) == most)
        .min_by_key(|&set| {
            let new = PitchSet::from_bits(set.bits() & !current.bits());
            // The new note heard most recently decides; never heard is best.
            let freshest = new
                .iter()
                .map(|p| heard[usize::from(p.semitone())].map_or(0, |i| i + 1))
                .max()
                .unwrap_or(0);
            // How far the moving notes go, each to its nearest new note.
            let moves: u8 = new
                .iter()
                .map(|p| {
                    gone.iter()
                        .filter(|g| !set.contains(**g))
                        .map(|g| {
                            let up = g.semitones_up_to(p);
                            up.min(12 - up)
                        })
                        .min()
                        .unwrap_or(0)
                })
                .sum();
            // Then the form itself heard least recently, so a walk shut in
            // a few notes still goes round all its forms.
            let used = before.iter().rposition(|&f| f == set).map_or(0, |i| i + 1);
            (freshest, used, moves, set.bits())
        })
        .unwrap_or(current)
}

/// How an instrument plays each form.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Texture {
    /// The three notes together, held for the whole form. The notes
    /// two forms share stay where they are, and the one that changes
    /// moves to the nearest note: smooth voice leading.
    Chords,
    /// The same chords, struck again every this many steps.
    Pulse(usize),
    /// The notes one after another, each this many steps long, the new
    /// note first: a melody in which every step is one of the
    /// micromode's intervals.
    Line(usize),
    /// The notes of the chord one after another, up and down, each this
    /// many steps long.
    Arpeggio(usize),
    /// One note of each form, held, low down: the root of a chord
    /// micromode, otherwise the first note of the normal form.
    Bass,
}

/// How one instrument writes a walk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MicroWriting {
    pub texture: Texture,
    /// For a line: in which octaves the notes go (see [`Register`]).
    /// Chords and arpeggios are always voiced smoothly, a bass in one
    /// octave.
    pub register: Register,
    /// The lowest note it writes from.
    pub low: Note,
    pub velocity: u8,
}

impl Default for MicroWriting {
    fn default() -> Self {
        MicroWriting {
            texture: Texture::Chords,
            register: Register::Nearest,
            low: Note::new(55),
            velocity: crate::DEFAULT_VELOCITY,
        }
    }
}

/// The notes of `form` voiced as a chord, as close as can be to
/// `before`: the notes they share stay, the others move to the nearest
/// note of the new form left over. With nothing before, the chord is in
/// close position from `low`.
fn voice(form: PitchSet, before: &[Note], low: Note) -> Vec<Note> {
    let normal = form.normal_form();
    if before.is_empty() {
        let mut chord: Vec<Note> = Vec::new();
        for &pitch_class in &normal {
            let note = match chord.last() {
                Some(&below) => above(pitch_class, below),
                None => in_octave(pitch_class, low, 0),
            };
            chord.push(note);
        }
        return chord;
    }
    let mut left: Vec<PitchClass> = normal
        .iter()
        .copied()
        .filter(|p| !before.iter().any(|n| n.pitch_class() == *p))
        .collect();
    let mut chord = Vec::new();
    for &note in before {
        if form.contains(note.pitch_class())
            && !chord
                .iter()
                .any(|n: &Note| n.pitch_class() == note.pitch_class())
        {
            chord.push(note);
        } else if !left.is_empty() {
            // The nearest of the notes still to place.
            let (i, moved) = left
                .iter()
                .enumerate()
                .map(|(i, &p)| (i, nearest(p, note, low)))
                .min_by_key(|(_, n)| (i32::from(n.midi()) - i32::from(note.midi())).abs())
                .expect("left is not empty");
            left.remove(i);
            chord.push(moved);
        }
    }
    // A form with fewer shared notes than voices before it.
    for p in left {
        let last = chord.last().copied().unwrap_or(low);
        chord.push(nearest(p, last, low));
    }
    chord.sort();
    chord.dedup();
    chord
}

/// Writes `forms` into a pattern `length` steps long, each form
/// lasting `steps` steps, the way `writing` says. The forms start again
/// from the first if the pattern is longer than they are.
pub fn write_forms(
    forms: &[PitchSet],
    steps: usize,
    writing: &MicroWriting,
    length: usize,
) -> Vec<NoteEvent> {
    let steps = steps.max(1);
    let mut notes = Vec::new();
    if forms.is_empty() {
        return notes;
    }
    let event = |pitch: Note, start: usize, lasts: usize| NoteEvent {
        velocity: writing.velocity,
        ..NoteEvent::new(pitch, start, lasts.min(length - start).max(1))
    };
    let mut chord: Vec<Note> = Vec::new();
    let mut last: Option<Note> = None;
    let mut bass: Option<Note> = None;
    let mut place = 0;
    let mut start = 0;
    let mut previous: Option<PitchSet> = None;
    while start < length {
        let form = forms[(start / steps) % forms.len()];
        let end = (start + steps).min(length);
        chord = voice(form, &chord, writing.low);
        match writing.texture {
            Texture::Chords => {
                for &pitch in &chord {
                    notes.push(event(pitch, start, end - start));
                }
            }
            Texture::Pulse(every) => {
                let every = every.max(1);
                for at in (start..end).step_by(every) {
                    for &pitch in &chord {
                        notes.push(event(pitch, at, every.min(end - at)));
                    }
                }
            }
            Texture::Line(each) => {
                // The new notes first, then the others, low to high.
                let order: Vec<PitchClass> = {
                    let normal = form.normal_form();
                    let old = previous.unwrap_or_default();
                    let (new, kept): (Vec<PitchClass>, Vec<PitchClass>) =
                        normal.into_iter().partition(|p| !old.contains(*p));
                    new.into_iter().chain(kept).collect()
                };
                let each = each.max(1);
                for (i, at) in (start..end).step_by(each).enumerate() {
                    let pitch_class = order[i % order.len()];
                    let pitch = match (writing.register, last) {
                        (Register::Wide, _) => {
                            in_octave(pitch_class, writing.low, [0, 2, 1][place % 3])
                        }
                        (Register::Close, _) | (_, None) => in_octave(pitch_class, writing.low, 0),
                        (Register::Nearest, Some(last)) => nearest(pitch_class, last, writing.low),
                    };
                    place += 1;
                    last = Some(pitch);
                    notes.push(event(pitch, at, each.min(end - at)));
                }
            }
            Texture::Arpeggio(each) => {
                // Up and down: 0 1 2 1, and round again.
                let shape: Vec<usize> = match chord.len() {
                    0 => Vec::new(),
                    1 => vec![0],
                    n => (0..n).chain((1..n - 1).rev()).collect(),
                };
                let each = each.max(1);
                for (i, at) in (start..end).step_by(each).enumerate() {
                    if let Some(&index) = shape.get(i % shape.len().max(1)) {
                        notes.push(event(chord[index], at, each.min(end - at)));
                    }
                }
            }
            Texture::Bass => {
                let pitch_class = bass_note(form);
                let pitch = match bass {
                    Some(before) => nearest(pitch_class, before, writing.low),
                    None => in_octave(pitch_class, writing.low, 0),
                };
                // A bass stays within an octave and a half of its lowest note.
                let pitch = if pitch.midi() >= writing.low.midi().saturating_add(15) {
                    pitch.transpose(-12).unwrap_or(pitch)
                } else {
                    pitch
                };
                bass = Some(pitch);
                notes.push(event(pitch, start, end - start));
            }
        }
        previous = Some(form);
        start = end;
    }
    notes
}

/// The note a bass plays for `form`: the root of a triad, or else the
/// first note of the normal form, where the group starts.
pub fn bass_note(form: PitchSet) -> PitchClass {
    let chord = Micromode::of(form).and_then(|m| m.chord(form));
    match chord {
        Some(chord) => chord.root,
        None => form.normal_form().first().copied().unwrap_or(PitchClass::C),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harmony::Micromode;

    fn set(semitones: &[u8]) -> PitchSet {
        PitchSet::from_semitones(semitones)
    }

    #[test]
    fn a_common_tone_walk_keeps_two_notes_and_goes_round() {
        for micromode in Micromode::all() {
            let first = micromode.prime;
            let forms = walk(micromode, first, Walk::CommonTones, 24);
            let mut heard = PitchSet::default();
            for pair in forms.windows(2) {
                assert_eq!(
                    Micromode::of(pair[1]),
                    Some(micromode),
                    "{}",
                    micromode.name
                );
                assert_ne!(pair[0], pair[1]);
                let shared = PitchSet::from_bits(pair[0].bits() & pair[1].bits()).len();
                if micromode.number == 12 {
                    // Augmented triads share no notes: a semitone up.
                    assert_eq!(pair[1], pair[0].transpose(1));
                } else {
                    assert_eq!(shared, 2, "{} {:?}", micromode.name, pair);
                }
            }
            for form in &forms {
                for p in form.iter() {
                    heard.insert(p);
                }
            }
            let reach = match micromode.number {
                // One whole-tone scale.
                6 | 8 => 6,
                // One diminished seventh chord.
                10 => 4,
                _ => 12,
            };
            assert_eq!(heard.len(), reach, "{}", micromode.name);
            // It goes round all the forms it can reach before repeating one.
            let distinct: std::collections::HashSet<u16> =
                forms.iter().take(4).map(|f| f.bits()).collect();
            assert_eq!(distinct.len(), 4, "{}", micromode.name);
        }
    }

    #[test]
    fn triads_walk_by_neo_riemannian_moves() {
        let triads = Micromode::number(11).unwrap();
        let c_minor = set(&[0, 3, 7]);
        let forms = walk(triads, c_minor, Walk::CommonTones, 8);
        for pair in forms.windows(2) {
            // P, L or R: the other quality, sharing two notes.
            let (a, b) = (
                triads.chord(pair[0]).unwrap(),
                triads.chord(pair[1]).unwrap(),
            );
            assert_ne!(a.quality, b.quality, "{a:?} {b:?}");
        }
    }

    #[test]
    fn a_sequence_moves_every_note() {
        let mayor_1 = Micromode::number(6).unwrap();
        let forms = walk(mayor_1, mayor_1.prime, Walk::Transpose(2), 6);
        assert_eq!(forms[1], set(&[2, 4, 6]));
        // By whole tones, Mayor 1 stays in one whole-tone scale.
        let all: PitchSet = forms.iter().flat_map(|f| f.iter()).collect();
        assert_eq!(all, set(&[0, 2, 4, 6, 8, 10]));
    }

    #[test]
    fn chords_keep_their_shared_notes_in_place() {
        let menor_3 = Micromode::number(3).unwrap();
        let forms = walk(menor_3, menor_3.prime, Walk::CommonTones, 8);
        let writing = MicroWriting::default();
        let notes = write_forms(&forms, 4, &writing, 32);
        assert_eq!(notes.len(), 24);
        for (k, form) in forms.iter().enumerate() {
            let chord: Vec<&NoteEvent> = notes.iter().filter(|n| n.start == 4 * k).collect();
            let heard: PitchSet = chord.iter().map(|n| n.pitch.pitch_class()).collect();
            assert_eq!(heard, *form);
            if k > 0 {
                let before: Vec<Note> = notes
                    .iter()
                    .filter(|n| n.start == 4 * (k - 1))
                    .map(|n| n.pitch)
                    .collect();
                // The two shared notes are the very same keys.
                let kept = chord.iter().filter(|n| before.contains(&n.pitch)).count();
                assert_eq!(kept, 2, "form {k}");
            }
        }
    }

    #[test]
    fn every_texture_fills_the_pattern_with_the_forms() {
        let micromode = Micromode::number(5).unwrap();
        let forms = walk(micromode, micromode.prime, Walk::CommonTones, 4);
        for texture in [
            Texture::Chords,
            Texture::Pulse(2),
            Texture::Line(2),
            Texture::Arpeggio(1),
            Texture::Bass,
        ] {
            for register in Register::ALL {
                let writing = MicroWriting {
                    texture,
                    register,
                    low: Note::new(48),
                    ..MicroWriting::default()
                };
                let notes = write_forms(&forms, 8, &writing, 30);
                assert!(!notes.is_empty());
                for note in &notes {
                    assert!(note.end() <= 30, "{texture:?}");
                    let form = forms[note.start / 8];
                    assert!(form.contains(note.pitch.pitch_class()), "{texture:?}");
                    assert!(note.pitch.midi() >= 48 && note.pitch.midi() < 48 + 36);
                }
            }
        }
    }

    #[test]
    fn the_bass_plays_the_root_of_a_triad() {
        assert_eq!(bass_note(set(&[9, 0, 4])), PitchClass::A);
        assert_eq!(bass_note(set(&[11, 2, 5])), PitchClass::B);
        assert_eq!(bass_note(set(&[0, 1, 4])), PitchClass::C);
        assert_eq!(bass_note(set(&[11, 0, 3])), PitchClass::B);
    }
}
