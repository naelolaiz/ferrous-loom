//! Comping and bass lines: how a progression is *played*.
//!
//! The same four chords can be a ballad, a reggae tune or a house track.
//! What changes is the rhythm the chords are played in (jazz musicians
//! call this "comping", short for accompanying) and what the bass does.
//! Each style here is a short table of *hits*: when a note starts, how
//! long it lasts, which notes of the chord it plays, and how hard.

use crate::harmony::Chord;
use crate::pattern::NoteEvent;
use crate::pitch::Note;

use super::voicing::bass_root;

/// Which notes of a chord voicing a hit plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Voices {
    /// The whole chord.
    All,
    /// Every note but the lowest: the lighter "up-strokes" of a strum.
    Upper,
    /// One note, counted from the bottom. Counting past the top goes on
    /// in the next octave, so on a three-note chord, 3 is the bottom
    /// note an octave higher: arpeggios climb instead of jumping back.
    Nth(usize),
}

/// One note or chord in a comping or bass pattern.
#[derive(Debug, Clone, Copy)]
struct Hit<T> {
    /// When it starts, in steps from the start of the pattern's period.
    at: usize,
    /// How long it lasts, in steps.
    len: usize,
    what: T,
    velocity: u8,
}

/// A shorter way to write a [`Hit`] in the tables below.
///
/// A macro, not a function: the tables are borrowed as `&'static`
/// slices, and Rust only makes a borrowed value live forever (it
/// "promotes" it) when it is written out as plain values, not as the
/// result of a function call.
macro_rules! hit {
    ($at:expr, $len:expr, $what:expr, $velocity:expr) => {
        Hit {
            at: $at,
            len: $len,
            what: $what,
            velocity: $velocity,
        }
    };
}

/// The ways of playing chords.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Comping {
    /// Each chord held for as long as it lasts: pads and slow ballads.
    Held,
    /// Steady eighth notes: rock and pop piano or guitar.
    Pulse,
    /// Short chords on every beat, the tango's "marcato".
    Quarters,
    /// Short chords between the beats: the reggae and ska "skank".
    Offbeat,
    /// The jazz "Charleston": on beat 1, then just before beat 3.
    Charleston,
    /// Syncopated piano stabs, as in house music.
    Stabs,
    /// A strummed guitar: down, down-up, up-down-up.
    Strum,
    /// The bossa nova guitar's two-bar syncopation.
    Bossa,
    /// 3 + 3 + 2: the tresillo of Cuban music, everywhere in pop now.
    Tresillo,
    /// 3 + 3 + 2 in every half bar: the reggaeton "dembow".
    Dembow,
    /// Chords chopped into sixteenths: the trance "gate".
    Gate,
    /// Short, syncopated sixteenth-note stabs.
    Funk,
    /// On beat 1, then again just before beat 3, held: lazy lo-fi chords.
    Lazy,
    /// Long-short pairs, the way blues and boogie players swing.
    Shuffle,
    /// Gospel piano: chords on the beats with quick pushes between them.
    Gospel,
    /// A ballad piano: the chord, then its notes one by one.
    Ballad,
    /// Up the chord in eighth notes.
    ArpUp,
    /// Up and down the chord in eighth notes.
    ArpUpDown,
    /// Up and down the chord in sixteenth notes, as synths do.
    Arp16,
    /// Low, high, middle, high: the classical "Alberti bass".
    Alberti,
    /// A salsa piano montuno: a syncopated, repeating figure.
    Montuno,
    /// The flamenco rumba strum, with its quick double up-strokes.
    Rumba,
    /// For 3/4: chords on beats 2 and 3, after the bass on beat 1.
    Waltz,
    /// For 3/4: one chord on beat 2, held to the end of the bar.
    Sway,
    /// For 3/4 and 6/8: up and down the chord in eighth notes, one bar
    /// at a time.
    Arp3,
    /// Long, short, long, long: the Cuban habanera, grandmother of the tango.
    Habanera,
}

