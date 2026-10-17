//! The piano roll: a window for drawing notes, with harmony helpers.
//!
//! Like the channel rack, [`PianoRoll::show`] never changes the project:
//! it returns [`RollAction`]s for the app to apply. Unlike the rack, the
//! piano roll has state of its own that must last from one frame to the
//! next (which channel it edits, what a click draws, the selected notes,
//! a drag in progress), so it is a struct with methods instead of a
//! single function.

use eframe::egui::{
    self, Align, Align2, Color32, FontId, Key, KeyboardShortcut, Modifiers, Painter, Pos2, Rect,
    Response, Sense, Stroke, StrokeKind, Ui, Vec2, pos2, vec2,
};
use loom_core::harmony::keys::{self, ChordSize};
use loom_core::harmony::{Harmony, HintLevel, analyze, next_chords};
use loom_core::tools::{self, ArpeggioOrder};
use loom_core::{
    Chord, ChordQuality, ChordStamp, Note, NoteEvent, Pattern, PitchClass, PitchSet, Project,
    Scale, ScaleKind, Spelling, TICKS_PER_STEP, TimeSignature, tidy_notes,
};

use crate::micromodes::{self, MicroAction, MicromodeWindow};
use crate::theme;
use crate::twelve_tone::{self, RowAction, RowWindow, SetAction, SetWindow};

/// Something the user asked the piano roll to do.
#[derive(Debug, Clone, PartialEq)]
pub enum RollAction {
    /// Add one note, or the notes of a chord.
    AddNotes {
        channel: usize,
        notes: Vec<NoteEvent>,
    },
    RemoveNote {
        channel: usize,
        index: usize,
    },
    /// Replace a note that was moved or resized.
    ReplaceNote {
        channel: usize,
        index: usize,
        note: NoteEvent,
    },
    /// Replace some notes with others, all at once: a moved selection,
    /// a tool, new velocities. `label` names the undo step.
    ReplaceNotes {
        channel: usize,
        old: Vec<NoteEvent>,
        new: Vec<NoteEvent>,
        label: &'static str,
    },
    /// Play some notes right now, without adding them.
    Preview {
        channel: usize,
        pitches: Vec<Note>,
    },
    SetScale(Scale),
    /// Show another pattern: one where the channel has notes.
    SelectPattern(usize),
    /// Open the channel rack, where drum steps are edited.
    OpenRack,
}

/// The range of notes shown, from C1 to B7.
const LOWEST: u8 = 24;
const HIGHEST: u8 = 107;

const ROW_HEIGHT: f32 = 12.0;
const KEY_WIDTH: f32 = 54.0;
/// The narrowest a step is at 100%, so a long pattern can still be read;
/// it then scrolls sideways.
const MIN_STEP_WIDTH: f32 = 8.0;
const LANE_HEIGHT: f32 = 22.0;
const VELOCITY_HEIGHT: f32 = 56.0;
/// How close to a note's right end (in pixels) a click resizes it instead of moving it.
const EDGE_GRAB: f32 = 6.0;

/// The note lengths offered for new notes, in steps.
const NOTE_LENGTHS: [(usize, &str); 5] = [
    (1, "1 step"),
    (2, "2 steps"),
    (4, "1 beat"),
    (8, "2 beats"),
    (16, "1 bar"),
];

/// How fast an arpeggio goes: how long each of its notes is, in steps.
const ARPEGGIO_RATES: [(usize, &str); 3] = [(1, "1 step"), (2, "2 steps"), (4, "1 beat")];

const INVERSIONS: [&str; 6] = [
    "Root position",
    "1st inversion",
    "2nd inversion",
    "3rd inversion",
    "4th inversion",
    "5th inversion",
];

/// The same, short enough for the toolbar.
const INVERSIONS_SHORT: [&str; 6] = [
    "Root", "1st inv.", "2nd inv.", "3rd inv.", "4th inv.", "5th inv.",
];

/// Below this height (in points, under the toolbar) the piano roll
/// leaves out its hint lines, to give the grid the room.
const COMPACT_HEIGHT: f32 = 420.0;

const ROOT_ROW: Color32 = Color32::from_rgb(78, 70, 60);
const SCALE_ROW: Color32 = Color32::from_rgb(60, 70, 76);
const OTHER_ROW: Color32 = Color32::from_rgb(44, 51, 56);
const ROW_LINE: Color32 = Color32::from_rgb(36, 42, 46);
const OCTAVE_LINE: Color32 = Color32::from_rgb(22, 26, 29);
const STEP_LINE: Color32 = Color32::from_rgb(52, 60, 66);
const BEAT_LINE: Color32 = Color32::from_rgb(34, 40, 44);
const BAR_LINE: Color32 = Color32::from_rgb(20, 24, 27);
const WHITE_KEY: Color32 = Color32::from_rgb(222, 224, 228);
const BLACK_KEY: Color32 = Color32::from_rgb(40, 42, 46);
const KEY_TEXT: Color32 = Color32::from_rgb(60, 62, 68);
const KEY_HELD: Color32 = Color32::from_rgb(255, 152, 48);
const SCALE_MARK: Color32 = Color32::from_rgb(120, 190, 255);
const ROW_MARK: Color32 = Color32::from_rgb(54, 96, 168);
const ROOT_MARK: Color32 = Color32::from_rgb(255, 152, 48);
const NOTE: Color32 = Color32::from_rgb(118, 214, 140);
const NOTE_OUTSIDE: Color32 = Color32::from_rgb(240, 120, 110);
const NOTE_BORDER: Color32 = Color32::from_rgb(24, 40, 30);
const NOTE_TEXT: Color32 = Color32::from_rgb(16, 28, 20);
const SELECTED_BORDER: Color32 = Color32::from_rgb(255, 255, 255);
const STAMP_GHOST: Color32 = Color32::from_rgba_premultiplied(60, 70, 64, 60);
const OTHER_CHANNEL: Color32 = Color32::from_rgba_premultiplied(70, 76, 90, 110);
const OTHER_CHANNEL_BORDER: Color32 = Color32::from_rgb(120, 128, 146);
const SELECTION_BOX: Color32 = Color32::from_rgba_premultiplied(40, 60, 90, 60);
const PLAYHEAD: Color32 = Color32::from_rgba_premultiplied(64, 38, 12, 60);
const LANE_BACKGROUND: Color32 = Color32::from_rgb(36, 42, 46);
const LANE_CHORD: Color32 = Color32::from_rgb(72, 88, 120);
const LANE_TEXT: Color32 = Color32::from_rgb(235, 238, 245);
const NO_NOTES_BACKGROUND: Color32 = Color32::from_rgb(70, 52, 30);
const NO_NOTES_TEXT: Color32 = Color32::from_rgb(150, 156, 170);

/// A drag that started on a note and lasts until the mouse button is released.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Drag {
    /// Moving a note. `grab` is how many steps after the note's start the
    /// pointer took hold of it, so the note does not jump under the pointer.
    Move { index: usize, grab: usize },
    /// Changing a note's length by dragging its right end.
    Resize { index: usize },
}

/// Where the chord that next-chord hints follow comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChordAt {
    /// The chord at a step of the pattern.
    Step(usize),
    /// The keys held down on a keyboard.
    Keys,
}

impl Drag {
    fn index(self) -> usize {
        match self {
            Drag::Move { index, .. } | Drag::Resize { index } => index,
        }
    }
}

/// Moving the selected notes together.
#[derive(Debug, Clone, PartialEq)]
struct GroupDrag {
    /// The cell where the drag started.
    from_step: usize,
    from_pitch: Note,
    /// The notes as they were when the drag started.
    originals: Vec<NoteEvent>,
    /// The notes as they are now, after the moves so far.
    moved: Vec<NoteEvent>,
    /// Whether the notes have moved at all. With snap to scale on, notes
    /// snap only once they are moved, as in FL Studio: dragging a
    /// selection up a semitone and back down snaps it into the key.
    edited: bool,
}

/// Something that changes the selected notes (or every note, when none
/// is selected), from the Tools menu or a key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tool {
    Transpose(i32),
    InKey(i32),
    Shift(isize),
    Quantize,
    SnapToKey,
    Invert { up: bool },
    Arpeggiate,
    Delete,
    SelectAll,
    SelectNone,
}

/// What the grid needs to know besides the notes, gathered in one value
/// so the drawing functions do not need a long list of arguments.
#[derive(Debug, Clone, Copy)]
struct View<'a> {
    scale: Scale,
    step_width: f32,
    playhead: Option<usize>,
    held: &'a [Note],
}

/// Where the grid is on screen, and how to turn steps and pitches into
/// pixels and back. It is `Copy`, so it can be handed around freely.
#[derive(Debug, Clone, Copy)]
struct Grid {
    /// Where all the steps and rows are, even those scrolled out of view.
    rect: Rect,
    step_width: f32,
    /// How tall a row (one pitch) is: the vertical zoom.
    row_height: f32,
    length: usize,
    time: TimeSignature,
    /// The left edge of the keyboard. The keyboard stays at the left of
    /// the view while the grid scrolls sideways under it.
    keys_left: f32,
}

impl Grid {
    /// The keyboard, at the left of the view.
    fn keys(self) -> Rect {
        Rect::from_min_max(
            pos2(self.keys_left, self.rect.top()),
            pos2(self.keys_left + KEY_WIDTH, self.rect.bottom()),
        )
    }

    /// The part of the grid not under the keyboard, where notes are drawn
    /// and clicked.
    fn cells(self) -> Rect {
        let mut cells = self.rect;
        cells.min.x = cells.min.x.max(self.keys().right());
        cells
    }

    /// The x coordinate where `step` begins.
    fn x(self, step: usize) -> f32 {
        self.rect.left() + step as f32 * self.step_width
    }

    /// The step under an x coordinate, kept inside the grid.
    fn step_at(self, x: f32) -> usize {
        let last_step = self.length.saturating_sub(1) as f32;
        ((x - self.rect.left()) / self.step_width)
            .floor()
            .clamp(0.0, last_step) as usize
    }

    /// The y coordinate of the top of a pitch's row (high notes at the top).
    fn row_top(self, midi: u8) -> f32 {
        self.rect.top() + (HIGHEST - midi) as f32 * self.row_height
    }

    /// Where a note is drawn: from its exact start to its exact end, so
    /// a triplet sits between two steps, as it is heard.
    fn note_rect(self, note: &NoteEvent) -> Rect {
        let top = self.row_top(note.pitch.midi());
        let tick = self.step_width / TICKS_PER_STEP as f32;
        let start = self.x(note.start) + f32::from(note.nudge) * tick;
        let end = if note.end() > self.length {
            self.x(self.length)
        } else {
            self.x(note.end()) - f32::from(note.trim) * tick
        };
        Rect::from_min_max(pos2(start, top), pos2(end, top + self.row_height))
    }

