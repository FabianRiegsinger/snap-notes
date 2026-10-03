# Snap Notes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a portable Windows 10 note-taking app — colorful bars docked to the right screen edge with Dock-style magnification, click-to-edit panels, and JSON persistence.

**Architecture:** Single-window `iced` app (Elm architecture). A custom `Widget` renders the bar strip with Gaussian magnification. The window is a thin borderless transparent always-on-top strip at the right edge that resizes as panels open. Notes persist as JSON next to the executable.

**Tech Stack:** Rust, iced 0.14 (wgpu, advanced features), serde/serde_json, uuid, chrono, rand, single-instance, tray-item (Windows), winres (build)

**Spec:** `docs/superpowers/specs/2026-10-02-snap-notes-design.md`

## Global Constraints

- Target: `x86_64-pc-windows-msvc`; must also compile on macOS/Linux for development
- Single portable `.exe`, no runtime dependencies beyond OS-provided ones
- All Windows-specific code behind `#[cfg(target_os = "windows")]`
- 12-color palette with exact hex values from spec
- Animation durations: magnification ~150ms spring, slide-out ~200ms ease-out, expand ~300ms ease-in-out
- `notes.json` lives next to the executable (discovered via `std::env::current_exe()`)

## Review Focus

1. **Missing or empty `notes.json` on first launch** — app must create an empty store, not crash or show an error
2. **Very long note content in peek preview** — must clip to the magnified bar bounds, never overflow
3. **Rapid hover in/out interrupting animations** — `AnimationState::set_target` mid-flight must produce smooth reversal, no jumps
4. **Window resize/move during expand/shrink** — `iced::window::resize` + `move_to` called together must not flash white or show stale frames
5. **Save debounce under rapid typing** — timer resets on each keystroke; only the final state is written; no partial JSON on disk

---

## File Structure

```
Cargo.toml
build.rs                 # winres icon embedding (Windows only)
assets/icon.ico          # app icon
src/
  main.rs                # entry point, window config
  app.rs                 # App state, Message enum, update/view/subscription
  note.rs                # Note struct, NoteColor, PALETTE constant
  store.rs               # NoteStore: load/save/auto-save debounce
  animation.rs           # Gaussian scaling, easing fns, AnimationState, MagnificationState
  bar_strip.rs           # BarStrip custom Widget
  note_panel.rs          # note_panel() view function
  color_picker.rs        # color_picker() view function
  platform.rs            # #[cfg(windows)] tray icon, single instance
```

---

### Task 1: Data Model and Persistence

**Files:**
- Create: `src/note.rs`
- Create: `src/store.rs`

**Interfaces:**
- Consumes: nothing
- Produces:
  - `NoteColor` — wraps `[f32; 4]` RGBA, implements `Serialize`/`Deserialize` via hex string
  - `PALETTE: [NoteColor; 12]` — the 12 curated colors from spec
  - `NoteColor::random() -> NoteColor` — picks randomly from `PALETTE`
  - `Note { id: Uuid, color: NoteColor, content: String, order: usize, created_at: DateTime<Utc>, updated_at: DateTime<Utc> }`
  - `Note::new(color: NoteColor) -> Note`
  - `NoteStore::load(path: PathBuf) -> NoteStore` — reads JSON or returns empty store
  - `NoteStore::save(&self) -> io::Result<()>`
  - `NoteStore::notes(&self) -> &[Note]`
  - `NoteStore::note_mut(&mut self, id: Uuid) -> Option<&mut Note>`
  - `NoteStore::add_note(&mut self) -> Uuid` — random color, appended at end, returns id
  - `NoteStore::delete_note(&mut self, id: Uuid)`
  - `NoteStore::reorder(&mut self, from_index: usize, to_index: usize)`
  - `NoteStore::mark_dirty(&mut self)` — records current instant
  - `NoteStore::should_save(&self) -> bool` — true if dirty and 500ms+ since last mark
  - `NoteStore::did_save(&mut self)` — clears dirty flag

- [ ] **Step 1: Write tests for `NoteColor` serialization and `PALETTE`**

