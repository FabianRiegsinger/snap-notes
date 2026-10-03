# Snap Notes

A lightweight, portable note-taking app for Windows. Notes live as thin colorful bars docked to the vertical center of your screen's right edge — always visible, never in the way.

## Features

- **Dock-style magnification** — hover over the bar strip and nearby bars scale up with smooth spring physics, just like the macOS Dock
- **Peek preview** — rest on a bar and it widens into a small preview of the note; hover the preview to keep it open, click it to open the note
- **Click to open** — the bar morphs into a sticky note with a title header and a body; Esc, ✕, clicking outside or clicking the bar again folds it back
- **Color coded** — each note gets a color from a 20-color palette; click the color indicator to change it
- **Drag anywhere** — drag an open note by its top grip; it reopens where you left it (double-click the grip to dock it again)
- **Click-through** — empty screen areas pass clicks to the apps behind, so the dock never gets in the way
- **Resize** — drag any edge or corner of an open note; each note remembers its size
- **Quick add** — the `+` below the bars (whole strip width is clickable) or Cmd/Ctrl+N
- **Drag to reorder** — hold and drag bars to rearrange your notes
- **Scroll overflow** — mousewheel scrolls the strip when you have more notes than screen space
- **Auto-save** — edits are saved automatically (500ms debounce, atomic writes)
- **Portable** — single `.exe`, no installation; data stored as `notes.json` next to the executable

## Tech Stack

- **Rust** with the [iced](https://iced.rs) GUI framework (0.14, wgpu-based rendering)
- Custom widget for the bar strip with Gaussian magnification and spring animation
- Critically damped spring physics for all animations
- JSON persistence with atomic file writes

## Build

```bash
cargo build --release
```

The release binary is optimized for size (`opt-level = "z"`, LTO, stripped).

## Usage

Run the executable. A narrow strip of colored bars appears at the right edge of your screen, always on top. Hover to magnify, click to open a note, use the `+` button below the bars (or Cmd/Ctrl+N) to add notes.

## License

MIT