    /// The step and pitch under a point. Points outside the grid give the
    /// nearest cell, so a drag can go past the edges.
    fn cell_at(self, pos: Pos2) -> (usize, Note) {
        let row = ((pos.y - self.rect.top()) / self.row_height)
            .floor()
            .clamp(0.0, (HIGHEST - LOWEST) as f32) as u8;
        (self.step_at(pos.x), Note::new(HIGHEST - row))
    }

    /// The size of the text written on notes and keys, which grows and
    /// shrinks with the rows.
    fn text_size(self) -> f32 {
        (self.row_height * 0.8).clamp(7.0, 14.0)
    }
}

/// What the chord and velocity lanes need to line up with the grid.
#[derive(Debug, Clone, Copy)]
struct Lanes {
    step_width: f32,
    /// How far the grid is scrolled sideways, in pixels.
    scroll_x: f32,
    length: usize,
}

impl Lanes {
    /// A lane in `rect`, its label where the keyboard is, its steps lined
    /// up with the grid's.
    fn grid(self, rect: Rect, time: TimeSignature) -> Grid {
        let left = rect.left() + KEY_WIDTH - self.scroll_x;
        Grid {
            rect: Rect::from_min_max(
                pos2(left, rect.top()),
                pos2(left + self.step_width * self.length as f32, rect.bottom()),
            ),
            step_width: self.step_width,
            row_height: rect.height(),
            length: self.length,
            time,
            keys_left: rect.left(),
        }
    }
}

/// The piano roll window and everything it remembers between frames.
pub struct PianoRoll {
    pub open: bool,
    channel: usize,
    /// The pattern shown last time, to notice when it changes.
    pattern: usize,
    stamp: ChordStamp,
    /// How many times stamped chords are inverted (0 is root position).
    inversion: usize,
    /// The length of new notes, in steps. Resizing a note changes it, so
    /// the next note drawn is as long as the last one edited.
    note_length: usize,
    drag: Option<Drag>,
    group_drag: Option<GroupDrag>,
    /// Where a selection box started, while Ctrl is held and the button is down.
    selection_box: Option<Pos2>,
    /// The selected notes, by value. A note that is no longer in the
    /// pattern (after an undo, say) is simply not found, and so not
    /// selected: the selection can never point at the wrong note.
    selection: Vec<NoteEvent>,
    /// Show the notes of the other channels behind this one's, and name
    /// the chords they make together.
    ghosts: bool,
    arpeggio: ArpeggioOrder,
    arpeggio_rate: usize,
    /// Scroll to the channel's notes the next time the grid is drawn.
    scroll_to_notes: bool,
    /// The lowest and highest pitch in view when the grid was last drawn.
    in_view: Option<(u8, u8)>,
    /// How far the grid was scrolled last frame, and where on screen its
    /// view was: to keep the notes under the pointer in place when zooming.
    scrolled: Vec2,
    view_rect: Option<Rect>,
    /// Where the "no notes here" message was drawn over the grid last
    /// frame, so a click on it does not also draw a note under it.
    banner: Option<Rect>,
    /// Where to scroll the grid to the next time it is drawn.
    scroll_to: Option<Vec2>,
    /// Whether the window was open last frame, to notice it opening.
    was_open: bool,
    /// How much wider and taller than usual the steps and rows are: the
    /// piano roll's horizontal and vertical zoom.
    zoom: Vec2,
    /// The step the pointer is over, if any, to name the chord there.
    hovered_step: Option<usize>,
    /// The last step the pointer was over. Unlike `hovered_step`, it stays
    /// when the pointer leaves, so the next-chord hints stay up while you
    /// move to the buttons.
    last_step: Option<usize>,
    /// Whether the arrow keys and Delete go to the piano roll: from a
    /// click inside the window until a click outside it.
    has_keyboard: bool,
    /// Where the window was drawn last frame.
    window_rect: Option<Rect>,
    /// The project's time signature, which says where the bar and beat
    /// lines go. Copied from the project on every frame.
    time: TimeSignature,
    /// How much the next-chord hints explain. Set by the app from the
    /// harmony keys bar, where it is chosen.
    pub hints: HintLevel,
    /// Whether notes drawn or moved land on the notes of the key, like
    /// FL Studio's "Snap to scale".
    snap_to_scale: bool,
    /// Whether notes played on the typing or MIDI keyboard are moved to
    /// the nearest note of the key, like FL Studio's "Snap incoming MIDI".
    /// The app reads it.
    pub snap_played: bool,
    /// The twelve-tone row window and the set browser.
    row_window: RowWindow,
    set_window: SetWindow,
    /// Kröpfl's micromodes, linked to the set browser.
    micromode_window: MicromodeWindow,
}

impl PianoRoll {
    /// A closed piano roll for `channel`.
    pub fn new(channel: usize) -> Self {
        PianoRoll {
            open: false,
            channel,
            pattern: 0,
            stamp: ChordStamp::Single,
            inversion: 0,
            note_length: 4,
            drag: None,
            group_drag: None,
            selection_box: None,
            selection: Vec::new(),
            ghosts: true,
            arpeggio: ArpeggioOrder::Up,
            arpeggio_rate: 1,
            scroll_to_notes: true,
            in_view: None,
            scrolled: Vec2::ZERO,
            view_rect: None,
            banner: None,
            scroll_to: None,
            was_open: false,
            zoom: Vec2::splat(1.0),
            hovered_step: None,
            last_step: None,
            has_keyboard: false,
            window_rect: None,
            time: TimeSignature::COMMON,
            hints: HintLevel::default(),
            snap_to_scale: false,
            snap_played: false,
            row_window: RowWindow::new(),
            set_window: SetWindow::new(),
            micromode_window: MicromodeWindow::new(),
        }
    }

    /// `pitch`, moved to the nearest note of `scale` if snap to scale is on.
    fn snapped(&self, pitch: Note, scale: Scale) -> Note {
        match self.snap_to_scale {
            true => tools::nearest_in_key(pitch, scale),
            false => pitch,
        }
    }

    /// The id of the piano roll window.
    pub fn id() -> egui::Id {
        egui::Id::unique("piano-roll")
    }

    /// Whether a window is one the piano roll opens: the row window, the
    /// set browser or the micromode window.
    pub fn owns(id: egui::Id) -> bool {
        id == twelve_tone::row_window_id()
            || id == twelve_tone::set_window_id()
            || id == micromodes::window_id()
    }

    /// The channel shown: the selected channel, which keyboards play.
    pub fn channel(&self) -> usize {
        self.channel
    }

    /// Shows another channel, without opening the window.
    pub fn select(&mut self, channel: usize) {
        let open = self.open;
        self.open_for(channel);
        self.open = open;
    }

    /// Opens the piano roll on a channel.
    pub fn open_for(&mut self, channel: usize) {
        if channel != self.channel {
            self.scroll_to_notes = true;
            self.selection.clear();
        }
        self.open = true;
        self.channel = channel;
        self.stop_dragging();
    }

    /// Keeps pointing at the same channel after channel `removed` is
    /// deleted, or closes the window if it was this one. `left` is how
    /// many channels are left: the window then points at the channel
    /// that took the removed one's place (or the last one), so it opens
    /// again on a channel that is there.
    pub fn channel_removed(&mut self, removed: usize, left: usize) {
        if removed == self.channel {
            self.open = false;
            self.selection.clear();
            self.channel = self.channel.min(left.saturating_sub(1));
        } else if removed < self.channel {
            self.channel -= 1;
        }
        self.stop_dragging();
    }

    /// Called after a channel was inserted above or at the one shown (by
    /// undo, for example), so the piano roll keeps showing the same channel.
    pub fn channel_inserted(&mut self, inserted: usize) {
        if inserted <= self.channel {
            self.channel += 1;
        }
        self.stop_dragging();
    }

    fn stop_dragging(&mut self) {
        self.drag = None;
        self.group_drag = None;
        self.selection_box = None;
    }

