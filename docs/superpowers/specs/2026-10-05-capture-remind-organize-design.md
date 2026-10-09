# Capture, Remind, Organize — Design Spec

## Overview

This spec covers six features, approved by the user in chat. Recommended choices were taken throughout.

- Global hotkey for a new note.
- Dropping files on the strip.
- A note from the clipboard.
- Reminders.
- Checklist progress on bars.
- Pinning and stacking notes.

All six are built on the branch `feat/capture-remind-organize`.

## Decisions

| # | Topic | Decision |
|---|---|---|
| H1 | Global hotkey | Cmd+Shift+Space on macOS, Ctrl+Shift+Space elsewhere, via the `global-hotkey` crate (same project as `tray-icon`). It shows the notes if hidden, creates a note and opens it in edit mode with the body focused. If registration fails, the error is logged to stderr and nothing else changes. A toggle "Global hotkey (Cmd/Ctrl+Shift+Space)" in the App group defaults to on. It persists as `app.global_hotkey` and re-registers live. The App group is macOS/Windows only, so on Linux the hotkey is always on. |
| D1 | Drop on strip | File drops only (winit delivers paths, never text or links). Dropping on `+` or on empty strip space creates a note. Dropping on a bar appends to that note. `.txt`/`.md` (≤ 1 MB, valid UTF-8): the file text is the body, and the file stem is the title for new notes. Images (the existing types): an image reference via `images::import`. Anything else: the file path as a line. Drops on an open note keep today's behavior (image insert). |
| C1 | Clipboard note | Tray item "New Note from Clipboard" (id `clip`), after "New Note". Also Alt/Option-click on `+`. Text becomes the body; an image becomes an image note via `images::import_png`. An empty clipboard does nothing. |
| R1 | Reminder syntax | In the title: `@HH:MM` (today, or tomorrow if already past), `@tomorrow` (09:00), `@tomorrow HH:MM`, `@mon`…`@sun` with an optional `HH:MM` (next such day, 09:00 default), `@YYYY-MM-DD` with an optional `HH:MM`. Local time. Only the first `@…` match counts. An invalid tag is no reminder. The tag stays in the title as typed. |
| R2 | Firing | A note stores `reminder_fired: Option<DateTime<Utc>>`, the due time it last fired for. A reminder is due when its time ≤ now and it hasn't fired for that time. On firing: a system notification (`notify-rust`, title "Snap Notes", body = note title without the tag), the bar pulses, and `reminder_fired` is set and saved. The pulse stops when the note is opened. Missed reminders fire at startup. While any reminder is pending, a 30 s subscription checks; with none pending there is no timer. Editing the tag to a new time re-arms it. |
| R3 | Reminder UI | A Lucide `bell` icon in the open note's header (left of the title text area) and in the peek header, shown while a reminder is set. The header tooltip and peek show the due time (`Tue 15:00`). |
| P1 | Checklist progress | `rich::task_progress(content) -> Option<(done, total)>` counts real task items (via the parser). A bar with tasks fills from the bottom: the done/total share is drawn in the bar color at full alpha over the bar at 45 %. All done (done == total > 0): the fill covers the whole bar (fully tinted). The peek header shows `done/total`. |
| N1 | Pin | `Note.pinned: bool` (serde default false). A Lucide `pin` button in the header toggles it. Pinned notes stay first in strip order: pinning moves the note to the end of the pinned group, unpinning to the start of the unpinned group. Drag-reordering is clamped within the note's group. A pinned bar shows a 2 px notch (a darker cap) at its top. |
| N2 | Stack | `Note.stack: Option<Uuid>`, the id of the stack's top note; the top itself has `None`. The strip shows one bar per top-level note; stacked notes are hidden from the strip. Dropping a dragged bar onto the middle 50 % of another bar stacks it under that bar's note. Dropping between bars reorders, as today. A stack bar is drawn with two thin offset "card edge" lines to its left. Its peek lists the titles of the top note and every member, in order; clicking a title opens that note. An "Unstack" button in the stack's peek clears `stack` on all members, which reappear right after the top in strip order. Deleting the top note promotes the first member to top. Opening a stacked note opens it normally; its bar is the stack's bar. Search, export, copy and reminders treat stacked notes individually. A stacked note's due reminder pulses the stack's bar. A pinned top pins the whole stack. |
| I1 | Icons | Lucide `pin`, `bell` and `layers` are added to the subset. |

