//! Saving projects to files and loading them back.
//!
//! A project file is plain text in RON ("Rusty Object Notation"), a
//! format that looks like Rust's own struct and enum syntax:
//!
//! ```text
//! (
//!     format: 1,
//!     tempo: 130.0,
//!     scale: (root: C, kind: Major),
//!     channels: [
//!         (name: "Kick", kind: Sampler(Builtin(Kick)), volume: 0.8, muted: false),
//!         ...
//! ```
//!
//! The `serde` crate does the work. `#[derive(Serialize, Deserialize)]`
//! writes the code that turns a struct into text and back, field by
//! field, and the `ron` crate decides what the text looks like.
//!
//! The file has its own struct, [`ProjectFile`], instead of saving
//! [`Project`] directly. `Project` has rules to keep (one row of notes
//! per channel, clips that point at real patterns), and a file could
//! break any of them. So loading reads a `ProjectFile`, which accepts
//! anything shaped right, and then builds the `Project` through its
//! normal methods, which check everything, as if a user had clicked it
//! all in.

use std::fmt;
use std::fs;
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::meter::TimeMarker;
use crate::pattern::{MAX_STEPS, MAX_STEPS_BEFORE_FORMAT_3, NoteEvent};
use crate::playlist::{Clip, LoopRegion, MAX_TRACKS, Section, Track};
use crate::project::{
    Channel, ChannelKind, MAX_CHANNELS, MAX_LOOSE_TIMING, MAX_PATTERNS, Project, SampleSource,
};
use crate::tempo::TempoMarker;
use crate::{Scale, Tempo, TimeSignature};

/// The file name extension of Ferrous Loom projects, without the dot.
pub const EXTENSION: &str = "loom";

/// The version of the file layout. A later version of Ferrous Loom that
/// changes the layout will raise this, so it knows how to read old files
/// and so this version can refuse files it would misread.
///
/// Format 2 (step 7) added channels that play an external MIDI synth.
/// A project without one is still written as format 1, which is exactly
/// the same text, so older versions of Ferrous Loom can open it. See
/// [`format_for`].
///
/// Format 3 (step 8) added nine built-in drums, patterns longer than
/// four bars, and time signatures. Older versions would not know the
/// drums, would cut the long patterns short without a word, and would
/// draw a waltz in bars of four, so those projects need format 3.
///
/// Format 4 added the song's structure: clips that repeat their pattern,
/// named sections that can repeat, and a loop region. Older versions
/// would play every repeat once, so a song that uses them needs format 4.
///
/// Format 5 added time markers, which change the time signature partway
/// through a song. Older versions would draw every bar in the first time
/// signature, so a song with markers needs format 5.
///
/// Format 6 added two kinds of key for music without one: a twelve-tone
/// row and a pitch-class set. Older versions would not know them and could
/// not open the file, so they are told it is newer instead.
///
/// Format 7 added tempo changes through a song, and loose timing. Older
/// versions would play the whole song at its first tempo, and every
/// note on the grid, so a song that uses either needs format 7.
///
/// Format 8 added notes that start or end between two steps, such as
/// the triplets of an imported MIDI file (see [`NoteEvent::nudge`]).
/// Older versions would move them onto the grid, so a project with one
/// needs format 8. Format 8 also added a channel's General MIDI program
/// ([`Channel::program`]), which older versions would drop.
pub const FORMAT_VERSION: u32 = 8;

