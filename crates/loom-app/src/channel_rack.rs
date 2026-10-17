//! The channel rack: one row per channel, with its steps in the selected
//! pattern.
//!
//! [`show`] only draws and reads the mouse. It does not change the
//! project: it returns a list of [`RackAction`]s, and the app applies
//! them. Keeping drawing and changing apart means the same action does
//! the same thing wherever it comes from (a click today, a keyboard
//! shortcut or an undo history tomorrow).

use std::path::PathBuf;

use eframe::egui::{self, Color32, Rect, Ui};
use loom_core::{ChannelKind, Drum, MAX_CHANNELS, Project, file};

use crate::widgets;

/// Something the user asked the rack to do.
#[derive(Debug, Clone, PartialEq)]
pub enum RackAction {
    ToggleStep {
        channel: usize,
        step: usize,
    },
    ClearStep {
        channel: usize,
        step: usize,
    },
    Preview(usize),
    SetVolume {
        channel: usize,
        volume: f32,
    },
    ToggleMute(usize),
    Remove(usize),
    OpenPianoRoll(usize),
    /// Show another pattern in the rack.
    SelectPattern(usize),
    AddDrum(Drum),
    AddSynth,
    /// Add a channel that plays an external synth.
    AddMidiOut,
    /// Change the MIDI channel (0 to 15) of a MIDI Out channel.
    SetMidiChannel {
        channel: usize,
        midi_channel: u8,
    },
    /// Load a WAV file into a channel, or into a new channel if `None`.
    LoadSample {
        channel: Option<usize>,
        path: PathBuf,
    },
}

const NAME_WIDTH: f32 = 110.0;
const DROP_HIGHLIGHT: Color32 = Color32::from_rgba_premultiplied(255, 152, 48, 40);

/// Draws the rack for pattern `pattern` and returns what the user did
/// this frame. `selected` is the selected channel, which the piano roll
/// shows and the keyboards play; `playhead` is the step of the pattern
/// being played, if any.
pub fn show(
    ui: &mut Ui,
    project: &Project,
    pattern: usize,
    selected: usize,
    playhead: Option<usize>,
) -> Vec<RackAction> {
    let mut actions = Vec::new();
    let pattern_index = pattern;
    let Some(pattern) = project.pattern(pattern) else {
        return actions;
    };
    // The time signature where the pattern is used in the song.
    let time = project.pattern_time(pattern_index);
    let mut row_rects: Vec<Rect> = Vec::with_capacity(project.channels().len());
    let length = pattern.length();
    let files_hovering = ui.ctx().input(|input| !input.raw.hovered_files.is_empty());
    let pointer = ui.ctx().input(|input| input.pointer.hover_pos());

    drums_elsewhere(ui, project, pattern, &mut actions);

    // The rack scrolls when the window is smaller than its rows. When all
    // the rows fit, the mouse wheel scrolls the steps sideways, without
    // having to hold Shift.
    let size = natural_size(ui.style(), project, pattern_index, widgets::zoom(ui));
    ui.style_mut().always_scroll_the_only_direction = true;
    let vertical = size.y > ui.available_height();
    egui::ScrollArea::new([true, vertical])
        .auto_shrink(false)
        .show(ui, |ui| {
            for (index, channel) in project.channels().iter().enumerate() {
                let row = ui.horizontal(|ui| {
                    if widgets::mute_light(ui, !channel.muted)
                        .on_hover_text("Click to mute or unmute")
                        .clicked()
                    {
                        actions.push(RackAction::ToggleMute(index));
                    }

                    let mut volume = channel.volume;
                    let knob = widgets::knob(ui, &mut volume, 0.8)
                        .on_hover_text(format!("Volume {:.0}%", volume * 100.0));
                    if knob.changed() {
                        actions.push(RackAction::SetVolume {
                            channel: index,
                            volume,
                        });
                    }

                    let name = egui::Button::new(&channel.name)
                        .min_size(egui::vec2(NAME_WIDTH, widgets::STEP_SIZE.y) * widgets::zoom(ui))
                        .selected(index == selected)
                        .truncate();
                    let name = ui.add(name).on_hover_text(
                        "Click to hear it and select it: the piano roll shows the selected \
                         channel, and keyboards play it. Right-click for options. Drop a WAV \
                         file here to load it.",
                    );
                    if name.clicked() {
                        actions.push(RackAction::Preview(index));
                    }
                    name.context_menu(|ui| {
                        if ui.button("Piano roll").clicked() {
                            actions.push(RackAction::OpenPianoRoll(index));
                            ui.close();
                        }
                        if let ChannelKind::MidiOut { channel: current } = channel.kind {
                            ui.menu_button("MIDI channel", |ui| {
                                for midi_channel in 0..16 {
                                    let label = format!("{}", midi_channel + 1);
                                    if ui
                                        .selectable_label(current == midi_channel, label)
                                        .clicked()
                                    {
                                        actions.push(RackAction::SetMidiChannel {
                                            channel: index,
                                            midi_channel,
                                        });
                                        ui.close();
                                    }
                                }
                            });
                        }
                        if ui.button("Remove channel").clicked() {
                            actions.push(RackAction::Remove(index));
                            ui.close();
                        }
                    });

                    ui.add_space(6.0 * widgets::zoom(ui));
                    // Plain steps are shown as buttons; melodies and chords as
                    // a small preview of the piano roll.
                    if pattern.is_step_row(index) {
                        for step in 0..length {
                            let on = pattern.is_on(index, step);
                            // Beats alternate colours, so you can count them,
                            // starting again in every bar so all bars look
                            // alike: in 3/4, dark, light, dark.
                            let beat_group = step % time.steps_per_bar() / time.steps_per_beat();
                            let response =
                                widgets::step_button(ui, on, beat_group, playhead == Some(step));
                            if response.clicked() && !widgets::dismissing(ui.ctx()) {
                                actions.push(RackAction::ToggleStep {
                                    channel: index,
                                    step,
                                });
                            } else if response.secondary_clicked() {
                                actions.push(RackAction::ClearStep {
                                    channel: index,
                                    step,
                                });
                            }
                        }
                    } else {
                        let preview =
                            widgets::mini_roll(ui, pattern.notes(index), length, playhead)
                                .on_hover_text("Click to open the piano roll");
                        if preview.clicked() {
                            actions.push(RackAction::OpenPianoRoll(index));
                        }
                    }
                });
                row_rects.push(row.response.rect);
            }

            ui.add_space(4.0);
            let rack_full = project.channels().len() >= MAX_CHANNELS;
            ui.add_enabled_ui(!rack_full, |ui| {
                ui.menu_button("➕ Add channel", |ui| {
                    for drum in Drum::ALL {
                        if ui.button(drum.name()).clicked() {
                            actions.push(RackAction::AddDrum(drum));
                            ui.close();
                        }
                    }
                    ui.separator();
                    if ui.button("Loom Osc (synth)").clicked() {
                        actions.push(RackAction::AddSynth);
                        ui.close();
                    }
                    if ui
                        .button("MIDI Out (external synth)")
                        .on_hover_text(
                            "Plays its notes on a synth outside Ferrous Loom: pick where in \
                             MIDI > Output.",
                        )
                        .clicked()
                    {
                        actions.push(RackAction::AddMidiOut);
                        ui.close();
                    }
                });
            });
        });

    // Highlight the row a dragged file would land on.
    let target_row = pointer.and_then(|p| row_rects.iter().position(|rect| rect.contains(p)));
    if files_hovering && let Some(row) = target_row {
        ui.painter()
            .rect_filled(row_rects[row], 3.0, DROP_HIGHLIGHT);
    }

    // A file dropped on a row loads into that channel; anywhere else, into
    // a new one. Project files are left for the app to open.
    let dropped: Vec<PathBuf> = ui.ctx().input(|input| {
        input
            .raw
            .dropped_files
            .iter()
            .map(|dropped| dropped.path().to_path_buf())
            .filter(|path| !is_project_file(path))
            .collect()
    });
    for (i, path) in dropped.into_iter().enumerate() {
        // Only the first file replaces the channel under the pointer.
        let channel = if i == 0 { target_row } else { None };
        actions.push(RackAction::LoadSample { channel, path });
    }

    actions
}

