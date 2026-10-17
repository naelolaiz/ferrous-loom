//! Listening to a pattern: which chords its notes are playing.
//!
//! To play the same song in a new rhythm, we first have to know its
//! chords. A pattern does not store them, only notes, so this module
//! works them out the way a musician does by ear: it listens to the
//! notes that sound in each stretch of time and asks which chord they
//! fit best.
//!
//! Musicians and music software call this **chord recognition**. The
//! method here is the classic one, in two parts:
//!
//! 1. **Scoring.** For a stretch of the pattern, each possible chord (12
//!    roots times every quality) gets a score: points for the time its
//!    notes are heard, a heavier loss for the time other notes are
//!    heard, a small loss for each of its notes that is never played,
//!    and a bonus when the bass plays its root.
//! 2. **Cutting.** The pattern is cut into stretches at beat lines, in
//!    the way that gives the highest total score. Each cut costs a
//!    little, so a chord is only split in two when that really fits the
//!    notes better. Finding the best cuts is a short piece of *dynamic
//!    programming*: the best way to cut the first `i` beats is the best
//!    way to cut the first `j`, plus one stretch from `j` to `i`, for
//!    the best `j`.
//!
//! Because a whole stretch is scored at once, an arpeggio counts as its
//! chord even though its notes are heard one at a time.

use crate::harmony::{Chord, ChordQuality, PitchSet};
use crate::meter::TimeSignature;
use crate::pattern::NoteEvent;
use crate::pitch::PitchClass;

/// One stretch of a pattern and the chord it plays, if any.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HeardChord {
    /// `None` where no chord is heard, such as a bar of silence.
    pub chord: Option<Chord>,
    pub start: usize,
    pub length: usize,
}

/// How much a note heard outside the chord counts against it, compared
/// with a note of the chord.
const OUTSIDE: f32 = 2.0;
/// The loss for each note of the chord that is never heard, per step
/// that some note of the chord channel sounds.
const MISSING: f32 = 0.25;
/// The loss for each note the chord has, per step that some note
/// sounds. Without it, a big chord such as a thirteenth, which holds
/// almost every note, would always win.
const SIZE: f32 = 0.2;
/// The loss for each step a note of the chord is not heard while the
/// chord channel plays. Arpeggios leave out most notes most of the
/// time, so this is small; it is what tells "C, then C7" from two bars
/// of C7.
const GAP: f32 = 0.25;
/// The bonus when the bass plays the chord's root as the stretch starts.
/// The root then counts as heard, too, even if the chord channel leaves
/// it out. The rest of the bass line is not listened to: it plays
/// passing notes that belong to no chord.
const BASS_ROOT: f32 = 1.0;
/// The loss for each step of total silence (no chord, no bass) inside a
/// chord. A rest of a beat stays part of the chord around it; a bar or
/// more of silence is heard as no chord.
const SILENT: f32 = 0.25;
/// The cost of each cut between two stretches, on a bar line. Chords
/// mostly change on bar lines, less often halfway through a bar, and
/// least often on the other beats, so cuts there cost more.
const CUT: f32 = 2.0;
const CUT_HALF_BAR: f32 = 2.5;
const CUT_BEAT: f32 = 3.0;

/// What sounds at each step, added up so that what sounds between two
/// steps is one subtraction: `sums[s][pc]` is how many of the steps
/// before `s` the pitch class `pc` sounds at in the chord channel,
/// `busy[s]` how many steps before `s` have any chord note, and `quiet[s]`
/// how many have no note at all, not even in the bass.
struct Heard {
    sums: Vec<[f32; 12]>,
    busy: Vec<f32>,
    quiet: Vec<f32>,
    /// The pitch class of the lowest bass note that starts at each step,
    /// if any.
    bass: Vec<Option<PitchClass>>,
}

/// What sounds in one stretch.
struct Stretch {
    heard: [f32; 12],
    busy: f32,
    quiet: f32,
    bass: Option<PitchClass>,
}

