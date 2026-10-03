# Settings System — Design Spec

## Overview

Let the user adjust the app's visual parameters from inside the app. Today about 50 visual values are hardcoded constants spread across `app.rs`, `bar_strip.rs`, `animation.rs`, `note_panel.rs`, `peek.rs` and `note.rs`. This feature moves a curated subset of them into a persisted `Settings` model, exposes them in an in-app settings panel with live preview, and adds an editor for the 20-color note palette.

## Decisions

- **Entry point:** a gear slot in the bar strip below the add button, plus the shortcut Cmd+, (macOS) / Ctrl+, (others). Both toggle the panel.
- **Scope:** curated sliders in five groups plus a palette editor. Values not listed below stay as constants.
- **Apply model:** every change applies live and autosaves. Each group has a Reset button that restores its defaults. There is no Save/Cancel.
- **Palette edits recolor existing notes** whose color equals the old hex value.
- **Palette slot replacement** is picked from a fixed preset grid (no free color input).
- **Data flow:** `App` owns `Settings` and passes `&Settings` explicitly into view and layout functions. No global state.

## Settings Model (`src/settings.rs`)

```rust
pub struct Settings {
    pub bars: BarSettings,
    pub hover: HoverSettings,
    pub notes: NoteSettings,
    pub motion: MotionSettings,
    pub window: WindowSettings,
    pub palette: Vec<NoteColor>, // always 20 entries
}
```

Every struct derives `Serialize`/`Deserialize` with `#[serde(default)]` at struct level, so missing keys, older files and new fields all fall back to defaults. Defaults equal today's constants, so a fresh install looks exactly as it does now.

| Group | Field | Default (current constant) | Range |
|---|---|---|---|
| bars | `width` | 6.0 (`BAR_REST_WIDTH`) | 3–12 |
| bars | `height` | 30.0 (`BAR_REST_HEIGHT`) | 16–60 |
| bars | `gap` | 12.0 (`BAR_GAP`) | 4–24 |
| hover | `magnification` | 4.0 (`MAX_MAG`) | 1–6 |
| hover | `spread` | 60.0 (`SPREAD`) | 20–120 |
| hover | `peek_delay_secs` | 1.0 (`PEEK_DELAY`) | 0.3–3 |
| notes | `size` | 320.0 (`NOTE_SIZE`) | 240–440 |
| notes | `expanded_size` | 560.0 (`NOTE_EXPANDED_SIZE`) | 400–800, and ≥ `size` |
| notes | `paper_tint` | -0.12 (`PAPER_LIGHTEN`) | -0.3–0.3 |
| notes | `idle_control_alpha` | 0.3 (`IDLE_CONTROL_ALPHA`) | 0–1 |
| motion | `speed` | 1.0 | 0.5–2 |
| motion | `stiffness` | 300.0 (`STIFFNESS`) | 150–600 |
| window | `height_fraction` | 0.9 (`HEIGHT_FRACTION`) | 0.5–1.0 |

- `motion.stiffness` drives `AnimationState::new` springs (used by note expand). The magnification spring (`MAGNIFICATION_STIFFNESS`) stays fixed so hover feel does not change.
- `motion.speed` divides `OPEN_SECS`, `CLOSE_SECS`, `PEEK_OPEN_SECS` and `PEEK_CLOSE_SECS`. A speed of 2 makes the animations twice as fast.
- Each range lives next to its field as a `RangeInclusive<f32>` constant. The slider reads the range from there, and `Settings::clamp()` applies it on load.
- `clamp()` also forces `expanded_size >= size` and resets `palette` to the default when it does not have exactly 20 entries.
- `PALETTE` in `note.rs` remains and becomes the default palette.

### Persistence

- The file is `settings.json` in the same directory as `notes.json` (next to the executable).
- `SettingsStore` follows the `NoteStore` pattern: `load(path)`, `save()` via temp file plus rename, `mark_dirty()`, and `save_if_due()` with a 500 ms debounce. It is saved from the existing `SaveTick`.
- A missing file, unreadable file or invalid JSON loads `Settings::default()` and never panics. An invalid file is not overwritten until the user changes a setting.

### Derived Values

- `OPEN_WIDTH` becomes a function of `&Settings`: `STRIP_WIDTH + NOTE_GAP + notes.expanded_size + NOTE_MARGIN`. Its callers (`main.rs` window size, `App::docked_width`) take it from settings.
- On passthrough platforms (macOS, Windows) the window covers the whole monitor, so `window.height_fraction` limits the strip's layout band (vertically centered, `height * fraction`) instead of the window height.
- When `notes.expanded_size` or `window.height_fraction` changes, `App` calls `dock_window()` to resize and reposition the window.

