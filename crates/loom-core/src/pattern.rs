//! Patterns: the notes each channel plays.
//!
//! Since step 3, a pattern stores *notes* for each channel: a pitch, a
//! start step and a length. The step sequencer and the piano roll are
//! two views of the same notes. A step that is "on" in the channel rack
//! is simply a note one step long, at the default pitch [`STEP_NOTE`].
//!
//! Since step 4, a project has many patterns, and the playlist arranges
//! them into a song. Each pattern still holds notes for every channel.

use serde::{Deserialize, Serialize};

use crate::{Note, TimeSignature};

/// The longest pattern supported, in steps: sixteen bars.
///
/// Until step 8 this was 64 (four bars). It grew so that a whole
/// twelve-bar blues fits in one pattern; patterns longer than 64 steps
/// are saved as format 3 (see [`format_for`](crate::file::format_for)).
pub const MAX_STEPS: usize = 256;

/// The longest pattern older versions of Ferrous Loom can hold.
pub const MAX_STEPS_BEFORE_FORMAT_3: usize = 64;

/// The pitch a step of the step sequencer plays. Samplers play their
/// sound at its original speed at this pitch.
pub const STEP_NOTE: Note = Note::C4;

/// How hard notes are played unless told otherwise, from 1 to 127.
pub const DEFAULT_VELOCITY: u8 = 100;

/// How many *ticks* a step is cut into, for notes that start or end
/// between two steps.
///
/// The grid is sixteenth notes, but music does not always fall on it: a
/// triplet fits three notes where the grid has two or four, a pianist
/// plays a little ahead of the beat, and a MIDI file can put a note
/// anywhere. 24 ticks to the step is 96 to the beat, the resolution of
/// the MIDI files Ferrous Loom writes. It divides by 2, 3, 4, 6 and 8, so
/// eighth- and sixteenth-note triplets and thirty-second notes all land
/// exactly on a tick.
pub const TICKS_PER_STEP: usize = 24;

/// One note of a pattern. Times are counted in steps (sixteenth notes).
///
/// A note that starts or ends between two steps still has a `start` and
/// a `length` in steps, covering every step it sounds in, so everything
/// that works with steps (the chord names, the step buttons, moving and
/// copying notes) keeps working. `nudge` and `trim` then say how far
/// inside its first and last steps it really starts and ends, in ticks.
/// Almost every note has both at 0: it sits on the grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NoteEvent {
    pub pitch: Note,
    /// The step the note starts on, counted from 0.
    pub start: usize,
    /// How many steps the note is held for, at least 1.
    pub length: usize,
    /// How hard the note is played, from 1 to 127 (as in MIDI).
    pub velocity: u8,
    /// How many ticks after the start of its first step the note really
    /// starts, from 0 to [`TICKS_PER_STEP`] - 1. Project files leave it
    /// out when it is 0, so files of notes on the grid read as before.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub nudge: u8,
    /// How many ticks before the end of its last step the note really
    /// ends, from 0 to [`TICKS_PER_STEP`] - 1.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub trim: u8,
}

/// `serde` passes the field by reference, so this takes a `&u8`.
fn is_zero(ticks: &u8) -> bool {
    *ticks == 0
}

/// The most ticks `nudge` and `trim` can be: one less than a step.
const MAX_OFFSET: u8 = TICKS_PER_STEP as u8 - 1;

impl NoteEvent {
    pub fn new(pitch: Note, start: usize, length: usize) -> Self {
        NoteEvent {
            pitch,
            start,
            length,
            velocity: DEFAULT_VELOCITY,
            nudge: 0,
            trim: 0,
        }
    }

    /// A note that starts on tick `start` (counting [`TICKS_PER_STEP`]
    /// to the step) and is held for `ticks` ticks, at least one. It gets
    /// the steps it sounds in as its `start` and `length`, and the rest
    /// as its `nudge` and `trim`.
    ///
    /// Eighth-note triplets start every 32 ticks: on ticks 0, 32 and 64,
    /// that is step 0, step 1 nudged by 8 ticks, and step 2 nudged by 16.
    pub fn exact(pitch: Note, start: usize, ticks: usize) -> Self {
        let end = start + ticks.max(1);
        let first = start / TICKS_PER_STEP;
        let last = end.div_ceil(TICKS_PER_STEP);
        NoteEvent {
            nudge: (start % TICKS_PER_STEP) as u8,
            trim: (last * TICKS_PER_STEP - end) as u8,
            ..NoteEvent::new(pitch, first, last - first)
        }
    }

