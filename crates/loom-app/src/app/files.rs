//! New, Open, Save and Save As, and asking before unsaved changes are lost.
//!
//! This is a child module of `app` (its file is `app/files.rs`), so it can
//! use the private fields of [`LoomApp`]: in Rust, private means "visible
//! in this module and the modules inside it". The `impl LoomApp` block
//! below adds more methods to the same struct; a type can have as many
//! `impl` blocks as you like, in different files of the same crate.

use std::path::{Path, PathBuf};

use eframe::egui::{self, Key, KeyboardShortcut, Modifiers};
use loom_core::{Project, file};

use super::LoomApp;
use super::export::MidiExport;
use super::library::Demo;
use crate::channel_rack::is_project_file;
use crate::library_browser::Tried;
use crate::piano_roll::PianoRoll;

const NEW: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::N);
const OPEN: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::O);
const SAVE: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::S);
const SAVE_AS: KeyboardShortcut =
    KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::S);
const QUIT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Q);

/// Something that would throw away the current project, waiting for the
/// user to say what to do with unsaved changes.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum Pending {
    New,
    Open,
    OpenPath(PathBuf),
    Demo(Demo),
    Import,
    ImportMidi,
    Quit,
}

/// The answer to "Save changes?".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Choice {
    Save,
    Discard,
    Cancel,
}

/// What the app remembers about the project's file.
pub(super) struct FileState {
    /// Where the project was last opened from or saved to.
    path: Option<PathBuf>,
    /// A copy of the project as it was when last opened or saved. If the
    /// project differs from it, there are unsaved changes. Comparing the
    /// two (thanks to the derived `PartialEq`) means even undoing a change
    /// by hand counts as "no changes".
    saved: Project,
    pending: Option<Pending>,
    /// Set once the user agreed to quit, so closing is no longer stopped.
    quitting: bool,
    /// The window title last sent, so a new one is only sent when it changes.
    title: String,
}

impl FileState {
    pub(super) fn new(project: &Project) -> Self {
        FileState {
            path: None,
            saved: project.clone(),
            pending: None,
            quitting: false,
            title: String::new(),
        }
    }

    /// The file's name, such as "song.loom", or "Untitled".
    fn display_name(&self) -> String {
        self.path.as_deref().and_then(Path::file_name).map_or_else(
            || "Untitled".to_string(),
            |name| name.to_string_lossy().into_owned(),
        )
    }

    /// A name for an exported file: "song.wav" for "song.loom", when
    /// `extension` is "wav".
    pub(super) fn export_name(&self, extension: &str) -> String {
        let stem = self.path.as_deref().and_then(Path::file_stem).map_or_else(
            || "Untitled".to_string(),
            |stem| stem.to_string_lossy().into_owned(),
        );
        format!("{stem}.{extension}")
    }
}

/// A file dialog that shows project files.
fn project_dialog() -> rfd::FileDialog {
    rfd::FileDialog::new().add_filter("Ferrous Loom project", &[file::EXTENSION])
}

impl LoomApp {
    fn has_unsaved_changes(&self) -> bool {
        self.project != self.file.saved
    }

