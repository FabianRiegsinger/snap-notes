# Snap Notes — Design Spec

## Overview

A portable Windows 10 note-taking application built with Rust and the `iced` GUI framework. Notes appear as thin, colorful vertical bars docked to the right edge of the screen, always on top of other windows. Hovering triggers a macOS-Dock-style magnification effect with a read-only content preview. Clicking opens an editable panel. The app ships as a single `.exe` with no installation required.

## Goals

- Fast startup, minimal resource usage (zero CPU when idle)
- Single portable `.exe`, no runtime dependencies
- Unobtrusive but always accessible
- Smooth, polished animations throughout

## Tech Stack

- **Language:** Rust
- **GUI:** `iced` (wgpu-based GPU-accelerated rendering)
- **Platform interop:** `windows` crate for Win32 API (tray icon, window flags, click-through)
- **Build tooling:** `winres` for embedding the app icon
- **Target:** `x86_64-pc-windows-msvc`

## Architecture

Four main components:

### 1. Window Manager

A borderless, transparent, always-on-top window spanning the right edge of the screen.

- No title bar, no frame, transparent background
- Transparent regions are click-through (do not steal input from windows below)
- A thin hover-detection margin (~30px) extends left of the visible bars to detect the cursor approaching and trigger magnification; this margin captures mouse events but remains visually transparent
- Resizes dynamically when a note panel opens or expands
- Single-instance enforcement: launching a second instance brings the first to focus

### 2. Bar Strip Widget (custom `iced` widget)

Renders the vertical stack of colored bars and the "+" button at the bottom.

- Each bar represents one note, colored by the user's chosen color
- Bars are ~4-6px wide at rest, flush against the right screen edge
- Bars stack top-to-bottom with small gaps between them
- A "+" button sits at the bottom of the strip, visible on hover
- If bars overflow the screen height, the strip scrolls via mousewheel

**Magnification animation:**

Tracks cursor Y-position and applies a Gaussian scaling function to each bar based on distance from cursor:

```
scale = 1.0 + max_magnification * e^(-distance² / spread²)
```

- The hovered bar scales to ~5x width and ~1.5x height
- Neighboring bars scale proportionally with smooth falloff
- Spring-based easing, ~150ms transition
- Animation cancels cleanly if interrupted (mouse leaves mid-magnification)

### 3. Note Panel

Appears when a bar is clicked. Slides out leftward from the bar.

**Editing state** (~300px wide, anchored to right edge):

- Scrollable, editable text area with the note's full content
- Scrollbar appears automatically when content exceeds panel height
- Color indicator at the top (clickable to open color picker)
- "x" button — triggers a confirmation dialog ("Delete this note?") before removing
- "Expand" button (top-right corner) — transitions to expanded state

**Expanded state** (centered on screen):

- Window animates to screen center
- Resizes to fit all note content plus ~100px extra vertical space for comfortable editing
- "Shrink back" button returns to the editing state at the right edge

**Transition animations:**

- Slide-out for editing: ease-out, ~200ms
- Expand to center: ease-in-out, ~300ms
- All transitions cancel cleanly if interrupted

### 4. Persistence Layer

Notes stored in a `notes.json` file located next to the `.exe`.

**Note schema:**

```json
{
  "notes": [
    {
      "id": "uuid-v4",
      "color": "#FF6B6B",
      "content": "Note text...",
      "order": 0,
      "created_at": "2026-10-02T14:30:00Z",
      "updated_at": "2026-10-02T15:12:00Z"
    }
  ]
}
```

- Auto-save: debounced 500ms after last keystroke, writes full JSON
- On reorder (drag), all affected `order` values are reassigned and saved

## Note States

### 1. Resting (idle)

- Thin colored bar (~4-6px wide) flush against the right screen edge
- No animation running, zero CPU usage

### 2. Peek (hover)

- Cursor approaching the bar strip triggers magnification
- Hovered bar grows wider, showing a truncated read-only preview of note content (first few lines, clipped)
- Neighboring bars scale with Gaussian falloff
- Read-only — no editing in this state

### 3. Editing (click)

- Panel slides out leftward from the clicked bar
- Full content visible and editable in a scrollable text area
- Color picker, delete ("x"), and expand button accessible

### 4. Expanded (full-size)

- Note detaches from the right edge, animates to screen center
- Resizes to show all content plus extra space
- "Shrink back" button returns to editing state at the right edge

## Interaction Details

### Creating a note

- Hover over the bar strip area to reveal the "+" button at the bottom
- Click "+" to create a new note with a randomly assigned color from a curated palette of ~12 distinguishable colors
- The new note appears at the bottom of the stack (above the "+" button)
- Opens immediately in editing state

### Reordering notes

- Click and hold a bar to initiate drag
- Drag vertically to reposition among other bars (macOS Dock-style drag reorder)
- Release to drop in the new position
- Order persists across sessions

### Changing a note's color

- While in editing or expanded state, click the color indicator at the top of the panel
- A small color picker appears with the ~12 curated color swatches
- Click a swatch to apply the color immediately

### Deleting a note

- Click the "x" button on an open note panel
- A confirmation dialog appears: "Delete this note?"
- Confirm to permanently remove the note and its bar

## Color Palette

12 curated colors for auto-assignment and the color picker:

| Name       | Hex       |
|------------|-----------|
| Coral      | `#FF6B6B` |
| Peach      | `#FFA07A` |
| Amber      | `#FFD93D` |
| Mint       | `#6BCB77` |
| Teal       | `#4ECDC4` |
| Sky        | `#45B7D1` |
| Periwinkle | `#7C83FD` |
| Lavender   | `#B983FF` |
| Rose       | `#F78FB3` |
| Slate      | `#778899` |
| Sand       | `#DEB887` |
| Sage       | `#87AE73` |

## Window Behavior

### System tray

- App appears only as a system tray icon (small colored square)
- Right-click tray menu: "Show/Hide bars", "Exit"
- No taskbar button

### Screen positioning

- On startup, detect primary monitor resolution
- Dock bar strip to the right edge, vertically centered
- Strip height adapts to the number of notes

### Single instance

- On launch, check if an instance is already running
- If so, bring the existing instance to focus instead of opening a second one

## Build & Distribution

- `cargo build --release` produces the single `.exe`
- App icon embedded via `winres`
- Target: `x86_64-pc-windows-msvc`
- Expected binary size: ~8-15 MB
- No installer, no runtime dependencies
- `notes.json` created automatically on first note creation, next to the `.exe`