```rust
// src/note.rs — tests module
#[test]
fn note_color_roundtrip_hex() {
    let color = NoteColor::from_hex("#FF6B6B");
    assert_eq!(color.to_hex(), "#FF6B6B");
    let json = serde_json::to_string(&color).unwrap();
    let back: NoteColor = serde_json::from_str(&json).unwrap();
    assert_eq!(back.to_hex(), "#FF6B6B");
}

#[test]
fn palette_has_12_distinct_colors() {
    assert_eq!(PALETTE.len(), 12);
    let hexes: HashSet<_> = PALETTE.iter().map(|c| c.to_hex()).collect();
    assert_eq!(hexes.len(), 12);
}

#[test]
fn random_color_is_from_palette() {
    for _ in 0..50 {
        let c = NoteColor::random();
        assert!(PALETTE.iter().any(|p| p.to_hex() == c.to_hex()));
    }
}
```

- [ ] **Step 2: Run tests — expect failures (no implementation)**

Run: `cargo test --lib note`
Expected: compilation errors

- [ ] **Step 3: Implement `NoteColor`, `PALETTE`, and `Note` in `src/note.rs`**

`NoteColor` stores `[f32; 4]` internally. Custom `Serialize`/`Deserialize` converts to/from `"#RRGGBB"` hex strings. `Note::new` sets `created_at`/`updated_at` to `Utc::now()`, generates a `Uuid::new_v4()`.

- [ ] **Step 4: Run tests — expect pass**

Run: `cargo test --lib note`
Expected: all pass

- [ ] **Step 5: Write tests for `NoteStore`**

```rust
// src/store.rs — tests module
#[test]
fn load_missing_file_returns_empty_store() {
    let store = NoteStore::load(PathBuf::from("/nonexistent/notes.json"));
    assert!(store.notes().is_empty());
}

#[test]
fn add_save_load_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("notes.json");
    let mut store = NoteStore::load(path.clone());
    let id = store.add_note();
    store.note_mut(id).unwrap().content = "Hello".into();
    store.save().unwrap();
    let loaded = NoteStore::load(path);
    assert_eq!(loaded.notes().len(), 1);
    assert_eq!(loaded.notes()[0].content, "Hello");
}

#[test]
fn reorder_moves_note() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = NoteStore::load(dir.path().join("n.json"));
    let a = store.add_note();
    let b = store.add_note();
    let c = store.add_note();
    store.reorder(0, 2);
    assert_eq!(store.notes()[2].id, a);
    assert_eq!(store.notes()[0].id, b);
}

#[test]
fn debounce_timing() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = NoteStore::load(dir.path().join("n.json"));
    store.add_note();
    store.mark_dirty();
    assert!(!store.should_save()); // just marked, not 500ms yet
}
```

- [ ] **Step 6: Run tests — expect failures**

Run: `cargo test --lib store`

- [ ] **Step 7: Implement `NoteStore` in `src/store.rs`**

`load` reads JSON via `serde_json::from_reader`, falls back to empty `Vec`. `save` writes atomically (write to `.tmp`, rename). `reorder` uses `Vec::remove` + `Vec::insert`, then reassigns all `order` fields. `should_save` compares `Instant::elapsed()` against 500ms.

- [ ] **Step 8: Run all tests**

Run: `cargo test --lib`
Expected: all pass

- [ ] **Step 9: Commit**

```bash
git add src/note.rs src/store.rs
git commit -m "feat: add Note data model and JSON persistence"
```

---

### Task 2: Animation Engine

**Files:**
- Create: `src/animation.rs`

**Interfaces:**
- Consumes: nothing
- Produces:
  - `gaussian_scale(distance: f32, max_mag: f32, spread: f32) -> f32` — returns `1.0 + max_mag * e^(-d²/s²)`
  - `ease_out(t: f32) -> f32` — `1.0 - (1.0 - t)³`
  - `ease_in_out(t: f32) -> f32` — cubic ease-in-out
  - `AnimationState::new(value: f32) -> Self`
  - `AnimationState::set_target(&mut self, target: f32)`
  - `AnimationState::tick(&mut self, dt: f32) -> bool` — spring physics step, returns `true` if still moving
  - `AnimationState::value(&self) -> f32`
  - `MagnificationState::new() -> Self`
  - `MagnificationState::update(&mut self, cursor_y: Option<f32>, bar_centers: &[f32], dt: f32) -> bool` — updates per-bar scales, returns `true` if animating
  - `MagnificationState::scale(&self, index: usize) -> f32` — current scale for bar at index