    /// Draws the window, if it is open, and returns what the user did.
    /// It shows the notes of the selected channel in pattern `pattern`;
    /// `playhead` is the step of that pattern being played, if any, and
    /// `held` the keys held down on a keyboard. The window stays inside
    /// `area`, the part of the main window the playlist uses. Its steps
    /// are `zoom.x` times wider than usual and its rows `zoom.y` times taller.
    #[allow(clippy::too_many_arguments)]
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        project: &Project,
        pattern: usize,
        playhead: Option<usize>,
        held: &[Note],
        area: egui::Rect,
        zoom: Vec2,
    ) -> Vec<RollAction> {
        let mut actions = Vec::new();
        if zoom != self.zoom {
            self.zoom_to(ctx, zoom);
        }
        let Some(pattern_notes) = project.pattern(pattern) else {
            self.open = false;
            return actions;
        };
        let opened = self.open && !self.was_open;
        let new_pattern = pattern != self.pattern;
        if new_pattern {
            self.pattern = pattern;
            self.scroll_to_notes = true;
            self.selection.clear();
            self.stop_dragging();
        }
        if opened || new_pattern {
            self.follow_notes(pattern_notes);
        }
        if opened {
            self.scroll_to_notes = true;
        }
        let Some(channel) = project.channels().get(self.channel) else {
            self.open = false;
            return actions;
        };

        // A click inside the window gives it the keyboard; a click
        // anywhere else takes the keyboard away again.
        let press = ctx.input(|input| crate::widgets::press_pos(input, None));
        if let Some(pos) = press {
            self.has_keyboard = self.window_rect.is_some_and(|rect| rect.contains(pos));
        }

        // `open` is a copy: the window needs `&mut open` while the closure
        // below needs `&mut self`, and both cannot borrow `self` at once.
        let mut open = self.open;
        let window = egui::Window::new(format!(
            "Piano roll · {} · {}",
            pattern_notes.name, channel.name
        ))
        .id(Self::id())
        .open(&mut open)
        .default_pos([150.0, 160.0])
        .default_size([1120.0, 580.0])
        .constrain_to(area)
        .show(ctx, |ui| {
            self.contents(ui, project, pattern_notes, playhead, held, &mut actions)
        });
        self.window_rect = window.map(|window| window.response.rect);
        self.open = open;
        self.was_open = open;
        if !open {
            self.has_keyboard = false;
        }
        if open {
            self.post_tonal_windows(ctx, project, pattern_notes, held, area, &mut actions);
        }
        actions
    }

    /// Changes the zoom, keeping the step and the pitch under the pointer
    /// where they are, or those in the middle of the view if the pointer
    /// is elsewhere: as in FL Studio, you zoom into what you point at.
    fn zoom_to(&mut self, ctx: &egui::Context, zoom: Vec2) {
        let ratio = zoom / self.zoom;
        self.zoom = zoom;
        let Some(view) = self.view_rect else {
            return;
        };
        let anchor = ctx
            .pointer_hover_pos()
            .filter(|pos| view.contains(*pos))
            .unwrap_or(view.center())
            - view.min;
        // The grid starts after the keyboard, which does not stretch.
        let from = self.scroll_to.unwrap_or(self.scrolled);
        let x = KEY_WIDTH + (from.x + anchor.x - KEY_WIDTH) * ratio.x - anchor.x;
        let y = (from.y + anchor.y) * ratio.y - anchor.y;
        self.scroll_to = Some(vec2(x.max(0.0), y.max(0.0)));
    }

    /// The row window and the set browser, when they are open.
    fn post_tonal_windows(
        &mut self,
        ctx: &egui::Context,
        project: &Project,
        pattern: &Pattern,
        held: &[Note],
        area: egui::Rect,
        actions: &mut Vec<RollAction>,
    ) {
        let scale = project.scale;
        twelve_tone::remember(ctx, scale);
        let length = pattern.length();
        let visible: Vec<NoteEvent> = pattern
            .notes(self.channel)
            .iter()
            .filter(|note| note.start < length)
            .copied()
            .collect();
        if self.row_window.open {
            // The notes a row is read from, in the order they start.
            let mut drawn = if self.selection.is_empty() {
                visible.clone()
            } else {
                self.selection.clone()
            };
            drawn.sort_by_key(|note| (note.start, note.pitch));
            let drawn: Vec<Note> = drawn.iter().map(|note| note.pitch).collect();
            for action in self.row_window.show(ctx, area, scale, &drawn, length) {
                match action {
                    RowAction::UseAsKey(row) => actions.push(RollAction::SetScale(row)),
                    RowAction::Write(notes) => {
                        let notes: Vec<NoteEvent> =
                            notes.into_iter().filter(|n| fits_view(&[*n])).collect();
                        self.selection.clear();
                        self.replace(pattern, actions, "write a row", visible.clone(), notes);
                    }
                }
            }
        }
        let selected: PitchSet = if held.is_empty() {
            self.selection
                .iter()
                .map(|note| note.pitch.pitch_class())
                .collect()
        } else {
            held.iter().map(|note| note.pitch_class()).collect()
        };
        if self.set_window.open {
            for action in self.set_window.show(ctx, area, scale, selected) {
                match action {
                    SetAction::Stamp(set) => {
                        self.stamp = ChordStamp::Set(set);
                        self.inversion = 0;
                    }
                    SetAction::UseAsKey(set) => actions.push(RollAction::SetScale(set)),
                    SetAction::ShowMicromode(trio) => {
                        self.micromode_window.pick(trio);
                        self.micromode_window.open = true;
                    }
                    SetAction::OpenMicromodes => self.micromode_window.open = true,
                }
            }
        }
        if self.micromode_window.open {
            for action in self.micromode_window.show(ctx, area, scale, selected) {
                match action {
                    MicroAction::Stamp(set) => {
                        self.stamp = ChordStamp::Set(set);
                        self.inversion = 0;
                    }
                    MicroAction::UseAsKey(set) => actions.push(RollAction::SetScale(set)),
                    MicroAction::Hear(pitches) => actions.push(RollAction::Preview {
                        channel: self.channel,
                        pitches,
                    }),
                    MicroAction::ShowSet(set) => {
                        self.set_window.pick(set);
                        self.set_window.open = true;
                    }
                }
            }
        }
    }

    /// When the channel shown has no notes in `pattern` but another
    /// channel plays a melody or chords there, shows that channel instead,
    /// so the piano roll never opens on an empty grid when there is
    /// something to see.
    fn follow_notes(&mut self, pattern: &Pattern) {
        if !pattern.notes(self.channel).is_empty() {
            return;
        }
        if let Some(channel) = busiest_channel(pattern) {
            self.channel = channel;
            self.selection.clear();
            self.scroll_to_notes = true;
        }
    }

    /// Said over the top of the grid when the channel has no notes in the
    /// pattern: what is going on, and buttons to go where the notes are.
    /// Returns where it was drawn.
    ///
    /// It lies over the grid instead of taking a line above it, so the
    /// grid does not jump up when the first note is drawn and the message
    /// goes away: the note would land rows below where it was clicked.
    fn no_notes(
        &mut self,
        ui: &mut Ui,
        project: &Project,
        pattern: &Pattern,
        actions: &mut Vec<RollAction>,
    ) -> Rect {
        let channel = project
            .channels()
            .get(self.channel)
            .map_or("This channel", |channel| channel.name.as_str());
        // Other patterns where this channel has notes.
        let elsewhere: Vec<(usize, &str)> = project
            .patterns()
            .iter()
            .enumerate()
            .filter(|&(index, other)| {
                index != self.pattern && !other.notes(self.channel).is_empty()
            })
            .map(|(index, other)| (index, other.name.as_str()))
            .collect();
        // Other channels with notes in this pattern.
        let others: Vec<(usize, &str, bool)> = (0..pattern.rows())
            .filter(|&other| other != self.channel && !pattern.notes(other).is_empty())
            .filter_map(|other| {
                let name = project.channels().get(other)?.name.as_str();
                Some((other, name, pattern.is_step_row(other)))
            })
            .collect();
        let mut switch_to = None;
        let frame = egui::Frame::new()
            .fill(NO_NOTES_BACKGROUND)
            .corner_radius(4.0)
            .inner_margin(egui::Margin::symmetric(8, 4))
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.strong(format!(
                        "{channel} has no notes in \"{}\". Click the grid to draw some.",
                        pattern.name
                    ));
                    if !elsewhere.is_empty() {
                        ui.label(format!("{channel} plays in:"));
                        for &(index, name) in &elsewhere {
                            if ui
                                .button(name)
                                .on_hover_text(format!("Show the pattern \"{name}\""))
                                .clicked()
                            {
                                actions.push(RollAction::SelectPattern(index));
                            }
                        }
                    }
                    let melodies: Vec<_> = others.iter().filter(|other| !other.2).collect();
                    if !melodies.is_empty() {
                        ui.label("Notes here are on:");
                        for &&(index, name, _) in &melodies {
                            if ui.button(name).clicked() {
                                switch_to = Some(index);
                            }
                        }
                    }
                    if others.iter().any(|other| other.2) {
                        ui.label("The drums in this pattern are edited in the");
                        if ui.button("Channel rack").clicked() {
                            actions.push(RollAction::OpenRack);
                        }
                    }
                });
            });
        if let Some(channel) = switch_to {
            self.open_for(channel);
        }
        frame.response.rect
    }

    fn contents(
        &mut self,
        ui: &mut Ui,
        project: &Project,
        pattern: &Pattern,
        playhead: Option<usize>,
        held: &[Note],
        actions: &mut Vec<RollAction>,
    ) {
        let scale = project.scale;
        // A pattern used where the song is in 3/4 is drawn in bars of 3/4.
        self.time = project.pattern_time(self.pattern);
        // Selected notes that are no longer there are forgotten.
        let notes = pattern.notes(self.channel);
        self.selection.retain(|note| notes.contains(note));

        // The other channels whose notes are shown faintly behind this
        // one's: those that play melodies or chords. Drum channels, whose
        // notes are all plain steps, would only add noise.
        let others: Vec<usize> = (0..pattern.rows())
            .filter(|&other| other != self.channel)
            .filter(|&other| pattern.notes(other).iter().any(|note| !note.is_step()))
            .collect();
        let mut heard = vec![self.channel];
        if self.ghosts {
            heard.extend(&others);
        }

        let tool = self.toolbar(ui, scale, others.is_empty(), actions);
        // In a short window the grid comes first: the hint and set lines
        // move into the tooltip of the line naming the chord, so a small
        // screen still shows a dozen rows of notes.
        let compact = ui.available_height() < COMPACT_HEIGHT;
        // A chord held on a keyboard comes first: it is what you are
        // thinking about right now.
        let held_chord = match analyze(held.iter().copied()) {
            Harmony::Chord { chord, .. } => Some((ChordAt::Keys, chord)),
            _ => None,
        };
        let current = held_chord.or_else(|| {
            let step = self.last_step.or(playhead)?;
            match analyze(sounding(pattern, &heard, step)) {
                Harmony::Chord { chord, .. } => Some((ChordAt::Step(step), chord)),
                _ => None,
            }
        });
        let next_hint = self.chords_in_key(ui, scale, current, compact, actions);

        // The step under the pointer was found while drawing the last
        // frame; one frame late is too fast for anyone to notice.
        let (info, set) = if held.is_empty() {
            match self.hovered_step.or(playhead) {
                Some(step) => {
                    let harmony = analyze(sounding(pattern, &heard, step));
                    let set: PitchSet = sounding(pattern, &heard, step)
                        .map(|note| note.pitch_class())
                        .collect();
                    (
                        format!("Step {}: {}", step + 1, harmony.describe(scale)),
                        set,
                    )
                }
                None => (
                    "Point at a step to name the chord there.".to_string(),
                    PitchSet::EMPTY,
                ),
            }
        } else {
            let harmony = analyze(held.iter().copied());
            let set: PitchSet = held.iter().map(|note| note.pitch_class()).collect();
            (format!("Keys down: {}", harmony.describe(scale)), set)
        };
        let info = ui.label(info);
        // Any group of notes, chord or not, has a set class. Shown for
        // three notes or more, and for the selection.
        let selected: PitchSet = self
            .selection
            .iter()
            .map(|note| note.pitch.pitch_class())
            .collect();
        let mut sets = Vec::new();
        if set.len() >= 3 {
            sets.push(format!("Set: {}", twelve_tone::describe_set(set, scale)));
        }
        if selected.len() >= 2 && self.selection.len() >= 2 {
            sets.push(format!(
                "Selected: {}",
                twelve_tone::describe_set(selected, scale)
            ));
        }
        // The line is always there, even empty, so the grid below does not
        // jump up and down as the pointer moves over it.
        if sets.is_empty() {
            sets.push(match self.hints {
                HintLevel::Minimal => " ".to_string(),
                _ => "Set: point at three notes or more, or select some, to name their set."
                    .to_string(),
            });
        }
        if compact {
            let sets = sets.join("\n");
            let more: Vec<&str> = [next_hint.as_str(), sets.as_str()]
                .into_iter()
                .filter(|text| !text.trim().is_empty())
                .collect();
            if !more.is_empty() {
                info.on_hover_text(more.join("\n\n"));
            }
        } else {
            ui.weak(sets.join("    ")).on_hover_text(
                "Pitch-class set theory names any group of notes: its Forte number, its \
                     notes packed as tightly as they go (normal form), the same packed from 0 \
                     with its inversion (prime form), and how many of each interval it holds. \
                     Sets... opens the list of every set.",
            );
        }
        ui.add_space(2.0);

        let length = pattern.length();
        // At 100% the whole pattern fits the window. Zooming in makes the
        // steps wider, and the grid scrolls sideways (Shift+wheel).
        let fit = ((ui.available_width() - KEY_WIDTH) / length as f32).max(MIN_STEP_WIDTH);
        let step_width = fit * self.zoom.x;
        self.hovered_step = None;

        // The chord lane's place is taken now, but it is drawn after the
        // grid, once it is known how far the grid is scrolled sideways.
        let (lane_rect, lane_response) =
            ui.allocate_exact_size(vec2(ui.available_width(), LANE_HEIGHT), Sense::hover());
        // The velocity lane goes below the grid, so the grid leaves room
        // for it: the lane, the gap, and egui's spacing between widgets.
        // Taking even a pixel too much would make the window grow a
        // little on every frame.
        let below = VELOCITY_HEIGHT + 4.0 + 2.0 * ui.spacing().item_spacing.y;
        let grid_height = (ui.available_height() - below).max(96.0);
        // Not animated: after a zoom, the notes that were under the
        // pointer must be there again on the very next frame.
        let grid_area = egui::ScrollArea::both()
            .id_salt("piano-roll-grid")
            .animated(false)
            .auto_shrink(false)
            .max_height(grid_height);
        let grid_area = match self.scroll_to.take() {
            Some(offset) => grid_area.scroll_offset(offset),
            None => grid_area,
        };
        crate::widgets::no_rubber_band(ui);
        let output = grid_area.show(ui, |ui| {
            let ghosts = if self.ghosts { &others[..] } else { &[] };
            let view = View {
                scale,
                step_width,
                playhead,
                held,
            };
            self.grid(ui, pattern, ghosts, view, actions)
        });
        self.scrolled = output.state.clamped_offset();
        self.view_rect = Some(output.inner_rect);
        self.banner = notes.is_empty().then(|| {
            // Drawn after the grid, so it is painted over it.
            let view = output.inner_rect;
            let place = Rect::from_min_max(
                view.min + vec2(KEY_WIDTH + 6.0, 6.0),
                pos2(view.right() - 6.0, view.bottom()),
            );
            let mut over = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(place)
                    .layout(egui::Layout::top_down(Align::Min)),
            );
            self.no_notes(&mut over, project, pattern, actions)
        });
        // How far the grid was scrolled sideways when it was drawn, which
        // the lanes above and below it follow.
        let scroll_x = output.inner;
        let lanes = Lanes {
            step_width,
            scroll_x,
            length,
        };
        self.chord_lane(
            ui,
            (lane_rect, &lane_response),
            pattern,
            &heard,
            scale,
            lanes,
            playhead,
        );
        ui.add_space(4.0);
        self.velocity_lane(ui, pattern, lanes, actions);

        if let Some(step) = self.hovered_step {
            self.last_step = Some(step);
        }
        let key_tool = if self.has_keyboard && !ui.ctx().text_edit_focused() {
            self.tool_from_keys(ui.ctx())
        } else {
            None
        };
        if let Some(tool) = tool.or(key_tool) {
            self.use_tool(tool, pattern, scale, actions);
        }
    }

    /// The row of menus at the top: key, scale, what to draw, how long,
    /// and the tools. Returns the tool picked from the Tools menu.
    fn toolbar(
        &mut self,
        ui: &mut Ui,
        scale: Scale,
        alone: bool,
        actions: &mut Vec<RollAction>,
    ) -> Option<Tool> {
        let mut picked = None;
        ui.horizontal_wrapped(|ui| {
            // Each label stays whole and moves to the next line when it
            // does not fit, rather than being squeezed into a column.
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
            ui.label("Key");
            let mut root = scale.root;
            egui::ComboBox::from_id_salt("scale-root")
                .width(64.0)
                .selected_text(scale.spell(root).to_string())
                .show_ui(ui, |ui| {
                    for pitch_class in PitchClass::ALL {
                        ui.selectable_value(&mut root, pitch_class, both_names(pitch_class));
                    }
                })
                .response
                .on_hover_text(KEY_HELP);
            if let Some(new) = twelve_tone::kind_menu(ui, "scale-kind", scale, root, 120.0) {
                // Picking a row or a set opens the window to change it.
                match new.kind {
                    ScaleKind::Row(_) if scale.kind.is_tonal() => self.row_window.open = true,
                    ScaleKind::Set(_) if scale.kind.is_tonal() => self.set_window.open = true,
                    _ => {}
                }
                actions.push(RollAction::SetScale(new));
            }
            let names: Vec<String> = scale
                .pitch_classes()
                .map(|p| scale.spell(p).to_string())
                .collect();
            ui.weak(names.join(" "));
            self.snap_button(ui);
            ui.toggle_value(&mut self.row_window.open, "Row…")
                .on_hover_text(
                    "Twelve-tone rows: type, draw or pick a row, see all its forms in the \
                     matrix, and write them into the pattern",
                );
            ui.toggle_value(&mut self.set_window.open, "Sets…")
                .on_hover_text(
                    "Pitch-class sets: every group of notes there is, by Forte number, to \
                     draw with or use as a scale",
                );
            ui.toggle_value(&mut self.micromode_window.open, "Micromodes…")
                .on_hover_text(
                    "Kröpfl's micromodes: the twelve groups of three notes, the smallest sets, \
                     to hear, draw with or use as a key",
                );

            ui.separator();
            theme::keep_together(ui, 200.0, |ui| {
                ui.label("Draw");
                egui::ComboBox::from_id_salt("chord-stamp")
                    .width(150.0)
                    .truncate()
                    .selected_text(self.stamp.label())
                    .show_ui(ui, |ui| {
                        for stamp in ChordStamp::all() {
                            if stamp == ChordStamp::Fixed(ChordQuality::ALL[0]) {
                                ui.separator();
                            }
                            ui.selectable_value(&mut self.stamp, stamp, stamp.label());
                        }
                        ui.separator();
                        if let ChordStamp::Set(_) = self.stamp {
                            let _ = ui.selectable_label(true, self.stamp.label());
                        }
                        if ui.button("A set from the set browser…").clicked() {
                            self.set_window.open = true;
                        }
                    })
                    .response
                    .on_hover_text("What a click on the grid places: one note, or a whole chord.");
            });

            // A chord of n notes has n - 1 inversions. A single note has
            // none, but the list stays in its place, greyed out, so the
            // toolbar does not rearrange itself when a chord is picked.
            let chord_size = self.stamp.notes_at(Note::C4, scale).len();
            let chord = chord_size > 1;
            if chord {
                self.inversion = self.inversion.min(chord_size - 1);
            }
            theme::keep_together(ui, 90.0, |ui| {
                if !chord {
                    ui.disable();
                }
                egui::ComboBox::from_id_salt("inversion")
                    .width(76.0)
                    .selected_text(INVERSIONS_SHORT[if chord { self.inversion } else { 0 }])
                    .show_ui(ui, |ui| {
                        for (times, name) in INVERSIONS.iter().enumerate().take(chord_size) {
                            ui.selectable_value(&mut self.inversion, times, *name);
                        }
                    })
                    .response
                    .on_hover_text(
                        "Which note of the chord is lowest. Each inversion moves the \
                         lowest note up an octave, above the others. The note you click \
                         stays the root, and the chord is arranged around it.",
                    )
                    .on_disabled_hover_text(
                        "A single note has no inversions: pick a chord under Draw to \
                         choose which of its notes is lowest.",
                    );
            });

            // The label and its list wrap onto the next row together.
            theme::keep_together(ui, 140.0, |ui| {
                ui.label("Length");
                let length_name = NOTE_LENGTHS
                    .iter()
                    .find(|(steps, _)| *steps == self.note_length)
                    .map(|(_, name)| name.to_string())
                    .unwrap_or_else(|| format!("{} steps", self.note_length));
                egui::ComboBox::from_id_salt("note-length")
                    .width(76.0)
                    .selected_text(length_name)
                    .show_ui(ui, |ui| {
                        for (steps, name) in NOTE_LENGTHS {
                            ui.selectable_value(&mut self.note_length, steps, name);
                        }
                    });
            });

            ui.separator();
            if let Some(tool) = self.tools_menu(ui) {
                picked = Some(tool);
            }
            ui.add_enabled(!alone, egui::Checkbox::new(&mut self.ghosts, "Ghost notes"))
                .on_hover_text(
                    "Show the notes of the other channels faintly, and name the chords \
                     all of them make together.",
                )
                .on_disabled_hover_text("No other channel plays melodies or chords yet.");
        });
        picked
    }

    /// The Tools menu. Returns the tool picked, if any.
    fn tools_menu(&mut self, ui: &mut Ui) -> Option<Tool> {
        let ctx = ui.ctx().clone();
        let mut picked = None;
        // Menus normally close on any click. This one has choices for the
        // arpeggio, so it stays open until a tool is used or you click
        // somewhere else.
        let stay_open = egui::containers::menu::MenuConfig::new()
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside);
        let menu = egui::containers::menu::MenuButton::new("Tools").config(stay_open);
        menu.ui(ui, |ui| {
            ui.weak("Changes the selected notes, or every note if none is selected.");
            let mut item =
                |ui: &mut Ui, text: &str, tool: Tool, shortcut: Option<KeyboardShortcut>| {
                    let mut button = egui::Button::new(text);
                    if let Some(shortcut) = shortcut {
                        button = button.shortcut_text(ctx.format_shortcut(&shortcut));
                    }
                    if ui.add(button).clicked() {
                        picked = Some(tool);
                        ui.close();
                    }
                };
            item(ui, "Up a semitone", Tool::Transpose(1), Some(KEY_UP));
            item(ui, "Down a semitone", Tool::Transpose(-1), Some(KEY_DOWN));
            item(ui, "Up an octave", Tool::Transpose(12), Some(OCTAVE_UP));
            item(
                ui,
                "Down an octave",
                Tool::Transpose(-12),
                Some(OCTAVE_DOWN),
            );
            item(ui, "Up a note in the key", Tool::InKey(1), Some(IN_KEY_UP));
            item(
                ui,
                "Down a note in the key",
                Tool::InKey(-1),
                Some(IN_KEY_DOWN),
            );
            ui.separator();
            item(ui, "A step earlier", Tool::Shift(-1), Some(KEY_LEFT));
            item(ui, "A step later", Tool::Shift(1), Some(KEY_RIGHT));
            item(ui, "A beat earlier", Tool::Shift(-4), Some(BEAT_LEFT));
            item(ui, "A beat later", Tool::Shift(4), Some(BEAT_RIGHT));
            item(ui, "Quantize to the steps", Tool::Quantize, None);
            ui.separator();
            item(ui, "Snap to the key", Tool::SnapToKey, None);
            item(ui, "Invert chords up", Tool::Invert { up: true }, None);
            item(ui, "Invert chords down", Tool::Invert { up: false }, None);
            ui.separator();
            ui.horizontal(|ui| {
                ui.label("Arpeggio");
                for order in ArpeggioOrder::ALL {
                    ui.selectable_value(&mut self.arpeggio, order, order.name());
                }
            });
            ui.horizontal(|ui| {
                ui.label("Each note");
                for (steps, name) in ARPEGGIO_RATES {
                    ui.selectable_value(&mut self.arpeggio_rate, steps, name);
                }
            });
            item(ui, "Arpeggiate chords", Tool::Arpeggiate, None);
            ui.separator();
            item(ui, "Select all", Tool::SelectAll, Some(SELECT_ALL));
            item(ui, "Delete selected notes", Tool::Delete, Some(DELETE));
        });
        picked
    }

    /// FL Studio's snap to scale: a click turns it on and off, and a
    /// right click opens its menu, which can also snap the notes played on
    /// the keyboards.
    fn snap_button(&mut self, ui: &mut Ui) {
        ui.toggle_value(&mut self.snap_to_scale, "Snap to scale")
            .on_hover_text(
                "Notes you draw or move land on the notes of the key. Notes already \
                 there stay put until you move them: to snap them all, select them \
                 (Ctrl+A) and drag them up a semitone and back. Right-click for more.",
            )
            .context_menu(|ui| {
                ui.checkbox(&mut self.snap_to_scale, "Snap notes you draw or move");
                ui.checkbox(&mut self.snap_played, "Snap notes you play")
                    .on_hover_text(
                        "Keys played on the typing keyboard or a MIDI keyboard sound, \
                         and are recorded, as the nearest note of the key.",
                    );
            });
    }

    /// Buttons for the seven chords of the key, to hear how they sound.
    /// The chords that often follow `current` (the chord held on a
    /// keyboard, or at the last step pointed at) are highlighted.
    fn chords_in_key(
        &self,
        ui: &mut Ui,
        scale: Scale,
        current: Option<(ChordAt, Chord)>,
        compact: bool,
        actions: &mut Vec<RollAction>,
    ) -> String {
        if let Some(groups) = keys::group_keys(scale, ChordSize::Triads) {
            self.groups_in_key(ui, scale, &groups, actions);
            return String::new();
        }
        if !scale.is_heptatonic() {
            return String::new();
        }
        let suggestions = current
            .map(|(_, chord)| next_chords(scale, chord))
            .unwrap_or_default();
        ui.horizontal_wrapped(|ui| {
            ui.label(format!("Chords in {}", scale.name()));
            for degree in 0..7 {
                let Some(chord) = scale.triad(degree) else {
                    continue;
                };
                let numeral = scale.roman_numeral(chord).unwrap_or_default();
                let text = format!("{numeral} · {}", scale.chord_name(chord));
                let full_name = format!("{} {}", scale.spell_root(chord), chord.quality.name());
                let mut hint = match self.hints {
                    HintLevel::Detailed => format!(
                        "{full_name}. Click to hear it. Pick \"Chord in key\" under Draw to place chords like it."
                    ),
                    _ => format!("{full_name}. Click to hear it."),
                };
                let suggestion = suggestions.iter().find(|s| s.degree == degree);
                if let Some(suggestion) = suggestion {
                    // In minor keys the suggested V is major, while the
                    // key's own chord on that degree is minor.
                    let played_as = if suggestion.chord == chord {
                        String::new()
                    } else {
                        format!(", played as {}", scale.chord_name(suggestion.chord))
                    };
                    hint = match self.hints {
                        HintLevel::Detailed => format!(
                            "Often comes next{played_as}. {}\n\n{hint}",
                            suggestion.reason
                        ),
                        HintLevel::Short => {
                            format!("Often next{played_as}: {}.\n\n{hint}", suggestion.short)
                        }
                        HintLevel::Minimal => format!("Often next{played_as}.\n\n{hint}"),
                    };
                }
                let button = egui::Button::new(text).selected(suggestion.is_some());
                if ui.add(button).on_hover_text(hint).clicked() {
                    let root = Note::new(Note::C4.midi() + chord.root.semitone());
                    actions.push(RollAction::Preview {
                        channel: self.channel,
                        pitches: chord.notes_from(root),
                    });
                }
            }
        });
        let hint = match current {
            None => match self.hints {
                HintLevel::Detailed => "Point at a chord in the grid, or hold one on a keyboard, \
                                        to see which chords often follow it."
                    .to_string(),
                HintLevel::Short => {
                    "Point at or hold a chord to see what often follows.".to_string()
                }
                HintLevel::Minimal => String::new(),
            },
            Some((at, chord)) => {
                let name = scale.chord_name(chord);
                let place = match at {
                    ChordAt::Step(step) => format!("at step {}", step + 1),
                    ChordAt::Keys => "on the keys".to_string(),
                };
                let numeral = scale
                    .roman_numeral(chord)
                    .map(|numeral| format!(" ({numeral})"))
                    .unwrap_or_default();
                let next: Vec<String> = suggestions
                    .iter()
                    .map(|s| {
                        let numeral = scale.roman_numeral(s.chord).unwrap_or_default();
                        format!("{} ({numeral})", scale.chord_name(s.chord))
                    })
                    .collect();
                let major_or_minor = keys::has_functions(scale.kind);
                // Why nothing is suggested: the root is not in the key, the
                // chord is not one of the key's own, or nothing usually
                // follows it.
                let why = if !scale.contains(chord.root) {
                    "has a root outside the key"
                } else if !scale.is_chord_of_key(chord) {
                    "is not one of the key's chords"
                } else {
                    "has no usual next chord here"
                };
                match (self.hints, next.is_empty(), major_or_minor) {
                    (HintLevel::Minimal, true, true) => format!("{name}: {why}"),
                    (_, true, true) => format!("{name} {place} {why}."),
                    (HintLevel::Minimal, true, false) => String::new(),
                    (_, true, false) => {
                        "Next-chord hints are given in major and minor keys.".to_string()
                    }
                    (HintLevel::Detailed, false, _) => format!(
                        "After {name}{numeral} {place}, try: {}. Hover the highlighted chords to see why.",
                        next.join(", ")
                    ),
                    (HintLevel::Short, false, _) => {
                        format!("After {name}{numeral}, try: {}.", next.join(", "))
                    }
                    (HintLevel::Minimal, false, _) => format!("Next: {}", next.join(", ")),
                }
            }
        };
        if compact {
            return hint;
        }
        if !hint.is_empty() {
            ui.weak(hint);
        }
        String::new()
    }

    /// For a row or a set, which have no chords of the key: buttons for
    /// its groups of notes, the row's trichords or the chords stacked
    /// from the set, to hear how they sound.
    fn groups_in_key(
        &self,
        ui: &mut Ui,
        scale: Scale,
        groups: &[keys::GroupKey],
        actions: &mut Vec<RollAction>,
    ) {
        ui.horizontal_wrapped(|ui| {
            let what = match scale.kind {
                ScaleKind::Row(_) => format!("Trichords of {}", scale.name()),
                _ => format!("Chords of {}", scale.name()),
            };
            ui.label(what);
            for group in groups {
                let names: Vec<String> = group.names.iter().map(ToString::to_string).collect();
                let class = group.pitch_set().set_class();
                let text = format!("{} · {}", group.label, names.join(" "));
                let hover = format!(
                    "{}: set {}. Click to hear it.",
                    names.join(" "),
                    class.describe()
                );
                if ui.button(text).on_hover_text(hover).clicked() {
                    actions.push(RollAction::Preview {
                        channel: self.channel,
                        pitches: group.play(),
                    });
                }
            }
        });
    }

    /// The strip above the grid that names the chord at every step, in
    /// `rect`, the place taken for it.
    #[allow(clippy::too_many_arguments)]
    fn chord_lane(
        &mut self,
        ui: &mut Ui,
        (rect, response): (Rect, &Response),
        pattern: &Pattern,
        heard: &[usize],
        scale: Scale,
        lanes: Lanes,
        playhead: Option<usize>,
    ) {
        let length = lanes.length;
        let lane = lanes.grid(rect, self.time);
        let cells = lane.cells();
        if let Some(pos) = response.hover_pos()
            && cells.contains(pos)
        {
            self.hovered_step = Some(lane.cell_at(pos).0);
        }

        let painter = ui.painter_at(rect);
        painter.text(
            pos2(rect.left() + 4.0, rect.center().y),
            Align2::LEFT_CENTER,
            "Chords",
            FontId::proportional(11.0),
            ui.visuals().weak_text_color(),
        );
        painter.rect_filled(cells, 2.0, LANE_BACKGROUND);
        let painter = painter.with_clip_rect(cells);

        // Work out the harmony and the bass at every step, then join runs
        // where both stay the same: `chunk_by` splits a slice wherever
        // two neighbours differ. Two chords without a name both read "?",
        // but stay two blocks.
        let harmonies: Vec<_> = (0..length)
            .map(|step| {
                let bass = sounding(pattern, heard, step)
                    .min()
                    .map(|note| note.pitch_class());
                (analyze(sounding(pattern, heard, step)), bass)
            })
            .collect();
        let mut start = 0;
        for run in harmonies.chunk_by(|a, b| a == b) {
            let end = start + run.len();
            if let Some(label) = run[0].0.label_in(scale) {
                let chord_rect = Rect::from_min_max(
                    pos2(lane.x(start), lane.rect.top()),
                    pos2(lane.x(end), lane.rect.bottom()),
                )
                .shrink2(vec2(1.0, 2.0));
                painter.rect_filled(chord_rect, 3.0, LANE_CHORD);
                // A long chord's name stays in view while it scrolls.
                let shown = chord_rect.intersect(cells);
                painter.with_clip_rect(chord_rect).text(
                    shown.center(),
                    Align2::CENTER_CENTER,
                    label,
                    FontId::proportional(12.0),
                    LANE_TEXT,
                );
            }
            start = end;
        }

        if let Some(step) = playhead.filter(|&step| step < length) {
            let x = lane.x(step);
            painter.rect_filled(
                Rect::from_min_max(
                    pos2(x, lane.rect.top()),
                    pos2(x + lanes.step_width, lane.rect.bottom()),
                ),
                0.0,
                PLAYHEAD,
            );
        }
    }

    /// The keyboard and the note grid, inside the scroll area. Returns how
    /// far the grid is scrolled sideways, in pixels.
    fn grid(
        &mut self,
        ui: &mut Ui,
        pattern: &Pattern,
        ghosts: &[usize],
        view: View,
        actions: &mut Vec<RollAction>,
    ) -> f32 {
        let View {
            scale,
            step_width,
            playhead,
            held,
        } = view;
        let length = pattern.length();
        let rows = (HIGHEST - LOWEST + 1) as f32;
        let row_height = ROW_HEIGHT * self.zoom.y;
        let size = vec2(KEY_WIDTH + step_width * length as f32, rows * row_height);
        let (rect, response) = ui.allocate_exact_size(size, Sense::click_and_drag());
        let visible = ui.clip_rect().intersect(rect);
        let grid = Grid {
            rect: Rect::from_min_max(pos2(rect.left() + KEY_WIDTH, rect.top()), rect.max),
            step_width,
            row_height,
            length,
            time: self.time,
            keys_left: visible.left(),
        };

        let notes = pattern.notes(self.channel);
        // Whether the view jumps this frame. It moves only after the grid
        // is drawn, so what is in view now is not worth remembering.
        let jumping = self.scroll_to_notes;
        if self.scroll_to_notes {
            // Centre the view between the lowest and highest note, or on
            // middle C when there are no notes yet, from the first step.
            // The middle is that of the rows from the highest to the
            // lowest: halving pitches would be a row off when they span an
            // odd number of semitones.
            let pitches = || notes.iter().map(|note| note.pitch.midi());
            let lowest = pitches().min().unwrap_or(Note::C4.midi());
            let highest = pitches().max().unwrap_or(Note::C4.midi());
            let top = grid.row_top(highest.clamp(LOWEST, HIGHEST));
            let bottom = grid.row_top(lowest.clamp(LOWEST, HIGHEST)) + row_height;
            let target =
                Rect::from_min_max(pos2(rect.left(), top), pos2(rect.left() + 1.0, bottom));
            ui.scroll_to_rect(target, Some(Align::Center));
            self.scroll_to_notes = false;
        }
        if !jumping {
            self.in_view = Some((
                grid.cell_at(visible.left_bottom() - vec2(0.0, 1.0))
                    .1
                    .midi(),
                grid.cell_at(visible.left_top()).1.midi(),
            ));
        }
        let hit = self.handle_input(ui, &response, grid, pattern, scale, actions);

        let painter = ui.painter_at(visible);
        paint_rows(&painter, grid, scale);
        paint_step_lines(&painter, grid);
        if notes.is_empty() {
            painter.text(
                visible.center() + vec2(KEY_WIDTH / 2.0, 0.0),
                Align2::CENTER_CENTER,
                "No notes here yet: click to draw one",
                FontId::proportional(18.0),
                NO_NOTES_TEXT,
            );
        }
        if let Some(step) = playhead.filter(|&step| step < length) {
            let x = grid.x(step);
            let column = Rect::from_min_max(
                pos2(x, grid.rect.top()),
                pos2(x + step_width, grid.rect.bottom()),
            );
            painter.rect_filled(column, 0.0, PLAYHEAD);
        }
        // The other channels first, so this channel's notes are drawn on top.
        for &other in ghosts {
            for note in pattern
                .notes(other)
                .iter()
                .filter(|note| note.start < length)
            {
                let rect = grid.note_rect(note).shrink2(vec2(0.5, 1.5));
                painter.rect_filled(rect, 2.0, OTHER_CHANNEL);
                painter.rect_stroke(
                    rect,
                    2.0,
                    Stroke::new(1.0, OTHER_CHANNEL_BORDER),
                    StrokeKind::Inside,
                );
            }
        }
        self.paint_notes(&painter, grid, notes, scale);

        // Show what a click would place: an outline of the note or chord.
        let ctrl = ui.input(|input| input.modifiers.command);
        if self.drag.is_none()
            && self.group_drag.is_none()
            && self.selection_box.is_none()
            && hit.is_none()
            && !ctrl
            && let Some(pos) = response.hover_pos()
            && grid.cells().contains(pos)
        {
            let (step, pitch) = grid.cell_at(pos);
            for stamp_pitch in self.stamp_notes(self.snapped(pitch, scale), scale) {
                let stamp = NoteEvent::new(stamp_pitch, step, self.note_length);
                let stamp_rect = grid.note_rect(&stamp).shrink2(vec2(0.5, 1.0));
                painter.rect_filled(stamp_rect, 2.0, STAMP_GHOST);
                painter.rect_stroke(stamp_rect, 2.0, Stroke::new(1.0, NOTE), StrokeKind::Inside);
            }
        }

        if let Some(origin) = self.selection_box
            && let Some(pos) = ui.input(|input| input.pointer.interact_pos())
        {
            let area = Rect::from_two_pos(origin, pos);
            painter.rect_filled(area, 0.0, SELECTION_BOX);
            painter.rect_stroke(
                area,
                0.0,
                Stroke::new(1.0, SELECTED_BORDER),
                StrokeKind::Inside,
            );
        }
        // The keyboard goes on top, since the grid scrolls under it.
        paint_keys(&painter, grid, scale, held);
        visible.left() - rect.left()
    }

    /// The notes a click on `pitch` places, in the inversion chosen.
    ///
    /// Inverting moves the clicked note (the root) up an octave, which
    /// would put the whole chord above the pointer. So the chord moves
    /// back down an octave, and the root stays where you clicked, with
    /// the other notes around it.
    fn stamp_notes(&self, pitch: Note, scale: Scale) -> Vec<Note> {
        let notes = tools::inversion(&self.stamp.notes_at(pitch, scale), self.inversion);
        if notes.contains(&pitch) {
            return notes;
        }
        notes
            .iter()
            .map(|note| note.transpose(-12))
            .collect::<Option<Vec<Note>>>()
            .unwrap_or(notes)
    }

    /// Reads the mouse for the grid and the keyboard. Returns the index of
    /// the note under the pointer, if any.
    ///
    /// - left click on a key: hear it
    /// - left click on empty grid: add a note (or chord); keep the button
    ///   down to drag the new note
    /// - left drag a note: move it; drag its right end: resize it
    /// - left drag a selected note: move all the selected notes
    /// - Ctrl and drag: select the notes in a box; Ctrl and click a note:
    ///   add it to the selection, or take it out
    /// - right click (or right drag over) notes: delete them
    fn handle_input(
        &mut self,
        ui: &Ui,
        response: &Response,
        grid: Grid,
        pattern: &Pattern,
        scale: Scale,
        actions: &mut Vec<RollAction>,
    ) -> Option<usize> {
        let channel = self.channel;
        let notes = pattern.notes(channel);
        let dismissing = crate::widgets::dismissing(ui.ctx());
        let (primary_pressed, primary_down, secondary, pointer, press_origin, ctrl) =
            ui.input(|input| {
                let pointer = &input.pointer;
                (
                    pointer.primary_pressed(),
                    pointer.primary_down(),
                    // A quick click can press and release within one frame,
                    // so "pressed this frame" counts as well as "held down".
                    pointer.secondary_pressed() || pointer.secondary_down(),
                    pointer.interact_pos(),
                    crate::widgets::press_pos(input, Some(egui::PointerButton::Primary)),
                    input.modifiers.command,
                )
            });

        // On the frame the button goes down, look where it went down: the
        // pointer may already have moved on, if the hand was quick.
        let hovered = match response.hover_pos() {
            Some(hover) if primary_pressed => press_origin.or(Some(hover)),
            hover => hover,
        };
        // The "no notes here" message lies over the grid, and a click that
        // closes a menu does nothing else.
        let hovered = hovered
            .filter(|pos| !self.banner.is_some_and(|banner| banner.contains(*pos)))
            .filter(|_| !dismissing);
        let hit = hovered
            .filter(|pos| grid.cells().contains(*pos))
            .and_then(|pos| note_at(notes, grid, pos));

        if let Some(pos) = hovered {
            if grid.keys().contains(pos) && primary_pressed {
                let (_, pitch) = grid.cell_at(pos);
                actions.push(RollAction::Preview {
                    channel,
                    pitches: vec![pitch],
                });
            } else if grid.cells().contains(pos) {
                let (step, pitch) = grid.cell_at(pos);
                self.hovered_step = Some(step);

                if primary_pressed && ctrl {
                    match hit {
                        Some(index) => {
                            let note = notes[index];
                            if let Some(at) = self.selection.iter().position(|&n| n == note) {
                                self.selection.remove(at);
                            } else {
                                self.selection.push(note);
                            }
                        }
                        None => {
                            self.selection.clear();
                            self.selection_box = Some(pos);
                        }
                    }
                } else if primary_pressed {
                    match hit {
                        Some(index) => {
                            let note = notes[index];
                            // A short note keeps most of its length for
                            // moving it.
                            let rect = grid.note_rect(&note);
                            let grab = EDGE_GRAB.min(rect.width() / 3.0);
                            let near_end = pos.x > rect.right() - grab;
                            let selected = self.selection.contains(&note);
                            if selected && !near_end {
                                self.group_drag = Some(GroupDrag {
                                    from_step: step,
                                    from_pitch: pitch,
                                    originals: self.selection.clone(),
                                    moved: self.selection.clone(),
                                    edited: false,
                                });
                            } else {
                                if !selected {
                                    self.selection.clear();
                                }
                                self.drag = Some(if near_end {
                                    Drag::Resize { index }
                                } else {
                                    Drag::Move {
                                        index,
                                        grab: step.saturating_sub(note.start),
                                    }
                                });
                            }
                            actions.push(RollAction::Preview {
                                channel,
                                pitches: vec![note.pitch],
                            });
                        }
                        None => {
                            self.selection.clear();
                            let pitches = self.stamp_notes(self.snapped(pitch, scale), scale);
                            let new_notes: Vec<NoteEvent> = pitches
                                .iter()
                                .map(|&p| NoteEvent::new(p, step, self.note_length))
                                .collect();
                            // A single new note can be dragged straight
                            // away. It will be added at the end of the list.
                            if new_notes.len() == 1 {
                                self.drag = Some(Drag::Move {
                                    index: notes.len(),
                                    grab: 0,
                                });
                            }
                            actions.push(RollAction::Preview { channel, pitches });
                            actions.push(RollAction::AddNotes {
                                channel,
                                notes: new_notes,
                            });
                        }
                    }
                } else if secondary && let Some(index) = hit {
                    actions.push(RollAction::RemoveNote { channel, index });
                }
            }
        }

        if !primary_down {
            self.stop_dragging();
        }

        // A selection box selects every note it touches.
        if let Some(origin) = self.selection_box
            && let Some(pos) = pointer
        {
            let area = Rect::from_two_pos(origin, pos);
            self.selection = notes
                .iter()
                .filter(|note| note.start < grid.length && grid.note_rect(note).intersects(area))
                .copied()
                .collect();
        }

        // Moving the selection: every note keeps its distance from the
        // cell the drag started on.
        if let Some(mut drag) = self.group_drag.take()
            && let Some(pos) = pointer
        {
            let (step, pitch) = grid.cell_at(pos);
            let steps = step as isize - drag.from_step as isize;
            let semitones = i32::from(pitch.midi()) - i32::from(drag.from_pitch.midi());
            drag.edited |= steps != 0 || semitones != 0;
            let snap = self.snap_to_scale && drag.edited;
            let new = tools::shift(&drag.originals, steps, grid.length)
                .and_then(|shifted| tools::transpose(&shifted, semitones))
                .map(|moved| match snap {
                    true => tools::snap_to_scale(&moved, scale),
                    false => moved,
                })
                .filter(|moved| fits_view(moved));
            if let Some(new) = new
                && new != drag.moved
            {
                let pitch_changed =
                    new.first().map(|n| n.pitch) != drag.moved.first().map(|n| n.pitch);
                if let Some(moved) =
                    self.replace(pattern, actions, "move notes", drag.moved.clone(), new)
                {
                    if pitch_changed {
                        actions.push(RollAction::Preview {
                            channel,
                            pitches: moved.iter().map(|note| note.pitch).collect(),
                        });
                    }
                    drag.moved = moved;
                }
            }
            self.group_drag = Some(drag);
        }

        // The drag follows the pointer even outside the grid.
        if let Some(drag) = self.drag
            && let Some(pos) = pointer
            && let Some(&note) = notes.get(drag.index())
        {
            let (step, pitch) = grid.cell_at(pos);
            let pitch = self.snapped(pitch, scale);
            let changed = match drag {
                Drag::Move { grab, .. } => NoteEvent {
                    pitch,
                    start: step.saturating_sub(grab),
                    ..note
                },
                // A resized note ends on the grid, where the pointer is.
                Drag::Resize { .. } => {
                    let length = (step + 1).saturating_sub(note.start).max(1);
                    self.note_length = length;
                    let trim = if length == note.length { note.trim } else { 0 };
                    NoteEvent {
                        length,
                        trim,
                        ..note
                    }
                }
            };
            if changed != note {
                if changed.pitch != note.pitch {
                    actions.push(RollAction::Preview {
                        channel,
                        pitches: vec![changed.pitch],
                    });
                }
                // A resized note that was selected stays selected.
                if let Some(selected) = self.selection.iter_mut().find(|n| **n == note) {
                    *selected = changed;
                }
                actions.push(RollAction::ReplaceNote {
                    channel,
                    index: drag.index(),
                    note: changed,
                });
            }
        }
        hit
    }

    /// The strip under the grid that shows how hard each note is played.
    /// Click or drag in it to set the velocity of the notes on a step
    /// (only the selected ones, if any notes are selected).
    fn velocity_lane(
        &mut self,
        ui: &mut Ui,
        pattern: &Pattern,
        lanes: Lanes,
        actions: &mut Vec<RollAction>,
    ) {
        let length = lanes.length;
        let step_width = lanes.step_width;
        let (rect, response) = ui.allocate_exact_size(
            vec2(ui.available_width(), VELOCITY_HEIGHT),
            Sense::click_and_drag(),
        );
        let lane = lanes.grid(rect, self.time);
        let cells = lane.cells();
        let notes = pattern.notes(self.channel);

        if response.is_pointer_button_down_on()
            && !crate::widgets::dismissing(ui.ctx())
            && let Some(pos) = response.interact_pointer_pos()
            && (cells.contains(pos) || response.dragged())
        {
            let step = lane.step_at(pos.x);
            self.hovered_step = Some(step);
            let height = (lane.rect.bottom() - pos.y) / lane.rect.height();
            let velocity = (height * 127.0).round().clamp(1.0, 127.0) as u8;
            let selected_here = notes
                .iter()
                .any(|note| note.start == step && self.selection.contains(note));
            let old: Vec<NoteEvent> = notes
                .iter()
                .filter(|note| note.start == step && note.velocity != velocity)
                .filter(|note| !selected_here || self.selection.contains(note))
                .copied()
                .collect();
            let new = old
                .iter()
                .map(|&note| NoteEvent { velocity, ..note })
                .collect();
            self.replace(pattern, actions, "change velocity", old, new);
        } else if let Some(pos) = response.hover_pos() {
            self.hovered_step = Some(lane.step_at(pos.x));
        }

        let painter = ui.painter_at(rect);
        painter.text(
            pos2(rect.left() + 4.0, rect.top() + 8.0),
            Align2::LEFT_CENTER,
            "Velocity",
            FontId::proportional(11.0),
            ui.visuals().weak_text_color(),
        );
        painter.rect_filled(cells, 2.0, LANE_BACKGROUND);
        let painter = painter.with_clip_rect(cells);
        for step in (0..=length).filter(|&step| lane.time.is_beat_start(step)) {
            let color = if lane.time.is_bar_start(step) {
                BAR_LINE
            } else {
                STEP_LINE
            };
            painter.vline(lane.x(step), lane.rect.y_range(), Stroke::new(1.0, color));
        }
        let bar_width = (step_width * 0.4).clamp(2.0, 8.0);
        for note in notes.iter().filter(|note| note.start < length) {
            let height = lane.rect.height() * f32::from(note.velocity) / 127.0;
            let nudge = f32::from(note.nudge) * step_width / TICKS_PER_STEP as f32;
            let left = lane.x(note.start) + nudge + 2.0;
            let bar = Rect::from_min_max(
                pos2(left, lane.rect.bottom() - height),
                pos2(left + bar_width, lane.rect.bottom()),
            );
            let color = if self.selection.contains(note) {
                SELECTED_BORDER
            } else {
                NOTE
            };
            painter.rect_filled(bar, 1.0, color.gamma_multiply(0.85));
            painter.circle_filled(pos2(bar.center().x, bar.top()), bar_width * 0.6, color);
        }
    }

    /// Replaces `old` with `new` (tidied first) if the pattern allows it,
    /// keeps the selection on the same notes, and returns the notes as
    /// they now are. Returns `None`, doing nothing, if it is not allowed.
    fn replace(
        &mut self,
        pattern: &Pattern,
        actions: &mut Vec<RollAction>,
        label: &'static str,
        old: Vec<NoteEvent>,
        new: Vec<NoteEvent>,
    ) -> Option<Vec<NoteEvent>> {
        let new = tidy_notes(new);
        if old == new || !pattern.can_replace_notes(self.channel, &old, &new) {
            return None;
        }
        let was_selected = old.iter().any(|note| self.selection.contains(note));
        self.selection.retain(|note| !old.contains(note));
        if was_selected {
            self.selection.extend(&new);
        }
        actions.push(RollAction::ReplaceNotes {
            channel: self.channel,
            old,
            new: new.clone(),
            label,
        });
        Some(new)
    }

    /// The tool asked for with a key, if any.
    fn tool_from_keys(&self, ctx: &egui::Context) -> Option<Tool> {
        let pressed = |shortcut| ctx.input_mut(|input| input.consume_shortcut(&shortcut));
        // egui counts Shift+↑ and Alt+↑ as ↑ too, so the longer shortcuts
        // are checked first.
        let keys = [
            (IN_KEY_UP, Tool::InKey(1)),
            (IN_KEY_DOWN, Tool::InKey(-1)),
            (OCTAVE_UP, Tool::Transpose(12)),
            (OCTAVE_DOWN, Tool::Transpose(-12)),
            (KEY_UP, Tool::Transpose(1)),
            (KEY_DOWN, Tool::Transpose(-1)),
            (BEAT_LEFT, Tool::Shift(-4)),
            (BEAT_RIGHT, Tool::Shift(4)),
            (KEY_LEFT, Tool::Shift(-1)),
            (KEY_RIGHT, Tool::Shift(1)),
            (SELECT_ALL, Tool::SelectAll),
            (DELETE, Tool::Delete),
            (BACKSPACE, Tool::Delete),
            (ESCAPE, Tool::SelectNone),
        ];
        keys.into_iter()
            .find(|&(shortcut, _)| pressed(shortcut))
            .map(|(_, tool)| tool)
    }

    /// Uses a tool on the selected notes, or on every note of the channel
    /// when none is selected.
    fn use_tool(
        &mut self,
        tool: Tool,
        pattern: &Pattern,
        scale: Scale,
        actions: &mut Vec<RollAction>,
    ) {
        let length = pattern.length();
        let visible: Vec<NoteEvent> = pattern
            .notes(self.channel)
            .iter()
            .filter(|note| note.start < length)
            .copied()
            .collect();
        let targets = if self.selection.is_empty() {
            visible.clone()
        } else {
            self.selection.clone()
        };
        let (label, new) = match tool {
            Tool::SelectAll => {
                self.selection = visible;
                return;
            }
            Tool::SelectNone => {
                self.selection.clear();
                return;
            }
            // Delete only ever deletes a selection, never every note.
            Tool::Delete if self.selection.is_empty() => return,
            Tool::Delete => ("delete notes", Some(Vec::new())),
            Tool::Transpose(semitones) => {
                ("transpose notes", tools::transpose(&targets, semitones))
            }
            Tool::InKey(steps) => (
                "move notes in key",
                tools::transpose_in_key(&targets, scale, steps),
            ),
            Tool::Shift(steps) => ("move notes", tools::shift(&targets, steps, length)),
            Tool::Quantize => ("quantize notes", Some(tools::quantize(&targets, length))),
            Tool::SnapToKey => (
                "snap notes to key",
                Some(tools::snap_to_scale(&targets, scale)),
            ),
            Tool::Invert { up } => ("invert chords", tools::invert(&targets, up)),
            Tool::Arpeggiate => (
                "arpeggiate chords",
                Some(tools::arpeggiate(
                    &targets,
                    self.arpeggio,
                    self.arpeggio_rate,
                )),
            ),
        };
        let Some(new) = new.filter(|new| fits_view(new)) else {
            return;
        };
        let Some(new) = self.replace(pattern, actions, label, targets, new) else {
            return;
        };
        // Let the user hear what moved: the first chord (or note) of it.
        if matches!(
            tool,
            Tool::Transpose(_) | Tool::InKey(_) | Tool::Invert { .. }
        ) && let Some(first) = tools::chords(&new).into_iter().next()
        {
            actions.push(RollAction::Preview {
                channel: self.channel,
                pitches: first.iter().map(|note| note.pitch).collect(),
            });
        }
    }

    fn paint_notes(&self, painter: &Painter, grid: Grid, notes: &[NoteEvent], scale: Scale) {
        let visible = notes
            .iter()
            .enumerate()
            .filter(|(_, note)| note.start < grid.length);
        for (index, note) in visible {
            let rect = grid.note_rect(note).shrink2(vec2(0.5, 1.0));
            // Notes outside the key are drawn red, so they stand out.
            let mut fill = if scale.contains(note.pitch.pitch_class()) {
                NOTE
            } else {
                NOTE_OUTSIDE
            };
            // Softer notes are a little darker, as in the velocity lane.
            fill = fill.gamma_multiply(0.55 + 0.45 * f32::from(note.velocity) / 127.0);
            if self.drag.map(Drag::index) == Some(index) {
                fill = fill.gamma_multiply(1.2);
            }
            let (border, width) = if self.selection.contains(note) {
                (SELECTED_BORDER, 2.0)
            } else {
                (NOTE_BORDER, 1.0)
            };
            painter.rect_filled(rect, 2.0, fill);
            painter.rect_stroke(rect, 2.0, Stroke::new(width, border), StrokeKind::Inside);
            // The name stays in view when the start of the note is
            // scrolled away under the keyboard.
            let shown = rect.intersect(grid.cells());
            if shown.width() > 26.0 {
                painter.with_clip_rect(shown).text(
                    shown.left_center() + vec2(3.0, 0.0),
                    Align2::LEFT_CENTER,
                    scale.note_name(note.pitch),
                    FontId::proportional(grid.text_size()),
                    NOTE_TEXT,
                );
            }
        }
    }
}