/// The oldest format that can hold `project`.
///
/// Writing the lowest version that works, instead of always the newest,
/// means a file only becomes unreadable to an older Ferrous Loom when it
/// really uses something that version does not know.
pub fn format_for(project: &Project) -> u32 {
    let uses_new_drums = project.channels().iter().any(|channel| {
        matches!(
            channel.kind,
            ChannelKind::Sampler(SampleSource::Builtin(drum)) if drum.is_new_in_format_3()
        )
    });
    let has_long_patterns = project
        .patterns()
        .iter()
        .any(|pattern| pattern.length() > MAX_STEPS_BEFORE_FORMAT_3);
    let uses_midi_out = project
        .channels()
        .iter()
        .any(|channel| matches!(channel.kind, ChannelKind::MidiOut { .. }));
    let has_time_signature = !project.time_signature.is_common()
        || project
            .patterns()
            .iter()
            .any(|pattern| pattern.time.is_some());
    let playlist = project.playlist();
    let has_structure = !playlist.sections().is_empty()
        || playlist.loop_region().is_some()
        || playlist.clips().iter().any(|clip| clip.repeats != 1);
    let has_notes_off_the_grid = project
        .patterns()
        .iter()
        .any(|pattern| pattern.all_notes().any(|(_, note)| !note.is_on_grid()));
    let has_programs = project
        .channels()
        .iter()
        .any(|channel| channel.program.is_some());
    if has_notes_off_the_grid || has_programs {
        8
    } else if !playlist.tempo_markers().is_empty() || project.loose_timing > 0 {
        7
    } else if !project.scale.kind.is_tonal() {
        6
    } else if !playlist.time_markers().is_empty() {
        5
    } else if has_structure {
        4
    } else if uses_new_drums || has_long_patterns || has_time_signature {
        3
    } else if uses_midi_out {
        2
    } else {
        1
    }
}

/// A project as it is written in a file.
#[derive(Debug, Serialize, Deserialize)]
struct ProjectFile {
    format: u32,
    tempo: Tempo,
    scale: Scale,
    /// Left out for 4/4, so those files read exactly as before. A file
    /// without it is in 4/4: `default` fills in `TimeSignature::default()`.
    #[serde(default, skip_serializing_if = "TimeSignature::is_common")]
    time_signature: TimeSignature,
    channels: Vec<Channel>,
    patterns: Vec<PatternFile>,
    tracks: Vec<Track>,
    clips: Vec<Clip>,
    /// Left out when there are none, like `time_signature`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    sections: Vec<Section>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    loop_region: Option<LoopRegion>,
    /// Left out when the time signature never changes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    time_markers: Vec<TimeMarker>,
    /// Left out when the tempo never changes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    tempo_markers: Vec<TempoMarker>,
    /// Left out when every note plays on the grid.
    #[serde(default, skip_serializing_if = "is_zero")]
    loose_timing: u8,
}

fn is_zero(value: &u8) -> bool {
    *value == 0
}

/// A pattern as it is written in a file.
#[derive(Debug, Serialize, Deserialize)]
struct PatternFile {
    name: String,
    /// Left out when it is the pattern's place in the list, as it is for
    /// every pattern until one is removed, so most files read exactly as
    /// before. Older versions skip it and colour patterns by place.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    color: Option<usize>,
    /// Left out when the pattern takes the song's time signature.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    time: Option<TimeSignature>,
    length: usize,
    /// One list of notes per channel, in the order of `channels`.
    notes: Vec<Vec<NoteEvent>>,
}

/// Just the first field of a file, read before anything else.
///
/// Fields that a struct does not list are skipped while reading, so this
/// reads the version of any project file, even one from a future version
/// whose other fields this code would not understand.
#[derive(Deserialize)]
struct Header {
    format: u32,
}

/// Everything that can go wrong while saving or loading.
#[derive(Debug)]
pub enum FileError {
    /// The file could not be read or written: missing, no permission, disk full...
    Io(io::Error),
    /// The text is not a valid project file. The error says where.
    Parse(ron::error::SpannedError),
    /// The project could not be turned into text (should never happen).
    Write(ron::Error),
    /// The file was saved by a newer version of Ferrous Loom.
    NewerFormat(u32),
    /// The file is shaped right, but what it says does not make sense,
    /// such as a clip that plays a pattern that does not exist.
    Invalid(String),
}

