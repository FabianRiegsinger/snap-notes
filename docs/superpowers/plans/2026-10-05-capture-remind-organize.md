# Capture, Remind, Organize Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Global hotkey, file drops on the strip, a note from the clipboard, reminders, checklist progress on bars, and pinned and stacked notes.

**Architecture:**
- A new pure `strip_model` maps strip entries (bars) to note indices, so stacks can hide members. Every bar-index path moves to entry indices.
- Pure `reminder` parsing and due logic.
- Thin `hotkey` and `notify` wrappers over `global-hotkey` and `notify-rust`.
- Everything else extends the existing store, strip, peek, note header and tray.

**Tech Stack:** Rust 2021, iced 0.14, global-hotkey, notify-rust, chrono, existing arboard/rfd/images.

**Spec:** `docs/superpowers/specs/2026-10-05-capture-remind-organize-design.md`

## Global Constraints

- Spec decisions H1, D1, C1, R1–R3, P1, N1, N2 and I1 are binding. Exact strings:
  - Toggle label: `"Global hotkey (Cmd/Ctrl+Shift+Space)"`.
  - Tray item: `"New Note from Clipboard"`, id `clip`, placed after "New Note".
  - Peek button: `"Unstack"`.
  - Notification title: `"Snap Notes"`.
- New note fields `pinned`, `stack` and `reminder_fired` are serde-defaulted. Old `notes.json` files must load unchanged.
- With no stacks and no pins, strip behavior and every existing test stay as they are.
- UI uses theme tokens and colors, Lucide icons and the pressed style.
- CI stays green: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test --bin snap-notes`.
- Comments and naming follow the surrounding code; no section banners.

## Review Focus

1. **Entry and note index confusion after the stack refactor.** A wrong mapping opens, deletes or recolors the wrong note.
   - Test (Task 2): `stacked_strip_opens_the_right_note`. Stack notes 1 under 0, click entry 1, and the opened id is note 2's.
2. **Reminders.**
   - A reminder must never fire twice across a restart. Test (Task 4): `fired_reminder_does_not_refire_after_reload`, which saves, reloads, then ticks.
   - A title edited during the 30 s window. Test: `editing_reminder_time_rearms`.
3. **Deleting a stack's top note or a stack member** must not orphan or lose notes. Test (Task 6): `deleting_stack_top_promotes_member`.
4. **Drag that ends ambiguously** (the edge between "reorder" and "stack"). The middle 50 % stacks, anything else reorders, never both. Test (Task 6): `drop_zone_edges`.
5. **Hotkey while notes are hidden or settings are open.** It shows the notes, closes panels and opens the new note. Test (Task 7): `hotkey_shows_hidden_notes_and_opens_new_note`.

---

### Task 1: Data model, icons, and checklist progress

**Files:** `src/note.rs`, `src/store.rs`, `src/rich.rs`, `src/icons.rs`, `assets/fonts/subset.sh`, `assets/fonts/lucide.subset.ttf`

**Interfaces:**
- **Note fields:** `pub pinned: bool`, `pub stack: Option<Uuid>`, `pub reminder_fired: Option<DateTime<Utc>>`, all `#[serde(default)]`.
- **Store methods:**
  - `set_pinned(id, pinned)`, keeping pinned notes first (N1).
  - `reorder` clamped within the pin group.
  - `stack(id, onto_top)`.
  - `unstack(top)`.
  - `delete_note` promotes the first member when the top is deleted.
  - Every mutation bumps the revision.
- **Icons:** `Icon::Pin`, `Icon::Bell`, `Icon::Layers`.
- **Progress:** `rich::task_progress(content: &str) -> Option<(usize, usize)>`.