const fn shortcut(modifiers: Modifiers, key: Key) -> KeyboardShortcut {
    KeyboardShortcut::new(modifiers, key)
}

const KEY_UP: KeyboardShortcut = shortcut(Modifiers::NONE, Key::ArrowUp);
const KEY_DOWN: KeyboardShortcut = shortcut(Modifiers::NONE, Key::ArrowDown);
const OCTAVE_UP: KeyboardShortcut = shortcut(Modifiers::SHIFT, Key::ArrowUp);
const OCTAVE_DOWN: KeyboardShortcut = shortcut(Modifiers::SHIFT, Key::ArrowDown);
const IN_KEY_UP: KeyboardShortcut = shortcut(Modifiers::ALT, Key::ArrowUp);
const IN_KEY_DOWN: KeyboardShortcut = shortcut(Modifiers::ALT, Key::ArrowDown);
const KEY_LEFT: KeyboardShortcut = shortcut(Modifiers::NONE, Key::ArrowLeft);
const KEY_RIGHT: KeyboardShortcut = shortcut(Modifiers::NONE, Key::ArrowRight);
const BEAT_LEFT: KeyboardShortcut = shortcut(Modifiers::SHIFT, Key::ArrowLeft);
const BEAT_RIGHT: KeyboardShortcut = shortcut(Modifiers::SHIFT, Key::ArrowRight);
const SELECT_ALL: KeyboardShortcut = shortcut(Modifiers::COMMAND, Key::A);
const DELETE: KeyboardShortcut = shortcut(Modifiers::NONE, Key::Delete);
const BACKSPACE: KeyboardShortcut = shortcut(Modifiers::NONE, Key::Backspace);
const ESCAPE: KeyboardShortcut = shortcut(Modifiers::NONE, Key::Escape);

