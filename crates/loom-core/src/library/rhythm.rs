//! Rhythms: drum patterns written one line per drum.
//!
//! Drummers and drum machines think of a beat as a grid: one row per
//! drum, one column per step. The library writes exactly that, as text:
//!
//! ```text
//! Kick   "X... .... X... ...."
//! Snare  ".... X... .... X..."
//! Hi-hat "x.x. x.x. x.x. x.x."
//! ```
//!
//! Each character is one step. `X` is a loud hit, `x` a normal one, `o`
//! a soft "ghost" note, and `.` silence. `h` and `l` play the drum five
//! semitones higher or lower, for the open and closed sounds of congas
//! and toms. Spaces only make the line easier to read, and `|` separates
//! bars when a rhythm is longer than one.

use crate::meter::TimeSignature;
use crate::pattern::{NoteEvent, STEP_NOTE};
use crate::project::Drum;

use super::Genre;

/// One drum pattern.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rhythm {
    pub name: &'static str,
    pub genre: Genre,
    pub time: TimeSignature,
    /// The tempo it is usually played at, in BPM.
    pub tempo: f32,
    /// One line per drum.
    pub lines: &'static [(Drum, &'static str)],
    /// The name of a fill in [`FILLS`] that ends a section, or "".
    pub fill: &'static str,
    pub about: &'static str,
}

/// One hit: how hard, and how many semitones up or down.
type Hit = (u8, i32);

/// Reads one character of a drum line.
fn hit(character: char) -> Option<Option<Hit>> {
    match character {
        '.' => Some(None),
        'X' => Some(Some((120, 0))),
        'x' => Some(Some((96, 0))),
        'o' => Some(Some((56, 0))),
        'h' => Some(Some((96, 5))),
        'l' => Some(Some((96, -5))),
        _ => None,
    }
}

/// Reads a drum line into bars of steps. Returns `None` if a character
/// is not one of the ones above.
pub fn parse_line(line: &str) -> Option<Vec<Vec<Option<Hit>>>> {
    line.split('|')
        .map(|bar| {
            bar.chars()
                .filter(|character| !character.is_whitespace())
                .map(hit)
                .collect()
        })
        .collect()
}

impl Rhythm {
    /// How many bars the rhythm lasts before it repeats.
    pub fn bars(&self) -> usize {
        self.lines
            .first()
            .map_or(1, |(_, line)| line.split('|').count())
    }

    /// How many steps the rhythm lasts before it repeats.
    pub fn length(&self) -> usize {
        self.bars() * self.time.steps_per_bar()
    }

    /// The drums the rhythm plays.
    pub fn drums(&self) -> impl Iterator<Item = Drum> + '_ {
        self.lines.iter().map(|(drum, _)| *drum)
    }

    /// The notes `drum` plays over `length` steps of a pattern whose bars
    /// last `bar` steps, repeating the rhythm bar by bar.
    ///
    /// The rhythm is laid out a bar at a time, so it keeps its place in
    /// the bar even when the pattern's bars are another length: a 4/4
    /// beat in 3/4 loses its last beat in every bar instead of drifting
    /// across the bar lines.
    pub fn notes(&self, drum: Drum, bar: usize, length: usize) -> Vec<NoteEvent> {
        let Some((_, line)) = self.lines.iter().find(|(d, _)| *d == drum) else {
            return Vec::new();
        };
        let bars = parse_line(line).unwrap_or_default();
        if bars.is_empty() || bar == 0 {
            return Vec::new();
        }
        let mut notes = Vec::new();
        for start in (0..length).step_by(bar) {
            let steps = &bars[(start / bar) % bars.len()];
            for (step, hit) in steps.iter().enumerate().take(bar) {
                let at = start + step;
                if let (Some((velocity, shift)), true) = (hit, at < length) {
                    let pitch = STEP_NOTE.transpose(*shift).unwrap_or(STEP_NOTE);
                    notes.push(NoteEvent {
                        velocity: *velocity,
                        ..NoteEvent::new(pitch, at, 1)
                    });
                }
            }
        }
        notes
    }

    /// The rhythm called `name` in [`RHYTHMS`] or [`FILLS`].
    pub fn named(name: &str) -> Option<&'static Rhythm> {
        RHYTHMS
            .iter()
            .chain(FILLS)
            .find(|rhythm| rhythm.name == name)
    }

    /// The fill that goes with this rhythm, if any.
    pub fn fill(&self) -> Option<&'static Rhythm> {
        FILLS.iter().find(|fill| fill.name == self.fill)
    }
}

/// A shorter way to write a 4/4 rhythm in the tables below.
const fn rhythm(
    name: &'static str,
    genre: Genre,
    tempo: f32,
    lines: &'static [(Drum, &'static str)],
    fill: &'static str,
    about: &'static str,
) -> Rhythm {
    Rhythm {
        name,
        genre,
        time: TimeSignature::COMMON,
        tempo,
        lines,
        fill,
        about,
    }
}

impl Rhythm {
    const fn in_time(self, time: TimeSignature) -> Rhythm {
        Rhythm { time, ..self }
    }
}