## Threading Settings Through the Code

- `bar_strip.rs`: `compute_layout` and the `BarStrip` widget take `&BarSettings`. They use its width, height and gap in place of the constants.
- `animation.rs`: `MagnificationState` takes magnification and spread as parameters; `AnimationState::new` callers pass `motion.stiffness`. `Morph` open/close durations are scaled by `motion.speed`.
- `app.rs`: note sizes, peek delay and height fraction come from `self.settings`.
- `note_panel.rs`: `PostIt` gains `paper_tint` and `idle_control_alpha` fields.
- `note.rs`: `NoteColor::random()` becomes `NoteColor::random_from(&[NoteColor])`. `NoteStore::add_note` and `seed_templates` take the palette.
- `color_picker.rs`: it takes the palette slice instead of reading `PALETTE`.

## Settings UI

### Gear Slot

- `StripLayout` gains `settings_button: Rectangle` and `settings_hit_area: Rectangle`, laid out directly below the add button with the same gap as between bars.
- It is drawn in the same hollow "empty slot" style as the add button and magnifies along with the bars, so `magnification_centers` includes it. A ⚙ glyph fades in as it magnifies, the same way the "+" does.
- A click publishes `Message::ToggleSettings`. Hit areas take part in scroll bounds and the strip's interactive region in the same way as the add button.

### Panel (`src/settings_panel.rs`)

- **Open and close:** the panel morphs out of the gear rect using the existing `Morph` and `morph_frame`, with the same timing as a note.
- **Position:** it is anchored left of the strip at the gear's height and clamped to the window like a note.
- **Size:** width is 360 px. Height is `min(window_height - 2 * NOTE_MARGIN, 600)`. Content scrolls inside it.
- **Styling:** a dark neutral card (not paper-colored) with the same shadow as notes.
- **Exclusivity:** opening settings closes any open note, and opening a note closes settings.
- **Dismissal:** Esc, a click outside the panel, the gear click, or the shortcut closes the panel.
- **Passthrough:** `App::is_interactive` treats the panel rect as interactive, so passthrough does not swallow clicks on it.

Content, top to bottom:

1. A header with "Settings" and a close (×) button.
2. One section per group (Bars, Hover, Notes, Motion, Window). Each section has a title row with a Reset button, then one row per field: label, formatted value (for example `6 px`, `1.0 s`, `90 %`, `1.5×`) and an `iced::widget::slider` over the field's range. Moving a slider sends `Message::SettingChanged(SettingKey, f32)` and applies immediately.
3. A Palette section with a Reset button:
   - The 20 current swatches are shown in a 5×4 grid. Clicking a swatch selects it with a highlight ring and opens a preset grid below.
   - The preset grid has 60 colors: 12 hues × 5 tints, defined as a constant in `settings.rs`.
   - Clicking a preset replaces the selected slot and recolors every note whose color equals the old hex value. That marks `NoteStore` dirty.
   - Clicking the selected swatch again closes the preset grid.

### Messages

```rust
ToggleSettings,
CloseSettings,
SettingChanged(SettingKey, f32),
ResetGroup(SettingsGroup),
PaletteSlotSelected(Option<usize>),
PaletteColorChosen(NoteColor),
```

`SettingKey` is an enum with one variant per field. `Settings::set(key, value)` clamps and assigns, and `Settings::get(key)` reads.

## Error Handling

- I/O errors on settings save are ignored, as with notes. The next change retries.
- Invalid values from disk are clamped. They never reach layout code unclamped.

## Testing

- `settings.rs` unit tests:
  - The default round-trips through JSON.
  - `{}` and partial JSON load the defaults for missing fields.
  - Out-of-range values clamp.
  - `expanded_size < size` is corrected.
  - A wrong palette length resets the palette.
  - Corrupt JSON loads the defaults.
  - `set`/`get` cover every `SettingKey`.
- `store.rs` test: `recolor(old, new)` changes only notes with the old color.
- `bar_strip.rs` test: `compute_layout` places the gear slot below the add button, and the hit area contains it.
- Verification: `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`, and a manual run checking that every slider updates the app live and persists across restart.

## Out of Scope

- Fonts, ink color, glyph-width estimates, caret margin, corner radii and other constants not listed above.
- Free-form color input (hex or HSL).
- Themes and light/dark switching.
- Import/export of settings.