/// The pitches held during `step` on any of `channels`.
fn sounding<'a>(
    pattern: &'a Pattern,
    channels: &'a [usize],
    step: usize,
) -> impl Iterator<Item = Note> + 'a {
    channels
        .iter()
        .flat_map(move |&channel| pattern.sounding_at(channel, step))
}

/// Whether every note is on a row the piano roll shows.
fn fits_view(notes: &[NoteEvent]) -> bool {
    notes
        .iter()
        .all(|note| (LOWEST..=HIGHEST).contains(&note.pitch.midi()))
}

/// The index of the note under `pos`. Later notes are drawn on top, so
/// they are searched first.
fn note_at(notes: &[NoteEvent], grid: Grid, pos: Pos2) -> Option<usize> {
    notes
        .iter()
        .enumerate()
        .rev()
        .filter(|(_, note)| note.start < grid.length)
        .find(|(_, note)| grid.note_rect(note).contains(pos))
        .map(|(index, _)| index)
}

/// What the key menus say about changing the key, here and in the
/// harmony keys bar.
pub(crate) const KEY_HELP: &str = "The key of the song. A new key or mode moves the \
    chords, bass and melody into it (the drums stay), so the song plays in the new key. \
    Ctrl+Z puts them back.";

/// "C" for white keys, "C#/Db" for black keys.
pub(crate) fn both_names(pitch_class: PitchClass) -> String {
    let sharp = pitch_class.name(Spelling::Sharps);
    let flat = pitch_class.name(Spelling::Flats);
    if sharp == flat {
        sharp.to_string()
    } else {
        format!("{sharp}/{flat}")
    }
}