const THREE_FOUR: TimeSignature = TimeSignature::new(3, 4);
const SIX_EIGHT: TimeSignature = TimeSignature::new(6, 8);
const TWELVE_EIGHT: TimeSignature = TimeSignature::new(12, 8);

use Drum::*;
use Genre::*;

/// The library of drum patterns, sorted by genre.
pub static RHYTHMS: &[Rhythm] = &[
    // ----- Pop -----
    rhythm(
        "Pop beat",
        Pop,
        110.0,
        &[
            (Kick, "X... .... X.x. ...."),
            (Snare, ".... X... .... X..."),
            (HiHat, "x.x. x.x. x.x. x.x."),
        ],
        "Snare roll",
        "The backbeat: kick on 1 and 3, snare on 2 and 4, hi-hat on every eighth note.",
    ),
    rhythm(
        "Pop ballad",
        Pop,
        72.0,
        &[
            (Kick, "X... .... ..x. ...."),
            (Rim, ".... x... .... x..."),
            (HiHat, "x.x. x.x. x.x. x.x."),
        ],
        "Tom fill",
        "A soft beat with a rim click in place of the snare.",
    ),
    rhythm(
        "Dance pop",
        Pop,
        122.0,
        &[
            (Kick, "X... X... X... X..."),
            (Clap, ".... X... .... X..."),
            (OpenHat, "..x. ..x. ..x. ..x."),
            (Shaker, "xoxo xoxo xoxo xoxo"),
        ],
        "House build",
        "A kick on every beat, a clap on 2 and 4, and an open hat between the beats.",
    ),
    rhythm(
        "Afrobeats",
        Pop,
        105.0,
        &[
            (Kick, "X... ..X. ..X. ...."),
            (Clap, ".... X... .... X..."),
            (Rim, "...x ..x. ...x ..x."),
            (Shaker, "x.xx x.xx x.xx x.xx"),
            (Conga, "..o. .o.. ..o. h.h."),
        ],
        "Latin fill",
        "A rolling West African groove: the kick plays 3 + 3 + 2 and the shaker never stops.",
    ),
    // ----- Rock -----
    rhythm(
        "Rock beat",
        Rock,
        120.0,
        &[
            (Kick, "X... ..x. X.x. ...."),
            (Snare, ".... X... .... X..."),
            (HiHat, "x.x. x.x. x.x. x.x."),
        ],
        "Tom fill",
        "The backbeat with an extra kick pushing into beat 3.",
    ),
    rhythm(
        "Half-time rock",
        Rock,
        140.0,
        &[
            (Kick, "X... .... ..x. ...."),
            (Snare, ".... .... X... ...."),
            (HiHat, "x.x. x.x. x.x. x.x."),
        ],
        "Tom fill",
        "The snare only on beat 3: the song feels half as fast as its tempo.",
    ),
    rhythm(
        "Punk",
        Rock,
        170.0,
        &[
            (Kick, "X... ..X. X... ..X."),
            (Snare, ".... X... .... X..."),
            (HiHat, "X.x. X.x. X.x. X.x."),
        ],
        "Snare roll",
        "The backbeat, fast and hard.",
    ),
    rhythm(
        "Rock ride",
        Rock,
        128.0,
        &[
            (Kick, "X... ..x. X... ...."),
            (Snare, ".... X... .... X..."),
            (Ride, "x.x. x.x. x.x. x.x."),
        ],
        "Tom fill",
        "The backbeat with the eighths on the ride cymbal, for a bigger, washier sound.",
    ),
    rhythm(
        "Surf beat",
        Rock,
        160.0,
        &[
            (Kick, "X... X... X... X..."),
            (Snare, ".... X... .... X..."),
            (LowTom, "..x. ..x. ..x. .xx."),
            (HiHat, "x.x. x.x. x.x. x.x."),
        ],
        "Tom fill",
        "A kick on every beat and rumbling toms: 1960s surf rock.",
    ),
    rhythm(
        "Rock and roll",
        Rock,
        168.0,
        &[
            (Kick, "X... ..x. X... ..x."),
            (Snare, ".... X... .... X..."),
            (Ride, "x..x x..x x..x x..x"),
        ],
        "Snare roll",
        "The 1950s backbeat, with a long-short ride cymbal halfway between swing and straight.",
    ),
    rhythm(
        "Metal",
        Rock,
        180.0,
        &[
            (Kick, "XxXx XxXx XxXx XxXx"),
            (Snare, ".... X... .... X..."),
            (Crash, "X... .... .... ...."),
            (Ride, "x... x... x... x..."),
        ],
        "Tom fill",
        "Two bass drums in sixteenths under a heavy backbeat.",
    ),
    // ----- Blues -----
    rhythm(
        "Shuffle",
        Blues,
        96.0,
        &[
            (Kick, "X... .... X... ...."),
            (Snare, ".... X... .... X..."),
            (HiHat, "x..x x..x x..x x..x"),
        ],
        "Snare roll",
        "Long-short pairs: each beat split unevenly, three sixteenths and one.",
    ),
    rhythm(
        "Slow blues",
        Blues,
        66.0,
        &[
            (Kick, "X..... ...... X..... ......"),
            (Snare, "...... X..... ...... X....."),
            (HiHat, "x.x.x. x.x.x. x.x.x. x.x.x."),
        ],
        "",
        "Twelve-eight: four slow beats, each split in three, for the blues ballad.",
    )
    .in_time(TWELVE_EIGHT),
    // ----- Jazz -----
    rhythm(
        "Swing",
        Jazz,
        140.0,
        &[
            (Ride, "x... x..x x... x..x"),
            (HiHat, ".... x... .... x..."),
            (Kick, "o... .... o... ...."),
        ],
        "",
        "Ding, ding-a, ding, ding-a on the ride, and the hi-hat pedal on 2 and 4.",
    ),
    rhythm(
        "Jazz ballad",
        Jazz,
        68.0,
        &[
            (Ride, "o... o..o o... o..o"),
            (Shaker, "o.o. o.o. o.o. o.o."),
            (HiHat, ".... x... .... x..."),
            (Kick, "o... .... .... ...."),
        ],
        "",
        "Brushes and a quiet ride: the drums only keep time.",
    ),
    rhythm(
        "Jazz waltz",
        Jazz,
        150.0,
        &[
            (Ride, "x... x..x x..x"),
            (HiHat, ".... x... x..."),
            (Kick, "o... .... ...."),
        ],
        "",
        "The swing ride pattern in three beats.",
    )
    .in_time(THREE_FOUR),
    rhythm(
        "Second line",
        Jazz,
        100.0,
        &[
            (Kick, "X... ..X. X... ..X."),
            (Snare, ".oxo X.o. .oxo X.ox"),
            (Cowbell, "x... x... x... x..."),
        ],
        "Snare roll",
        "New Orleans street parade drumming: a bass drum that answers itself under a busy snare.",
    ),
    // ----- Soul, funk and R&B -----
    rhythm(
        "Motown",
        Soul,
        112.0,
        &[
            (Kick, "X... ..x. X... ..x."),
            (Snare, "x... X... x... X..."),
            (Shaker, ".... X... .... X..."),
            (HiHat, "x.x. x.x. x.x. x.x."),
        ],
        "Snare roll",
        "The snare on all four beats and a tambourine on 2 and 4.",
    ),
    rhythm(
        "Funk",
        Soul,
        102.0,
        &[
            (Kick, "X..x ..X. ..x. ...."),
            (Snare, ".... X..o .o.. X..o"),
            (HiHat, "xoxo xoxo xoxo xoxo"),
        ],
        "Snare roll",
        "Sixteenths on the hi-hat, ghost notes on the snare, and a kick that never sits still.",
    ),
    rhythm(
        "Neo soul",
        Soul,
        84.0,
        &[
            (Kick, "X... ...x ..x. ...."),
            (Snare, ".... X... .... X..."),
            (HiHat, "x.xo x.x. x.xo x.x."),
        ],
        "Hip hop fill",
        "A laid-back backbeat with a kick that comes a little early.",
    ),
    rhythm(
        "R&B",
        Soul,
        90.0,
        &[
            (Kick, "X... ...x .x.. ...."),
            (Clap, ".... X... .... X..."),
            (HiHat, "x.x. x.x. x.xx x.x."),
        ],
        "Hip hop fill",
        "A clap on 2 and 4, a syncopated kick and a quick double hi-hat.",
    ),
    rhythm(
        "New jack swing",
        Soul,
        104.0,
        &[
            (Kick, "X..X ..X. X..X ..X."),
            (Snare, ".... X... .... X..."),
            (Clap, ".... X... .... X..."),
            (HiHat, "x.xx x.xx x.xx x.xx"),
        ],
        "Hip hop fill",
        "Late-1980s R&B: a hip hop kick and snare with a skipping hi-hat.",
    ),
    // ----- Hip hop -----
    rhythm(
        "Boom bap",
        HipHop,
        90.0,
        &[
            (Kick, "X... ...x ..X. ...."),
            (Snare, ".... X... .... X..."),
            (HiHat, "x.x. x.x. x.x. x.x."),
        ],
        "Hip hop fill",
        "The \"boom\" of the kick and the \"bap\" of the snare: 1990s hip hop.",
    ),
    rhythm(
        "Lo-fi",
        HipHop,
        75.0,
        &[
            (Kick, "X... .... ..X. ...."),
            (Snare, ".... X... .... X..."),
            (HiHat, "x.x. x.x. x.x. x.xo"),
        ],
        "Hip hop fill",
        "A slow, soft hip hop beat to study to.",
    ),
    rhythm(
        "Trap",
        HipHop,
        140.0,
        &[
            (Kick, "X... .... ..X. .... | .... ..X. .... ...."),
            (Clap, ".... .... X... .... | .... .... X... ...."),
            (HiHat, "x.x. x.x. xxx. x.x. | x.x. x.xx x.x. xxxx"),
        ],
        "",
        "Half-time: the clap only on beat 3, with quick rolls on the hi-hat.",
    ),
    rhythm(
        "Drill",
        HipHop,
        142.0,
        &[
            (Kick, "X... .... ..X. ..X."),
            (Snare, ".... .... X... ...."),
            (HiHat, "x..x ..x. x..x ..x."),
            (Rim, "...x .... ...x ...."),
        ],
        "",
        "Half-time, with a hi-hat in 3 + 3 + 2 and kicks that dodge the snare.",
    ),
    rhythm(
        "G-funk",
        HipHop,
        92.0,
        &[
            (Kick, "X... ..x. X.X. ...."),
            (Clap, ".... X... .... X..."),
            (HiHat, "x.x. x.x. x.x. x.xx"),
            (Shaker, "..x. ..x. ..x. ..x."),
        ],
        "Hip hop fill",
        "1990s West Coast hip hop: a laid-back, funky kick and a clap on 2 and 4.",
    ),
    // ----- Electronic -----
    rhythm(
        "Four on the floor",
        Electronic,
        124.0,
        &[
            (Kick, "X... X... X... X..."),
            (Clap, ".... X... .... X..."),
            (OpenHat, "..x. ..x. ..x. ..x."),
            (Shaker, "xoxo xoxo xoxo xoxo"),
        ],
        "House build",
        "A kick on every beat: the heartbeat of house and disco.",
    ),
    rhythm(
        "Techno",
        Electronic,
        130.0,
        &[
            (Kick, "X... X... X... X..."),
            (OpenHat, "..x. ..x. ..x. ..x."),
            (HiHat, "xoxo xoxo xoxo xoxo"),
            (Rim, "...x ..x. ...x .x.."),
        ],
        "House build",
        "A relentless kick with rim shots weaving between the beats.",
    ),
    rhythm(
        "Trance",
        Electronic,
        138.0,
        &[
            (Kick, "X... X... X... X..."),
            (Clap, ".... X... .... X..."),
            (OpenHat, "..x. ..x. ..x. ..x."),
            (HiHat, "xox. xox. xox. xox."),
        ],
        "House build",
        "Four on the floor, a little faster, built for long builds and drops.",
    ),
    rhythm(
        "Disco",
        Electronic,
        118.0,
        &[
            (Kick, "X... X... X... X..."),
            (Snare, ".... X... .... X..."),
            (HiHat, "x... x... x... x..."),
            (OpenHat, "..x. ..x. ..x. ..x."),
        ],
        "Tom fill",
        "Closed hat on the beat, open hat between: the 1970s dance floor.",
    ),
    rhythm(
        "Drum and bass",
        Electronic,
        174.0,
        &[
            (Kick, "X... .... ..X. ...."),
            (Snare, ".... X... .... X..."),
            (HiHat, "x.x. x.x. x.x. x.x."),
        ],
        "Snare roll",
        "A fast two-step: the kick on 1 and the \"and\" of 3.",
    ),
    rhythm(
        "Dubstep",
        Electronic,
        140.0,
        &[
            (Kick, "X... .... .... ..x."),
            (Snare, ".... .... X... ...."),
            (HiHat, "x.x. x.x. x.x. x.x."),
        ],
        "",
        "Half-time, heavy and slow-feeling, with a big snare on beat 3.",
    ),
    rhythm(
        "UK garage",
        Electronic,
        132.0,
        &[
            (Kick, "X... ...x ..X. ...."),
            (Snare, ".... X... .... X..."),
            (HiHat, "..x. ..x. ..x. ..x."),
            (Shaker, "xoxo xoxo xoxo xoxo"),
        ],
        "",
        "The skippy two-step of UK garage: a kick that dodges the beat.",
    ),
    rhythm(
        "Breakbeat",
        Electronic,
        130.0,
        &[
            (Kick, "X.x. .... ..X. ...."),
            (Snare, ".... X..o .o.. X..o"),
            (HiHat, "x.x. x.x. x.x. x.x."),
        ],
        "Snare roll",
        "A funky drum break, the raw material of hip hop and jungle.",
    ),
    rhythm(
        "Synthwave",
        Electronic,
        100.0,
        &[
            (Kick, "X... .... X... ...."),
            (Snare, ".... X... .... X..."),
            (Clap, ".... X... .... X..."),
            (HiHat, "x.x. x.x. x.x. x.x."),
        ],
        "Tom fill",
        "A big 1980s backbeat, with a clap on top of the snare.",
    ),
    rhythm(
        "Moombahton",
        Electronic,
        108.0,
        &[
            (Kick, "X... X... X... X..."),
            (Snare, "...X ..X. ...X ..X."),
            (OpenHat, "..x. ..x. ..x. ..x."),
        ],
        "House build",
        "House slowed down to reggaeton's tempo: four on the floor under a dembow snare.",
    ),
    rhythm(
        "Electro",
        Electronic,
        128.0,
        &[
            (Kick, "X... .... ..X. ...."),
            (Clap, ".... X... .... X..."),
            (HiHat, "xoxo xoxo xoxo xoxo"),
            (Cowbell, "..x. .... x... ..x."),
        ],
        "Snare roll",
        "The drum machines of early-1980s electro: a broken kick, sixteenths on the hat, a cowbell.",
    ),
    // ----- Latin -----
    rhythm(
        "Bossa nova",
        Latin,
        130.0,
        &[
            (Kick, "X..x X... X..x X... | X..x X... X..x X..."),
            (Rim, "x..x ..x. ..x. .x.. | ..x. ..x. .x.. x..."),
            (HiHat, "o.o. o.o. o.o. o.o. | o.o. o.o. o.o. o.o."),
        ],
        "Latin fill",
        "A quiet kick, soft eighths, and the rim playing the bossa clave across two bars.",
    ),
    rhythm(
        "Samba",
        Latin,
        100.0,
        &[
            (Kick, "X..x X..x X..x X..x"),
            (LowTom, ".... X... .... X..."),
            (Snare, "x.oo x.oo x.oo x.oo"),
            (Shaker, "Xoxx Xoxx Xoxx Xoxx"),
            (Cowbell, "x.x. ..x. x... x.h."),
        ],
        "Latin fill",
        "The low drum (surdo) strongest on beat 2, shakers and bells in sixteenths.",
    ),
    rhythm(
        "Salsa",
        Latin,
        95.0,
        &[
            (Rim, "x..x ..x. ..x. x..."),
            (Cowbell, "X.x. X.x. X.x. X.x."),
            (Conga, "o.x. o.hh o.x. o.hh"),
        ],
        "Latin fill",
        "The son clave (3 + 2) on the rim, the bell on the beats, and the conga's tumbao.",
    ),
    rhythm(
        "Cumbia",
        Latin,
        95.0,
        &[
            (Kick, "X... .... X... ...."),
            (Shaker, "x.xx x.xx x.xx x.xx"),
            (Conga, "..x. ..x. ..x. ..h."),
            (Rim, ".... x... .... x..."),
        ],
        "Latin fill",
        "The scraper's long-short-short, and congas between the beats.",
    ),
    rhythm(
        "Reggaeton",
        Latin,
        95.0,
        &[
            (Kick, "X... X... X... X..."),
            (Snare, "...x ..x. ...x ..x."),
            (HiHat, "x.x. x.x. x.x. x.x."),
        ],
        "",
        "The dembow: a kick on every beat and a snare in 3 + 3 + 2.",
    ),
    rhythm(
        "Bolero",
        Latin,
        80.0,
        &[
            (Kick, "X... .... X... ...."),
            (Rim, ".... x... .... x..."),
            (Conga, "..o. ..hh ..o. hh.."),
            (Shaker, "x.x. x.x. x.x. x.x."),
        ],
        "Latin fill",
        "The slow Latin love song: soft bongos and maracas, and the rim on 2 and 4.",
    ),
    rhythm(
        "Bachata",
        Latin,
        128.0,
        &[
            (Kick, "X... X... X... X..."),
            (Conga, "x.hh x.h. x.hh x.hl"),
            (Shaker, "xxxx xxxx xxxx xxxx"),
        ],
        "Latin fill",
        "From the Dominican Republic: bongos in a steady pattern and the güira scraping every \
         sixteenth.",
    ),
    rhythm(
        "Merengue",
        Latin,
        132.0,
        &[
            (Kick, "X... X... X... X..."),
            (LowTom, "X..x .x.x X..x .xxx"),
            (Shaker, "x.xx x.xx x.xx x.xx"),
        ],
        "Latin fill",
        "Fast and in two: the tambora drum's rolls and a scraper that never stops.",
    ),
    rhythm(
        "Cha-cha-chá",
        Latin,
        116.0,
        &[
            (Kick, "X... .... X... ...."),
            (Cowbell, "X... X... X... X..."),
            (Conga, ".... ..h. ..hh h..."),
            (Rim, ".... .... .... x.x."),
        ],
        "Latin fill",
        "Named after its sound: the two quick notes and one long, \"cha-cha-chá\", on beat 4 and \
         the next beat 1.",
    ),
    // ----- Tango -----
    rhythm(
        "Tango marcato",
        Tango,
        118.0,
        &[(Kick, "X... x... X... x..."), (Rim, ".... .... .... ..xx")],
        "",
        "Tango has no drums: the bass and piano mark every beat. A soft kick does it here, \
         with the \"drag\" into the next bar.",
    ),
    rhythm(
        "Tango síncopa",
        Tango,
        118.0,
        &[(Kick, "X... ..X. .... X..."), (Rim, ".... .... x... ....")],
        "",
        "Three plus three plus two: the accents of the síncopa.",
    ),
    rhythm(
        "Milonga",
        Tango,
        110.0,
        &[
            (Kick, "X... ..X. .... X..."),
            (Shaker, "x.x. x.x. x.x. x.x."),
            (Rim, ".... x... .... x..."),
        ],
        "",
        "3 + 3 + 2 in every bar, light and quick.",
    ),
    rhythm(
        "Habanera",
        Tango,
        76.0,
        &[(Kick, "X..x X.x. X..x X.x."), (Rim, ".... x... .... x...")],
        "",
        "Long, short, long, long, twice a bar.",
    ),
    rhythm(
        "Tango vals",
        Tango,
        180.0,
        &[(Kick, "X... .... ...."), (Rim, ".... x... x...")],
        "",
        "A fast waltz: a strong one and two light beats.",
    )
    .in_time(THREE_FOUR),
    rhythm(
        "Tango nuevo",
        Tango,
        132.0,
        &[
            (Kick, "X..X ..X. .X.. ...."),
            (Rim, ".... .... .... X.X."),
            (Shaker, "x.xx x.xx x.xx x.xx"),
        ],
        "",
        "3 + 3 + 3 + 3 + 2 + 2 in sixteenths: the restless accents of the new tango of the 1950s.",
    ),
    // ----- South American folklore -----
    rhythm(
        "Zamba",
        Folklore,
        84.0,
        &[(LowTom, "X..... X....."), (Rim, "....x. ..x.x.")],
        "",
        "The slow Argentine dance in 6/8: the bombo legüero's deep head on the two beats, \
         its wooden rim between them.",
    )
    .in_time(SIX_EIGHT),
    rhythm(
        "Chacarera",
        Folklore,
        132.0,
        &[(LowTom, "X..... x.x..."), (Rim, "....x. ..x.x.")],
        "",
        "Santiago del Estero's dance: the bombo plays 6/8 on the head and 3/4 on the rim at \
         the same time, the two against three that the whole genre lives on.",
    )
    .in_time(SIX_EIGHT),
    rhythm(
        "Gato",
        Folklore,
        126.0,
        &[
            (LowTom, "X..... X..x.."),
            (Rim, "..x.x. ..x..."),
            (Shaker, "x.x.x. x.x.x."),
        ],
        "",
        "A quick, bright 6/8 for the gato, the pericón, the cielito, the escondido and the \
         other old dances of the Pampas.",
    )
    .in_time(SIX_EIGHT),
    rhythm(
        "Malambo",
        Folklore,
        144.0,
        &[(LowTom, "X..x.. X..x.."), (Rim, "x.x.x. x.x.x.")],
        "",
        "The gaucho's solo dance: bombo and the dancer's boots, in 3 + 3 against the eighths.",
    )
    .in_time(SIX_EIGHT),
    rhythm(
        "Chamamé",
        Folklore,
        120.0,
        &[(Kick, "X...x. ..x..."), (HiHat, "X.x.x. X.x.x.")],
        "",
        "From Corrientes: the guitars' bass walks in 3/4 while the accordion plays 6/8, \
         three against two in every bar.",
    )
    .in_time(SIX_EIGHT),
    rhythm(
        "Cueca",
        Folklore,
        126.0,
        &[
            (Kick, "X..... X....."),
            (Clap, "..X.X. ..X.X."),
            (Shaker, "x.x.x. x.x.x."),
        ],
        "",
        "Chile's handkerchief dance: guitar on the two beats, hand claps on the eighths between.",
    )
    .in_time(SIX_EIGHT),
    rhythm(
        "Marinera",
        Folklore,
        120.0,
        &[
            (LowTom, "X..x.. X....."),
            (Snare, "....x. ..x.x."),
            (Shaker, "x.x.x. x.x.x."),
        ],
        "",
        "Peru's courtship dance on the cajón: a low tone, slaps that cross into 3/4, and \
         the quijada's rattle.",
    )
    .in_time(SIX_EIGHT),
    rhythm(
        "Bambuco",
        Folklore,
        108.0,
        &[
            (Rim, "X..... X....."),
            (Kick, "....X. ..X..."),
            (Shaker, "x.x.x. x.x.x."),
        ],
        "",
        "Colombia's bambuco: the tiple keeps 6/8 while the bass lands on the second and third \
         beats of 3/4, never on the downbeat.",
    )
    .in_time(SIX_EIGHT),
    rhythm(
        "Joropo",
        Folklore,
        176.0,
        &[
            (Kick, "X... ..X. ...."),
            (Shaker, "X.x. X.x. X.x."),
            (Rim, ".... x... x..."),
        ],
        "",
        "The Venezuelan and Colombian plains: maracas in 3/4, the bass in 6/8, and the cuatro \
         strumming across both.",
    )
    .in_time(THREE_FOUR),
    rhythm(
        "Vidala",
        Folklore,
        72.0,
        &[(Snare, "X... o... o.o.")],
        "",
        "The caja, a small hand drum, under a slow song of the Andean north-west of Argentina.",
    )
    .in_time(THREE_FOUR),
    rhythm(
        "Carnavalito",
        Folklore,
        116.0,
        &[
            (LowTom, "X... X... X... X..."),
            (Snare, "x.xx x.xx x.xx x.xx"),
            (Shaker, "x.x. x.x. x.x. x.x."),
        ],
        "",
        "The carnival dance of the Quebrada de Humahuaca: bombo on every beat and the caja's \
         long-short-short, in a quick 2/4 written here as pairs of bars.",
    ),
    rhythm(
        "Huayno",
        Folklore,
        100.0,
        &[
            (LowTom, "X... x... X... x..."),
            (Rim, "..x. ..x. ..x. ..x."),
            (Shaker, "x.x. x.x. x.x. x.x."),
        ],
        "",
        "The Andean huayno of Peru and Bolivia: a bombo on the beat, a tap off it, and the \
         charango's steady strum; 2/4, written as pairs of bars.",
    ),
    rhythm(
        "Candombe",
        Folklore,
        110.0,
        &[
            (Rim, "X..X ..X. X..X ..X."),
            (LowTom, "X... ..X. X... X..."),
            (Conga, ".xx. .xx. .xx. .xx."),
        ],
        "",
        "Montevideo's candombe: the stick on the drum's wooden shell plays the clave, \
         3 + 3 + 2 twice, under the piano, repique and chico drums.",
    ),
    // ----- Flamenco -----
    rhythm(
        "Rumba flamenca",
        Flamenco,
        105.0,
        &[
            (Kick, "X... ..X. X... ...."),
            (Rim, ".... X... .... X..x"),
            (Clap, "x.x. ..x. x.x. ..x."),
        ],
        "Latin fill",
        "Hand claps (palmas) and the bass and slap of a cajón.",
    ),
    // ----- Reggae and ska -----
    rhythm(
        "One drop",
        Reggae,
        76.0,
        &[
            (Kick, ".... .... X... ...."),
            (Rim, ".... .... X... ...."),
            (HiHat, "x.x. x.x. x.x. x.x."),
        ],
        "",
        "Nothing on beat 1: kick and rim fall together on beat 3.",
    ),
    rhythm(
        "Steppers",
        Reggae,
        74.0,
        &[
            (Kick, "X... X... X... X..."),
            (Rim, ".... .... X... ...."),
            (HiHat, "x.x. x.x. x.x. x.x."),
        ],
        "",
        "A kick on every beat, marching forward.",
    ),
    rhythm(
        "Ska",
        Reggae,
        160.0,
        &[
            (Kick, "X... .... X... ...."),
            (Snare, ".... X... .... X..."),
            (HiHat, "..x. ..x. ..x. ..x."),
        ],
        "Snare roll",
        "A fast backbeat with the hi-hat on the offbeats, like the guitar.",
    ),
    rhythm(
        "Rocksteady",
        Reggae,
        80.0,
        &[
            (Kick, "X... .... ..X. ...."),
            (Rim, ".... X... .... X..."),
            (HiHat, "..x. ..x. ..x. ..x."),
        ],
        "",
        "Between ska and reggae, in the mid-1960s: slower, with the hi-hat between the beats.",
    ),
    rhythm(
        "Dancehall",
        Reggae,
        96.0,
        &[
            (Kick, "X..X ..X. X..X ..X."),
            (Snare, ".... X... .... X..."),
            (HiHat, "x.x. x.x. x.x. x.x."),
        ],
        "",
        "Digital Jamaican dance music: the kick in 3 + 3 + 2 under a snare on 2 and 4.",
    ),
    // ----- Country and folk -----
    rhythm(
        "Country train",
        Folk,
        112.0,
        &[
            (Kick, "X... .... X... ...."),
            (Snare, "o.oo X.oo o.oo X.oo"),
        ],
        "Snare roll",
        "Brushes on the snare in sixteenths, like a train on the tracks.",
    ),
    rhythm(
        "Folk",
        Folk,
        100.0,
        &[
            (Kick, "X... .... X... ...."),
            (Shaker, "x.x. x.x. x.x. x.x."),
            (Rim, ".... x... .... x..."),
        ],
        "",
        "Light percussion that stays out of the guitar's way.",
    ),
    rhythm(
        "Two-step",
        Folk,
        120.0,
        &[
            (Kick, "X... .... X... ...."),
            (Snare, ".... X... .... X..."),
            (HiHat, "..x. ..x. ..x. ..x."),
        ],
        "Snare roll",
        "Boom-chick: the bass on 1 and 3, the snare on 2 and 4.",
    ),
    rhythm(
        "Waltz",
        Folk,
        150.0,
        &[(Kick, "X... .... ...."), (Snare, ".... o... o...")],
        "",
        "Oom-pah-pah: one strong beat and two light ones.",
    )
    .in_time(THREE_FOUR),
    rhythm(
        "Six-eight",
        Folk,
        90.0,
        &[
            (Kick, "X..... x....."),
            (Rim, "...... X....."),
            (HiHat, "x.x.x. x.x.x."),
        ],
        "",
        "Two beats to the bar, each made of three eighth notes.",
    )
    .in_time(SIX_EIGHT),
    rhythm(
        "Bluegrass",
        Folk,
        180.0,
        &[
            (Kick, "X... .... X... ...."),
            (Snare, ".... x... .... x..."),
            (Shaker, "x.x. x.x. x.x. x.x."),
        ],
        "",
        "Fast and light, with the snare soft on 2 and 4 like the mandolin's chop.",
    ),
    rhythm(
        "Tarantella",
        Folk,
        160.0,
        &[
            (Kick, "X..... X....."),
            (Shaker, "x.x.x. x.x.x."),
            (Rim, "..x.x. ..x.x."),
        ],
        "",
        "A whirling southern Italian dance in fast 6/8, with a tambourine.",
    )
    .in_time(SIX_EIGHT),
    // ----- Classical and film -----
    rhythm(
        "Epic toms",
        Classical,
        90.0,
        &[
            (Kick, "X... .... X... ...."),
            (Snare, ".... .... X... ...."),
            (LowTom, "X.x. ..x. X.x. ..x."),
            (HighTom, "...x .... ...x .x.."),
        ],
        "Tom fill",
        "Big toms in a driving pattern, as in film trailers.",
    ),
    rhythm(
        "March",
        Classical,
        110.0,
        &[
            (Kick, "X... .... X... ...."),
            (Snare, "X.xx X... X.xx X..."),
        ],
        "Snare roll",
        "The military snare: a roll into every other beat.",
    ),
];

