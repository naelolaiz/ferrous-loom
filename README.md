# Ferrous Loom

An open source, pattern-based music studio written in Rust, inspired by the
workflow of FL Studio. Build patterns in a step sequencer or piano roll,
arrange them into songs, and learn harmony along the way.

> Ferrous Loom is an independent project. It is not affiliated with
> Image-Line or FL Studio.

![Ferrous Loom playing the demo song: the playlist with its Intro, Verse and Chorus sections, the channel rack, and the harmony keys](screenshots/main-window.png)

## Status

Early development. Runs on Linux, macOS and Windows.

| Step | Contents | Status |
|------|----------|--------|
| 1 | Window, audio engine, test tone, CI | Done |
| 2 | Channel rack, step sequencer, drum kit, sampler, synth | Done |
| 3 | Piano roll with harmony helpers | Done |
| 4 | Playlist, saving, macOS and Windows | Done |
| 5 | Undo and redo, WAV export | Done |
| 6 | Harmony tools: selections, arpeggios, inversions, suggestions, ghost notes, velocities | Done |
| 7 | MIDI in and out: keyboards, external synths, recording, chord names as you play, `.mid` export | Done |
| 8 | Harmony keys, time signatures, a library of styles and rhythms, public-domain songs as demos | Done |
| Spike | Import a recording: tempo, key and chords heard from an MP3 or WAV file | Done |

## Build and run

