//! Reading Standard MIDI Files: the bytes of a `.mid` file, turned into
//! tracks of events with their times.
//!
//! The file is the one [`song_to_midi`](super::song_to_midi) writes,
//! read the other way round:
//!
//! ```text
//! MThd  length  format  tracks  division      the header chunk
//! MTrk  length  delta event delta event ...   one chunk per track
//! MTrk  length  delta event ...
//! ```
//!
//! Every number is *big-endian*, most significant byte first. Each event
//! comes after a *delta time*, the ticks since the event before it,
//! written as a variable-length number (see `Bytes::variable_length`).
//! Adding the deltas up gives each event's time from the start.
//!
//! Three kinds of event can follow a delta:
//!
//! - A **channel message**: the same bytes a keyboard sends down a MIDI
//!   cable, such as `90 3C 64` (note on, channel 1, C4, velocity 100).
//! - A **meta event**, `FF type length data`: information for the
//!   reader, such as the tempo, the time signature or a track's name.
//! - A **system exclusive** message, `F0 length data` or `F7 length
//!   data`: settings for one maker's synths. We skip those.
//!
//! One trick saves space: **running status**. When a message has the
//! same status byte as the one before it (another note on, on the same
//! channel), the status byte can be left out, and the data bytes follow
//! the delta straight away. A status byte has its top bit set and a data
//! byte does not, so the reader can always tell which one it is looking
//! at.
//!
//! Real files are not always perfect: a download cut short, a chunk
//! whose length is wrong. The reader is forgiving, like most programs
//! that open MIDI files: it keeps every event it could read, and says
//! the file was damaged, instead of refusing it all.

use std::fmt;

use crate::Scale;

/// Everything read from a MIDI file.
#[derive(Debug, Clone, PartialEq)]
pub struct MidiFile {
    /// 0: one track holding every channel. 1: several tracks played
    /// together, the first usually holding only the tempo and the time
    /// signature. 2: several tracks that are separate pieces, played one
    /// at a time (rare).
    pub format: u16,
    pub timing: Timing,
    pub tracks: Vec<Track>,
    /// Whether part of the file could not be read. The events before the
    /// damage are still there.
    pub damaged: bool,
}

/// How a file counts its ticks.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Timing {
    /// This many ticks to a beat (a quarter note), the usual way: the
    /// tempo then says how long a beat lasts. Often 96, 192, 480 or 960.
    PerBeat(u16),
    /// This many ticks to a second, whatever the tempo: the way film and
    /// video studios count time (SMPTE timecode), as frames per second
    /// times ticks per frame.
    PerSecond(f64),
}

/// One track: its events in time order.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Track {
    pub events: Vec<Event>,
}

/// An event and when it happens: `tick` ticks from the start.
#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    pub tick: u64,
    pub kind: EventKind,
}

/// The events Ferrous Loom uses. Others (aftertouch, pitch bend, lyrics,
/// system exclusive...) are read past and left out.
#[derive(Debug, Clone, PartialEq)]
pub enum EventKind {
    /// A key goes down. `channel` counts from 0; `key` is the MIDI note
    /// number, 60 for middle C.
    NoteOn { channel: u8, key: u8, velocity: u8 },
    /// A key comes up. A note on with velocity 0 is read as this too.
    NoteOff { channel: u8, key: u8 },
    /// A knob, slider or pedal moves: controller 7 is the channel's
    /// volume, 64 the sustain pedal.
    Controller { channel: u8, number: u8, value: u8 },
    /// The channel changes instrument: 0 is a grand piano, 40 a violin,
    /// in the General MIDI list.
    Program { channel: u8, program: u8 },
    /// From here on, a beat lasts this many microseconds (millionths of
    /// a second): 500,000 is 120 beats per minute.
    Tempo(u32),
    /// From here on, bars have `beats` beats of a `unit` note: 3 and 4
    /// for 3/4.
    TimeSignature { beats: u8, unit: u8 },
    /// The key signature: `sharps` sharps (or flats, when negative) and
    /// whether the key is minor. Two flats and minor is G minor.
    KeySignature { sharps: i8, minor: bool },
    /// The exact key, which only a file Ferrous Loom wrote holds: a key
    /// signature cannot say "E Dorian" (see [`super::scale_event`]).
    ExactKey(Scale),
    /// The name of the track, often the instrument it plays.
    TrackName(String),
    /// The name of the instrument a track plays.
    InstrumentName(String),
}

