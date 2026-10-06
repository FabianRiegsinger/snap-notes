# UX Clarity Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: use superpowers:subagent-driven-development to carry out this plan task by task.

**Goal:** Implement spec `docs/superpowers/specs/2026-10-06-ux-clarity-design.md`, decisions G1–G4, L1–L2, F1–F2, U1–U3 and P1.

**Architecture:**
- Pure logic goes first: `store` restore and `reminder` status.
- Then the strip's hint-chip infrastructure, with its cues.
- Then delete with undo, which is built on the chip.
- Then the panel polish, the hotkey error, and the README.

**Tech stack:** Rust 2021, iced 0.14. Tests use `cargo test --bin snap-notes`. Checks are `cargo clippy --all-targets -- -D warnings` and `cargo fmt --check`.

**Global rules:**
- Use TDD: write the RED test first, then the code.
- Match the surrounding style.
- Take every colour from the theme tokens.
- Make one commit per task, ending with the trailer `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.

---

### Task 1: Store restore and reminder status (pure)

**Files:** `src/store.rs`, `src/reminder.rs`.

**Interfaces:**
- `pub struct Deleted { pub note: Note, pub index: usize, pub promoted: Option<Uuid> }`. It derives `Clone` and `Debug`.
- `Store::delete_note(&mut self, id) -> Option<Deleted>`. Callers that ignore the result add `let _ =`, or simply drop it.
- `Store::restore(&mut self, deleted: Deleted)`, following U3. It renumbers and bumps the revision, but does not mark the store dirty, because the caller saves.
- `reminder::candidate(title: &str) -> bool`, following L2.
- `pub enum Status { None, Pending(String), Fired(String), Invalid }` and `reminder::status(note: &Note, now: DateTime<Local>) -> Status`.
  - The label comes from the existing label helper.
  - The result is `Fired` when `reminder_fired` equals the resolved due time.
  - The result is `Invalid` when there is a candidate but no parse.

**Tests:**
- `delete_restore_round_trips_plain_note`
- `delete_restore_round_trips_stack_top`, which checks that the promoted member and the other members are re-attached
- `delete_restore_round_trips_member`
- `delete_restore_round_trips_pinned`
- `restore_after_add_keeps_invariants`
- `candidate_detects_tags_not_emails`
- `status_pending_fired_invalid_none`

**Commit:** `feat: restorable deletes and reminder status`

### Task 2: Hint chips, stack cue, slot tooltips, empty-clipboard shake

**Files:** `src/bar_strip.rs`, `src/peek.rs` (to reuse its paper style), `src/app.rs`.

**Steps:**
1. **G1.** Add `fn draw_chip(renderer, theme, anchor: Rectangle /*strip-side*/, y: f32, text: &str, alpha: f32) -> Rectangle`, which returns the chip rect.
   - Place it the way `peek_layout` places the peek beside the strip, respecting the strip side.
   - Measure the text with the same `Paragraph` approach `peek.rs` uses.
   - Fade it in over 120 ms. Keep a small per-chip alpha in `StripState`, or compute it from the timestamps.
2. **G2.** While dragging, when `stack_target` is `Some(t)`, draw a "Stack" chip centred on bar `t`.
3. **G3.**
   - `StripState` tracks `slot_hover: Option<(Slot, Instant)>`, where `Slot` is `Add` or `Search`.
   - After 600 ms the label is drawn: on macOS "New note · Option-click: from clipboard", elsewhere "New note · Alt-click: from clipboard"; on macOS "Search (Cmd+F)", elsewhere "Search (Ctrl+F)".
   - The widget must request a redraw at the 600 ms deadline, via `shell.request_redraw_at` or the app's animation tick. Read how the widget redraws today.
   - The chip hides on leave or on any press. It is not drawn while the peek is visible.
   - Pure helper: `fn slot_tooltip(hover: Option<(Slot, Instant)>, now: Instant, peek_visible: bool) -> Option<&'static str>`.
4. **F2.**
   - `App.clipboard_empty_at: Option<Instant>`, set in the empty branch of `ClipboardNote`.
   - Expiry: `ClipboardEmptyExpired(Instant)` after 1.5 s, using the existing `delayed` helper.
   - `BarStrip` gets `add_shake: f32`, the x offset computed by the app from `clipboard_empty_at`: `3.0 * sin(t * 2π * 10) * (1 - t/0.3)` for t < 0.3 s, otherwise 0. It also gets `clipboard_hint: bool` to draw the chip "Clipboard is empty" beside `+`.
   - The app keeps animating while t < 0.3 s.

**Tests:**
- `stack_target_shows_stack_chip` (pure: the hint for drag state)
- `slot_tooltip_after_delay_and_hidden_with_peek`
- `add_shake_settles`
- `empty_clipboard_sets_hint_and_expires`

**Commit:** `feat: hint chips for stacking, slots and an empty clipboard`

### Task 3: Delete with undo

**Files:** `src/app.rs`, `src/peek.rs`, `src/bar_strip.rs`, `src/note_panel.rs`.