    /// The File menu at the left of the transport bar.
    pub(super) fn file_menu(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let item = |text, shortcut: KeyboardShortcut| {
            egui::Button::new(text).shortcut_text(ctx.format_shortcut(&shortcut))
        };
        ui.menu_button("File", |ui| {
            if ui.add(item("New", NEW)).clicked() {
                self.request(&ctx, Pending::New);
                ui.close();
            }
            if ui.add(item("Open…", OPEN)).clicked() {
                self.request(&ctx, Pending::Open);
                ui.close();
            }
            self.demos_menu(ui);
            if ui
                .button("Import recording…")
                .on_hover_text("Open an MP3 or WAV file and add chords that follow it")
                .clicked()
            {
                self.request(&ctx, Pending::Import);
                ui.close();
            }
            if ui
                .button("Import MIDI file…")
                .on_hover_text("Open a .mid file: its notes, tempo, time signatures and key")
                .clicked()
            {
                self.request(&ctx, Pending::ImportMidi);
                ui.close();
            }
            ui.separator();
            if ui.add(item("Save", SAVE)).clicked() {
                self.save();
                ui.close();
            }
            if ui.add(item("Save as…", SAVE_AS)).clicked() {
                self.save_as();
                ui.close();
            }
            ui.separator();
            let export = ui
                .add_enabled(self.can_export(), egui::Button::new("Export WAV…"))
                .on_hover_text("Save the song as a sound file")
                .on_disabled_hover_text("Put some clips in the playlist first");
            if export.clicked() {
                self.export_song();
                ui.close();
            }
            let has_song = self.project.song_length() > 0;
            let song_midi = ui
                .add_enabled(has_song, egui::Button::new("Export song as MIDI…"))
                .on_hover_text("Save the notes of the song, to open in another studio")
                .on_disabled_hover_text("Put some clips in the playlist first");
            if song_midi.clicked() {
                self.export_midi(MidiExport::Song);
                ui.close();
            }
            if ui
                .button("Export pattern as MIDI…")
                .on_hover_text("Save the notes of the selected pattern")
                .clicked()
            {
                self.export_midi(MidiExport::Pattern);
                ui.close();
            }
            ui.separator();
            if ui.add(item("Quit", QUIT)).clicked() {
                self.request(&ctx, Pending::Quit);
                ui.close();
            }
        });
    }

    pub(super) fn file_shortcuts(&mut self, ctx: &egui::Context) {
        // `consume_shortcut` checks for the keys and, if they were pressed,
        // removes them so no text field sees them too. Ctrl+Shift+S is
        // checked before Ctrl+S, because Ctrl+S would also match it.
        let pressed = |shortcut| ctx.input_mut(|input| input.consume_shortcut(&shortcut));
        if pressed(SAVE_AS) {
            self.save_as();
        } else if pressed(SAVE) {
            self.save();
        }
        if pressed(NEW) {
            self.request(ctx, Pending::New);
        }
        if pressed(OPEN) {
            self.request(ctx, Pending::Open);
        }
        if pressed(QUIT) {
            self.request(ctx, Pending::Quit);
        }
    }

    /// Opens a project file, after asking about unsaved changes.
    pub(super) fn open_path(&mut self, ctx: &egui::Context, path: PathBuf) {
        self.request(ctx, Pending::OpenPath(path));
    }

    /// Does `action` now if nothing would be lost, or asks first.
    pub(super) fn request(&mut self, ctx: &egui::Context, action: Pending) {
        if self.has_unsaved_changes() {
            self.file.pending = Some(action);
        } else {
            self.perform(ctx, action);
        }
    }