/// Why a file could not be read at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadError {
    /// The file does not start like a MIDI file.
    NotMidi,
    /// The header is cut short or makes no sense.
    BadHeader,
}

impl fmt::Display for ReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ReadError::NotMidi => write!(f, "it is not a MIDI file"),
            ReadError::BadHeader => write!(f, "its header is damaged"),
        }
    }
}

impl std::error::Error for ReadError {}

/// Reads a MIDI file from its bytes.
///
/// A `.rmi` file (a MIDI file wrapped in a RIFF container, as Windows
/// used to save them) is read too: the MIDI file is found inside it.
pub fn read(bytes: &[u8]) -> Result<MidiFile, ReadError> {
    let bytes = unwrap_riff(bytes);
    let mut file = Bytes::new(bytes);
    if file.take(4) != Some(b"MThd".as_slice()) {
        return Err(ReadError::NotMidi);
    }
    let header_length = file.u32().ok_or(ReadError::BadHeader)? as usize;
    let header = file
        .take(header_length)
        .filter(|header| header.len() >= 6)
        .ok_or(ReadError::BadHeader)?;
    let format = u16::from_be_bytes([header[0], header[1]]);
    let timing = timing(u16::from_be_bytes([header[4], header[5]])).ok_or(ReadError::BadHeader)?;

    let mut tracks = Vec::new();
    let mut damaged = false;
    // Read every chunk to the end of the file. The header also says how
    // many tracks there are, but some programs write that number wrong,
    // so the chunks themselves are trusted instead.
    while let Some(id) = file.take(4) {
        let Some(length) = file.u32() else {
            damaged = true;
            break;
        };
        let length = length as usize;
        // A chunk that claims to be longer than what is left of the file
        // was cut short: read what there is.
        let data = file.take(length.min(file.remaining()));
        let data = data.unwrap_or_default();
        if data.len() < length {
            damaged = true;
        }
        // Chunks of other kinds may be added by future versions of the
        // standard, or by one program for itself: they are skipped.
        if id == b"MTrk" {
            let (track, complete) = read_track(data);
            damaged |= !complete;
            tracks.push(track);
        }
    }
    damaged |= file.remaining() > 0;
    Ok(MidiFile {
        format,
        timing,
        tracks,
        damaged,
    })
}

/// The MIDI file inside a RIFF ("RMID") file, or the bytes as they are.
///
/// RIFF is the container of WAV files too: after "RIFF", its size and
/// "RMID" come chunks, each a four-letter name, its length (least
/// significant byte first, unlike MIDI) and its bytes. The MIDI file is
/// the chunk called "data"; others hold a title or a copyright.
fn unwrap_riff(bytes: &[u8]) -> &[u8] {
    if bytes.len() < 12 || &bytes[..4] != b"RIFF" || &bytes[8..12] != b"RMID" {
        return bytes;
    }
    let mut riff = Bytes::new(&bytes[12..]);
    while let (Some(id), Some(length)) = (riff.take(4), riff.take(4)) {
        let length = u32::from_le_bytes([length[0], length[1], length[2], length[3]]) as usize;
        let data = riff.take(length.min(riff.remaining())).unwrap_or_default();
        if id == b"data" {
            return data;
        }
        // Chunks of an odd length are followed by a byte of padding.
        riff.take(length % 2);
    }
    bytes
}

