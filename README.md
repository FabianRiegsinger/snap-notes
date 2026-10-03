# Snap Notes

**Sticky notes that live on the edge of your screen.** Each note is a thin colored bar docked to the right edge, always on top and never in the way. Hover to magnify the bars like the macOS Dock, rest on one to peek inside, and click to unfold it into a full sticky note.

![Snap Notes: the bar strip on the right edge, a hover peek and an open note](docs/assets/overview.svg)

- **Out of the way:** about 6 px of screen per note. Clicks anywhere else go straight to the apps behind it.
- **One glance away:** peek at any note without opening it.
- **Portable:** a single executable, no installer. Your notes are a JSON file next to it.
- **Cross-platform:** macOS, Windows and Linux.

## What it can do

### The bar strip
- **Dock-style magnification:** move the cursor along the strip and nearby bars grow with smooth spring physics.
- **Hover peek:** rest on a bar and it widens into a small preview with the title, a divider and the first lines. Move onto the preview to keep it open, and click it to open the note.
- **Quick add:** click the `+` slot below the bars (the whole strip width counts), or press Cmd/Ctrl+N.
- **Reorder:** drag a bar up or down to move the note.
- **Scroll:** with more notes than fit on screen, scroll the strip with the mouse wheel.

### Notes
- **Unfold and fold:** a click morphs the bar into a sticky note. Esc, ✕, a click outside the note or a click on its bar folds it back.
- **Title and body:** each note has a title header and a scrolling body. The scrollbar only appears once the text overflows.
- **Move it anywhere:** drag the grip at the top. The note reopens where you left it, and double-clicking the grip docks it again.
- **Resize it:** drag any edge or corner. Each note remembers its own size.
- **Color code it:** pick from a 20-color palette with the swatch in the header.
- **Delete with confirmation:** the 🗑 button asks first, then folds the note away.

### Settings
Open them from the menu bar/tray icon or with Cmd/Ctrl+,. Every change applies live and is saved automatically, and each group has a Reset button.

| Group | What you can adjust |
|---|---|
| App | Show or hide the menu bar/tray icon and the Dock icon/taskbar button. At least one always stays visible. |
| Bars | Width, height and gap |
| Hover | Magnification, how far it spreads, and the peek delay (0–3 s) |
| Notes | Default size, paper tint (lighter/darker than the bar), and how faint the header controls are until you hover |
| Motion | Animation speed (0.25–3×) |
| Window | How much of the screen height the strip may use |
| Palette | Replace any of the 20 note colors from 60 presets. Notes using the old color follow along. |

### Menu bar / tray
On macOS and Windows a small icon offers **Show/Hide Notes**, **New Note**, **Settings…** and **Quit**. Hiding the notes clears the screen completely until you show them again.

## Keyboard shortcuts

| Shortcut | Action |
|---|---|
| Cmd/Ctrl+N | New note |
| Cmd/Ctrl+, | Open or close settings |
| Esc | Close settings, or fold the open note |

## Platform support

| | macOS | Windows | Linux |
|---|---|---|---|
| Bar strip, peek, notes, settings | ✓ | ✓ | ✓ |
| Click-through around the strip | ✓ | ✓ | The window shrinks to the strip instead |
| Menu bar / tray icon | ✓ | ✓ | – (the strip has a settings slot instead) |
| Hide Dock icon / taskbar button | ✓ | ✓ | – |
| Single instance | – | ✓ | – |

## Your data

Notes are stored in `notes.json` and settings in `settings.json`, next to the executable. Edits save automatically, with a 500 ms debounce and atomic writes, so a crash never leaves a half-written file. A hand-edited `settings.json` with a typo keeps every valid value and falls back to defaults only for the broken ones.

To move Snap Notes to another machine, copy the executable together with both JSON files.

## Install

Every push to `main` builds Snap Notes for **Windows x64, macOS arm64, Linux x64 and Linux arm64**. Download the binary from the build artifacts in [Actions](https://github.com/FabianRiegsinger/snap-notes/actions), or build it yourself:

```bash
cargo build --release
./target/release/snap-notes
```

The release binary is optimized for size (`opt-level = "z"`, LTO, stripped). Linux builds need `pkg-config`, `libxkbcommon-dev`, `libwayland-dev` and `libfontconfig1-dev`.

## Under the hood

- **Rust** with [iced](https://iced.rs) 0.14 (wgpu rendering)
- A custom bar-strip widget with Gaussian magnification and critically damped springs
- [`tray-icon`](https://crates.io/crates/tray-icon) for the menu bar/tray icon, plus native calls for click-through and Dock/taskbar visibility (AppKit on macOS, Win32 on Windows)
- JSON persistence with lenient loading and atomic writes

## License

MIT