    /// A step of the step sequencer: one step long, at [`STEP_NOTE`].
    pub fn step(start: usize) -> Self {
        NoteEvent::new(STEP_NOTE, start, 1)
    }

    /// The step just after the note ends.
    pub fn end(&self) -> usize {
        self.start + self.length
    }

    /// Whether the note is still held during `step`.
    pub fn sounds_at(&self, step: usize) -> bool {
        (self.start..self.end()).contains(&step)
    }

    /// Whether this note looks like a plain step-sequencer step. A step
    /// played a little late, as in a swung beat from a MIDI file, is
    /// still a step.
    pub fn is_step(&self) -> bool {
        self.pitch == STEP_NOTE && self.length == 1
    }

    /// Whether the note starts and ends exactly on steps.
    pub fn is_on_grid(&self) -> bool {
        self.nudge == 0 && self.trim == 0
    }

    /// The tick the note really starts on, counted from the start of the
    /// pattern.
    pub fn start_tick(&self) -> usize {
        self.start * TICKS_PER_STEP + usize::from(self.nudge)
    }

    /// The tick just after the note really ends.
    pub fn end_tick(&self) -> usize {
        self.end() * TICKS_PER_STEP - usize::from(self.trim)
    }

    /// How long the note is really held, in ticks: `length` steps, less
    /// its nudge and trim.
    pub fn ticks(&self) -> usize {
        self.end_tick() - self.start_tick()
    }

    /// The note moved to the nearest steps, both its start and its end:
    /// what *quantizing* does. A note shorter than a step keeps one.
    pub fn quantized(self) -> NoteEvent {
        let nearest = |tick: usize| (tick + TICKS_PER_STEP / 2) / TICKS_PER_STEP;
        let start = nearest(self.start_tick());
        let end = nearest(self.end_tick()).max(start + 1);
        NoteEvent {
            velocity: self.velocity,
            ..NoteEvent::new(self.pitch, start, end - start)
        }
    }

    /// Whether the two notes have the same pitch and start at the same
    /// moment. A pattern never holds two such notes: they would be one
    /// note played twice at once.
    pub fn same_place(&self, other: &NoteEvent) -> bool {
        self.pitch == other.pitch && self.start == other.start && self.nudge == other.nudge
    }

    /// Makes the note fit in the pattern grid: it must start before
    /// [`MAX_STEPS`] and last at least one step, and it is shortened if
    /// it would run past the end (ending on the grid there). `nudge` and
    /// `trim` stay below a step, and leave the note at least a tick
    /// long. Returns `None` if it cannot fit.
    fn fitted(self) -> Option<NoteEvent> {
        if self.start >= MAX_STEPS || self.length == 0 {
            return None;
        }
        let length = self.length.min(MAX_STEPS - self.start);
        let nudge = self.nudge.min(MAX_OFFSET);
        let trim = if length < self.length {
            0
        } else if length == 1 {
            self.trim.min(MAX_OFFSET - nudge)
        } else {
            self.trim.min(MAX_OFFSET)
        };
        Some(NoteEvent {
            length,
            nudge,
            trim,
            ..self
        })
    }
}

/// Makes a group of notes fit in a pattern: each note is fitted to the
/// grid (shortened if it runs past [`MAX_STEPS`], left out if it starts
/// beyond it), and only the first of several notes with the same pitch
/// and start is kept, because a pattern never holds two of those.
pub fn tidy_notes(notes: impl IntoIterator<Item = NoteEvent>) -> Vec<NoteEvent> {
    let mut tidy: Vec<NoteEvent> = Vec::new();
    for note in notes.into_iter().filter_map(NoteEvent::fitted) {
        let duplicate = tidy.iter().any(|other| other.same_place(&note));
        if !duplicate {
            tidy.push(note);
        }
    }
    tidy
}