/// The header's *division*: how the file counts ticks.
///
/// With the top bit clear it is ticks per beat. With it set, the high
/// byte is minus the frames per second (-24, -25, -29 for 29.97 or -30)
/// and the low byte the ticks per frame.
fn timing(division: u16) -> Option<Timing> {
    if division & 0x8000 == 0 {
        return (division > 0).then_some(Timing::PerBeat(division));
    }
    let frames = -i32::from(division.to_be_bytes()[0] as i8);
    let ticks_per_frame = f64::from(division & 0xFF);
    let frames_per_second = match frames {
        24 | 25 | 30 => f64::from(frames),
        29 => 30_000.0 / 1001.0,
        _ => return None,
    };
    (ticks_per_frame > 0.0).then_some(Timing::PerSecond(frames_per_second * ticks_per_frame))
}

/// Reads the events of one track chunk. Returns the track and whether it
/// was read to its end without trouble.
fn read_track(data: &[u8]) -> (Track, bool) {
    let mut bytes = Bytes::new(data);
    let mut track = Track::default();
    let mut tick: u64 = 0;
    // The status byte of the last channel message, for running status.
    let mut running: Option<u8> = None;
    while bytes.remaining() > 0 {
        let Some(delta) = bytes.variable_length() else {
            return (track, false);
        };
        tick += u64::from(delta);
        let Some(first) = bytes.byte() else {
            return (track, false);
        };
        let kind = match first {
            0xFF => {
                let (Some(kind), Some(length)) = (bytes.byte(), bytes.variable_length()) else {
                    return (track, false);
                };
                let Some(data) = bytes.take(length as usize) else {
                    return (track, false);
                };
                if kind == END_OF_TRACK {
                    return (track, true);
                }
                meta_event(kind, data)
            }
            0xF0 | 0xF7 => {
                let Some(length) = bytes.variable_length() else {
                    return (track, false);
                };
                if bytes.take(length as usize).is_none() {
                    return (track, false);
                }
                None
            }
            // System messages other than the two above only travel down
            // cables, never in files: this one is damaged.
            0xF1..=0xFE => return (track, false),
            _ => {
                // A data byte where a status byte could be: running
                // status, the same kind of message as the last one.
                let (status, first_data) = if first & 0x80 != 0 {
                    (first, bytes.byte())
                } else {
                    (running.unwrap_or(0), Some(first))
                };
                if status == 0 {
                    return (track, false);
                }
                running = Some(status);
                let one_byte = matches!(status & 0xF0, 0xC0 | 0xD0);
                let second_data = if one_byte { Some(0) } else { bytes.byte() };
                let (Some(a), Some(b)) = (first_data, second_data) else {
                    return (track, false);
                };
                if a > 0x7F || b > 0x7F {
                    return (track, false);
                }
                channel_message(status, a, b)
            }
        };
        if let Some(kind) = kind {
            track.events.push(Event { tick, kind });
        }
    }
    // The track ran out without its End of Track event. Many files do
    // that, and nothing is missing, so it counts as complete.
    (track, true)
}

/// Meta event types.
const TRACK_NAME: u8 = 0x03;
const INSTRUMENT_NAME: u8 = 0x04;
const END_OF_TRACK: u8 = 0x2F;
const TEMPO: u8 = 0x51;
const TIME_SIGNATURE: u8 = 0x58;
const KEY_SIGNATURE: u8 = 0x59;
const SEQUENCER_SPECIFIC: u8 = 0x7F;

/// The event a channel message stands for, if we use it.
fn channel_message(status: u8, a: u8, b: u8) -> Option<EventKind> {
    let channel = status & 0x0F;
    Some(match status & 0xF0 {
        0x80 => EventKind::NoteOff { channel, key: a },
        0x90 if b == 0 => EventKind::NoteOff { channel, key: a },
        0x90 => EventKind::NoteOn {
            channel,
            key: a,
            velocity: b,
        },
        0xB0 => EventKind::Controller {
            channel,
            number: a,
            value: b,
        },
        0xC0 => EventKind::Program {
            channel,
            program: a,
        },
        // Aftertouch (A0, D0) and pitch bend (E0).
        _ => return None,
    })
}

