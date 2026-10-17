//! Finding the key of a piece from its notes.
//!
//! A key is not only which notes are used, but which ones matter: in C
//! major, C, E and G (the notes of the home chord) are heard the most,
//! and the other notes of the scale more than the notes outside it.
//! In the 1980s the psychologists Carol Krumhansl and Edward Kessler
//! measured this. They played listeners a few chords that set up a key,
//! then a single note, and asked how well it fitted. The answers, for
//! each of the twelve notes counted up from the tonic, are the *key
//! profiles* below.
//!
//! To find the key of a piece, add up how long each pitch class sounds,
//! and compare those twelve numbers with the profile of each of the 24
//! major and minor keys (the profile turned so it starts on that key's
//! tonic). The comparison is a *correlation*: 1 when the two rise and
//! fall together, 0 when they have nothing to do with each other. The
//! key whose profile fits best is most likely the key of the piece.
//!
//! It is right most of the time, and when it is wrong it is wrong the
//! way a listener might be: it hears a closely related key, such as the
//! relative minor (A minor for C major), which uses the same notes.

use super::{Scale, ScaleKind};
use crate::PitchClass;

/// How well each note fits a major key, from the tonic up (Krumhansl
/// and Kessler, 1982).
const MAJOR: [f64; 12] = [
    6.35, 2.23, 3.48, 2.33, 4.38, 4.09, 2.52, 5.19, 2.39, 3.66, 2.29, 2.88,
];

/// The same for a minor key.
const MINOR: [f64; 12] = [
    6.33, 2.68, 3.52, 5.38, 2.60, 3.53, 2.54, 4.75, 3.98, 2.69, 3.34, 3.17,
];

/// Every major and minor key with how well `durations` fits it, from -1
/// to 1, best first. `durations[n]` is how long pitch class `n` (counted
/// up from C) sounds, in any unit.
pub fn key_fits(durations: &[f64; 12]) -> Vec<(Scale, f64)> {
    let mut fits: Vec<(Scale, f64)> = PitchClass::ALL
        .into_iter()
        .flat_map(|tonic| {
            [
                (ScaleKind::Major, &MAJOR),
                (ScaleKind::NaturalMinor, &MINOR),
            ]
            .map(|(kind, profile)| {
                // The profile turned to start on this tonic: the fit
                // of pitch class `n` is the profile's value for the
                // note `n - tonic` semitones above the tonic.
                let turned: [f64; 12] =
                    std::array::from_fn(|n| profile[(n + 12 - usize::from(tonic.semitone())) % 12]);
                (Scale::new(tonic, kind), correlation(durations, &turned))
            })
        })
        .collect();
    // `total_cmp` orders every float, even NaN, so the sort cannot fail.
    fits.sort_by(|a, b| b.1.total_cmp(&a.1));
    fits
}

/// Pearson's correlation of two lists of twelve numbers: how closely
/// one rises and falls with the other. 0 if either never changes.
fn correlation(a: &[f64; 12], b: &[f64; 12]) -> f64 {
    let mean = |values: &[f64; 12]| values.iter().sum::<f64>() / 12.0;
    let (mean_a, mean_b) = (mean(a), mean(b));
    let (mut both, mut only_a, mut only_b) = (0.0, 0.0, 0.0);
    for (x, y) in a.iter().zip(b) {
        let (dx, dy) = (x - mean_a, y - mean_b);
        both += dx * dy;
        only_a += dx * dx;
        only_b += dy * dy;
    }
    if only_a == 0.0 || only_b == 0.0 {
        0.0
    } else {
        both / (only_a * only_b).sqrt()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// How long each pitch class of `notes` (MIDI numbers) sounds, each
    /// for `beats`.
    fn durations(notes: &[(u8, f64)]) -> [f64; 12] {
        let mut durations = [0.0; 12];
        for &(midi, beats) in notes {
            durations[usize::from(midi % 12)] += beats;
        }
        durations
    }

    fn best(notes: &[(u8, f64)]) -> String {
        key_fits(&durations(notes))[0].0.name()
    }

    #[test]
    fn a_major_scale_and_its_chords() {
        // C major: the scale once, then C, F, G and C chords.
        let mut notes: Vec<(u8, f64)> = [60, 62, 64, 65, 67, 69, 71, 72]
            .iter()
            .map(|&n| (n, 1.0))
            .collect();
        for chord in [[60, 64, 67], [65, 69, 72], [67, 71, 74], [60, 64, 67]] {
            notes.extend(chord.iter().map(|&n| (n, 2.0)));
        }
        assert_eq!(best(&notes), "C major");
    }

    #[test]
    fn a_minor_tune() {
        // "Greensleeves" in A minor, the opening phrase.
        let tune = [
            (69, 1.0),
            (72, 2.0),
            (74, 1.0),
            (76, 1.5),
            (77, 0.5),
            (76, 1.0),
            (74, 2.0),
            (71, 1.0),
            (67, 1.5),
            (69, 0.5),
            (71, 1.0),
            (72, 2.0),
            (69, 1.0),
            (69, 1.5),
            (68, 0.5),
            (69, 1.0),
            (71, 2.0),
            (68, 1.0),
            (64, 2.0),
            (69, 3.0),
        ];
        assert_eq!(best(&tune), "A minor");
    }

    #[test]
    fn every_key_is_scored_once() {
        let fits = key_fits(&durations(&[(62, 1.0), (66, 1.0), (69, 1.0)]));
        assert_eq!(fits.len(), 24);
        assert_eq!(fits[0].0.name(), "D major");
        assert!(fits.windows(2).all(|pair| pair[0].1 >= pair[1].1));
    }

    #[test]
    fn silence_fits_nothing() {
        assert!(key_fits(&[0.0; 12]).iter().all(|(_, fit)| *fit == 0.0));
    }
}