/// A pattern: a name, a list of notes for each channel, and a length.
///
/// `name` is public because any name is fine. `length` and `rows` are
/// private, because they have rules to keep: the length stays within
/// 1..=[`MAX_STEPS`], and there is one row per channel of the project.
#[derive(Debug, Clone, PartialEq)]
pub struct Pattern {
    pub name: String,
    /// Which colour it is drawn in, counted along the app's palette. It
    /// is given when the pattern is added to a project and stays with it,
    /// so removing another pattern does not change it.
    pub color: usize,
    /// The time signature it is written in while no clip of the song
    /// plays it, if not the song's: a waltz tried from the library is in
    /// 3/4 even in a song in 4/4. Once it is in the song, the song's time
    /// signature where it plays counts (see `Project::pattern_time`).
    pub time: Option<TimeSignature>,
    length: usize,
    rows: Vec<Vec<NoteEvent>>,
}

impl Pattern {
    /// Creates an empty pattern with `channels` rows and the given length.
    ///
    /// `impl Into<String>` accepts anything that can become a `String`:
    /// a `&str` such as `"Beat"`, a `String`, or the result of `format!`.
    pub fn new(name: impl Into<String>, channels: usize, length: usize) -> Self {
        Pattern {
            name: name.into(),
            color: 0,
            time: None,
            length: length.clamp(1, MAX_STEPS),
            rows: vec![Vec::new(); channels],
        }
    }

    /// How many steps are played before the pattern loops.
    pub fn length(&self) -> usize {
        self.length
    }

    /// Changes the length. Notes beyond the new length are kept, so
    /// making the pattern longer again brings them back.
    pub fn set_length(&mut self, length: usize) {
        self.length = length.clamp(1, MAX_STEPS);
    }

    /// The number of rows (one per channel).
    pub fn rows(&self) -> usize {
        self.rows.len()
    }

    /// Replaces all the notes of a channel. The notes are tidied first
    /// (see [`tidy_notes`]). Returns `false`, changing nothing, if there
    /// is no such channel.
    pub fn set_notes(
        &mut self,
        channel: usize,
        notes: impl IntoIterator<Item = NoteEvent>,
    ) -> bool {
        match self.rows.get_mut(channel) {
            Some(row) => {
                *row = tidy_notes(notes);
                true
            }
            None => false,
        }
    }

    /// The notes of one channel, in the order they were added. A channel
    /// that does not exist has no notes.
    pub fn notes(&self, channel: usize) -> &[NoteEvent] {
        match self.rows.get(channel) {
            Some(row) => row,
            None => &[],
        }
    }

    /// Whether the pattern has no notes at all.
    pub fn is_empty(&self) -> bool {
        self.rows.iter().all(Vec::is_empty)
    }

    /// Every note of every channel, with the channel it belongs to.
    pub fn all_notes(&self) -> impl Iterator<Item = (usize, &NoteEvent)> {
        self.rows
            .iter()
            .enumerate()
            .flat_map(|(channel, row)| row.iter().map(move |note| (channel, note)))
    }

    /// Whether every note of the channel is a plain step, so the channel
    /// rack can show the row as step buttons.
    pub fn is_step_row(&self, channel: usize) -> bool {
        self.notes(channel).iter().all(NoteEvent::is_step)
    }

    /// Whether a note starts on this step (a step is "on").
    pub fn is_on(&self, channel: usize, step: usize) -> bool {
        self.notes(channel).iter().any(|note| note.start == step)
    }

    /// Switches a step on (adds a step note) or off (removes every note
    /// starting there). Returns `false` if it is outside the grid.
    pub fn set(&mut self, channel: usize, step: usize, on: bool) -> bool {
        if step >= MAX_STEPS {
            return false;
        }
        let Some(row) = self.rows.get_mut(channel) else {
            return false;
        };
        row.retain(|note| note.start != step);
        if on {
            row.push(NoteEvent::step(step));
        }
        true
    }

    /// Flips a step and returns its new value, or `None` if it is outside the grid.
    pub fn toggle(&mut self, channel: usize, step: usize) -> Option<bool> {
        let on = !self.is_on(channel, step);
        self.set(channel, step, on).then_some(on)
    }

    /// Adds a note and returns its index in [`Pattern::notes`]. Returns
    /// `None` if the channel does not exist, the note does not fit, or
    /// the same pitch already starts on the same step.
    pub fn add_note(&mut self, channel: usize, note: NoteEvent) -> Option<usize> {
        let note = note.fitted()?;
        let row = self.rows.get_mut(channel)?;
        if row.iter().any(|other| other.same_place(&note)) {
            return None;
        }
        row.push(note);
        Some(row.len() - 1)
    }