impl Comping {
    /// How many steps the pattern lasts before it repeats, and its hits.
    fn pattern(self) -> (usize, &'static [Hit<Voices>]) {
        use Voices::*;
        match self {
            // Held is handled on its own: it lasts as long as the chord.
            Comping::Held => (16, &[hit!(0, 16, All, 88)]),
            Comping::Pulse => (4, &[hit!(0, 2, All, 96), hit!(2, 2, All, 78)]),
            Comping::Quarters => (4, &[hit!(0, 2, All, 100)]),
            Comping::Offbeat => (4, &[hit!(2, 1, All, 92)]),
            Comping::Charleston => (16, &[hit!(0, 3, All, 96), hit!(6, 2, All, 86)]),
            Comping::Stabs => (
                16,
                &[
                    hit!(0, 2, All, 100),
                    hit!(3, 2, All, 88),
                    hit!(6, 3, All, 96),
                    hit!(10, 2, All, 88),
                    hit!(12, 3, All, 94),
                ],
            ),
            Comping::Strum => (
                16,
                &[
                    hit!(0, 4, All, 100),
                    hit!(4, 2, All, 90),
                    hit!(6, 4, Upper, 74),
                    hit!(10, 2, Upper, 74),
                    hit!(12, 2, All, 90),
                    hit!(14, 2, Upper, 74),
                ],
            ),
            Comping::Bossa => (
                32,
                &[
                    hit!(0, 3, All, 92),
                    hit!(3, 3, All, 80),
                    hit!(6, 4, All, 88),
                    hit!(10, 3, All, 80),
                    hit!(13, 3, All, 86),
                    hit!(18, 4, All, 84),
                    hit!(22, 3, All, 80),
                    hit!(26, 3, All, 86),
                    hit!(29, 3, All, 80),
                ],
            ),
            Comping::Tresillo => (
                16,
                &[
                    hit!(0, 3, All, 100),
                    hit!(6, 3, All, 90),
                    hit!(12, 3, All, 94),
                ],
            ),
            Comping::Dembow => (
                8,
                &[
                    hit!(0, 3, All, 96),
                    hit!(3, 3, All, 84),
                    hit!(6, 2, All, 90),
                ],
            ),
            Comping::Gate => (
                16,
                &[
                    hit!(0, 1, All, 104),
                    hit!(1, 1, All, 80),
                    hit!(3, 1, All, 92),
                    hit!(4, 1, All, 98),
                    hit!(6, 1, All, 92),
                    hit!(7, 1, All, 80),
                    hit!(9, 1, All, 92),
                    hit!(10, 1, All, 98),
                    hit!(12, 1, All, 92),
                    hit!(14, 1, All, 92),
                ],
            ),
            Comping::Funk => (
                16,
                &[
                    hit!(0, 1, All, 106),
                    hit!(3, 1, Upper, 80),
                    hit!(6, 1, All, 96),
                    hit!(7, 1, Upper, 72),
                    hit!(10, 1, All, 96),
                    hit!(12, 1, Upper, 82),
                    hit!(14, 1, All, 92),
                ],
            ),
            Comping::Lazy => (16, &[hit!(0, 6, All, 84), hit!(7, 9, All, 72)]),
            Comping::Shuffle => (4, &[hit!(0, 3, All, 94), hit!(3, 1, Upper, 70)]),
            Comping::Gospel => (
                16,
                &[
                    hit!(0, 3, All, 100),
                    hit!(3, 1, Upper, 70),
                    hit!(4, 3, All, 90),
                    hit!(7, 1, Upper, 70),
                    hit!(8, 3, All, 96),
                    hit!(11, 1, Upper, 70),
                    hit!(12, 2, All, 90),
                    hit!(14, 2, Upper, 76),
                ],
            ),
            Comping::Ballad => (
                16,
                &[
                    hit!(0, 4, All, 90),
                    hit!(4, 2, Nth(1), 70),
                    hit!(6, 2, Nth(2), 72),
                    hit!(8, 4, All, 84),
                    hit!(12, 2, Nth(1), 70),
                    hit!(14, 2, Nth(2), 72),
                ],
            ),
            Comping::ArpUp => (
                8,
                &[
                    hit!(0, 2, Nth(0), 92),
                    hit!(2, 2, Nth(1), 80),
                    hit!(4, 2, Nth(2), 84),
                    hit!(6, 2, Nth(3), 80),
                ],
            ),
            Comping::ArpUpDown => (
                16,
                &[
                    hit!(0, 2, Nth(0), 92),
                    hit!(2, 2, Nth(1), 78),
                    hit!(4, 2, Nth(2), 84),
                    hit!(6, 2, Nth(3), 78),
                    hit!(8, 2, Nth(4), 88),
                    hit!(10, 2, Nth(3), 78),
                    hit!(12, 2, Nth(2), 84),
                    hit!(14, 2, Nth(1), 78),
                ],
            ),
            Comping::Arp16 => (
                8,
                &[
                    hit!(0, 1, Nth(0), 96),
                    hit!(1, 1, Nth(1), 78),
                    hit!(2, 1, Nth(2), 86),
                    hit!(3, 1, Nth(3), 78),
                    hit!(4, 1, Nth(4), 90),
                    hit!(5, 1, Nth(3), 78),
                    hit!(6, 1, Nth(2), 86),
                    hit!(7, 1, Nth(1), 78),
                ],
            ),
            Comping::Alberti => (
                8,
                &[
                    hit!(0, 2, Nth(0), 88),
                    hit!(2, 2, Nth(2), 74),
                    hit!(4, 2, Nth(1), 80),
                    hit!(6, 2, Nth(2), 74),
                ],
            ),
            Comping::Montuno => (
                16,
                &[
                    hit!(0, 2, Nth(0), 96),
                    hit!(3, 1, Nth(2), 82),
                    hit!(4, 2, Nth(1), 88),
                    hit!(6, 2, Nth(3), 90),
                    hit!(8, 2, Nth(2), 84),
                    hit!(10, 1, Nth(1), 80),
                    hit!(11, 2, Nth(3), 90),
                    hit!(14, 2, Nth(2), 86),
                ],
            ),
            Comping::Rumba => (
                16,
                &[
                    hit!(0, 2, All, 106),
                    hit!(2, 1, Upper, 70),
                    hit!(3, 1, Upper, 70),
                    hit!(4, 2, All, 94),
                    hit!(6, 2, Upper, 76),
                    hit!(8, 2, All, 100),
                    hit!(10, 1, Upper, 70),
                    hit!(11, 1, Upper, 70),
                    hit!(12, 2, All, 94),
                    hit!(14, 2, Upper, 76),
                ],
            ),
            Comping::Waltz => (12, &[hit!(4, 3, All, 80), hit!(8, 3, All, 74)]),
            Comping::Sway => (12, &[hit!(4, 8, All, 76)]),
            Comping::Habanera => (
                8,
                &[
                    hit!(0, 3, All, 96),
                    hit!(3, 1, Upper, 74),
                    hit!(4, 2, All, 86),
                    hit!(6, 2, All, 84),
                ],
            ),
            Comping::Arp3 => (
                12,
                &[
                    hit!(0, 2, Nth(0), 86),
                    hit!(2, 2, Nth(1), 72),
                    hit!(4, 2, Nth(2), 78),
                    hit!(6, 2, Nth(3), 74),
                    hit!(8, 2, Nth(2), 76),
                    hit!(10, 2, Nth(1), 72),
                ],
            ),
        }
    }

