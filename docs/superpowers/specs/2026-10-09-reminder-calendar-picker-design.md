# Reminder Calendar Picker — Design Spec

## Overview

Reminders today are title tags only (`@HH:MM`, `@tomorrow`, `@mon`–`@sun`, `@YYYY-MM-DD`). Add a small calendar popover to pick a **date and time**, opened from a dedicated toolbar button or by clicking the header’s rendered trigger time. The picker writes absolute tags into the title so tags remain the source of truth.

Branch: `feat/reminder-calendar-picker`.

## Goals and success criteria

- Toolbar (edit mode) has a Reminder button that opens a popover with month calendar, hour/minute controls, **Clear**, and **Done**.
- Clicking the header’s rendered trigger time (`Tue 15:00`, Pending or Fired) opens the same popover.
- **Done** replaces any existing reminder tag with `@YYYY-MM-DD HH:MM` (existing parse form) and updates `reminder_set_at` via `retitle`.
- **Clear** removes the reminder tag from the title (same strip as notification display) and clears the reminder anchor.
- Outside click and Escape dismiss without applying.
- Manual `@` tags continue to work; after a picker save they become absolute date + time.
- Past datetimes remain allowed (due on next tick), matching today’s absolute-date behavior.

## Decisions

| # | Topic | Decision |
|---|---|---|
| C1 | Storage | Title tags only. No parallel reminder field. |
| C2 | Tag written | `@YYYY-MM-DD HH:MM` as one tag (space before time), matching `reminder::parse`. |
| C3 | Entry points | Toolbar Reminder button; header reminder label when Pending or Fired. Invalid tags are not clickable. |
| C4 | Default draft | If the note has a resolvable reminder, prefill from it. Else next whole hour from now (rolls to next day after 23:00). |
| C5 | Clear | Shown in the picker; strips the valid reminder tag; closes the picker. |
| C6 | Popover | Reuse the color-bubble overlay pattern, anchored to the control that opened it (toolbar button while editing; header label when not editing). |
| C7 | Dismiss | Escape / outside press closes without write; Done and Clear apply then close. Opening the color picker closes the reminder picker and vice versa. |

## Architecture

- **`reminder.rs`:** `with_reminder(title, at)`, `without_reminder(title)`, `default_at(now)`, `format_tag(date, time)` — pure string/time helpers; apply goes through `retitle`.
- **`reminder_picker.rs`:** Calendar + time steppers + Clear/Done UI; draft state type.
- **`toolbar.rs`:** Reminder (bell) button → toggle open.
- **`note_panel.rs`:** Clickable header reminder; bubble hosts picker content when open.
- **`app.rs`:** `reminder_picker: Option<ReminderDraft>`; open/toggle/dismiss/done/clear/date/time/month messages.

## Testing

- Unit: `with_reminder` / `without_reminder` replace or append tags and leave non-reminder `@` / email alone; `default_at` next hour.
- App: open from toolbar and from header; Done updates title + anchor; Clear strips tag; dismiss leaves title unchanged.

## Out of scope

- Recurring reminders.
- Separate reminder storage / sync.
- Changing OS notification behavior.
- Relative-tag shortcuts inside the picker UI (`tomorrow`, weekday chips).