impl fmt::Display for FileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FileError::Io(err) => write!(f, "{err}"),
            FileError::Parse(err) => write!(f, "not a valid project file: {err}"),
            FileError::Write(err) => write!(f, "could not write the project: {err}"),
            FileError::NewerFormat(version) => write!(
                f,
                "this file uses format {version}, made by a newer version of Ferrous Loom \
                 (this one reads up to format {FORMAT_VERSION})"
            ),
            FileError::Invalid(reason) => write!(f, "the project file is damaged: {reason}"),
        }
    }
}

impl std::error::Error for FileError {
    /// The lower-level error this one wraps, if any.
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            FileError::Io(err) => Some(err),
            FileError::Parse(err) => Some(err),
            FileError::Write(err) => Some(err),
            FileError::NewerFormat(_) | FileError::Invalid(_) => None,
        }
    }
}

// These `From` implementations let the `?` operator turn the errors of
// `std::fs` and `ron` into a `FileError` on its own.

impl From<io::Error> for FileError {
    fn from(err: io::Error) -> Self {
        FileError::Io(err)
    }
}

impl From<ron::error::SpannedError> for FileError {
    fn from(err: ron::error::SpannedError) -> Self {
        FileError::Parse(err)
    }
}

impl From<ron::Error> for FileError {
    fn from(err: ron::Error) -> Self {
        FileError::Write(err)
    }
}

impl From<&Project> for ProjectFile {
    fn from(project: &Project) -> Self {
        let channels = project.channels().len();
        ProjectFile {
            format: format_for(project),
            tempo: project.tempo,
            scale: project.scale,
            time_signature: project.time_signature,
            channels: project.channels().to_vec(),
            patterns: project
                .patterns()
                .iter()
                .enumerate()
                .map(|(index, pattern)| PatternFile {
                    name: pattern.name.clone(),
                    color: (pattern.color != index).then_some(pattern.color),
                    time: pattern.time,
                    length: pattern.length(),
                    notes: (0..channels)
                        .map(|channel| pattern.notes(channel).to_vec())
                        .collect(),
                })
                .collect(),
            tracks: project.playlist().tracks().to_vec(),
            clips: project.playlist().clips().to_vec(),
            sections: project.playlist().sections().to_vec(),
            loop_region: project.playlist().loop_region(),
            time_markers: project.playlist().time_markers().to_vec(),
            tempo_markers: project.playlist().tempo_markers().to_vec(),
            loose_timing: project.loose_timing,
        }
    }
}

impl TryFrom<ProjectFile> for Project {
    type Error = FileError;