impl Heard {
    fn new(chords: &[NoteEvent], bass: &[NoteEvent], length: usize) -> Self {
        let mut sounding = vec![PitchSet::EMPTY; length];
        for note in chords {
            let end = (note.start + note.length).min(length);
            for step in sounding.iter_mut().take(end).skip(note.start) {
                step.insert(note.pitch.pitch_class());
            }
        }
        let mut starting: Vec<Option<crate::Note>> = vec![None; length];
        let mut sounding_bass = vec![false; length];
        for note in bass {
            let end = (note.start + note.length).min(length);
            if let Some(step) = starting.get_mut(note.start) {
                *step = Some(step.map_or(note.pitch, |low| low.min(note.pitch)));
            }
            for step in sounding_bass.iter_mut().take(end).skip(note.start) {
                *step = true;
            }
        }
        let mut heard = Heard {
            sums: vec![[0.0; 12]],
            busy: vec![0.0],
            quiet: vec![0.0],
            bass: starting
                .iter()
                .map(|note| note.map(|n| n.pitch_class()))
                .collect(),
        };
        for (set, &low) in sounding.iter().zip(&sounding_bass) {
            let mut next = heard.sums[heard.sums.len() - 1];
            for pitch_class in set.iter() {
                next[pitch_class.semitone() as usize] += 1.0;
            }
            heard.sums.push(next);
            let busy = heard.busy[heard.busy.len() - 1];
            heard
                .busy
                .push(busy + if set.is_empty() { 0.0 } else { 1.0 });
            let quiet = heard.quiet[heard.quiet.len() - 1];
            let silent = set.is_empty() && !low;
            heard.quiet.push(quiet + if silent { 1.0 } else { 0.0 });
        }
        heard
    }

    /// What sounds from step `start` to step `end`.
    fn between(&self, start: usize, end: usize) -> Stretch {
        let mut heard = [0.0; 12];
        for (pc, time) in heard.iter_mut().enumerate() {
            *time = self.sums[end][pc] - self.sums[start][pc];
        }
        Stretch {
            heard,
            busy: self.busy[end] - self.busy[start],
            quiet: self.quiet[end] - self.quiet[start],
            bass: self.bass[start],
        }
    }
}

/// Every chord there is: twelve roots times every quality.
fn every_chord() -> Vec<(Chord, PitchSet)> {
    PitchClass::ALL
        .into_iter()
        .flat_map(|root| {
            ChordQuality::ALL.into_iter().map(move |quality| {
                let chord = Chord::new(root, quality);
                (chord, chord.pitch_set())
            })
        })
        .collect()
}

/// The chord that fits a stretch best, and its score. `None` is "no
/// chord", which scores as if every note heard were outside it.
fn best_chord(chords: &[(Chord, PitchSet)], stretch: &Stretch) -> (Option<Chord>, f32) {
    let total: f32 = stretch.heard.iter().sum();
    let mut best = (None, -OUTSIDE * total);
    for &(chord, set) in chords {
        let mut inside = 0.0;
        let mut missing = 0.0;
        let mut gaps = 0.0;
        let mut size = 0.0;
        for pitch_class in set.iter() {
            let time = stretch.heard[pitch_class.semitone() as usize];
            inside += time;
            // A root only the bass plays costs nothing for not being in
            // the chord channel: pianists leave it to the bass. Neither
            // does the fifth of a chord of four notes or more, which
            // pianists leave out too, as it adds little colour (see
            // `voicing`).
            let from_bass = time == 0.0 && stretch.bass == Some(pitch_class);
            let fifth = time == 0.0 && set.len() >= 4 && chord.root.transpose(7) == pitch_class;
            if !from_bass {
                size += 1.0;
                if !fifth {
                    gaps += (stretch.busy - time).max(0.0);
                    if time == 0.0 {
                        missing += 1.0;
                    }
                }
            }
        }
        let outside = total - inside;
        let mut score = inside
            - OUTSIDE * outside
            - stretch.busy * (MISSING * missing + SIZE * size)
            - GAP * gaps
            - SILENT * stretch.quiet;
        if stretch.bass == Some(chord.root) {
            score += BASS_ROOT;
        }
        if score > best.1 {
            best = (Some(chord), score);
        }
    }
    best
}

/// The chords played by `chords` (the notes of a chord channel) over a
/// bass line `bass`, in a pattern `length` steps long whose bars are in
/// `time`. Chords change only on beat lines. Stretches next to each other
/// with the same chord are joined, so each change starts a new entry.
pub fn hear_chords(
    chords: &[NoteEvent],
    bass: &[NoteEvent],
    length: usize,
    time: TimeSignature,
) -> Vec<HeardChord> {
    if length == 0 {
        return Vec::new();
    }
    let heard = Heard::new(chords, bass, length);
    let candidates = every_chord();
    let beat = time.steps_per_beat().max(1);
    let bar = time.steps_per_bar().max(1);
    let lines: Vec<usize> = (0..length).step_by(beat).chain([length]).collect();

    // best[i]: the best score for the beats before line i, and where its
    // last stretch starts and what it plays.
    let mut best: Vec<(f32, usize, Option<Chord>)> = vec![(0.0, 0, None); lines.len()];
    for i in 1..lines.len() {
        best[i].0 = f32::NEG_INFINITY;
        for j in 0..i {
            let (chord, score) = best_chord(&candidates, &heard.between(lines[j], lines[i]));
            let cut = if lines[j].is_multiple_of(bar) {
                CUT
            } else if (lines[j] * 2).is_multiple_of(bar) {
                CUT_HALF_BAR
            } else {
                CUT_BEAT
            };
            let total = best[j].0 + score - cut;
            if total > best[i].0 {
                best[i] = (total, j, chord);
            }
        }
    }

    // Walk back from the end to read the cuts, then join neighbours that
    // play the same chord.
    let mut stretches = Vec::new();
    let mut i = lines.len() - 1;
    while i > 0 {
        let (_, j, chord) = best[i];
        stretches.push(HeardChord {
            chord,
            start: lines[j],
            length: lines[i] - lines[j],
        });
        i = j;
    }
    stretches.reverse();
    let half_bar = |step: usize| (step * 2).is_multiple_of(bar);
    let mut joined: Vec<HeardChord> = Vec::new();
    for stretch in stretches {
        match joined.last_mut() {
            Some(last) if last.chord == stretch.chord => last.length += stretch.length,
            // An arpeggio shows its notes one by one: a C that turns into
            // C7 a beat later is one C7 still unfolding. A change on a
            // bar line or half way through the bar is taken as meant.
            Some(last) if !half_bar(stretch.start) && grows(last.chord, stretch.chord) => {
                last.chord = bigger(last.chord, stretch.chord);
                last.length += stretch.length;
            }
            _ => joined.push(stretch),
        }
    }
    joined
}