    fn perform(&mut self, ctx: &egui::Context, action: Pending) {
        match action {
            Pending::New => self.replace_project(Project::template(), None),
            Pending::Open => {
                // This waits until the user picks a file or cancels. The
                // window does not redraw meanwhile, but the music goes on:
                // it runs on the audio thread.
                if let Some(path) = project_dialog().pick_file() {
                    self.open_now(path);
                }
            }
            Pending::OpenPath(path) => self.open_now(path),
            Pending::Demo(demo) => self.open_demo(demo),
            Pending::Import => self.import_recording(),
            Pending::ImportMidi => self.import_midi(),
            Pending::Quit => {
                self.file.quitting = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }

    fn open_now(&mut self, path: PathBuf) {
        if loom_listen::is_audio_file(&path) {
            self.import_path(path);
            return;
        }
        if loom_core::midi_file::is_midi_file(&path) {
            self.import_midi_path(&path);
            return;
        }
        match file::load(&path) {
            Ok(project) => self.replace_project(project, Some(path)),
            Err(err) => self.message = Some(format!("Could not open {}: {err}", path.display())),
        }
    }

    /// Swaps in another project and tells the audio thread about it.
    pub(super) fn replace_project(&mut self, project: Project, path: Option<PathBuf>) {
        self.project = project;
        self.auditions.clear();
        self.song_library = Tried::default();
        self.song_versions.clear();
        self.file.saved = self.project.clone();
        self.file.path = path;
        self.pattern = 0;
        self.playing = false;
        self.cursor = 0;
        self.marker = 0;
        self.piano_roll = PianoRoll::new(super::first_synth(&self.project));
        self.harmony_keys.forget_chords();
        self.message = None;
        self.notice = None;
        // Undo cannot go back into another file, and its samples may
        // not be needed any more.
        self.history.clear();
        self.samples.clear();
        self.send_whole_project();
    }

    /// Saves to the project's file, or asks for one if it has none yet.
    /// Returns whether the project was saved.
    fn save(&mut self) -> bool {
        match self.file.path.clone() {
            Some(path) => self.save_to(path),
            None => self.save_as(),
        }
    }

    /// Asks where to save, then saves. Returns whether the project was saved.
    fn save_as(&mut self) -> bool {
        let suggested = match &self.file.path {
            Some(_) => self.file.display_name(),
            None => format!("Untitled.{}", file::EXTENSION),
        };
        let Some(mut path) = project_dialog().set_file_name(suggested).save_file() else {
            return false;
        };
        if path.extension().is_none() {
            path.set_extension(file::EXTENSION);
        }
        self.save_to(path)
    }

    fn save_to(&mut self, path: PathBuf) -> bool {
        match file::save(&self.project, &path) {
            Ok(()) => {
                self.file.saved = self.project.clone();
                self.file.path = Some(path);
                self.message = None;
                true
            }
            Err(err) => {
                self.message = Some(format!("Could not save {}: {err}", path.display()));
                false
            }
        }
    }

    /// A project file dropped on the window is opened. While the channel
    /// rack is closed, dropped samples become new channels (when it is
    /// open, the rack handles them, so they can land on a row).
    pub(super) fn handle_dropped_files(&mut self, ctx: &egui::Context) {
        let dropped: Vec<PathBuf> = ctx.input(|input| {
            input
                .raw
                .dropped_files
                .iter()
                .map(|dropped| dropped.path().to_path_buf())
                .collect()
        });
        let (projects, samples): (Vec<PathBuf>, Vec<PathBuf>) =
            dropped.into_iter().partition(|path| is_project_file(path));
        if let Some(path) = projects.into_iter().next() {
            self.open_path(ctx, path);
        }
        if !self.rack_open {
            for path in samples {
                self.load_sample(None, path);
            }
        }
    }

    /// Shows "Save changes?" while an action is waiting for an answer.
    /// Also stops the window from closing while there are unsaved changes.
    pub(super) fn unsaved_changes_dialog(&mut self, ctx: &egui::Context) {
        let close_requested = ctx.input(|input| input.viewport().close_requested());
        if close_requested && !self.file.quitting && self.has_unsaved_changes() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.file.pending = Some(Pending::Quit);
        }

        let Some(pending) = self.file.pending.clone() else {
            return;
        };
        let name = self.file.display_name();
        let mut choice = None;
        let modal = egui::Modal::new(egui::Id::unique("unsaved-changes")).show(ctx, |ui| {
            ui.set_width(320.0);
            ui.heading("Save changes?");
            ui.add_space(4.0);
            ui.label(format!("\"{name}\" has changes that are not saved yet."));
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("Save").clicked() {
                    choice = Some(Choice::Save);
                }
                if ui.button("Don't save").clicked() {
                    choice = Some(Choice::Discard);
                }
                if ui.button("Cancel").clicked() {
                    choice = Some(Choice::Cancel);
                }
            });
        });
        // Escape, or a click outside the dialog, means "Cancel".
        if modal.should_close() && choice.is_none() {
            choice = Some(Choice::Cancel);
        }

        match choice {
            Some(Choice::Save) => {
                self.file.pending = None;
                if self.save() {
                    self.perform(ctx, pending);
                }
            }
            Some(Choice::Discard) => {
                self.file.pending = None;
                self.perform(ctx, pending);
            }
            // Nothing happens, which is easy to miss: say so.
            Some(Choice::Cancel) => {
                self.file.pending = None;
                self.notice = Some(format!("Cancelled: \"{name}\" stays open as it is."));
            }
            None => {}
        }
    }

    /// Shows the file name in the window's title bar, with a dot in front
    /// when there are unsaved changes.
    pub(super) fn update_title(&mut self, ctx: &egui::Context) {
        let changed = if self.has_unsaved_changes() {
            "● "
        } else {
            ""
        };
        let title = format!("{changed}{} · Ferrous Loom", self.file.display_name());
        if title != self.file.title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.file.title = title;
        }
    }
}
