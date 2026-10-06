# Auto-hide — Design Spec

## Overview

This adds a third visibility mode next to the permanent show/hide that exists today. When it is on, the note strip hides itself while it is not in use. Holding the cursor against the strip's segment of the right screen edge slides it back in, the way the macOS Dock behaves.

The user approved approach A in chat, with these choices:
- trigger zone: the strip's band;
- timing: Dock-like;
- reminders: slide in, and stay while pulsing.

Branch: `feat/auto-hide`, based on `feat/ux-clarity`.

## Goals and success criteria

- With auto-hide on and nothing in use, the strip is off-screen, and clicks anywhere except the edge zone pass through to other apps.
- Resting the cursor at the right edge, within the strip's band, for 150 ms slides the strip in within 200 ms.
- Moving away hides it again 800 ms later, unless something is in use.
- A fired reminder is never missed, because the strip slides in and stays.
- With auto-hide off, behaviour is exactly as today.

## Decisions

| # | Topic | Decision |
|---|---|---|
| A1 | Setting | Toggle **"Auto-hide"** in Settings → Window, available on all platforms. Default off. Persisted as `window.auto_hide`. Turning it off reveals the strip at once. |
| A2 | States | `Shown → Hiding → Hidden → Revealing → Shown`. Hiding and Revealing are 200 ms slides; the eased offset uses the existing motion speed setting. A reveal that starts mid-hide reverses from the current offset, and the same applies the other way round. |
| A3 | Offset | The strip, peek, chips and toast are drawn shifted right by `offset × (STRIP_WIDTH + margin)`, where `offset` runs from 0 (shown) to 1 (hidden). When `offset` is 1, nothing of the strip is visible. |
| A4 | Edge zone | The rightmost 2 px of the window, vertically within the strip's band: from the top of the first bar (or the add slot when there are no bars) minus the bar gap, to `hit_bottom()`. This matches the band `is_interactive` already uses for the strip. |
| A5 | Reveal | The cursor stays inside the edge zone for 150 ms; leaving the zone resets the dwell. While the window is Hidden, it is interactive only in the edge zone. |
| A6 | Hide | The strip hides 800 ms after the cursor was last over the strip column (full band), the peek or the toast. The grace timer resets on every re-entry. |
| A7 | Blockers | Hiding never starts, and a running hide reverses to Shown, while any of the following holds: a note is open or folding; search, settings or export is open; a bar drag or file hover is in progress; the undo toast or the clipboard hint is showing; any note is pulsing; the export save dialog is open. |
| A8 | Reminders | A reminder that fires reveals the strip at once, with no dwell. The strip stays shown until no note is pulsing (A7), then the normal grace period applies. |
| A9 | Other reveals | Any action that shows a note or panel reveals the strip first, with no dwell: the global hotkey, the tray items New Note, New Note from Clipboard, Search… and Settings…, and the keyboard shortcuts. The note or panel that opens then blocks hiding (A7). |
| A10 | Tray Show/Hide | **Hide** works as today: the strip is fully hidden, with no edge zone and no reveal. **Show** returns to Shown, and auto-hide applies again after the grace period. Auto-hide is a separate layer under `visible`. |
| A11 | Linux (no passthrough) | While Hidden, the docked window shrinks to 2 px wide. It keeps its height and position, so it covers the band. A `CursorEntered` event, or `CursorMoved` inside the window, counts as being in the edge zone. The 150 ms dwell is measured from the entry, and it is cancelled by `CursorLeft`. When a reveal starts, the window returns to its normal docked width first, then the slide plays. When a hide finishes, the window shrinks. |
| A12 | Startup | With auto-hide on, the app starts Hidden, without playing a slide. |
| A13 | Focus | Revealing never steals keyboard focus. Only the actions in A9 focus the window, as they do today. |

## Architecture

- **`src/autohide.rs` (new, pure, unit-tested).**
  - `pub enum Phase { Shown, Hiding, Hidden, Revealing }`.
  - `pub struct AutoHide { phase, offset: f32, edge_since: Option<Instant>, idle_since: Option<Instant> }`.
  - `pub struct Inputs { enabled: bool, in_edge: bool, in_use_area: bool, blocked: bool, force_reveal: bool }`.
  - `pub fn step(&mut self, inputs: Inputs, now: Instant, dt: f32) -> bool` returns whether it is still animating. It encodes A2, A5–A8 and A12.
  - `pub fn offset(&self) -> f32`, `pub fn is_hidden(&self) -> bool` (offset 1 and phase Hidden), and `pub fn wants_frames(&self) -> bool` for slides and pending timers.
  - The constants REVEAL_DWELL (150 ms), HIDE_GRACE (800 ms) and SLIDE_SECS (0.2) live here.
- **`settings.rs` / `settings_panel.rs`:** `WindowSettings.auto_hide: bool` (serde default false) and the toggle in the Window group.
- **`app.rs`:**
  - New field `auto_hide: AutoHide`.
  - A helper `fn auto_hide_inputs(&self, cursor) -> Inputs` computes the blockers from A7.
  - The step runs on Tick and on CursorPolled/CursorMoved, and a timer subscription covers the pending dwell and grace deadlines.
  - `is_interactive` becomes edge-zone-only while hidden.
  - `view` passes `x_offset` to the strip and to its overlays.
  - Linux: `docked_width` and `needs_wide_window` take the hidden state into account (A11).
  - Reveal hooks: the reminder fire path and the A9 actions set `force_reveal`.
- **`bar_strip.rs`:** an `x_offset: f32` applied to all drawing. Hit-testing is off while the offset is above 0, so a sliding strip ignores clicks.
- **`platform.rs`:** no change. Cursor polling already runs while passthrough is on.

## Error handling

- When the cursor position can't be read (`cursor_in_window` returns `None`), it counts as not in the edge zone and not in the use area. The strip then stays hidden, or hides after the grace period. This is the same as today's handling.
- A monitor or size change while Hidden recomputes the band. On Linux the 2 px window is re-docked.

## Testing

- **`autohide`:**
  - the dwell reveal (149 ms: no reveal; 150 ms: Revealing);
  - leaving resets the dwell;
  - the grace hide;
  - re-entry resets the grace;
  - every blocker prevents a hide and reverses a running one;
  - `force_reveal` skips the dwell;
  - a pulse keeps the strip shown;
  - disabled means always Shown;
  - startup Hidden without a slide;
  - the offset is monotonic during slides;
  - a mid-slide reversal.
- **`app`:**
  - the setting round-trips, and its default is off;
  - with auto-hide on and an idle app, the strip ends Hidden and `is_interactive` is true only in the edge zone;
  - a firing reminder reveals it;
  - `HotkeyPressed` reveals it, and the opened note blocks hiding;
  - tray Hide overrides auto-hide, and Show returns to it;
  - turning the setting off reveals at once;
  - Linux: the window width while hidden is 2 px (pure helper with the passthrough flag).
- **`bar_strip`:** with `x_offset` at 1, nothing is hit and the drawn rects are off the right edge.
- **Manual (macOS):**
  - reveal and hide feel;
  - clicks pass through while hidden;
  - a reminder reveal;
  - the hotkey reveal.

## README

- Settings → Window: the Auto-hide toggle.
- How to reveal the strip: hold the cursor at the screen edge where the strip sits.
- When the strip stays shown: while you use a note or panel, and while a reminder pulses.

## Out of scope

- Configurable delays.
- Edges other than the right.
- Hot corners.
- Multi-monitor edge detection beyond the monitor the strip is docked on.