- [ ] **Step 1: Write tests for math functions and `AnimationState`**

```rust
#[test]
fn gaussian_at_zero_distance() {
    let s = gaussian_scale(0.0, 4.0, 50.0);
    assert!((s - 5.0).abs() < 0.001); // 1.0 + 4.0
}

#[test]
fn gaussian_far_away_is_one() {
    let s = gaussian_scale(500.0, 4.0, 50.0);
    assert!((s - 1.0).abs() < 0.01);
}

#[test]
fn ease_out_boundaries() {
    assert!((ease_out(0.0)).abs() < 0.001);
    assert!((ease_out(1.0) - 1.0).abs() < 0.001);
}

#[test]
fn animation_reaches_target() {
    let mut anim = AnimationState::new(0.0);
    anim.set_target(1.0);
    for _ in 0..200 { anim.tick(1.0 / 60.0); }
    assert!((anim.value() - 1.0).abs() < 0.01);
}

#[test]
fn animation_reversal_no_jump() {
    let mut anim = AnimationState::new(0.0);
    anim.set_target(1.0);
    for _ in 0..5 { anim.tick(1.0 / 60.0); }
    let mid = anim.value();
    anim.set_target(0.0);
    anim.tick(1.0 / 60.0);
    // value should move toward 0, not jump
    assert!(anim.value() < mid);
}
```

- [ ] **Step 2: Run tests — expect failures**

Run: `cargo test --lib animation`

- [ ] **Step 3: Implement `animation.rs`**

Spring physics for `AnimationState`: critically damped spring (`stiffness = 300.0`, `damping = 2.0 * stiffness.sqrt()`). `MagnificationState` holds a `Vec<AnimationState>`, one per bar. `update` computes Gaussian target for each bar from cursor distance, then ticks each spring.

- [ ] **Step 4: Run tests — expect pass**

Run: `cargo test --lib animation`

- [ ] **Step 5: Commit**

```bash
git add src/animation.rs
git commit -m "feat: add animation engine with spring physics and Gaussian magnification"
```

---

### Task 3: App Shell and Bar Strip Widget

**Files:**
- Create: `Cargo.toml`
- Create: `src/main.rs`
- Create: `src/app.rs`
- Create: `src/bar_strip.rs`

**Interfaces:**
- Consumes: `Note`, `NoteStore`, `NoteColor` (Task 1); `MagnificationState`, `AnimationState` (Task 2)
- Produces:
  - `Message` enum (all variants listed below — later tasks add handling, not new variants)
  - `App` struct with `update`, `view`, `subscription`
  - `BarStrip` custom widget (implements `iced::advanced::widget::Widget`)

`Message` enum (complete — all tasks handle subsets):

```rust
pub enum Message {
    // Bar strip
    StripHover(Option<f32>),    // cursor Y in strip, or None if left
    BarClicked(usize),
    AddNote,
    // Editing
    NoteEdited(text_editor::Action),
    ClosePanel,
    DeleteRequested,
    ConfirmDelete(bool),
    // Color
    ToggleColorPicker,
    ColorChosen(NoteColor),
    // Expand
    ExpandNote,
    ShrinkNote,
    // Drag
    DragStart(usize),
    DragMove(f32),
    DragEnd,
    // Animation tick
    Tick(Instant),
    // Save
    SaveTick,
    // Platform
    ToggleVisibility,
    Quit,
}
```

- [ ] **Step 1: Create `Cargo.toml` with all dependencies**