    fn try_from(file: ProjectFile) -> Result<Self, Self::Error> {
        let invalid = |reason: String| Err(FileError::Invalid(reason));

        let mut project = Project::empty();
        project.tempo = file.tempo;
        project.scale = file.scale;
        if file.time_signature.steps_per_bar() > MAX_STEPS {
            return invalid(format!(
                "its time signature {} is too long",
                file.time_signature
            ));
        }
        project.time_signature = file.time_signature;

        if file.channels.len() > MAX_CHANNELS {
            return invalid(format!("it has more than {MAX_CHANNELS} channels"));
        }
        let channels = file.channels.len();
        for channel in file.channels {
            project.add_channel(channel);
        }

        if file.patterns.is_empty() || file.patterns.len() > MAX_PATTERNS {
            return invalid(format!("it must have 1 to {MAX_PATTERNS} patterns"));
        }
        for (index, saved) in file.patterns.into_iter().enumerate() {
            // `Project::empty` already has one pattern; add the others.
            if index > 0 {
                project.add_pattern();
            }
            let Some(pattern) = project.pattern_mut(index) else {
                return invalid(format!("pattern {} could not be added", index + 1));
            };
            pattern.name = saved.name;
            pattern.color = saved.color.unwrap_or(index);
            if saved
                .time
                .is_some_and(|time| time.steps_per_bar() > MAX_STEPS)
            {
                return invalid(format!("pattern \"{}\" has too long a bar", pattern.name));
            }
            pattern.time = saved.time;
            pattern.set_length(saved.length);
            if saved.notes.len() != channels {
                return invalid(format!(
                    "pattern \"{}\" has notes for {} channels, but there are {channels}",
                    pattern.name,
                    saved.notes.len()
                ));
            }
            for (channel, notes) in saved.notes.into_iter().enumerate() {
                for note in notes {
                    if pattern.add_note(channel, note).is_none() {
                        return invalid(format!(
                            "pattern \"{}\" has a note that does not fit, or a duplicate, \
                             at step {} on channel {}",
                            pattern.name,
                            note.start + 1,
                            channel + 1
                        ));
                    }
                }
            }
        }

        if file.tracks.is_empty() || file.tracks.len() > MAX_TRACKS {
            return invalid(format!("it must have 1 to {MAX_TRACKS} playlist tracks"));
        }
        project.replace_tracks(file.tracks);
        for (index, clip) in file.clips.into_iter().enumerate() {
            // A file names all its tracks: a clip below them is a mistake,
            // not a new track as it is when placed by hand.
            let on_a_track = clip.track < project.playlist().tracks().len();
            if !on_a_track || project.add_clip(clip).is_none() {
                return invalid(format!(
                    "clip {} (pattern {}, track {}) does not fit in the playlist",
                    index + 1,
                    clip.pattern + 1,
                    clip.track + 1
                ));
            }
        }
        for section in file.sections {
            if project.add_section(section.clone()).is_none() {
                return invalid(format!(
                    "section \"{}\" is empty, too long, repeats too often or overlaps another",
                    section.name
                ));
            }
        }
        if !project.set_loop_region(file.loop_region) {
            return invalid("its loop region is empty or too long".to_string());
        }
        for marker in file.time_markers {
            if marker.start == 0 || !project.set_time_marker(marker) {
                return invalid(format!(
                    "its change to {} at step {} is too late, too long or on the first step",
                    marker.time,
                    marker.start + 1
                ));
            }
        }
        for marker in file.tempo_markers {
            if marker.start == 0 || !project.set_tempo_marker(marker) {
                return invalid(format!(
                    "its change to {} BPM at step {} is too late or on the first step",
                    marker.tempo.bpm(),
                    marker.start + 1
                ));
            }
        }
        project.loose_timing = file.loose_timing.min(MAX_LOOSE_TIMING);
        project.remove_blank_tracks();
        Ok(project)
    }
}

/// Turns a project into the text of a project file.
pub fn to_text(project: &Project) -> Result<String, FileError> {
    // Pretty-printed, so the file can be read (and fixed) by a person.
    // Things nested deeper than 5 levels go on one line, so each note
    // takes one line instead of one line per field. Lines always end in
    // "\n" (RON would use "\r\n" on Windows), so a project is the same
    // file on every system.
    let style = ron::ser::PrettyConfig::new().depth_limit(5).new_line("\n");
    let mut text = ron::ser::to_string_pretty(&ProjectFile::from(project), style)?;
    text.push('\n');
    Ok(text)
}

/// Reads a project from the text of a project file.
pub fn from_text(text: &str) -> Result<Project, FileError> {
    let header: Header = ron::from_str(text)?;
    if header.format > FORMAT_VERSION {
        return Err(FileError::NewerFormat(header.format));
    }
    let file: ProjectFile = ron::from_str(text)?;
    Project::try_from(file)
}

/// Saves a project to a file, replacing the file if it exists.
///
/// The text is first written to a temporary file next to it, which is
/// then renamed. Renaming is a single step for the operating system, so
/// if the computer crashes halfway, the old file is still intact.
pub fn save(project: &Project, path: &Path) -> Result<(), FileError> {
    let text = to_text(project)?;
    let temporary = path.with_extension(format!("{EXTENSION}.tmp"));
    fs::write(&temporary, text)?;
    fs::rename(&temporary, path)?;
    Ok(())
}