/// Paints the row backgrounds. Rows of notes in the key are lighter, and
/// the key's root is tinted, so the scale shows at a glance.
fn paint_rows(painter: &Painter, grid: Grid, scale: Scale) {
    for midi in LOWEST..=HIGHEST {
        let pitch_class = PitchClass::from_semitone(midi);
        let top = grid.row_top(midi);
        let in_scale = scale.contains(pitch_class);
        let is_root = pitch_class == scale.root;

        let row = Rect::from_min_size(
            pos2(grid.rect.left(), top),
            vec2(grid.rect.width(), grid.row_height),
        );
        let fill = match (is_root, in_scale) {
            (true, _) => ROOT_ROW,
            (false, true) => SCALE_ROW,
            (false, false) => OTHER_ROW,
        };
        painter.rect_filled(row, 0.0, fill);
        let line = if pitch_class == PitchClass::C {
            OCTAVE_LINE
        } else {
            ROW_LINE
        };
        painter.hline(row.x_range(), row.bottom(), Stroke::new(1.0, line));
    }
}

/// Paints the keyboard at the left of the view, with dots on the notes of
/// the key (or numbers, for a row). Keys held down on a keyboard light up.
fn paint_keys(painter: &Painter, grid: Grid, scale: Scale, held: &[Note]) {
    let keys = grid.keys();
    painter.rect_filled(keys, 0.0, OCTAVE_LINE);
    for midi in LOWEST..=HIGHEST {
        let pitch_class = PitchClass::from_semitone(midi);
        let top = grid.row_top(midi);
        let in_scale = scale.contains(pitch_class);
        let is_root = pitch_class == scale.root;
        let key = Rect::from_min_size(pos2(keys.left(), top), vec2(KEY_WIDTH, grid.row_height));
        let key_fill = if held.contains(&Note::new(midi)) {
            KEY_HELD
        } else if pitch_class.is_black_key() {
            BLACK_KEY
        } else {
            WHITE_KEY
        };
        painter.rect_filled(key.shrink2(vec2(0.0, 0.5)), 1.0, key_fill);
        if let ScaleKind::Row(_) = scale.kind {
            // A row numbers its notes in order, 1 to 12, instead of dots.
            let place = scale.degree_of(pitch_class).unwrap_or(0) + 1;
            let mark = if is_root { ROOT_MARK } else { ROW_MARK };
            painter.text(
                pos2(key.right() - 3.0, key.center().y),
                Align2::RIGHT_CENTER,
                place.to_string(),
                FontId::monospace(grid.text_size() - 1.0),
                mark,
            );
        } else if in_scale {
            let mark = if is_root { ROOT_MARK } else { SCALE_MARK };
            painter.circle_filled(pos2(key.right() - 7.0, key.center().y), 3.0, mark);
        }
        if pitch_class == PitchClass::C {
            painter.text(
                pos2(key.left() + 4.0, key.center().y),
                Align2::LEFT_CENTER,
                Note::new(midi).to_string(),
                FontId::proportional(grid.text_size()),
                KEY_TEXT,
            );
        }
    }
}