    /// The notes for a chord voiced as `voicing`, starting at step
    /// `start` and lasting `span` steps.
    pub fn play(self, voicing: &[Note], start: usize, span: usize) -> Vec<NoteEvent> {
        if voicing.is_empty() || span == 0 {
            return Vec::new();
        }
        if self == Comping::Held {
            return voicing
                .iter()
                .map(|&pitch| NoteEvent {
                    velocity: 88,
                    ..NoteEvent::new(pitch, start, span)
                })
                .collect();
        }
        let (period, hits) = self.pattern();
        let mut notes = Vec::new();
        for offset in (0..span).step_by(period) {
            for hit in hits {
                let at = offset + hit.at;
                if at >= span {
                    continue;
                }
                let length = hit.len.min(span - at);
                for pitch in pick(voicing, hit.what) {
                    notes.push(NoteEvent {
                        velocity: hit.velocity,
                        ..NoteEvent::new(pitch, start + at, length)
                    });
                }
            }
        }
        notes
    }
}

/// The notes of `voicing` that `voices` asks for.
fn pick(voicing: &[Note], voices: Voices) -> Vec<Note> {
    match voices {
        Voices::All => voicing.to_vec(),
        Voices::Upper => voicing.iter().skip(1).copied().collect(),
        Voices::Nth(index) => {
            let octave = (index / voicing.len()) as i32;
            voicing[index % voicing.len()]
                .transpose(12 * octave)
                .into_iter()
                .collect()
        }
    }
}

/// A note of the bass line, chosen from the chord.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tone {
    Root,
    /// The fifth above the root.
    Fifth,
    /// The fifth below the root: the low note of "oom-pah".
    LowFifth,
    Octave,
    Third,
    /// The chord's seventh, or its sixth if it has no seventh.
    Seventh,
    /// The next chord's root, played early: an "anticipation".
    Next,
    /// One semitone from the next chord's root, leading into it.
    Approach,
}

