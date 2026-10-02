# Work Notes

A lightweight, portable note-taking app for Windows. Notes live as thin colorful bars docked to the right edge of your screen — always visible, never in the way.

## Features

- **Dock-style magnification** — hover over the bar strip and nearby bars scale up with smooth spring physics, just like the macOS Dock
- **Peek preview** — magnified bars show a snippet of the note's content
- **Click to edit** — a panel slides out from the right edge with a full text editor
- **Color coded** — each note gets a color from a 12-color palette; click the color indicator to change it
- **Expand to center** — grow the editing panel for longer notes, shrink it back when done
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

Run the executable. A narrow strip of colored bars appears at the right edge of your screen, always on top. Hover to magnify, click to edit, use the `+` button at the bottom to add notes.

## License

MIT