/// The event a meta event stands for, if we use it and it makes sense.
fn meta_event(kind: u8, data: &[u8]) -> Option<EventKind> {
    match (kind, data) {
        (TRACK_NAME, _) => Some(EventKind::TrackName(text(data))),
        (INSTRUMENT_NAME, _) => Some(EventKind::InstrumentName(text(data))),
        // Three bytes: microseconds per beat.
        (TEMPO, &[a, b, c]) => {
            let micros = u32::from_be_bytes([0, a, b, c]);
            (micros > 0).then_some(EventKind::Tempo(micros))
        }
        // Beats, then the beat's note as a power of two (2 for a quarter,
        // 3 for an eighth), then two numbers about metronome clicks that
        // we do not need.
        (TIME_SIGNATURE, &[beats, power, ..]) if beats > 0 && power <= 6 => {
            Some(EventKind::TimeSignature {
                beats,
                unit: 1 << power,
            })
        }
        // Sharps (negative for flats), then 0 for major or 1 for minor.
        (KEY_SIGNATURE, &[sharps, mode]) => {
            let sharps = sharps as i8;
            ((-7..=7).contains(&sharps) && mode <= 1).then_some(EventKind::KeySignature {
                sharps,
                minor: mode == 1,
            })
        }
        (SEQUENCER_SPECIFIC, _) => super::scale_from_event(data).map(EventKind::ExactKey),
        _ => None,
    }
}

/// Text from a meta event. The standard says nothing about how letters
/// beyond plain English are written: newer programs use UTF-8, older
/// ones Latin-1 (one byte per letter). UTF-8 is tried first, since
/// Latin-1 text is almost never valid UTF-8.
fn text(data: &[u8]) -> String {
    let text = match std::str::from_utf8(data) {
        Ok(text) => text.to_string(),
        Err(_) => data.iter().map(|&byte| char::from(byte)).collect(),
    };
    text.trim_matches(|c: char| c.is_whitespace() || c == '\0')
        .to_string()
}

/// A cursor over bytes: each read moves past what it read. Every read
/// returns `None` instead of going past the end, so a file cut short
/// can never make the program crash.
struct Bytes<'a> {
    data: &'a [u8],
    at: usize,
}

impl<'a> Bytes<'a> {
    fn new(data: &'a [u8]) -> Self {
        Bytes { data, at: 0 }
    }

    fn remaining(&self) -> usize {
        self.data.len() - self.at
    }

    fn byte(&mut self) -> Option<u8> {
        let byte = *self.data.get(self.at)?;
        self.at += 1;
        Some(byte)
    }