- [ ] **Step 1: Write the tests.**
  - `old_notes_json_loads_with_defaults`
  - `pinning_keeps_pinned_first`
  - `reorder_stays_within_pin_group`
  - `stack_and_unstack`
  - `deleting_stack_top_promotes_member`
  - `task_progress_counts_real_tasks` (tasks inside a fenced code block don't count, and a note with no tasks gives `None`)
  - icon tests extended (`ALL` has 21 entries)
- [ ] **Step 2: Run them.** Expected: RED.
- [ ] **Step 3: Implement.** Re-run `subset.sh` with `pin bell layers` added to the icon list.
- [ ] **Step 4: Verify.** Full suite, clippy and fmt are green.
- [ ] **Step 5: Commit** as `feat: note pins, stacks, progress and new icons`.

### Task 2: Strip model and entry-indexed strip

**Files:** create `src/strip_model.rs`; modify `src/bar_strip.rs`, `src/app.rs`, `src/peek.rs`

**Interfaces:**
- `pub struct Entry { pub top: usize, pub members: Vec<usize> }` and `pub fn entries(notes: &[Note]) -> Vec<Entry>`.
- `BarStrip` gains `entries: &'a [Entry]` and draws one bar per entry.
- Every bar-index message (`BarClicked`, `DragStart`, `DragMove`, `DragEnd`, peek and collapse indices, the open bar, dimmed) carries an entry index.
- `App` maps an entry index to a note through helpers `fn entry_note(&self, entry) -> Option<usize>` and `fn note_entry(&self, note_idx) -> Option<usize>`. Cache the entries keyed by store revision, like `sync_search`.
- Reorder by drag moves whole stacks: the top note plus its members stay contiguous in store order.

- [ ] **Step 1: Write the tests.**
  - `strip_model`: `entries_without_stacks_are_one_per_note`, `stack_groups_members`, `orphan_member_is_top_level`.
  - App: `stacked_strip_opens_the_right_note` and `reorder_moves_whole_stack`.
  - Every existing strip and app test must pass unchanged.
- [ ] **Step 2: Run them.** Expected: RED.
- [ ] **Step 3: Implement.** Keep the change mechanical: introduce entries, then convert each bar-index use. Members are hidden; the stack bar uses the top note's color.
- [ ] **Step 4: Verify.** Full suite, clippy and fmt are green.
- [ ] **Step 5: Commit** as `refactor: strip bars are entries, ready for stacks`.

### Task 3: Bar visuals for progress and pins

**Files:** `src/bar_strip.rs`, `src/peek.rs`

**Interfaces:**
- `fn progress_fill(rect: Rectangle, done: usize, total: usize) -> Rectangle` (bottom-up).
- The pinned notch is a 2 px darker cap at the top.
- Stack edges are two 1 px offset lines.
- The peek header shows `done/total`.

- [ ] **Step 1: Write the tests.**
  - `progress_fill_grows_from_bottom` (0/4 gives 0 height, 2/4 half, 4/4 full).
  - `all_done_bar_dims`, through a pure alpha helper.
  - `peek_header_shows_progress`, via `PeekText`.
- [ ] **Step 2: Run them.** Expected: RED.
- [ ] **Step 3: Implement** per P1, N1 and N2.
- [ ] **Step 4: Verify.** Expected: green.
- [ ] **Step 5: Commit** as `feat: checklist progress, pin notch and stack edges on bars`.

### Task 4: Reminders

**Files:** create `src/reminder.rs` and `src/notify.rs`; modify `src/app.rs`, `src/note_panel.rs`, `src/peek.rs`, `src/bar_strip.rs`, `Cargo.toml` (`notify-rust`, newest compatible)

**Interfaces:**
- `reminder::parse(title, now: DateTime<Local>) -> Option<DateTime<Local>>`
- `reminder::due(note, now) -> Option<DateTime<Utc>>`
- `reminder::display(title) -> String`
- `notify::reminder(title: &str)`
- `Message::ReminderTick`
- `App.pulsing: HashSet<Uuid>`, with a pulse phase driven by frame ticks only while non-empty.
- The subscription is `every(30 s)` only while some note has a pending reminder.
- At boot, a `ReminderTick` task fires missed reminders.

- [ ] **Step 1: Write the tests.** Use a fixed `now`, and keep `parse` and `due` pure.
  - `parses_time_today_or_tomorrow`
  - `parses_tomorrow_and_weekdays`
  - `parses_dates`
  - `invalid_tags_are_ignored`
  - `due_respects_fired`
  - `editing_reminder_time_rearms`
  - App: `reminder_tick_fires_once` (`reminder_fired` is set, the note's bar pulses, and the store is dirty), `fired_reminder_does_not_refire_after_reload`, `opening_note_stops_pulse`
- [ ] **Step 2: Run them.** Expected: RED.
- [ ] **Step 3: Implement** per R1–R3.
  - Put the notification call behind a small trait or function pointer so tests don't show real notifications. A `#[cfg(test)]` no-op is fine.
  - Bell icon goes in the header and the peek.
- [ ] **Step 4: Verify.** Expected: green.
- [ ] **Step 5: Commit** as `feat: reminders in note titles`.

### Task 5: Pin UI

**Files:** `src/note_panel.rs`, `src/app.rs`

**Interfaces:** `Message::TogglePin`, and a header pin button (`Icon::Pin`, active state drawn at full ink).

- [ ] **Step 1: Write the test.** `toggle_pin_moves_note_and_persists`.
- [ ] **Step 2: Run it.** Expected: RED.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Verify.** Expected: green.
- [ ] **Step 5: Commit** as `feat: pin notes to the top of the strip`.

### Task 6: Stack UI

**Files:** `src/bar_strip.rs`, `src/peek.rs`, `src/app.rs`

**Interfaces:**
- `Message::StackOnto { dragged: usize, target: usize }` (entry indices)
- `Message::Unstack(usize)`
- `Message::OpenStackMember(Uuid)`
- `fn drop_zone(bar: Rectangle, y: f32) -> DropZone { Before, Onto, After }`, where the middle 50 % is `Onto`.
- The stack peek lists members as clickable rows plus an `"Unstack"` button. The peek height fits the list.

- [ ] **Step 1: Write the tests.**
  - `drop_zone_edges`
  - `dragging_onto_bar_stacks`
  - `unstack_restores_members_after_top`
  - `opening_stack_member_opens_that_note`
  - `deleting_stack_top_promotes_member` (app level)
- [ ] **Step 2: Run them.** Expected: RED.
- [ ] **Step 3: Implement** per N2.
- [ ] **Step 4: Verify.** Expected: green.
- [ ] **Step 5: Commit** as `feat: stack notes by dragging bars onto each other`.

### Task 7: Global hotkey

**Files:** create `src/hotkey.rs`; modify `src/app.rs`, `src/settings.rs`, `src/settings_panel.rs`, `Cargo.toml` (`global-hotkey`, the version that matches `tray-icon` 0.26's ecosystem)

**Interfaces:**
- `hotkey::register() -> Option<Manager>` and `hotkey::events() -> Subscription<()>`
- `Message::HotkeyPressed`
- `AppSettings.global_hotkey: bool` (default true)
- `SettingToggle::GlobalHotkey`

- [ ] **Step 1: Write the tests.**
  - `hotkey_shows_hidden_notes_and_opens_new_note`
  - `hotkey_closes_panels_first`
  - `global_hotkey_setting_defaults_on_and_persists`
- [ ] **Step 2: Run them.** Expected: RED.
- [ ] **Step 3: Implement.**
  - Register in `WindowReady`, on the main thread, as tray creation does.
  - Re-register or unregister when the setting changes.
  - Read the global-hotkey docs in `~/.cargo/registry` for the macOS main-thread requirement.
- [ ] **Step 4: Verify.** Expected: green.
- [ ] **Step 5: Commit** as `feat: global hotkey for a new note`.

### Task 8: Strip file drops and clipboard notes

**Files:** `src/app.rs`, `src/bar_strip.rs`, `src/tray.rs`

**Interfaces:**
- `Message::StripFileDropped(Option<usize>, PathBuf)` (an entry index, or `None` for `+` and empty space)
- `Message::ClipboardNote`
- The tray id `clip`
- An Alt-click on `+` sends `ClipboardNote`
- Route drops: an open note keeps today's image insert; otherwise, use the drop position over the strip. Get the cursor position from `last_cursor`.

- [ ] **Step 1: Write the tests.**
  - `dropping_text_file_creates_note_with_title`
  - `dropping_on_bar_appends`
  - `dropping_image_creates_image_note`
  - `dropping_other_file_adds_its_path`
  - `oversized_or_binary_text_is_rejected`
  - `clipboard_note_from_text`
  - `clipboard_note_from_image`
  - `empty_clipboard_does_nothing` (via the existing `ClipboardContent` seam)
  - `tray_clip_item`
- [ ] **Step 2: Run them.** Expected: RED.
- [ ] **Step 3: Implement** per D1 and C1.
- [ ] **Step 4: Verify.** Expected: green.
- [ ] **Step 5: Commit** as `feat: file drops on the strip and notes from the clipboard`.

### Task 9: README

- [ ] **Step 1:** Document all six features, add the global hotkey row to the shortcut table, and list the tray item.
- [ ] **Step 2:** Run fmt, clippy and the tests, then commit as `docs: capture, reminders, pins and stacks`.
