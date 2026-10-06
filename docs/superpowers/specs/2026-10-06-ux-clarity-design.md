# UX Clarity — Design Spec

## Overview

This spec covers five improvements from the senior-design review, all approved by the user ("implement 1-5"). The recommended choices were taken throughout.

1. Visible cues for hidden gestures.
2. Live reminder feedback.
3. No silent failures.
4. Undo instead of a delete confirmation.
5. A clear pinned state.

All five are built on the branch `feat/ux-clarity`.

## Decisions

| # | Topic | Decision |
|---|---|---|
| G1 | Hint chip | A shared drawing helper draws a small pill beside the strip, in the spot where the peek appears, vertically centred on a given y. It uses the peek's paper colour, its border and shadow tokens, a 12 px label at 0.75 ink, and 8/4 px padding. It is drawn by `bar_strip`, never at the same time as the peek, and it fades in and out over 120 ms. |
| G2 | Stack cue | While a dragged bar is over another bar's middle (`stack_target` is `Some`), a chip reading "Stack" is shown beside that bar, together with the existing ring. |
| G3 | `+` / 🔍 tooltips | After hovering the `+` slot for 600 ms, a chip appears. On macOS it reads "New note · Option-click: from clipboard"; elsewhere it reads "New note · Alt-click: from clipboard". After hovering the search slot for 600 ms, a chip reads "Search (Cmd+F)" on macOS and "Search (Ctrl+F)" elsewhere. A chip hides as soon as the cursor leaves the slot or the user presses anything. While the peek is showing, no chip is shown. |
| G4 | Title placeholder | The title placeholder changes from "Title" to "Title — @15:00 adds a reminder". |
| L1 | Live reminder | In the open note's header, to the right of the title field, a status label appears while the title holds a reminder tag. If the tag parses, the label shows the bell and the due time (`reminder::label`, e.g. "Fri 09:00"), with "fired" appended once it has fired, at 0.65 ink. The label updates on every title keystroke. |
| L2 | Unrecognized tag | A tag *candidate* is an `@` at the start of the title or after whitespace, followed by a letter or digit. If the title has a candidate that doesn't parse, the label shows "not a reminder" at 0.45 ink and no bell. The header bell from R3 is replaced by this label, so there is one place for reminder state, and the tooltip is dropped. |
| F1 | Hotkey failure | `App.hotkey_error: Option<String>` is set when `register()` fails and cleared when it succeeds or is turned off. `hotkey::register` returns `Result<Manager, String>`. In Settings → App, under the hotkey toggle (macOS/Windows), a line in the danger/ink-0.6 tone reads "Couldn't register: <reason>". The Linux reason "needs an X11 display" is logged only, because Linux has no App group. |
| F2 | Empty clipboard | When `ClipboardNote` finds nothing, the `+` slot shakes (±3 px horizontally, 3 cycles, 300 ms) and a chip "Clipboard is empty" shows beside `+` for 1.5 s. This works from the tray and from Alt-click; from the tray, the notes are already shown. |
| U1 | Delete without confirm | Both the header trash and the peek trash delete at once: there is no confirm step. The header trash folds the note and then collapses its bar, as a confirmed delete does today. The peek trash collapses the bar. `confirm_delete`, `peek_confirm_delete`, `ConfirmDelete`, `PeekDeleteRequested`, `PeekDeleteConfirmed` and `PeekDeleteCancelled`, together with the peek's confirmation layout, are removed. They are replaced by `DeleteNote(Uuid)` from the header and `PeekDelete(usize)` from the peek. |
| U2 | Undo toast | After a delete, a toast chip "Note deleted · Undo" shows near the top of the strip, aligned with `+`, for 5 s. Clicking "Undo", or pressing Cmd/Ctrl+Z while no note is open, restores the note. Only the latest delete can be undone, and a new delete replaces the toast. The toast area is part of the window's interactive region, so passthrough must not swallow the click. |
| U3 | Restore | `Store::delete_note` returns `Option<Deleted { note: Note, index: usize, promoted: Option<Uuid> }>`. `Store::restore(Deleted)` re-inserts the note at `index`, clamped. If `promoted` is set, it re-attaches that note and all notes whose `stack` was moved to it back under the restored top, then renumbers. Restore keeps the store invariants (pinned first, members contiguous), saves immediately, and opens nothing. Images are safe, because the image sweep only runs at startup. |
| P1 | Pinned look | A pinned pin button gets a persistent wash (`theme.ink(0.10)`, the hover wash level) and full ink; unpinned stays at 0.65 ink with no wash. Its tooltip reads "Pin to top" or "Unpin". `pin_button` reuses `header_button` via an `active: bool` parameter instead of copying its style. The pinned notch on a bar grows from 2 px to 3 px. |