```toml
[package]
name = "snap-notes"
edition = "2021"

[dependencies]
iced = { version = "0.14", features = ["wgpu", "advanced"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
uuid = { version = "1", features = ["v4", "serde"] }
chrono = { version = "0.4", features = ["serde"] }
rand = "0.9"

[target.'cfg(windows)'.dependencies]
single-instance = "0.3"
tray-item = "0.10"
windows = { version = "0.58", features = ["Win32_UI_WindowsAndMessaging", "Win32_Foundation"] }
winres = "0.1"

[dev-dependencies]
tempfile = "3"

[profile.release]
opt-level = "z"
lto = true
strip = true
```

- [ ] **Step 2: Create `src/main.rs`**

Entry point: calls `iced::application` with `App::update`, `App::view`. Sets window to borderless, transparent, `Level::AlwaysOnTop`. Initial size: 40px wide, full screen height. Position: flush right edge of primary monitor. Set `visible_in_taskbar: false` in window settings to hide from taskbar. Add `mod` declarations for all modules.

- [ ] **Step 3: Implement `App` struct and `Message` in `src/app.rs`**

`App` holds: `store: NoteStore`, `magnification: MagnificationState`, `active_note: Option<Uuid>`, `editor_content: Option<text_editor::Content>`, `panel_slide: AnimationState` (0=closed, 1=open), `animating: bool`, plus fields for expand, color picker, drag, and confirm_delete (initialized to `None`/defaults).

`update`: handle `StripHover` (update magnification target), `BarClicked` (set active_note, create editor content from note, animate panel open), `AddNote` (store.add_note, open panel), `Tick` (tick all active animations, set `animating = false` when all settled), `SaveTick` (if `store.should_save()` then `store.save()`). Stub other variants with `Task::none()` for now.

`view`: if `active_note.is_some()`, show bar strip + note panel side by side in a `Row`; otherwise just the bar strip.

`subscription`: if `animating`, return `time::every(Duration::from_millis(16))` mapped to `Tick`. Always return `time::every(Duration::from_secs(1))` mapped to `SaveTick`.

- [ ] **Step 4: Implement `BarStrip` custom widget in `src/bar_strip.rs`**

Struct: `BarStrip<'a> { notes: &'a [Note], magnification: &'a MagnificationState, show_add_button: bool }`.

Implement `Widget<Message, Theme, iced::Renderer>`:
- `size()`: width = max bar width at current magnification, height = `Length::Fill`
- `layout()`: stack bars vertically with 4px gaps. Each bar's height = `BAR_REST_HEIGHT * magnification.scale(i)`, width = `BAR_REST_WIDTH * magnification.scale(i)`. Add "+" button node at bottom if `show_add_button`.
- `draw()`: for each bar, draw a filled rounded rectangle with the note's color. If scale > 2.0, render a clipped text preview (first ~40 chars of content). Draw "+" button as a circle with "+" text.
- `update()`: on `mouse::Event::CursorMoved`, emit `Message::StripHover(Some(y))`. On `CursorLeft`, emit `StripHover(None)`. On `ButtonPressed` over a bar, emit `BarClicked(index)`. On press over "+", emit `AddNote`.
- `mouse_interaction()`: `Interaction::Pointer` over bars/button, `Interaction::default()` elsewhere.

Provide a helper: `pub fn bar_strip<'a>(...) -> Element<'a, Message>` that wraps construction.

- [ ] **Step 5: Wire everything together — verify the app launches showing colored bars**

Run: `cargo run`
Verify: a thin transparent window appears at the right edge with colored bars (add 3 test notes via hardcoded initialization for visual verification). Hovering near bars triggers magnification animation. "+" button visible at bottom on hover.

- [ ] **Step 6: Remove hardcoded test notes, commit**

```bash
git add Cargo.toml src/main.rs src/app.rs src/bar_strip.rs
git commit -m "feat: app shell with bar strip widget and Dock-style magnification"
```

---

### Task 4: Note Editing Panel

**Files:**
- Create: `src/note_panel.rs`
- Modify: `src/app.rs` — wire panel into view and handle editing messages

**Interfaces:**
- Consumes: `Note` (Task 1), `AnimationState` (Task 2), `Message`, `App` (Task 3)
- Produces:
  - `note_panel(note: &Note, content: &text_editor::Content, slide_progress: f32) -> Element<Message>` — the editing panel view

