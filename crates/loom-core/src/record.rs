//! Recording: turning a key held on a keyboard into a note of a pattern.
//!
//! While a pattern loops, the audio thread knows where it is to a
//! fraction of a step (see [`Sequencer::position`](crate::Sequencer::position)).
//! A key goes down at one position and comes up at another. This module
//! turns the two into a note on the grid, moving each end to the nearest
//! step: that is called *quantizing*.
//!
//! It is a plain function in this crate, not part of the app, so it can
//! be tested with made-up positions, without a keyboard or a sound card.

use crate::{Note, NoteEvent};

/// The note for a key held from position `from` until `until`, in steps
/// of a pattern `length` steps long that loops.
///
/// - Each end goes to the nearest step, so a key pressed a little early
///   or late lands on the beat.
/// - A note is at least one step long: a quick tap still makes a note.
/// - A key pressed just before the loop starts again lands on step 0.
/// - A key that comes up after the loop has started again (`until` less
///   than `from`) is counted on into the next time round, and the note is
///   cut at the end of the pattern.
pub fn recorded_note(pitch: Note, velocity: u8, from: f64, until: f64, length: usize) -> NoteEvent {
    let length = length.max(1);
    let until = if until < from {
        until + length as f64
    } else {
        until
    };
    let start = from.round();
    let end = until.round().max(start + 1.0);
    // `as usize` turns a float into an integer, dropping the fraction
    // (there is none left after `round`). Negative numbers become 0.
    let start_step = start as usize % length;
    let steps = ((end - start) as usize).min(length - start_step);
    NoteEvent {
        velocity,
        ..NoteEvent::new(pitch, start_step, steps)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// (start, length) of the note recorded in a 16-step pattern.
    fn recorded(from: f64, until: f64) -> (usize, usize) {
        let note = recorded_note(Note::C4, 90, from, until, 16);
        assert_eq!((note.pitch, note.velocity), (Note::C4, 90));
        (note.start, note.length)
    }

    #[test]
    fn ends_go_to_the_nearest_step() {
        assert_eq!(recorded(2.0, 4.0), (2, 2));
        assert_eq!(recorded(1.8, 4.3), (2, 2), "a little early, a little late");
        assert_eq!(recorded(2.4, 4.6), (2, 3));
    }

    #[test]
    fn a_tap_is_one_step_long() {
        assert_eq!(recorded(3.1, 3.2), (3, 1));
        assert_eq!(recorded(3.4, 3.6), (3, 1));
    }

    #[test]
    fn just_before_the_loop_is_step_zero() {
        assert_eq!(recorded(15.7, 0.4), (0, 1));
        assert_eq!(recorded(15.6, 1.9), (0, 2));
    }

    #[test]
    fn a_note_across_the_loop_end_is_cut_there() {
        assert_eq!(recorded(14.0, 2.0), (14, 2));
    }
}