## Architecture

- **`src/strip_model.rs` (new, pure).** `pub struct Entry { pub top: usize, pub members: Vec<usize> }` holds indices into `store.notes()`. `pub fn entries(notes: &[Note]) -> Vec<Entry>` returns one entry per top-level note, in strip order. Members are listed in store order, and a member whose `stack` points at a missing note is treated as top-level. All bar-index code (`BarStrip`, `App` lookups, magnification, peek, collapse, dimmed, open bar, drag) works in **entry** indices and maps to note indices through `entries`. This is the core refactor; behavior with no stacks is unchanged.
- **`src/reminder.rs` (new, pure).** `pub fn parse(title: &str, now: DateTime<Local>) -> Option<DateTime<Local>>`. `pub fn due(note: &Note, now) -> Option<DateTime<Utc>>` returns the due time if a reminder is set and has not fired for that time. `pub fn display(title) -> String` returns the title without the tag.
- **`src/hotkey.rs` (new).** Registration and the subscription on `GlobalHotKeyEvent::receiver()`, following `tray.rs`'s event-stream pattern. It is cfg-free: `global-hotkey` supports all three platforms.
- **`src/notify.rs` (new).** `pub fn reminder(title: &str)` is fire-and-forget through `notify-rust`. Errors are logged.
- **`note.rs`.** New fields `pinned`, `stack` and `reminder_fired`, all serde-defaulted so old files load.
- **`store.rs`.** `set_pinned(id, bool)`, `stack(id, onto)`, `unstack(top)`, and a `delete_note` that promotes the first member. Each bumps the revision.
- **`bar_strip.rs`.**
  - Draws entries, the progress fill, the pinned notch, the stack edges and the reminder pulse (`pulse: f32` per entry from the app).
  - The drop target for stacking: `DragEnd` lands in the middle 50 % of another bar and becomes `StackOnto(entry)`.
  - File drops on `+`, empty space or a bar.
- **`peek.rs`.** Stack list, Unstack button, bell and progress text in the header.
- **`app.rs`.**
  - Messages: `HotkeyPressed`, `ClipboardNote`, `StripFileDropped(Option<usize>, PathBuf)`, `TogglePin`, `StackOnto { dragged, target }`, `Unstack(usize)`, `OpenStackMember(Uuid)`, `ReminderTick`.
  - Settings plumbing for `global_hotkey`.

## Error handling

- Hotkey registration failure: logged, and the app works without it.
- Notification failure: logged; the bar still pulses.
- A file that can't be read or is too big: logged, no note created.
- An empty clipboard: no-op.
- Malformed reminders: no reminder.

## Testing

- **`strip_model`:** entries with no stacks equal one per note; a stack groups its members; orphan members are treated as top-level; order is kept.
- **`reminder`:**
  - every syntax form against a fixed `now`;
  - a past `@HH:MM` rolls to tomorrow;
  - weekday rollover;
  - invalid tags;
  - the fired time suppresses a refire;
  - editing re-arms.
- **`rich::task_progress`:** counts, code blocks ignored, none.
- **`store`:**
  - pin ordering and the clamped reorder;
  - stack and unstack;
  - deleting the top promotes a member;
  - serde default for old JSON.
- **`app`:**
  - the hotkey message creates and opens a note in edit mode;
  - a clipboard note from text and from an image;
  - strip drops (`.txt` creates, a bar appends, an image, another file type);
  - `TogglePin`;
  - `StackOnto` and `Unstack`;
  - opening a stack member;
  - `ReminderTick` fires once and sets `reminder_fired`;
  - a missed reminder fires at startup;
  - the pulse clears on open.
- **`bar_strip`:** the progress fill height, the stack drop zone hit test, and the entry-indexed hit tests.
- **Manual:**
  - the hotkey from another app;
  - dropping a file from Finder;
  - a reminder notification;
  - stacking by drag;
  - pins.

## README

Document all six features, the hotkey in the shortcuts table, and the new tray item.

## Out of scope

- Text and link drag-and-drop from other apps (winit can't deliver it).
- A custom hotkey combination.
- Recurring reminders.
- Snooze.
- Nested stacks.