## Architecture

- **`bar_strip.rs`:** chip drawing (G1), hint state passed in from the app (`hint: Option<Hint { text, y, alpha }>`), the stack cue (G2), hover timing for the slot tooltips (G3, tracked in `StripState` with an `Instant`), the shake offset for `+` (F2), and the toast with its hit area (U2). Removing the peek's confirmation removes its rects.
- **`peek.rs`:** the confirmation layout is removed, and the trash maps to `PeekDelete`.
- **`reminder.rs`:** `pub fn candidate(title) -> bool` (L2), and `pub enum Status { None, Due(label), Fired(label), Invalid }` with `status(note, now)`.
- **`note_panel.rs`:** the title placeholder (G4), the reminder status label (L1/L2), and the pin button (P1).
- **`settings_panel.rs`:** the hotkey error line (F1).
- **`store.rs`:** `Deleted` and `restore` (U3).
- **`app.rs`:**
  - New messages: `DeleteNote`, `PeekDelete`, `UndoDelete`, `UndoExpired(Instant)`, `ClipboardEmptyExpired(Instant)`.
  - New state: `last_deleted: Option<(Deleted, Instant)>`, `hotkey_error`, `clipboard_empty_at: Option<Instant>`.
  - Cmd/Ctrl+Z with no open note triggers undo.
  - The passthrough region includes the toast.

## Error handling

- Hotkey: F1.
- Clipboard: F2.
- Restore of a stale `Deleted`, for example after Quit: not possible, because the state is in memory only.
- Undo after the toast expires: a no-op.

## Testing

- **`reminder`:** `candidate` and `status` for valid, invalid, fired and email-like titles (`me@x.com` is not a candidate).
- **`store`:**
  - delete then restore round-trips the notes vector exactly for a plain note, a stack top with 2 members, a member, and a pinned note;
  - restore after another note was added in between still keeps the invariants.
- **`app`:**
  - the header trash deletes without confirm and folds first;
  - the peek trash deletes at once;
  - `UndoDelete` restores the note, saves and clears the toast;
  - Cmd/Ctrl+Z with no note open undoes, and with a note open it is editor undo;
  - the toast expires after 5 s;
  - a second delete replaces the first;
  - `ClipboardNote` with an empty clipboard sets `clipboard_empty_at`;
  - `hotkey_error` is set and cleared (through an injectable register result or a pure helper).
- **`bar_strip`:**
  - the chip appears for the stack target;
  - the slot tooltip appears after 600 ms and hides on leave;
  - the toast's Undo hit area returns `UndoDelete`;
  - the shake offset is zero after 300 ms.
- **`note_panel` / `peek`:** the confirmation layout is gone; the pin button has a wash when active.
- **Manual:** all chips in both themes, the toast click through passthrough, and the shake.

## README

Update the delete text (no confirmation, undo for 5 s, and Cmd/Ctrl+Z), the Alt/Option-click and search tooltips, the reminder label in the header, and the hotkey error line.

## Out of scope

- Multi-level undo of deletes.
- Undo for stack, unstack or pin.
- Toasts for other actions.
- Keyboard navigation.