    /// Removes a note by its index and returns it.
    pub fn remove_note(&mut self, channel: usize, index: usize) -> Option<NoteEvent> {
        let row = self.rows.get_mut(channel)?;
        (index < row.len()).then(|| row.remove(index))
    }

    /// Replaces a note, for example after it was moved or resized.
    /// Returns `false` if there is no such note, the new one does not fit,
    /// or another note of the same pitch already starts on the same step.
    pub fn replace_note(&mut self, channel: usize, index: usize, note: NoteEvent) -> bool {
        let Some(note) = note.fitted() else {
            return false;
        };
        let Some(row) = self.rows.get_mut(channel) else {
            return false;
        };
        let duplicate = row
            .iter()
            .enumerate()
            .any(|(other_index, other)| other_index != index && other.same_place(&note));
        match row.get_mut(index) {
            Some(slot) if !duplicate => {
                *slot = note;
                true
            }
            _ => false,
        }
    }

    /// Whether [`Pattern::replace_notes`] would accept these notes: every
    /// note of `old` is in the channel, and none of `new` starts on the
    /// same step and pitch as a note that stays.
    pub fn can_replace_notes(&self, channel: usize, old: &[NoteEvent], new: &[NoteEvent]) -> bool {
        self.replaced(channel, old, new).is_some()
    }

    /// Takes the notes `old` out of a channel and puts `new` in their
    /// place, all at once: this is how a group of notes is moved,
    /// transposed or turned into an arpeggio. `new` is tidied first (see
    /// [`tidy_notes`]). Returns `false`, changing nothing, if a note of
    /// `old` is missing or a note of `new` would land on one that stays.
    pub fn replace_notes(&mut self, channel: usize, old: &[NoteEvent], new: &[NoteEvent]) -> bool {
        match self.replaced(channel, old, new) {
            Some(notes) => {
                self.rows[channel] = notes;
                true
            }
            None => false,
        }
    }

    /// The channel's notes after replacing `old` with `new`, if that is
    /// allowed. The notes that stay keep their order, and the new ones
    /// come after them.
    fn replaced(
        &self,
        channel: usize,
        old: &[NoteEvent],
        new: &[NoteEvent],
    ) -> Option<Vec<NoteEvent>> {
        let row = self.rows.get(channel)?;
        if !old.iter().all(|note| row.contains(note)) {
            return None;
        }
        let mut notes: Vec<NoteEvent> = row
            .iter()
            .filter(|note| !old.contains(note))
            .copied()
            .collect();
        let new = tidy_notes(new.iter().copied());
        let lands_on_another = new
            .iter()
            .any(|note| notes.iter().any(|other| other.same_place(note)));
        if lands_on_another {
            return None;
        }
        notes.extend(new);
        Some(notes)
    }