/// Vertical lines: faint for steps, stronger for beats, strongest for bars.
fn paint_step_lines(painter: &Painter, grid: Grid) {
    for step in 0..=grid.length {
        let color = match step {
            s if grid.time.is_bar_start(s) => BAR_LINE,
            s if grid.time.is_beat_start(s) => BEAT_LINE,
            _ => STEP_LINE,
        };
        painter.vline(grid.x(step), grid.rect.y_range(), Stroke::new(1.0, color));
    }
}

/// The channel with the most melody or chord notes in a pattern, if any
/// channel has some. Drum channels, whose notes are plain steps, are left
/// out: the channel rack is the place for them.
pub fn busiest_channel(pattern: &Pattern) -> Option<usize> {
    (0..pattern.rows())
        .map(|channel| {
            let melodic = pattern.notes(channel).iter().filter(|note| !note.is_step());
            (channel, melodic.count())
        })
        .filter(|&(_, count)| count > 0)
        .max_by_key(|&(channel, count)| (count, std::cmp::Reverse(channel)))
        .map(|(channel, _)| channel)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CHORDS: usize = 1;
    const SYNTH: usize = 4;

    /// Draws piano rolls without a window, a frame at a time.
    struct Screen {
        ctx: egui::Context,
        frame: usize,
    }

    impl Screen {
        fn new() -> Self {
            Screen {
                ctx: egui::Context::default(),
                frame: 0,
            }
        }

        /// Draws `frames` frames of `roll` showing `pattern` of `project`.
        fn run(
            &mut self,
            roll: &mut PianoRoll,
            project: &Project,
            pattern: usize,
            zoom: Vec2,
            frames: usize,
        ) {
            let screen = Rect::from_min_size(Pos2::ZERO, vec2(1280.0, 800.0));
            for _ in 0..frames {
                // Scrolling is animated, so time must pass.
                let input = egui::RawInput {
                    screen_rect: Some(screen),
                    time: Some(self.frame as f64 * 0.1),
                    ..Default::default()
                };
                self.frame += 1;
                let mut output = self.ctx.run_pass(input, |ctx| {
                    roll.show(ctx, project, pattern, None, &[], screen, zoom);
                });
                // Nothing is drawn, so the font textures are never uploaded.
                output.textures_delta.clear();
            }
        }
    }

    /// Draws `frames` frames of a piano roll on `pattern` of the demo song.
    fn run(roll: &mut PianoRoll, project: &Project, pattern: usize, frames: usize) {
        Screen::new().run(roll, project, pattern, Vec2::splat(1.0), frames);
    }

    /// The pitch in the middle of the view.
    fn middle(roll: &PianoRoll) -> u8 {
        let (lowest, highest) = roll.in_view.expect("the grid was drawn");
        lowest / 2 + highest / 2
    }

    #[test]
    fn zooming_keeps_the_middle() {
        let project = Project::demo();
        let mut roll = PianoRoll::new(SYNTH);
        roll.open = true;
        let mut screen = Screen::new();
        screen.run(&mut roll, &project, CHORDS, Vec2::splat(1.0), 20);
        let before = middle(&roll);
        for zoom in [1.1, 1.25, 1.5] {
            screen.run(&mut roll, &project, CHORDS, vec2(zoom, zoom), 1);
        }
        screen.run(&mut roll, &project, CHORDS, Vec2::splat(1.5), 5);
        assert!(
            middle(&roll).abs_diff(before) <= 1,
            "{before} became {}",
            middle(&roll)
        );
    }

    #[test]
    fn follows_the_channel_with_notes() {
        let project = Project::demo();
        // Opened on the kick, which plays nothing in "Chords".
        let mut roll = PianoRoll::new(0);
        roll.open = true;
        run(&mut roll, &project, CHORDS, 2);
        assert_eq!(roll.channel(), SYNTH);
    }

    #[test]
    fn stays_on_its_channel_when_no_other_has_melodies() {
        let project = Project::demo();
        let mut roll = PianoRoll::new(SYNTH);
        roll.open = true;
        run(&mut roll, &project, 0, 2);
        assert_eq!(roll.channel(), SYNTH);
    }

    #[test]
    fn opens_scrolled_to_the_notes() {
        let project = Project::demo();
        let mut roll = PianoRoll::new(SYNTH);
        roll.open = true;
        run(&mut roll, &project, CHORDS, 20);
        // The chords go from F2 (41) to G4 (67).
        let (lowest, highest) = roll.in_view.expect("the grid was drawn");
        assert!(
            lowest <= 41 && highest >= 67,
            "in view: {lowest}..={highest}"
        );
    }

    #[test]
    fn empty_pattern_shows_middle_c() {
        let project = Project::demo();
        let mut roll = PianoRoll::new(SYNTH);
        run(&mut roll, &project, 0, 3);
        roll.open = true;
        run(&mut roll, &project, 0, 20);
        let (lowest, highest) = roll.in_view.expect("the grid was drawn");
        assert!(
            lowest <= 60 && highest >= 60,
            "in view: {lowest}..={highest}"
        );
    }
}
