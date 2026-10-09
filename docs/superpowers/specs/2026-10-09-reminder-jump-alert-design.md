# Reminder Jump Alert — Design Spec

## Overview

When a note's `@HH:MM` (or similar) reminder fires, the minimized bar alerts until the note is opened. Today that alert is a glowing pulse. Add a **macOS Dock-style jump** as the default alert, keep pulse as an option, and choose the style in Settings → Motion.

Branch: `feat/reminder-jump-alert`.

## Goals and success criteria

- Default alert style is **Jump**. Users can switch to **Pulse** in Settings; the change applies live to any currently alerting bars.
- Jump: the alerting bar hops away from the screen edge (into the screen) until the note is opened.
- Hover on an alerting jumping bar: jump pauses immediately, the bar settles to rest, then sneak peek runs with the existing peek delay. Leaving hover resumes the jump.
- Opening the note clears the alert (same as today for pulse).
- Pulse mode keeps today's glow behavior (including continuing while hovered).
- OS notification still fires on due, unchanged.
- Existing reminder tests still pass with Jump as default (assertions that check `pulsing` / open-to-clear stay valid; pulse-amount checks gate on Pulse style or use jump offsets).

## Decisions

| # | Topic | Decision |
|---|---|---|
| R1 | Setting | `motion.reminder_alert`: `"jump"` \| `"pulse"`, default `"jump"`. Unknown/missing → Jump. Reset Motion restores Jump + speed default. UI: segmented control in Settings → Motion labelled **Reminder alert** (Jump / Pulse), same pattern as Screen edge. |
| R2 | Alert set | Keep `App.pulsing: HashSet<Uuid>` as the set of notes with an uncleared fired reminder. Opening the note removes the id. |
| R3 | Jump motion | Shared phase (`pulse_phase`) drives hops. Displacement along the **away** axis into the screen, up to ~10 px, one smooth hop per ~0.7 s (`sin` half-cycle). Hit-testing stays on the resting bar rect; only drawing is offset. |
| R4 | Hover settle | On cursor enter of an alerting bar while style is Jump: record current jump offset and ease it to 0 over ~0.15 s, then hold at rest while hovered. Peek delay / peek morph unchanged. On leave: clear settle and resume jump from the shared phase. |
| R5 | Pulse | Unchanged glow halo via `bar_pulse` / `pulse_halo`. No hover pause. |
| R6 | Draw path | `BarStrip` gets `jump: Vec<f32>` (away offset per bar). Pulse vec empty/zeros in Jump mode; jump vec empty/zeros in Pulse mode. |
| R7 | Live switch | Changing the setting only switches which draw path runs; the alerting set is unchanged. |

## Architecture

- **`settings.rs`:** `ReminderAlertStyle { Jump, Pulse }` with `as_str` / `parse` / `label`; field on `MotionSettings`; load in `from_json`.
- **`settings_panel.rs`:** Motion section adds the Jump/Pulse segmented row.
- **`app.rs`:** `Message::ReminderAlertChosen`; `bar_jump()`; hover settle state; Tick keeps frames while alerting or settling; pass `jump` into `BarStrip`.
- **`bar_strip.rs`:** `jump_translation` (negate of hide direction); apply jump offset when drawing bar (and stack edges / progress tied to that bar).

## Testing

- Settings: default Jump; parse `"pulse"`; unknown → Jump; reset Motion; JSON round-trip.
- App: fire reminder → jump offsets non-zero (Jump default); with Pulse → pulse > 0 and jump empty/zero.
- Hover settle: after enter, jump for that bar goes to 0; leave → non-zero again.
- Open note clears alerting set (existing test).
- Pure helpers: `jump_amount` / settle lerp unit tests.

## Out of scope

- Configurable jump amplitude/period.
- Stacking jump + pulse.
- Pausing pulse on hover.
- Changing OS notification behavior.