/// The ways of playing the bass.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Bass {
    /// No bass line.
    None,
    /// The root, held for as long as the chord lasts.
    Held,
    /// The root in steady eighth notes: rock.
    Pulse,
    /// The root on every beat.
    Quarters,
    /// Root on beat 1, the fifth below on beat 3: country, polka, cumbia.
    RootFifth,
    /// Root and octave in eighth notes: disco.
    Octaves,
    /// The root between the beats: house and trance.
    Offbeat,
    /// A walking jazz line: a note on every beat, leading to the next chord.
    Walking,
    /// The boogie-woogie line up and down the chord: root, third, fifth, sixth, seventh.
    Boogie,
    /// Root and fifth with the bossa nova's pickups.
    Bossa,
    /// The salsa tumbao, which plays the next chord's root before it arrives.
    Tumbao,
    /// A reggae one-drop line.
    Reggae,
    /// The 3 + 3 + 2 of reggaeton.
    Dembow,
    /// Syncopated sixteenths with octave "pops".
    Funk,
    /// Long, deep notes that follow the kick, as an 808 does in trap.
    Sub,
    /// A lazy lo-fi line that leans into the next chord.
    Lazy,
    /// Root and octave in sixteenth notes: synthwave.
    Octaves16,
    /// 3 + 3 + 2 once a bar: the tango's "síncopa" and the milonga.
    Tresillo,
    /// For 3/4: the root on beat 1, and the fifth below it in the next bar.
    Waltz,
    /// The habanera's long, short, long, long.
    Habanera,
}

impl Bass {
    fn pattern(self) -> (usize, &'static [Hit<Tone>]) {
        use Tone::*;
        match self {
            Bass::None | Bass::Held | Bass::Walking => (16, &[]),
            Bass::Pulse => (4, &[hit!(0, 2, Root, 100), hit!(2, 2, Root, 84)]),
            Bass::Quarters => (4, &[hit!(0, 2, Root, 104)]),
            Bass::RootFifth => (16, &[hit!(0, 4, Root, 104), hit!(8, 4, LowFifth, 96)]),
            Bass::Octaves => (4, &[hit!(0, 2, Root, 104), hit!(2, 2, Octave, 92)]),
            Bass::Offbeat => (4, &[hit!(2, 2, Root, 104)]),
            Bass::Boogie => (
                16,
                &[
                    hit!(0, 2, Root, 104),
                    hit!(2, 2, Third, 90),
                    hit!(4, 2, Fifth, 96),
                    hit!(6, 2, Seventh, 90),
                    hit!(8, 2, Octave, 100),
                    hit!(10, 2, Seventh, 90),
                    hit!(12, 2, Fifth, 96),
                    hit!(14, 2, Third, 90),
                ],
            ),
            Bass::Bossa => (
                16,
                &[
                    hit!(0, 6, Root, 104),
                    hit!(6, 2, LowFifth, 86),
                    hit!(8, 6, LowFifth, 96),
                    hit!(14, 2, Root, 86),
                ],
            ),
            Bass::Tumbao => (16, &[hit!(6, 6, Fifth, 96), hit!(12, 4, Next, 104)]),
            Bass::Reggae => (
                16,
                &[
                    hit!(0, 4, Root, 104),
                    hit!(6, 2, Fifth, 86),
                    hit!(8, 2, Octave, 92),
                    hit!(10, 2, Fifth, 84),
                    hit!(12, 4, Root, 96),
                ],
            ),
            Bass::Dembow => (
                8,
                &[
                    hit!(0, 3, Root, 104),
                    hit!(3, 3, Root, 88),
                    hit!(6, 2, Fifth, 92),
                ],
            ),
            Bass::Funk => (
                16,
                &[
                    hit!(0, 2, Root, 110),
                    hit!(3, 1, Root, 80),
                    hit!(6, 1, Octave, 96),
                    hit!(7, 1, Root, 72),
                    hit!(10, 2, Root, 96),
                    hit!(12, 1, Octave, 92),
                    hit!(14, 1, Seventh, 86),
                ],
            ),
            Bass::Sub => (
                16,
                &[
                    hit!(0, 6, Root, 110),
                    hit!(7, 2, Root, 96),
                    hit!(10, 6, Root, 104),
                ],
            ),
            Bass::Lazy => (
                16,
                &[
                    hit!(0, 7, Root, 96),
                    hit!(10, 4, Fifth, 80),
                    hit!(14, 2, Approach, 76),
                ],
            ),
            Bass::Octaves16 => (2, &[hit!(0, 1, Root, 100), hit!(1, 1, Octave, 82)]),
            Bass::Tresillo => (
                16,
                &[
                    hit!(0, 5, Root, 106),
                    hit!(6, 5, Root, 92),
                    hit!(12, 4, Fifth, 96),
                ],
            ),
            Bass::Waltz => (24, &[hit!(0, 4, Root, 100), hit!(12, 4, LowFifth, 90)]),
            Bass::Habanera => (
                8,
                &[
                    hit!(0, 3, Root, 104),
                    hit!(3, 1, Fifth, 84),
                    hit!(4, 2, Third, 92),
                    hit!(6, 2, Fifth, 90),
                ],
            ),
        }
    }