- [ ] **Step 1: Implement `note_panel()` in `src/note_panel.rs`**

Returns a `container` with width = `300.0 * slide_progress`, containing:
- Top row: colored circle (note's color, clickable → `ToggleColorPicker`), spacer, expand button → `ExpandNote`, "x" button → `DeleteRequested`
- Body: `text_editor(content).on_action(Message::NoteEdited)` wrapped in `scrollable`
- Background: white with subtle shadow (dark mode: dark gray)

- [ ] **Step 2: Handle editing messages in `app.rs` `update()`**

- `NoteEdited(action)`: apply action to `editor_content`, copy text back to `store.note_mut(id).content`, call `store.mark_dirty()`
- `ClosePanel`: animate panel closed, on completion clear `active_note`
- `DeleteRequested`: set `confirm_delete = Some(id)`
- `ConfirmDelete(true)`: `store.delete_note(id)`, close panel, save immediately
- `ConfirmDelete(false)`: clear `confirm_delete`

For `ConfirmDelete`, render a simple overlay with "Delete this note?" and Yes/No buttons when `confirm_delete.is_some()`.

- [ ] **Step 3: Verify panel behavior**

Run: `cargo run`
Verify: clicking a bar slides out the editing panel (~200ms). Text is editable. Scrollbar appears on long content. "x" shows confirmation. Closing works. Changes auto-save after 500ms idle.

- [ ] **Step 4: Commit**

```bash
git add src/note_panel.rs src/app.rs
git commit -m "feat: add note editing panel with slide-out animation and auto-save"
```

---

### Task 5: Color Picker and Expand/Shrink

**Files:**
- Create: `src/color_picker.rs`
- Modify: `src/app.rs` — handle color and expand messages

**Interfaces:**
- Consumes: `NoteColor`, `PALETTE` (Task 1), `AnimationState` (Task 2), `Message` (Task 3)
- Produces:
  - `color_picker(current: &NoteColor) -> Element<Message>` — grid of 12 color swatches

- [ ] **Step 1: Implement `color_picker()` in `src/color_picker.rs`**

Renders a 4x3 grid of `button`s, each a filled circle in a palette color. The current color has a checkmark overlay. Click emits `ColorChosen(color)`.

- [ ] **Step 2: Handle `ToggleColorPicker` and `ColorChosen` in `app.rs`**

- `ToggleColorPicker`: toggle `color_picker_open` bool
- `ColorChosen(color)`: set note's color, mark dirty, close picker

Show `color_picker` as a positioned overlay above the color indicator when `color_picker_open`.

- [ ] **Step 3: Handle `ExpandNote` and `ShrinkNote` in `app.rs`**

`App` gains `expand_animation: AnimationState` (0=docked, 1=centered) and `expanded: bool`.

- `ExpandNote`: set `expanded = true`, animate `expand_animation` to 1.0. Compute target window size from note content length (measure text lines × line height + 100px padding, capped at 80% screen). Return `window::resize(id, new_size)` and `window::move_to(id, centered_position)`.
- `ShrinkNote`: animate back to 0.0. On completion, return `window::resize` and `window::move_to` back to right-edge strip dimensions.

In `view`: interpolate window layout between docked and centered based on `expand_animation.value()`.

- [ ] **Step 4: Verify color picker and expand/shrink**

Run: `cargo run`
Verify: color indicator click shows picker, swatch click changes bar and panel header color. Expand button smoothly moves window to center with larger size. Shrink returns to right edge.

- [ ] **Step 5: Commit**

```bash
git add src/color_picker.rs src/app.rs
git commit -m "feat: add color picker and expand-to-center with shrink-back"
```

---

### Task 6: Drag Reorder

**Files:**
- Modify: `src/bar_strip.rs` — add drag detection and visual feedback
- Modify: `src/app.rs` — handle drag messages

**Interfaces:**
- Consumes: `NoteStore::reorder` (Task 1), `Message::DragStart/DragMove/DragEnd` (Task 3)
- Produces: drag behavior on the bar strip widget

- [ ] **Step 1: Add drag state to `BarStrip` and `App`**

`App` gains `drag: Option<DragState>` where `DragState { bar_index: usize, origin_y: f32, current_y: f32 }`.

In `BarStrip::update()`:
- `ButtonPressed` on a bar: emit `DragStart(index)` (same as click — App distinguishes by movement threshold of 5px)
- `CursorMoved` while dragging: emit `DragMove(y)`
- `ButtonReleased` while dragging: emit `DragEnd`

In `App::update()`:
- `DragStart`: record origin
- `DragMove`: if movement > 5px threshold, enter drag mode. Compute target insertion index from y position relative to bar centers
- `DragEnd`: call `store.reorder(from, to)`, mark dirty, clear drag state. If movement was < 5px, treat as a click → `BarClicked`

- [ ] **Step 2: Add drag visual feedback in `BarStrip::draw()`**

When `drag.is_some()`, draw the dragged bar at the cursor y-position with slight transparency. Draw a 2px horizontal line at the insertion point between bars. Other bars shift to make room (gap at target index).

- [ ] **Step 3: Verify drag reorder**

Run: `cargo run`
Verify: click-hold on a bar and drag vertically. Bar follows cursor. Indicator shows insertion point. Release drops bar in new position. Order persists after relaunch.

- [ ] **Step 4: Commit**

```bash
git add src/bar_strip.rs src/app.rs
git commit -m "feat: add drag-to-reorder for note bars"
```

---

### Task 7: Platform Integration and Release Build

**Files:**
- Create: `src/platform.rs`
- Create: `build.rs`
- Create: `assets/icon.ico` (generate a simple colored-bars icon)
- Modify: `src/main.rs` — call platform init before iced launch

**Interfaces:**
- Consumes: `Message::ToggleVisibility`, `Message::Quit` (Task 3)
- Produces:
  - `platform::enforce_single_instance()` — exits if another instance runs
  - `platform::setup_tray(sender: Sender<Message>)` — creates tray icon with menu
  - Platform-specific window adjustments

- [ ] **Step 1: Implement single-instance enforcement in `src/platform.rs`**

```rust
#[cfg(target_os = "windows")]
pub fn enforce_single_instance() {
    let instance = single_instance::SingleInstance::new("snap-notes-{unique-id}").unwrap();
    if !instance.is_single() { std::process::exit(0); }
    std::mem::forget(instance); // keep mutex alive
}

#[cfg(not(target_os = "windows"))]
pub fn enforce_single_instance() {}
```

- [ ] **Step 2: Implement system tray in `src/platform.rs`**

Use `tray-item` crate behind `#[cfg(windows)]`. Menu items: "Show/Hide" → sends `ToggleVisibility`, "Exit" → sends `Quit`. Use a channel to bridge tray events into iced's subscription system.

- [ ] **Step 3: Handle `ToggleVisibility` and `Quit` in `app.rs`**

- `ToggleVisibility`: toggle a `visible: bool` flag; return `window::change_mode` or equivalent to show/hide
- `Quit`: `store.save()`, then `window::close`

- [ ] **Step 4: Create `build.rs` for icon embedding**

```rust
fn main() {
    #[cfg(target_os = "windows")]
    {
        winres::WindowsResource::new()
            .set_icon("assets/icon.ico")
            .compile()
            .expect("failed to compile resources");
    }
}
```

- [ ] **Step 5: Add bar strip scrolling when notes overflow screen**

In `BarStrip`, if total bar height > available height, wrap in a scrollable region. Track scroll offset in widget state. Mousewheel events on the strip area scroll the bars.

- [ ] **Step 6: Verify full app on Windows (or cross-compile and test)**

Run: `cargo build --release --target x86_64-pc-windows-msvc`
Verify: `.exe` runs standalone, tray icon appears, bars dock to right edge, all features work, binary size ≤ 15MB.

- [ ] **Step 7: Commit**

```bash
git add src/platform.rs build.rs assets/ src/main.rs src/app.rs src/bar_strip.rs
git commit -m "feat: add Windows platform integration, tray icon, and release build"
```
