# Reminder Calendar Picker Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a small calendar popover to set note reminders as `@YYYY-MM-DD HH:MM` title tags, opened from the toolbar or the header trigger time.

**Architecture:** Pure tag rewrite helpers in `reminder.rs`; iced popover UI in `reminder_picker.rs` hosted via the existing color-bubble overlay; `App` holds optional draft state and wires Done/Clear through `retitle`.

**Tech Stack:** Rust, iced, chrono, existing snap-notes note chrome.

## Global Constraints

- Tags remain the source of truth; no parallel reminder field.
- Written tag form is `@YYYY-MM-DD HH:MM` (one tag, space before time).
- Past datetimes allowed.

---

### Task 1: Tag rewrite helpers

**Files:**
- Modify: `src/reminder.rs`
- Test: `src/reminder.rs` (inline `#[cfg(test)]`)

**Interfaces:**
- Produces: `format_tag(date, time) -> String`, `with_reminder(title, at) -> String`, `without_reminder(title) -> String`, `default_at(now) -> DateTime<Local>`

- [x] **Step 1: Write failing tests** for rewrite/clear/default_at
- [x] **Step 2: Implement helpers**
- [x] **Step 3: Run reminder helper tests** — PASS

### Task 2: Picker UI + app wiring

**Files:**
- Create: `src/reminder_picker.rs`
- Modify: `src/main.rs`, `src/toolbar.rs`, `src/note_panel.rs`, `src/app.rs`
- Test: `src/app.rs` / `src/reminder.rs` as needed

- [x] **Step 1: Add `ReminderDraft` + view in `reminder_picker.rs`**
- [x] **Step 2: Toolbar Reminder button + header click + bubble**
- [x] **Step 3: App messages: open/toggle/dismiss/done/clear/navigate**
- [x] **Step 4: Integration tests for Done/Clear/dismiss**
- [x] **Step 5: `cargo test` relevant modules; commit**