/// When a pattern has no drum steps but other patterns do, says so, with
/// a button for each of those patterns. Songs often keep their drums in a
/// pattern of their own ("A" and "A drums"), and without this the drum
/// rows of "A" just look broken.
fn drums_elsewhere(
    ui: &mut Ui,
    project: &Project,
    pattern: &loom_core::Pattern,
    actions: &mut Vec<RackAction>,
) {
    let with_drums = patterns_with_drums(project, pattern);
    if with_drums.is_empty() {
        return;
    }
    ui.horizontal_wrapped(|ui| {
        ui.label(format!(
            "No drum steps in \"{}\": click a step to add one. Drums are in:",
            pattern.name
        ));
        for (index, name) in with_drums {
            if ui.button(name).clicked() {
                actions.push(RackAction::SelectPattern(index));
            }
        }
    });
}

/// The patterns with drum steps, by index and name, when `pattern` has
/// none; empty when it has some.
fn patterns_with_drums<'a>(
    project: &'a Project,
    pattern: &loom_core::Pattern,
) -> Vec<(usize, &'a str)> {
    let has_steps = |pattern: &loom_core::Pattern| {
        (0..pattern.rows()).any(|row| pattern.notes(row).iter().any(|note| note.is_step()))
    };
    if has_steps(pattern) {
        return Vec::new();
    }
    project
        .patterns()
        .iter()
        .enumerate()
        .filter(|(_, other)| has_steps(other))
        .map(|(index, other)| (index, other.name.as_str()))
        .collect()
}

/// How big the rack is with nothing hidden: as wide as a row (the light,
/// the knob, the name, a small gap and the steps, with spacing between
/// them all) and as tall as all the rows, the "Add channel" button and
/// the line that says where the drums are, if it is shown. `zoom` is the
/// rack's zoom; `style` must already be scaled by it.
pub fn natural_size(
    style: &egui::Style,
    project: &Project,
    pattern: usize,
    zoom: f32,
) -> egui::Vec2 {
    let Some(pattern) = project.pattern(pattern) else {
        return egui::Vec2::ZERO;
    };
    let steps = pattern.length();
    let gap = style.spacing.item_spacing;
    let step = widgets::STEP_SIZE * zoom;
    let controls = (14.0 + 24.0 + NAME_WIDTH + 6.0) * zoom + 4.0 * gap.x;
    let width = controls + steps as f32 * (step.x + gap.x);
    let rows = project.channels().len() as f32 * (step.y + gap.y);
    let line = style.spacing.interact_size.y + gap.y;
    let add_channel = 4.0 + line;
    let hint = if patterns_with_drums(project, pattern).is_empty() {
        0.0
    } else {
        line
    };
    // Room for a scroll bar, so one never covers the last row or step.
    let bar = style.spacing.scroll.bar_width + style.spacing.scroll.bar_outer_margin;
    egui::vec2(width + bar, hint + rows + add_channel + bar)
}

/// Whether a path opens as a project: a `.loom` file, or a MIDI file,
/// which is imported.
pub fn is_project_file(path: &std::path::Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case(file::EXTENSION))
        || loom_core::midi_file::is_midi_file(path)
}