/// Whether two chords have the same root, and one holds all the notes
/// of the other.
fn grows(a: Option<Chord>, b: Option<Chord>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => {
            a.root == b.root
                && (a.pitch_set().is_subset_of(b.pitch_set())
                    || b.pitch_set().is_subset_of(a.pitch_set()))
        }
        _ => false,
    }
}

/// The one of two chords with more notes.
fn bigger(a: Option<Chord>, b: Option<Chord>) -> Option<Chord> {
    let size = |chord: Option<Chord>| chord.map_or(0, |c| c.pitch_set().len());
    if size(b) > size(a) { b } else { a }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harmony::ChordQuality::*;
    use crate::library::STYLES;
    use crate::pitch::PitchClass::*;

    fn heard(style_name: &str) -> Vec<(Option<Chord>, usize, usize)> {
        let style = crate::library::Style::named(style_name).unwrap();
        let backing = style.render(C);
        hear_chords(&backing.chords, &backing.bass, style.length(), style.time)
            .iter()
            .map(|h| (h.chord, h.start, h.length))
            .collect()
    }

    #[test]
    fn held_chords_are_heard() {
        let c = Some(Chord::new(C, Major));
        let g = Some(Chord::new(G, Major));
        let a = Some(Chord::new(A, Minor));
        let f = Some(Chord::new(F, Major));
        assert_eq!(
            heard("Four chords"),
            [(c, 0, 16), (g, 16, 16), (a, 32, 16), (f, 48, 16)]
        );
    }

    #[test]
    fn silence_is_no_chord() {
        assert_eq!(
            hear_chords(&[], &[], 16, TimeSignature::COMMON),
            [HeardChord {
                chord: None,
                start: 0,
                length: 16
            }]
        );
        assert!(hear_chords(&[], &[], 0, TimeSignature::COMMON).is_empty());
    }

    #[test]
    fn a_seventh_chord_with_its_fifth_left_out() {
        // C, E and Bb with C in the bass: C7, as players often voice it.
        let note = |midi, len| NoteEvent {
            velocity: 90,
            ..NoteEvent::new(crate::Note::new(midi), 0, len)
        };
        let chords = [note(64, 16), note(70, 16), note(72, 16)];
        let bass = [note(36, 16)];
        let heard = hear_chords(&chords, &bass, 16, TimeSignature::COMMON);
        assert_eq!(heard[0].chord, Some(Chord::new(C, Dominant7)));
    }

    /// The test that matters: every style in the library, played in its
    /// own way (arpeggios, stabs, waltzes...), is heard as the chords it
    /// was written with.
    #[test]
    fn every_style_is_heard_as_its_chart() {
        let mut wrong = Vec::new();
        for style in STYLES {
            let backing = style.render(C);
            let heard: Vec<(PitchSet, usize, usize)> =
                hear_chords(&backing.chords, &backing.bass, style.length(), style.time)
                    .iter()
                    .map(|h| {
                        (
                            h.chord.map_or(PitchSet::EMPTY, Chord::pitch_set),
                            h.start,
                            h.length,
                        )
                    })
                    .collect();
            let mut written: Vec<(PitchSet, usize, usize)> = Vec::new();
            for span in style.spans() {
                let set = span.chord.in_key(C).pitch_set();
                match written.last_mut() {
                    Some(last) if last.0 == set => last.2 += span.length,
                    _ => written.push((set, span.start, span.length)),
                }
            }
            if heard != written {
                wrong.push(style.name);
            }
        }
        assert!(wrong.is_empty(), "heard wrongly: {wrong:?}");
    }
}