/// Loads a project from a file.
pub fn load(path: &Path) -> Result<Project, FileError> {
    let text = fs::read_to_string(path)?;
    from_text(&text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Note;
    use crate::project::Drum;

    #[test]
    fn the_demo_survives_a_round_trip() {
        let demo = Project::demo();
        let text = to_text(&demo).unwrap();
        assert!(text.contains("tempo: 130.0"), "{text}");
        assert!(text.contains("kind: Sampler(Builtin(Kick))"), "{text}");
        let loaded = from_text(&text).unwrap();
        assert_eq!(loaded, demo);
    }

    #[test]
    fn saving_and_loading_a_file() {
        let mut project = Project::template();
        project.tempo = Tempo::new(97.5);
        project
            .pattern_mut(0)
            .unwrap()
            .add_note(4, NoteEvent::new(Note::A4, 2, 3));
        let path = std::env::temp_dir().join(format!("loom-test-{}.loom", std::process::id()));
        save(&project, &path).unwrap();
        let loaded = load(&path);
        std::fs::remove_file(&path).unwrap();
        assert_eq!(loaded.unwrap(), project);
    }

    #[test]
    fn patterns_keep_their_colours_in_a_file() {
        let mut project = Project::demo();
        let text = to_text(&project).unwrap();
        assert!(
            !text.contains("color"),
            "colours in their places are left out"
        );
        project.remove_pattern(0);
        let text = to_text(&project).unwrap();
        assert_eq!(from_text(&text).unwrap(), project);
    }

    #[test]
    fn a_missing_file_is_an_io_error() {
        let result = load(Path::new("/this/file/does/not/exist.loom"));
        assert!(matches!(result, Err(FileError::Io(_))));
    }

    #[test]
    fn rubbish_is_a_parse_error() {
        let error = from_text("this is not a project").unwrap_err();
        assert!(matches!(error, FileError::Parse(_)));
        assert!(error.to_string().starts_with("not a valid project file"));
    }

    #[test]
    fn files_from_the_future_are_refused() {
        // Format 9 might have fields we have never heard of. Only the
        // header is read, so the rest does not matter.
        let error = from_text("(format: 9, sparkles: true)").unwrap_err();
        assert!(matches!(error, FileError::NewerFormat(9)), "{error}");
    }

    #[test]
    fn lines_end_the_same_way_on_every_system() {
        let text = to_text(&Project::template()).unwrap();
        assert!(!text.contains('\r'));
    }

    #[test]
    fn midi_out_channels_need_format_two() {
        let mut project = Project::template();
        assert!(to_text(&project).unwrap().contains("format: 1,"));

        project.add_channel(Channel::midi_out(4)).unwrap();
        let text = to_text(&project).unwrap();
        assert!(text.contains("format: 2,"), "{text}");
        assert!(
            text.contains("MidiOut(\n                channel: 4,"),
            "{text}"
        );
        assert_eq!(from_text(&text).unwrap(), project);
    }

    #[test]
    fn new_drums_and_long_patterns_need_format_three() {
        let mut project = Project::template();
        project.add_channel(Channel::midi_out(0)).unwrap();
        assert!(to_text(&project).unwrap().contains("format: 2,"));

        let mut drums = project.clone();
        drums.add_channel(Channel::drum(Drum::Conga)).unwrap();
        let text = to_text(&drums).unwrap();
        assert!(text.contains("format: 3,"), "{text}");
        assert_eq!(from_text(&text).unwrap(), drums);

        let mut long = project;
        long.pattern_mut(0).unwrap().set_length(192);
        long.pattern_mut(0)
            .unwrap()
            .add_note(4, NoteEvent::new(Note::C4, 180, 12));
        let text = to_text(&long).unwrap();
        assert!(text.contains("format: 3,"), "{text}");
        assert_eq!(from_text(&text).unwrap(), long);
    }

    #[test]
    fn time_signatures_other_than_four_four_need_format_three() {
        let mut project = Project::template();
        let text = to_text(&project).unwrap();
        assert!(!text.contains("time_signature"), "4/4 is left out: {text}");
        assert_eq!(
            from_text(&text).unwrap().time_signature,
            TimeSignature::COMMON
        );

        project.time_signature = TimeSignature::new(3, 4);
        let text = to_text(&project).unwrap();
        assert!(text.contains("format: 3,"), "{text}");
        assert!(text.contains("beats: 3"), "{text}");
        assert_eq!(from_text(&text).unwrap(), project);
    }

    #[test]
    fn song_structure_needs_format_four() {
        let mut project = Project::demo();
        let text = to_text(&project).unwrap();
        assert!(text.contains("format: 1,"), "{text}");
        assert!(
            !text.contains("repeats") && !text.contains("sections"),
            "{text}"
        );

        let mut chorus = Section::new("Chorus", 32, 64);
        chorus.repeats = 2;
        project.add_section(chorus);
        project.add_section(Section::new("Outro", 96, 32));
        project.set_clip_repeats(0, 2);
        project.set_loop_region(Some(LoopRegion::new(32, 96)));
        let text = to_text(&project).unwrap();
        assert!(text.contains("format: 4,"), "{text}");
        assert!(text.contains("name: \"Chorus\""), "{text}");
        assert!(text.contains("repeats: 2"), "{text}");
        assert!(text.contains("loop_region"), "{text}");
        assert_eq!(from_text(&text).unwrap(), project);
    }

    #[test]
    fn time_markers_need_format_five() {
        let mut project = Project::demo();
        assert!(project.set_time_marker(TimeMarker::new(64, TimeSignature::new(3, 4))));
        let text = to_text(&project).unwrap();
        assert!(text.contains("format: 5,"), "{text}");
        assert!(text.contains("time_markers"), "{text}");
        let loaded = from_text(&text).unwrap();
        assert_eq!(loaded.meter(), project.meter());

        // A marker on step 0 is the project's own time signature.
        let broken = text.replace(
            "start: 64,\n            time",
            "start: 0,\n            time",
        );
        assert_ne!(broken, text, "{text}");
        assert!(from_text(&broken).is_err());
    }

    #[test]
    fn tempo_changes_and_loose_timing_need_format_seven() {
        let mut project = Project::demo();
        assert!(project.set_tempo_marker(TempoMarker::gliding(96, 90.0, 16)));
        assert!(project.set_tempo_marker(TempoMarker::new(120, 130.0)));
        let text = to_text(&project).unwrap();
        assert!(text.contains("format: 7,"), "{text}");
        assert!(text.contains("tempo_markers"), "{text}");
        assert!(text.contains("glide: 16"), "{text}");
        assert!(!text.contains("loose_timing"), "{text}");
        let loaded = from_text(&text).unwrap();
        assert_eq!(loaded, project);
        assert_eq!(loaded.tempo_map(), project.tempo_map());

        let mut loose = Project::demo();
        loose.loose_timing = 12;
        let text = to_text(&loose).unwrap();
        assert!(text.contains("format: 7,"), "{text}");
        assert_eq!(from_text(&text).unwrap(), loose);
    }

    #[test]
    fn notes_off_the_grid_need_format_eight() {
        let mut project = Project::demo();
        let synth = 4;
        let text = to_text(&project).unwrap();
        assert!(!text.contains("nudge"), "notes on the grid read as before");
        // A triplet note, 8 ticks late, held 32 ticks.
        let triplet = crate::NoteEvent::exact(crate::Note::C4, 32, 32);
        project.pattern_mut(0).unwrap().add_note(synth, triplet);
        let text = to_text(&project).unwrap();
        assert!(text.contains("format: 8,"), "{text}");
        assert!(text.contains("nudge: 8"), "{text}");
        assert!(text.contains("trim: 8"), "{text}");
        assert_eq!(from_text(&text).unwrap(), project);
    }

    #[test]
    fn a_general_midi_program_needs_format_eight() {
        let mut project = Project::demo();
        let text = to_text(&project).unwrap();
        assert!(
            !text.contains("program"),
            "channels without one read as before"
        );
        project.channel_mut(4).unwrap().program = Some(40);
        let text = to_text(&project).unwrap();
        assert!(text.contains("format: 8,"), "{text}");
        assert!(text.contains("program: Some(40)"), "{text}");
        assert_eq!(from_text(&text).unwrap(), project);
    }

    #[test]
    fn rows_and_sets_need_format_six() {
        let mut project = Project::demo();
        project.scale = Scale::row(crate::harmony::ROW_PRESETS[0].row);
        let text = to_text(&project).unwrap();
        assert!(text.contains("format: 6,"), "{text}");
        assert!(
            text.contains("kind: Row(\"0 1 3 9 2 11 4 10 7 8 5 6\")"),
            "{text}"
        );
        assert_eq!(from_text(&text).unwrap(), project);

        project.scale = Scale::set(
            crate::PitchClass::D,
            crate::PitchSet::from_semitones(&[2, 5, 8, 11]),
        );
        let text = to_text(&project).unwrap();
        assert!(text.contains("format: 6,"), "{text}");
        assert!(text.contains("kind: Set(\"0 3 6 9\")"), "{text}");
        assert_eq!(from_text(&text).unwrap(), project);

        // A row that repeats a note is not a row.
        let broken = text.replace("Set(\"0 3 6 9\")", "Row(\"0 0 2 3 4 5 6 7 8 9 10 11\")");
        assert!(from_text(&broken).is_err());
    }

    #[test]
    fn overlapping_sections_are_reported() {
        let mut project = Project::demo();
        project.add_section(Section::new("Verse", 0, 32));
        project.add_section(Section::new("Chorus", 32, 32));
        let text = to_text(&project).unwrap();
        let broken = text.replace(
            "start: 32,\n            length: 32",
            "start: 16,\n            length: 32",
        );
        assert_ne!(broken, text, "{text}");
        let error = from_text(&broken).unwrap_err();
        assert!(error.to_string().contains("\"Chorus\""), "{error}");
    }

    #[test]
    fn values_out_of_range_are_clamped() {
        let text = to_text(&Project::empty()).unwrap();
        let text = text.replace("tempo: 130.0", "tempo: 9000.0");
        let project = from_text(&text).unwrap();
        assert_eq!(project.tempo.bpm(), Tempo::MAX);
    }

    #[test]
    fn broken_rules_are_reported() {
        let text = to_text(&Project::demo()).unwrap();

        // A clip that plays pattern 99.
        let broken = text.replacen("pattern: 0,", "pattern: 98,", 1);
        let error = from_text(&broken).unwrap_err();
        assert!(matches!(error, FileError::Invalid(_)), "{error}");
        assert!(error.to_string().contains("pattern 99"), "{error}");

        // A pattern with notes for fewer channels than the project has.
        let mut file: ProjectFile = ron::from_str(&text).unwrap();
        file.patterns[0].notes.pop();
        let error = Project::try_from(file).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("\"Beat\" has notes for 4 channels")
        );
    }

    #[test]
    fn old_files_open_with_only_the_tracks_they_use() {
        // Files used to be saved with sixteen tracks, most of them empty.
        let mut file = ProjectFile::from(&Project::demo());
        file.tracks
            .extend((3..=16).map(crate::playlist::Track::numbered));
        // A track someone renamed is kept, and so are the ones above it.
        file.tracks[5].name = "Keep me".to_string();
        let project = Project::try_from(file).unwrap();
        let names: Vec<&str> = project
            .playlist()
            .tracks()
            .iter()
            .map(|track| track.name.as_str())
            .collect();
        assert_eq!(
            names,
            [
                "Chords", "Drums", "Track 3", "Track 4", "Track 5", "Keep me"
            ]
        );
    }

    #[test]
    fn clips_below_the_tracks_are_reported() {
        let mut file = ProjectFile::from(&Project::demo());
        file.clips[0].track = file.tracks.len();
        let error = Project::try_from(file).unwrap_err();
        assert!(error.to_string().contains("does not fit"), "{error}");
    }
}