Install Rust with [rustup](https://rustup.rs), then the system's build
tools:

- **Linux** (Debian/Ubuntu):
  `sudo apt install build-essential pkg-config libasound2-dev libxkbcommon-dev libxkbcommon-x11-0 libwayland-dev`
- **macOS**: `xcode-select --install`
- **Windows**: the Visual Studio Build Tools, which `rustup-init.exe`
  offers to install

```sh
cargo run --release
```

Press **Space** to play the demo song: two bars of a C, Am, F, G chord
progression, then a beat comes in, and a drum fill ends it. The play
button pauses while playing, and Stop goes back to where the playhead was
put. **Home** goes to the start, **Page Up** and **Page Down** skip a bar
back or forward, and clicking the bar numbers moves the playhead. The playlist
arranges patterns into a song: click to place a clip of the selected
pattern, drag clips around, right-click to delete, and double-click a
clip to edit it: melodies and chords open in the piano roll, drums in
the channel rack. Drag a clip's right edge to repeat its pattern. Drag
along the strip above the bar numbers to mark sections (Intro, Verse,
Chorus…), and click a section to rename it, play it several times in a
row, loop it or copy it. Drag along the bar numbers to loop part of the
song. Right-click a track's name to insert a track above or below it,
move it up or down, or delete it. **F6** shows or hides
the channel rack, where you click steps and drop WAV files to use your
own sounds. **F7** shows the piano roll, on the selected pattern: when
the channel has no notes there, it says so and offers the patterns that
do. Pick a key to highlight its notes,
draw notes or whole chords, and watch the chord names above the grid.
Select notes with Ctrl+click or a Ctrl+drag box, then move them with the
arrow keys (Alt+↑ stays in the key), invert or arpeggiate them from the
Tools menu, and set velocities in the lane under the grid. The chord
buttons light up with the chords that often come next.
**Ctrl+S** saves the project as a `.loom` file (⌘S on a Mac), **Ctrl+Z**
undoes any change and **Ctrl+Shift+Z** redoes it, and **File > Export
WAV…** saves the song as a sound file.

The **MIDI** menu connects a MIDI keyboard, which plays the channel
selected in the channel rack, and an output port for **MIDI Out**
channels, which play an external synth. No MIDI keyboard? Turn on
**Typing keyboard to piano** there, and the letter keys play notes. The
status bar names the chord you hold down, and **● Rec** records what
you play into the pattern while it loops. **File > Export song as
MIDI…** writes a `.mid` file that other studios can open.

The **tempo strip** at the top of the playlist draws the song's tempo as
a line. Click it to change the tempo from that bar on, at once or
gradually over a beat to eight bars, which is how a ritardando (slowing
down) or an accelerando (speeding up) is written; WAV and MIDI exports
play the changes too. **Loose**, next to it, makes notes land a few
milliseconds off the grid, as when a person plays (off by default). The
demos play like a performance: the tempo changes and fermatas their
scores mark, a little slowing down at the end of songs and hymns, and
velocities shaped by the beat and the phrase.

Along the bottom are the **harmony keys**: the key and time signature
of the project, and a big key for each chord of the key (I to vii°),
coloured by what it does: home, away or tension. Hold one, or press
**1** to **7**, to play its chord; the chords that often come next
light up, and while the music plays, the chord you hear lights up too.
The **Library** tab next to the pattern list holds 78 harmony styles
and 49 rhythms from pop, rock, blues, jazz, soul, hip hop, electronic,
Latin, tango, flamenco, reggae, folk and classical music: click one to
hear it in your key. Pick a rhythm after a style, and the style's chords
and bass are played the way that rhythm goes; right-click a rhythm and
**Play the whole song in this rhythm** changes the drums, chords and bass
of the whole song at once, keeping the melody. **File > Demos** opens
62 songs with their melodies, sorted by genre: public-domain tunes from *Ode to Joy* to
*Für Elise* and the *Habanera*, and new tunes written to show off the
blues, tango, flamenco and more. It also plays every style as a short
song.

For music without a home key, the key menus also offer a **twelve-tone
row** and a **pitch-class set**. **Row…** in the piano roll opens the
twelve-tone window: type, draw or pick a row (the rows of Schoenberg's
Suite Op. 25, Berg's *Lyric Suite*, and Webern's Symphony Op. 21 and
Concerto Op. 24, all public domain now, or a few written for Ferrous
Loom), see its matrix
with all 48 forms (P, I, R and RI on every note), and write any of them
into the pattern as a melody or as chords, with the durations, dynamics
and octaves following the row too if you like. **Sets…** lists all 224
pitch-class sets by Forte number, with their prime forms and interval
vectors, to draw with or use as a scale, and selecting notes names their
set. The Library's **Twelve-tone rows** and **File > Demos > Twelve-tone**
play seven ways of using a row.

**Micromodes…** opens Francisco Kröpfl's twelve micromodes: the twelve
groups of three notes (Forte's trichords, 3-1 to 3-12) in his three
families, minor, major and chords. Each is drawn on a clock, with its
prime form in solfège, its intervals and its hour on Peter Schat's Tone
Clock, and can be heard, drawn with or used as the key. Selecting notes
shows which micromodes they hold, and the Micromodes and Sets windows
link to each other: a three-note set is a micromode, and every larger
set is made of them. The Library's **Micromodes (Kröpfl)** and **File >
Demos > Micromodes** play music made from one micromode.

**File > Import recording…** opens an MP3, WAV, FLAC or Ogg file (or
drop it on the window) and listens to it: the tempo, time signature and
key are set from the recording, which plays on its own channel, and a
Harmony channel plays the chord heard on every beat, so the piano roll
and the harmony keys follow along. This is an early experiment; see
[`crates/loom-listen`](crates/loom-listen/README.md) for how it works and
how often it is right.

**File > Import MIDI file…** opens a `.mid` file (or drop it on the
window) as a new project: each track becomes a channel (the drums of
channel 10 one channel per drum), every note keeps its velocity and its
exact time, even between two steps, and the tempo changes, time
signatures and key come with it. The piano roll's Chords lane and the
harmony keys then name the chords as it plays.

Too small or too big? As in FL Studio, the playlist and the piano roll
zoom each direction on its own: **Ctrl** with the mouse wheel makes bars
and steps wider or narrower, and **Alt** with the wheel makes tracks and
keys taller or shorter, around what is under the pointer. **Ctrl** or
**Alt** with **+** and **−** (⌘ instead of Ctrl on a Mac) do the same in
the window you clicked last, and **Ctrl+0** takes both back to 100%. The
channel rack has one zoom for both directions. The playlist's two zooms
are also at the right end of the status bar, and **View > Interface
size** makes the text, buttons and menus bigger or smaller everywhere.
The View menu also shows or hides each window and panel, and **Alt+F8**
hides the Patterns and Library panel to give the playlist more room. The
zoom, the window size and where you left the windows are remembered for
next time.

Ready-built programs for all three systems are attached to every CI run
as `ferrous-loom-linux`, `ferrous-loom-macos` and `ferrous-loom-windows`.
They are not signed yet, so macOS and Windows warn before the first start.

## Screenshots

| | |
|---|---|
| ![The piano roll on the chords of the demo song, with the chord names above the grid and the chords that often come next lit up](screenshots/piano-roll.png) | ![The Library tab: Bossa nova has just been tried, and its tooltip shows its chords in C major](screenshots/library.png) |
| The piano roll names the chords as they play and lights up the ones that often come next. | The Library: 78 harmony styles and 49 rhythms to try in your key. |
| ![A recording imported with File > Import recording: the chords heard on every beat in the piano roll](screenshots/import-recording.png) | ![The menu of a track: insert above or below, move up or down, delete](screenshots/track-menu.png) |
| An imported recording, with the chords heard on every beat. | Right-click a track's name to insert, move or delete tracks. |
| ![The twelve-tone window beside the piano roll: the matrix of a row with all its forms, and the piano roll numbering the notes of the row](screenshots/twelve-tone-row.png) | ![The pitch-class set browser showing the octatonic scale, 8-28, beside the piano roll](screenshots/pitch-class-sets.png) |
| A twelve-tone row and its matrix: P, I, R and RI on every note. | The pitch-class sets by Forte number, to draw with or use as a scale. |
| ![The Micromodes window: the three families of Kröpfl's micromodes, Menor 3 drawn on a clock with its prime form do - do# - mi, and the micromodes in the selected C major chords](screenshots/micromodes.png) | ![The Sets window on 7-35, the diatonic scale, listing the micromodes inside its 35 groups of three](screenshots/micromodes-in-sets.png) |
| Kröpfl's twelve micromodes, and the ones in the notes you select. | Every set lists its micromodes, and each one links back. |

## Project layout

| Path | Contents |
|------|----------|
| `crates/loom-core` | Music types and theory: notes, scales, chords and chord naming, twelve-tone rows, pitch-class sets and micromodes, the harmony keys, tempo and time signatures, patterns, the playlist, project files, undo history and project diffs, the sequencer clock, MIDI messages and MIDI files, recording, and the library of styles, rhythms and songs. No audio, no UI. |
| `crates/loom-audio` | Real-time audio engine on top of `cpal`: pattern and song playback, drum synthesis, a pitched sampler, a polyphonic synth, MIDI Out channels, live keys, and offline rendering to WAV files. |
| `crates/loom-listen` | Listening to a recording: decoding audio files, finding the beats, the key and the chord on every beat, and turning them into a project. |
| `crates/loom-app` | The desktop app (`ferrous-loom`), built with `egui`, with MIDI through `midir`. |

## License

GPL-3.0-or-later. See [LICENSE](LICENSE).