    /// The next `count` bytes, or `None` if there are fewer left.
    fn take(&mut self, count: usize) -> Option<&'a [u8]> {
        let end = self.at.checked_add(count)?;
        let slice = self.data.get(self.at..end)?;
        self.at = end;
        Some(slice)
    }

    /// A four-byte number, most significant byte first.
    fn u32(&mut self) -> Option<u32> {
        let bytes = self.take(4)?;
        Some(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    /// A *variable-length quantity*: seven bits of the number in each
    /// byte, most significant first, with the top bit set on every byte
    /// but the last. `81 00` is 128. The standard allows four bytes at
    /// most, so the number fits in 28 bits.
    fn variable_length(&mut self) -> Option<u32> {
        let mut value = 0u32;
        for _ in 0..4 {
            let byte = self.byte()?;
            value = (value << 7) | u32::from(byte & 0x7F);
            if byte & 0x80 == 0 {
                return Some(value);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A file with a header and the given track chunks.
    fn file(format: u16, division: u16, tracks: &[&[u8]]) -> Vec<u8> {
        let mut bytes = b"MThd".to_vec();
        bytes.extend(6u32.to_be_bytes());
        bytes.extend(format.to_be_bytes());
        bytes.extend((tracks.len() as u16).to_be_bytes());
        bytes.extend(division.to_be_bytes());
        for track in tracks {
            bytes.extend(b"MTrk");
            bytes.extend((track.len() as u32).to_be_bytes());
            bytes.extend(*track);
        }
        bytes
    }

    fn kinds(track: &Track) -> Vec<(u64, EventKind)> {
        track
            .events
            .iter()
            .map(|event| (event.tick, event.kind.clone()))
            .collect()
    }

    #[test]
    fn variable_length_numbers() {
        let read = |bytes: &[u8]| Bytes::new(bytes).variable_length();
        assert_eq!(read(&[0x00]), Some(0));
        assert_eq!(read(&[0x7F]), Some(0x7F));
        assert_eq!(read(&[0x81, 0x00]), Some(0x80));
        assert_eq!(read(&[0xC0, 0x00]), Some(0x2000));
        assert_eq!(read(&[0xFF, 0xFF, 0xFF, 0x7F]), Some(0x0FFF_FFFF));
        assert_eq!(read(&[0x81]), None, "cut short");
        assert_eq!(read(&[0xFF, 0xFF, 0xFF, 0xFF, 0x7F]), None, "too long");
    }

    #[test]
    fn notes_with_running_status() {
        // Note on C4, then E4 without repeating the status byte, then
        // both off as note ons with velocity 0, again by running status.
        let track: &[u8] = &[
            0x00, 0x90, 60, 100, //
            0x00, 64, 90, //
            0x60, 60, 0, //
            0x00, 64, 0, //
            0x00, 0xFF, 0x2F, 0x00,
        ];
        let midi = read(&file(0, 96, &[track])).unwrap();
        assert_eq!(midi.timing, Timing::PerBeat(96));
        assert!(!midi.damaged);
        assert_eq!(
            kinds(&midi.tracks[0]),
            [
                (
                    0,
                    EventKind::NoteOn {
                        channel: 0,
                        key: 60,
                        velocity: 100
                    }
                ),
                (
                    0,
                    EventKind::NoteOn {
                        channel: 0,
                        key: 64,
                        velocity: 90
                    }
                ),
                (
                    96,
                    EventKind::NoteOff {
                        channel: 0,
                        key: 60
                    }
                ),
                (
                    96,
                    EventKind::NoteOff {
                        channel: 0,
                        key: 64
                    }
                ),
            ]
        );
    }

    #[test]
    fn meta_events() {
        let track: &[u8] = &[
            0x00, 0xFF, 0x03, 5, b'P', b'i', b'a', b'n', b'o', //
            0x00, 0xFF, 0x51, 3, 0x07, 0xA1, 0x20, // 500,000: 120 BPM
            0x00, 0xFF, 0x58, 4, 6, 3, 36, 8, // 6/8
            0x00, 0xFF, 0x59, 2, 0xFE, 1, // two flats, minor
            0x00, 0xFF, 0x01, 2, b'h', b'i', // a text event: left out
            0x00, 0xC0, 40, // program change: violin
            0x00, 0xB0, 7, 90, // volume
            0x00, 0xFF, 0x2F, 0x00,
        ];
        let midi = read(&file(1, 480, &[track])).unwrap();
        assert_eq!(
            kinds(&midi.tracks[0])
                .into_iter()
                .map(|(_, kind)| kind)
                .collect::<Vec<_>>(),
            [
                EventKind::TrackName("Piano".to_string()),
                EventKind::Tempo(500_000),
                EventKind::TimeSignature { beats: 6, unit: 8 },
                EventKind::KeySignature {
                    sharps: -2,
                    minor: true
                },
                EventKind::Program {
                    channel: 0,
                    program: 40
                },
                EventKind::Controller {
                    channel: 0,
                    number: 7,
                    value: 90
                },
            ]
        );
    }

    #[test]
    fn system_exclusive_is_skipped() {
        let track: &[u8] = &[
            0x00, 0xF0, 0x05, 0x7E, 0x7F, 0x09, 0x01, 0xF7, // GM reset
            0x10, 0x91, 62, 80, //
            0x00, 0xFF, 0x2F, 0x00,
        ];
        let midi = read(&file(0, 96, &[track])).unwrap();
        assert_eq!(
            kinds(&midi.tracks[0]),
            [(
                16,
                EventKind::NoteOn {
                    channel: 1,
                    key: 62,
                    velocity: 80
                }
            )]
        );
    }

    #[test]
    fn a_file_cut_short_keeps_what_was_read() {
        let track: &[u8] = &[
            0x00, 0x90, 60, 100, //
            0x60, 0x80, 60, 64, //
            0x00, 0xFF, 0x2F, 0x00,
        ];
        let whole = file(0, 96, &[track]);
        let cut = &whole[..whole.len() - 6];
        let midi = read(cut).unwrap();
        assert!(midi.damaged);
        assert_eq!(midi.tracks[0].events.len(), 1, "the note on survives");

        // Cutting anywhere never panics.
        for end in 0..whole.len() {
            let _ = read(&whole[..end]);
        }
    }

    #[test]
    fn not_a_midi_file() {
        assert_eq!(read(b"RIFF....WAVEfmt "), Err(ReadError::NotMidi));
        assert_eq!(read(b""), Err(ReadError::NotMidi));
        assert_eq!(read(b"MThd\0\0\0\x06\0\0"), Err(ReadError::BadHeader));
    }

    #[test]
    fn a_midi_file_in_a_riff_file() {
        let track: &[u8] = &[0x00, 0x90, 60, 100, 0x00, 0xFF, 0x2F, 0x00];
        let midi = file(0, 96, &[track]);
        let mut rmi = b"RIFF".to_vec();
        rmi.extend(((midi.len() + 12) as u32).to_le_bytes());
        rmi.extend(b"RMIDdata");
        rmi.extend((midi.len() as u32).to_le_bytes());
        rmi.extend(&midi);
        // A title after the MIDI file, as Windows wrote it.
        rmi.extend(b"LIST\x0a\0\0\0INFOINAM\x02\0");
        let read_back = read(&rmi);
        assert_eq!(read_back, read(&midi));
        assert!(!read_back.unwrap().damaged);
    }

    #[test]
    fn smpte_timing() {
        // 25 frames per second, 40 ticks per frame: 1000 ticks a second.
        let division = u16::from_be_bytes([(-25i8) as u8, 40]);
        assert_eq!(timing(division), Some(Timing::PerSecond(1000.0)));
        assert_eq!(timing(0), None);
    }

    #[test]
    fn latin_1_names() {
        assert_eq!(text("Canción".as_bytes()), "Canción");
        assert_eq!(text(b"Canci\xF3n\0"), "Canción");
    }

    #[test]
    fn reads_what_the_exporter_writes() {
        let mut project = crate::Project::template();
        let synth = project.channels().len() - 1;
        let pattern = project.pattern_mut(0).unwrap();
        pattern.add_note(synth, crate::NoteEvent::new(crate::Note::C4, 2, 4));
        let midi = read(&crate::pattern_to_midi(&project, 0)).unwrap();
        assert!(!midi.damaged);
        assert_eq!(midi.tracks.len(), 2, "the tempo track and the synth");
        let notes: Vec<_> = kinds(&midi.tracks[1])
            .into_iter()
            .filter(|(_, kind)| !matches!(kind, EventKind::TrackName(_)))
            .collect();
        assert_eq!(
            notes,
            [
                (
                    48,
                    EventKind::NoteOn {
                        channel: 0,
                        key: 60,
                        velocity: 100
                    }
                ),
                (
                    144,
                    EventKind::NoteOff {
                        channel: 0,
                        key: 60
                    }
                ),
            ]
        );
    }
}