    /// The pitches of a channel that are held during `step`.
    ///
    /// This returns an iterator instead of a `Vec`: nothing is collected
    /// or allocated, and the caller decides what to do with the pitches.
    /// The `'_` says the iterator borrows from `self`.
    pub fn sounding_at(&self, channel: usize, step: usize) -> impl Iterator<Item = Note> + '_ {
        self.notes(channel)
            .iter()
            .filter(move |note| note.sounds_at(step))
            .map(|note| note.pitch)
    }

    /// Adds an empty row at the bottom.
    pub(crate) fn push_row(&mut self) {
        self.rows.push(Vec::new());
    }

    /// Removes a row, moving the rows below it up.
    pub(crate) fn remove_row(&mut self, channel: usize) {
        if channel < self.rows.len() {
            self.rows.remove(channel);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_empty() {
        let pattern = Pattern::new("Test", 2, 16);
        assert_eq!(pattern.rows(), 2);
        assert_eq!(pattern.length(), 16);
        assert!(!pattern.is_on(0, 0));
        assert!(pattern.notes(0).is_empty());
        assert!(pattern.is_empty());
        assert!(
            pattern.notes(9).is_empty(),
            "missing channels have no notes"
        );
    }

    #[test]
    fn replacing_a_group_of_notes() {
        let mut pattern = Pattern::new("Test", 1, 16);
        let c = NoteEvent::new(Note::C4, 0, 4);
        let e = NoteEvent::new(Note::new(64), 0, 4);
        let g = NoteEvent::new(Note::new(67), 4, 4);
        for note in [c, e, g] {
            pattern.add_note(0, note);
        }
        let up = |note: NoteEvent| NoteEvent {
            pitch: Note::new(note.pitch.midi() + 2),
            ..note
        };

        // C and E move up a tone; G stays where it was.
        assert!(pattern.replace_notes(0, &[c, e], &[up(c), up(e)]));
        assert_eq!(pattern.notes(0), &[g, up(c), up(e)]);

        // The old notes are gone now, so replacing them again fails.
        assert!(!pattern.can_replace_notes(0, &[c], &[up(c)]));
        // A note may not land on one that stays.
        let onto_g = NoteEvent {
            start: 4,
            pitch: g.pitch,
            ..c
        };
        assert!(!pattern.replace_notes(0, &[up(c)], &[onto_g]));
        assert_eq!(
            pattern.notes(0).len(),
            3,
            "a refused change changes nothing"
        );
        // Two new notes on the same step and pitch become one.
        assert!(pattern.replace_notes(0, &[up(c), up(e)], &[c, c]));
        assert_eq!(pattern.notes(0), &[g, c]);
    }

    #[test]
    fn tidying_notes() {
        let long = NoteEvent::new(Note::C4, MAX_STEPS - 2, 8);
        let outside = NoteEvent::new(Note::C4, MAX_STEPS, 1);
        let first = NoteEvent::new(Note::A4, 0, 2);
        let same_place = NoteEvent::new(Note::A4, 0, 5);
        assert_eq!(
            tidy_notes([long, outside, first, same_place]),
            vec![NoteEvent { length: 2, ..long }, first]
        );
    }

    #[test]
    fn notes_between_steps() {
        // From tick 8 (a third of the way into step 0) for 32 ticks: it
        // sounds in steps 0 and 1, and ends 8 ticks before step 2.
        let note = NoteEvent::exact(Note::C4, 8, 32);
        assert_eq!(
            (note.start, note.length, note.nudge, note.trim),
            (0, 2, 8, 8)
        );
        assert_eq!(
            (note.start_tick(), note.end_tick(), note.ticks()),
            (8, 40, 32)
        );
        assert!(!note.is_on_grid());
        assert!(NoteEvent::exact(Note::C4, 48, 24).is_on_grid());
        // Quantized, it starts on step 0 and ends on step 2 (tick 40 is
        // nearer 48 than 24).
        assert_eq!(note.quantized(), NoteEvent::new(Note::C4, 0, 2));
    }

    #[test]
    fn two_notes_in_one_step() {
        // Thirty-second notes on one pitch: two start in each step.
        let mut pattern = Pattern::new("Test", 1, 16);
        let first = NoteEvent::exact(Note::C4, 0, 12);
        let second = NoteEvent::exact(Note::C4, 12, 12);
        assert!(pattern.add_note(0, first).is_some());
        assert!(pattern.add_note(0, second).is_some(), "it starts later");
        assert!(pattern.add_note(0, second).is_none(), "but only once");
    }

    #[test]
    fn a_nudged_note_always_lasts_a_tick() {
        let mut pattern = Pattern::new("Test", 1, 16);
        let squeezed = NoteEvent {
            nudge: 20,
            trim: 20,
            ..NoteEvent::new(Note::C4, 0, 1)
        };
        let index = pattern.add_note(0, squeezed).unwrap();
        assert_eq!(pattern.notes(0)[index].ticks(), 1);
        // Cut at the end of the longest pattern, it ends on the grid.
        let long = NoteEvent::exact(Note::C4, (MAX_STEPS - 1) * TICKS_PER_STEP, 100);
        let index = pattern.add_note(0, long).unwrap();
        assert_eq!(pattern.notes(0)[index].trim, 0);
    }

    #[test]
    fn toggling_a_step() {
        let mut pattern = Pattern::new("Test", 1, 16);
        assert_eq!(pattern.toggle(0, 3), Some(true));
        assert!(pattern.is_on(0, 3));
        assert_eq!(pattern.notes(0), &[NoteEvent::step(3)]);
        assert_eq!(pattern.toggle(0, 3), Some(false));
        assert!(!pattern.is_on(0, 3));
        assert!(pattern.notes(0).is_empty());
    }

    #[test]
    fn outside_the_grid_is_ignored() {
        let mut pattern = Pattern::new("Test", 1, 16);
        assert_eq!(pattern.toggle(5, 0), None);
        assert_eq!(pattern.toggle(0, MAX_STEPS), None);
        assert!(!pattern.set(1, 0, true));
        assert!(!pattern.is_on(9, 9));
    }

    #[test]
    fn length_is_clamped_and_keeps_hidden_notes() {
        let mut pattern = Pattern::new("Test", 1, 32);
        pattern.set(0, 20, true);
        pattern.set_length(16);
        assert_eq!(pattern.length(), 16);
        pattern.set_length(32);
        assert!(pattern.is_on(0, 20), "step 20 should survive shortening");

        pattern.set_length(0);
        assert_eq!(pattern.length(), 1);
        pattern.set_length(1000);
        assert_eq!(pattern.length(), MAX_STEPS);
    }

    #[test]
    fn steps_and_piano_roll_notes() {
        let mut pattern = Pattern::new("Test", 1, 16);
        pattern.set(0, 0, true);
        assert!(pattern.is_step_row(0));
        let a4 = NoteEvent::new(Note::A4, 4, 2);
        assert_eq!(pattern.add_note(0, a4), Some(1));
        assert!(
            !pattern.is_step_row(0),
            "a real melody is no longer a step row"
        );
    }

    #[test]
    fn adding_notes() {
        let mut pattern = Pattern::new("Test", 1, 16);
        let note = NoteEvent::new(Note::C4, 2, 4);
        assert_eq!(pattern.add_note(0, note), Some(0));
        assert_eq!(pattern.add_note(0, note), None, "no duplicates");
        assert_eq!(pattern.add_note(1, note), None, "no such channel");
        assert_eq!(pattern.add_note(0, NoteEvent::new(Note::C4, 3, 0)), None);
        // A note that would run past the end is shortened.
        let long = NoteEvent::new(Note::A4, MAX_STEPS - 2, 10);
        let index = pattern.add_note(0, long).unwrap();
        assert_eq!(pattern.notes(0)[index].length, 2);
    }

    #[test]
    fn moving_and_removing_notes() {
        let mut pattern = Pattern::new("Test", 1, 16);
        pattern.add_note(0, NoteEvent::new(Note::C4, 0, 4));
        let moved = NoteEvent::new(Note::A4, 8, 2);
        assert!(pattern.replace_note(0, 0, moved));
        assert_eq!(pattern.notes(0), &[moved]);
        assert!(!pattern.replace_note(0, 5, moved));
        // A note cannot be moved on top of an identical one.
        pattern.add_note(0, NoteEvent::new(Note::C4, 0, 4));
        assert!(
            !pattern.replace_note(0, 1, moved),
            "A4 already starts on step 8"
        );
        pattern.remove_note(0, 1);
        assert_eq!(pattern.remove_note(0, 0), Some(moved));
        assert_eq!(pattern.remove_note(0, 0), None);
    }

    #[test]
    fn all_notes_lists_every_channel() {
        let mut pattern = Pattern::new("Test", 3, 16);
        pattern.set(0, 0, true);
        pattern.set(2, 4, true);
        let found: Vec<(usize, usize)> = pattern
            .all_notes()
            .map(|(channel, note)| (channel, note.start))
            .collect();
        assert_eq!(found, vec![(0, 0), (2, 4)]);
        assert!(!pattern.is_empty());
    }

    #[test]
    fn which_notes_sound_at_a_step() {
        let mut pattern = Pattern::new("Test", 1, 16);
        pattern.add_note(0, NoteEvent::new(Note::new(60), 0, 4));
        pattern.add_note(0, NoteEvent::new(Note::new(64), 2, 4));
        let at = |step| {
            pattern
                .sounding_at(0, step)
                .map(|n| n.midi())
                .collect::<Vec<_>>()
        };
        assert_eq!(at(0), vec![60]);
        assert_eq!(at(3), vec![60, 64]);
        assert_eq!(at(4), vec![64], "a note ends just before its end step");
        assert!(at(6).is_empty());
    }
}