**Steps:**
1. **U1.** Remove the confirm flow:
   - remove `confirm_delete`, `peek_confirm_delete`, `Message::ConfirmDelete`, `PeekDeleteRequested`, `PeekDeleteConfirmed` and `PeekDeleteCancelled`, along with the peek's confirmation layout, rects and draw code;
   - remove the header's confirm UI.
2. Add the new messages:
   - `DeleteNote(Uuid)` comes from the header trash. It sets `pending_delete = Some(id)` and closes the morph, the same path the old confirmed delete took.
   - `PeekDelete(usize)` comes from the peek trash. It hides the peek, then: if that note is the active one, it follows the `DeleteNote` path; otherwise it calls `start_collapse(id)`.
3. **U3.** `finish_collapse` keeps the `Deleted` from `store.delete_note` as `last_deleted = Some((deleted, now))` and saves, as it does today. It schedules `UndoExpired(at)` after 5 s.
4. **U2.**
   - `UndoDelete`: `store.restore`, then `mark_dirty`, `save`, `did_save`, `last_deleted = None`, and `sync_entries` (`update` does that).
   - `UndoExpired(at)` clears the toast if it is still the same one.
   - Cmd/Ctrl+Z with no active note sends `UndoDelete`. Read the key handler; editor undo with a note open must stay unchanged.
   - `BarStrip` gets `toast: Option<f32>` (alpha). It draws the chip "Note deleted · Undo" aligned with the `+` slot, with "Undo" in the link or accent ink. The whole chip is the hit area for `UndoDelete`.
   - Include the toast rect in the interactive region used by `update_passthrough` and the strip-shrinking logic. Read `update_passthrough` and the window-size code. Ensure a click on the toast isn't passed through.
5. Rewrite the old delete tests to use the new messages, so their behaviour coverage is kept.

**Tests:**
- `header_trash_deletes_without_confirm`
- `peek_trash_deletes_at_once`
- `undo_restores_deleted_note_and_saves`
- `undo_restores_stack_top_with_members`
- `cmd_z_without_open_note_undoes_delete`
- `cmd_z_with_open_note_is_editor_undo`
- `undo_toast_expires`
- `second_delete_replaces_toast`
- `toast_click_sends_undo` (bar_strip)
- `toast_area_is_interactive` (passthrough helper)

**Commit:** `feat: delete at once, undo for five seconds`

### Task 4: Reminder label, title placeholder, pin look

**Files:** `src/note_panel.rs`, `src/bar_strip.rs` (notch), `src/app.rs` (pass the status).

**Steps:**
1. **G4.** The title placeholder becomes `"Title — @15:00 adds a reminder"`.
2. **L1/L2.**
   - `PostIt` gets `reminder: reminder::Status`, computed by the app from the active note and `Local::now()`. It is recomputed on each view, which is cheap for a single note.
   - Replace the header bell and its tooltip with a label right of the title field:
     - `Pending`: bell plus the label, at 0.65 ink.
     - `Fired`: bell plus the label plus " · fired", at 0.65 ink.
     - `Invalid`: "not a reminder", at 0.45 ink.
     - `None`: nothing.
   - Keep the peek's bell as it is.
3. **P1.**
   - Change `header_button` to take an `active: bool`. When active it has a persistent wash `theme.ink(0.10 * a)` and full ink.
   - `pin_button` uses it, with the tooltip "Pin to top" or "Unpin". Remove the duplicated style closure.
   - The bar notch becomes 3 px.

**Tests:**
- `title_placeholder_mentions_reminders`
- `reminder_label_text_for_each_status` (pure helper)
- `active_header_button_has_wash` (style helper)
- `pinned_notch_is_three_px`

**Commit:** `feat: live reminder label and a clearer pin`

### Task 5: Hotkey error in Settings

**Files:** `src/hotkey.rs`, `src/app.rs`, `src/settings_panel.rs`.

**Steps:**
1. **F1.**
   - `hotkey::register() -> Result<Manager, String>`. Keep the stderr logging.
   - `App.hotkey_error: Option<String>`. `sync_hotkey` sets it from the `Err`, and clears it on `Ok` or when the hotkey is turned off.
   - Factor out a pure `fn apply_registration(&mut self, result: Result<Manager, String>)`, or a helper that tests can call with an `Err`.
   - The settings panel's App group, under the hotkey toggle (macOS/Windows), shows `"Couldn't register: {reason}"` at 0.6 ink, or in a danger tone if the theme has one.
   - Pass it through the settings view's arguments.

**Tests:**
- `hotkey_error_set_and_cleared`
- a settings-panel view helper test, if a pure label helper exists

**Commit:** `feat: show why the global hotkey failed`

### Task 6: README

Update README.md:
- delete: no confirmation; undo for 5 s with the toast or Cmd/Ctrl+Z when no note is open;
- the `+` and search tooltips;
- the reminder label in the header ("not a reminder" for typos);
- the hotkey error line in Settings → App;
- the Stack chip while dragging.

**Commit:** `docs: undo, hints and reminder label in the README`