/// One-bar fills, for the end of a section.
pub static FILLS: &[Rhythm] = &[
    rhythm(
        "Snare roll",
        Pop,
        110.0,
        &[
            (Kick, "X... .... .... ...."),
            (Snare, "..x. x.x. xxxx XXXX"),
        ],
        "",
        "A roll on the snare that gets louder and faster.",
    ),
    rhythm(
        "Tom fill",
        Rock,
        120.0,
        &[
            (Kick, "X... .... .... ...."),
            (Snare, "x.x. x.x. .... ...."),
            (HighTom, ".... .... xxx. ...."),
            (LowTom, ".... .... ...x xXxX"),
        ],
        "",
        "Down the toms, from high to low.",
    ),
    rhythm(
        "House build",
        Electronic,
        124.0,
        &[
            (Kick, "X... X... X... X..."),
            (Snare, "x.x. x.x. xxxx XXXX"),
            (OpenHat, "..x. ..x. .... ...."),
        ],
        "",
        "Snare hits doubling up toward the drop.",
    ),
    rhythm(
        "Latin fill",
        Latin,
        100.0,
        &[
            (HighTom, "x.x. ..x. xxx. ...."),
            (LowTom, ".... .... ...x xxX."),
            (Cowbell, "X... .... .... ...."),
        ],
        "",
        "A timbales run into the next bar.",
    ),
    rhythm(
        "Hip hop fill",
        HipHop,
        90.0,
        &[
            (Kick, "X... ..x. .xx. ...."),
            (Snare, ".... X... ..x. x.xx"),
            (HiHat, "x.x. x.x. .... ...."),
        ],
        "",
        "The beat stutters before the next bar.",
    ),
    rhythm(
        "Crash ending",
        Pop,
        110.0,
        &[
            (Kick, "X... .... .... ...."),
            (Crash, "X... .... .... ...."),
        ],
        "",
        "One last hit, and the cymbal rings out.",
    ),
];

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn every_line_is_whole_bars_of_known_characters() {
        for rhythm in RHYTHMS.iter().chain(FILLS) {
            let bar = rhythm.time.steps_per_bar();
            let mut drums = HashSet::new();
            for (drum, line) in rhythm.lines {
                assert!(drums.insert(*drum), "{}: {drum:?} twice", rhythm.name);
                let bars = parse_line(line)
                    .unwrap_or_else(|| panic!("{}: bad character in {line}", rhythm.name));
                assert_eq!(bars.len(), rhythm.bars(), "{}: {line}", rhythm.name);
                for steps in bars {
                    assert_eq!(steps.len(), bar, "{}: {line}", rhythm.name);
                }
            }
        }
    }

    #[test]
    fn names_are_unique_and_fills_exist() {
        let mut names = HashSet::new();
        for rhythm in RHYTHMS.iter().chain(FILLS) {
            assert!(names.insert(rhythm.name), "{} twice", rhythm.name);
            assert!(
                rhythm.fill.is_empty() || rhythm.fill().is_some(),
                "{}: no fill called {}",
                rhythm.name,
                rhythm.fill
            );
        }
        assert!(RHYTHMS.len() >= 45, "a large library");
    }

    #[test]
    fn notes_follow_the_grid() {
        let beat = Rhythm::named("Pop beat").unwrap();
        let kick: Vec<usize> = beat
            .notes(Kick, 16, 32)
            .iter()
            .map(|note| note.start)
            .collect();
        assert_eq!(kick, [0, 8, 10, 16, 24, 26]);
        let loud = beat.notes(Snare, 16, 16);
        assert!(loud.iter().all(|note| note.velocity == 120));
        assert!(beat.notes(Conga, 16, 16).is_empty(), "not in this rhythm");
    }

    #[test]
    fn rhythms_keep_their_place_in_other_bars() {
        // A 4/4 beat in 3/4 loses its last beat in each bar.
        let beat = Rhythm::named("Pop beat").unwrap();
        let snare: Vec<usize> = beat
            .notes(Snare, 12, 24)
            .iter()
            .map(|note| note.start)
            .collect();
        assert_eq!(snare, [4, 16]);
        // Two-bar rhythms alternate their bars.
        let trap = Rhythm::named("Trap").unwrap();
        let kick: Vec<usize> = trap
            .notes(Kick, 16, 48)
            .iter()
            .map(|note| note.start)
            .collect();
        assert_eq!(kick, [0, 10, 22, 32, 42]);
    }

    #[test]
    fn congas_play_high_and_low() {
        let salsa = Rhythm::named("Salsa").unwrap();
        let pitches: Vec<u8> = salsa
            .notes(Conga, 16, 16)
            .iter()
            .map(|note| note.pitch.midi())
            .collect();
        assert_eq!(pitches[..4], [60, 60, 60, 65]);
    }
}