    /// The bass notes for `chord`, from step `start` for `span` steps.
    /// `next` is the chord after it, which some lines lead into.
    pub fn play(self, chord: Chord, next: Chord, start: usize, span: usize) -> Vec<NoteEvent> {
        let note = |tone: Tone, at: usize, length: usize, velocity: u8| NoteEvent {
            velocity,
            ..NoteEvent::new(tone_note(tone, chord, next), start + at, length)
        };
        match self {
            Bass::None => Vec::new(),
            Bass::Held => vec![note(Tone::Root, 0, span, 100)],
            Bass::Walking => walk(span)
                .into_iter()
                .map(|(at, tone)| note(tone, at, 4.min(span - at), if at == 0 { 104 } else { 92 }))
                .collect(),
            _ => {
                let (period, hits) = self.pattern();
                let mut notes = Vec::new();
                for offset in (0..span).step_by(period) {
                    // The next chord is only anticipated in the last period
                    // of this one; before that, it is this chord's root.
                    let last = offset + period >= span;
                    for hit in hits {
                        let at = offset + hit.at;
                        if at >= span {
                            continue;
                        }
                        let tone = match hit.what {
                            Tone::Next | Tone::Approach if !last => Tone::Root,
                            tone => tone,
                        };
                        notes.push(note(tone, at, hit.len.min(span - at), hit.velocity));
                    }
                }
                notes
            }
        }
    }
}

/// The beats of a walking bass line over `span` steps: the root first,
/// a note leading into the next chord last, and chord notes between.
fn walk(span: usize) -> Vec<(usize, Tone)> {
    const MIDDLE: [Tone; 6] = [
        Tone::Third,
        Tone::Fifth,
        Tone::Seventh,
        Tone::Octave,
        Tone::Fifth,
        Tone::Third,
    ];
    let beats = span.div_ceil(4);
    (0..beats)
        .map(|beat| {
            let tone = if beat == 0 {
                Tone::Root
            } else if beat == beats - 1 {
                Tone::Approach
            } else {
                MIDDLE[(beat - 1) % MIDDLE.len()]
            };
            (beat * 4, tone)
        })
        .collect()
}

/// The note a bass tone stands for.
fn tone_note(tone: Tone, chord: Chord, next: Chord) -> Note {
    let root = bass_root(chord);
    let intervals = chord.quality.intervals();
    let has = |interval: u8| intervals.contains(&interval);
    let up = |semitones: u8| Note::new(root.midi() + semitones);
    // The chord's own fifth: flat in a diminished chord (Bdim: F, not
    // F#), sharp in an augmented one.
    let fifth = if has(6) {
        6
    } else if has(8) {
        8
    } else {
        7
    };
    match tone {
        Tone::Root => root,
        Tone::Fifth => up(fifth),
        // The same fifth, an octave lower: under the root.
        Tone::LowFifth => Note::new(root.midi().saturating_sub(12 - fifth)),
        Tone::Octave => up(12),
        Tone::Third => up(if has(3) {
            3
        } else if has(4) {
            4
        } else if has(5) {
            5
        } else {
            2
        }),
        // A chord's own seventh or sixth comes first: the double-flat
        // seventh of a diminished seventh chord, the sixth of a minor
        // sixth chord. Only a plain minor chord borrows the flat seventh.
        Tone::Seventh => up(if has(10) {
            10
        } else if has(11) {
            11
        } else if has(9) {
            9
        } else if has(3) {
            10
        } else {
            9
        }),
        Tone::Next => nearest_root(root, next),
        Tone::Approach => {
            let target = nearest_root(root, next);
            if target > root {
                Note::new(target.midi() - 1)
            } else {
                Note::new(target.midi() + 1)
            }
        }
    }
}

