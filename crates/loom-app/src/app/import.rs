//! File > Import recording: an MP3, WAV, FLAC or Ogg file becomes a new
//! project that plays it, with a harmony track that follows its chords.
//! And File > Import MIDI file, at the end.
//!
//! Listening takes a few seconds, so it runs on a thread of its own, like
//! the WAV export, and the window keeps drawing meanwhile. The thread
//! decodes the file, analyzes it, saves it again as a WAV file with a
//! short lead-in (see [`loom_listen::arrange`]), and builds the project.
//! The UI thread only swaps the project in when it is ready.

use std::path::{Path, PathBuf};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use eframe::egui;
use loom_audio::PlayMode;
use loom_core::{Project, midi_file};
use loom_listen::{arrange, decode_file};

use super::LoomApp;

/// What the import thread gives back: the project and a summary of what
/// it heard, or why it failed.
type Outcome = Result<(Project, String), String>;

/// An import that is running.
pub(super) struct Import {
    worker: JoinHandle<Outcome>,
}

/// Where the recording is saved for the sampler: next to the original,
/// as "song.ferrous-loom.wav", or in the temporary folder if that folder
/// cannot be written to.
fn wav_path(original: &Path) -> PathBuf {
    let stem = original
        .file_stem()
        .map_or_else(|| "recording".into(), |stem| stem.to_string_lossy());
    let name = format!("{stem}.ferrous-loom.wav");
    original.with_file_name(&name)
}

/// The whole job, run on the import thread.
fn listen(path: &Path) -> Outcome {
    let recording = decode_file(path).map_err(|err| err.to_string())?;
    let analysis = loom_listen::analyze(&recording);
    if analysis.beats.len() < 2 {
        return Err("no beat could be heard in it".to_string());
    }
    let lead_in = arrange::lead_in(&analysis);

    let mut wav = wav_path(path);
    if arrange::write_wav(&recording, lead_in.seconds, &wav).is_err() {
        wav = std::env::temp_dir().join(wav.file_name().unwrap_or_default());
        arrange::write_wav(&recording, lead_in.seconds, &wav).map_err(|err| err.to_string())?;
    }

    let title = path
        .file_stem()
        .map_or_else(|| "Recording".into(), |stem| stem.to_string_lossy());
    let project = arrange::follow(&analysis, lead_in, wav, &title);
    let key = analysis
        .scale
        .map_or_else(|| "no clear key".to_string(), |scale| scale.name());
    let chords = analysis.chords.iter().filter(|c| c.is_some()).count();
    let summary = format!(
        "{title}: {:.1} BPM, {}/4, {key}, chords on {chords} of {} beats",
        analysis.bpm,
        analysis.beats_per_bar,
        analysis.beats.len()
    );
    Ok((project, summary))
}

impl LoomApp {
    /// Asks for a recording, then starts listening to it.
    pub(super) fn import_recording(&mut self) {
        if self.import.is_some() {
            return;
        }
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("Audio", &loom_listen::AUDIO_EXTENSIONS)
            .pick_file()
        {
            self.import_path(path);
        }
    }

    /// Starts listening to the recording at `path`. Opening an audio file
    /// (`ferrous-loom song.mp3`) comes here too.
    pub(super) fn import_path(&mut self, path: PathBuf) {
        if self.import.is_some() {
            return;
        }
        let name = path
            .file_name()
            .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
        self.notice = Some(format!("Listening to {name}…"));
        self.message = None;
        self.import = Some(Import {
            worker: thread::spawn(move || listen(&path)),
        });
    }

    /// Swaps the new project in when the import has finished, and plays it.
    pub(super) fn check_import(&mut self, ctx: &egui::Context) {
        let Some(import) = &self.import else {
            return;
        };
        if !import.worker.is_finished() {
            ctx.request_repaint_after(Duration::from_millis(100));
            return;
        }
        if let Some(import) = self.import.take() {
            match import.worker.join() {
                Ok(Ok((project, summary))) => {
                    self.replace_project(project, None);
                    self.notice = Some(summary);
                    self.set_mode(PlayMode::Song);
                }
                Ok(Err(err)) => {
                    self.notice = None;
                    self.message = Some(format!("Could not import: {err}"));
                }
                Err(_) => {
                    self.notice = None;
                    self.message = Some("Importing failed".to_string());
                }
            }
        }
    }
}

/// File > Import MIDI file. Reading a MIDI file takes a few thousandths
/// of a second, so unlike a recording it is done straight away.
impl LoomApp {
    /// Asks for a MIDI file, then imports it.
    pub(super) fn import_midi(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("MIDI file", &midi_file::EXTENSIONS)
            .pick_file()
        {
            self.import_midi_path(&path);
        }
    }

    /// Makes a new project from the MIDI file at `path`, in song mode,
    /// ready to play. Opening a MIDI file (`ferrous-loom song.mid`, or
    /// dropping it on the window) comes here too.
    pub(super) fn import_midi_path(&mut self, path: &Path) {
        let title = path
            .file_stem()
            .map_or_else(|| "MIDI file".into(), |stem| stem.to_string_lossy());
        let imported = std::fs::read(path)
            .map_err(|err| err.to_string())
            .and_then(|bytes| midi_file::read::read(&bytes).map_err(|err| err.to_string()))
            .and_then(|file| {
                midi_file::import::import(&file, &title).map_err(|err| err.to_string())
            });
        match imported {
            Ok(imported) => {
                let summary = imported.report.summary(&title, &imported.project);
                self.replace_project(imported.project, None);
                self.set_mode(PlayMode::Song);
                self.notice = Some(summary);
            }
            Err(err) => {
                self.message = Some(format!("Could not import {}: {err}", path.display()));
            }
        }
    }
}