/// The next chord's root in the octave nearest to `from`.
fn nearest_root(from: Note, next: Chord) -> Note {
    let low = bass_root(next);
    let high = Note::new(low.midi() + 12);
    let below = Note::new(low.midi().saturating_sub(12));
    [below, low, high]
        .into_iter()
        .min_by_key(|note| note.midi().abs_diff(from.midi()))
        .unwrap_or(low)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harmony::ChordQuality::*;
    use crate::pitch::PitchClass::*;

    fn c_major() -> Vec<Note> {
        vec![Note::new(60), Note::new(64), Note::new(67)]
    }

    fn starts(notes: &[NoteEvent]) -> Vec<usize> {
        let mut starts: Vec<usize> = notes.iter().map(|note| note.start).collect();
        starts.dedup();
        starts
    }

    #[test]
    fn held_chords_last_the_whole_span() {
        let notes = Comping::Held.play(&c_major(), 16, 32);
        assert_eq!(notes.len(), 3);
        assert!(
            notes
                .iter()
                .all(|note| note.start == 16 && note.length == 32)
        );
    }

    #[test]
    fn patterns_repeat_and_stop_at_the_end_of_the_chord() {
        let notes = Comping::Charleston.play(&c_major(), 0, 24);
        // Beat 1 and the "and" of 2 in the first bar, then only beat 1
        // and the "and" of 2 of the half bar that is left.
        assert_eq!(starts(&notes), [0, 6, 16, 22]);
        let last = notes.last().unwrap();
        assert_eq!(last.start + last.length, 24);
    }

    #[test]
    fn arpeggios_climb_into_the_next_octave() {
        let notes = Comping::ArpUp.play(&c_major(), 0, 8);
        let pitches: Vec<u8> = notes.iter().map(|note| note.pitch.midi()).collect();
        assert_eq!(pitches, [60, 64, 67, 72]);
    }

    #[test]
    fn a_walking_line_leads_into_the_next_chord() {
        let c = Chord::new(C, Dominant7);
        let f = Chord::new(F, Dominant7);
        let notes = Bass::Walking.play(c, f, 0, 16);
        let pitches: Vec<u8> = notes.iter().map(|note| note.pitch.midi()).collect();
        // C2, E2, G2, then E2 a semitone under F2.
        assert_eq!(pitches, [36, 40, 43, 40]);
    }

    #[test]
    fn the_tumbao_anticipates_only_at_the_end() {
        let c = Chord::new(C, Major);
        let g = Chord::new(G, Major);
        let notes = Bass::Tumbao.play(c, g, 0, 32);
        let pitches: Vec<u8> = notes.iter().map(|note| note.pitch.midi()).collect();
        // Fifth, root; fifth, then G (the next root, nearest to C2: G1).
        assert_eq!(pitches, [43, 36, 43, 31]);
    }

    #[test]
    fn the_low_fifth_is_the_chords_own_fifth() {
        // Bm7b5: the fifth under B is F, not F#.
        let b = Chord::new(B, HalfDiminished7);
        let notes = Bass::RootFifth.play(b, b, 0, 16);
        let pitches: Vec<u8> = notes.iter().map(|note| note.pitch.midi()).collect();
        assert_eq!(pitches, [47, 41]);
        // C: G under C.
        let c = Chord::new(C, Major);
        let notes = Bass::RootFifth.play(c, c, 0, 16);
        let pitches: Vec<u8> = notes.iter().map(|note| note.pitch.midi()).collect();
        assert_eq!(pitches, [36, 31]);
    }

    #[test]
    fn the_seventh_is_one_the_chord_has() {
        let seventh =
            |chord: Chord| tone_note(Tone::Seventh, chord, chord).midi() - bass_root(chord).midi();
        // Am6 has F#, not G.
        assert_eq!(seventh(Chord::new(A, Minor6)), 9);
        // Cdim7 has its double-flat seventh, Bbb.
        assert_eq!(seventh(Chord::new(C, Diminished7)), 9);
        // A plain minor chord borrows the flat seventh.
        assert_eq!(seventh(Chord::new(A, Minor)), 10);
        assert_eq!(seventh(Chord::new(C, Major7)), 11);
    }

    #[test]
    fn boogie_walks_up_the_chord() {
        let c = Chord::new(C, Dominant7);
        let notes = Bass::Boogie.play(c, c, 0, 16);
        let pitches: Vec<u8> = notes.iter().map(|note| note.pitch.midi()).collect();
        assert_eq!(pitches, [36, 40, 43, 46, 48, 46, 43, 40]);
    }
}
