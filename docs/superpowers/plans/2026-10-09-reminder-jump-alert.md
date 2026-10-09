# Reminder Jump Alert Implementation Plan

> **For agentic workers:** Implement task-by-task. Steps use checkbox syntax.

**Goal:** Default Dock-style jump alert for fired reminders, with pulse as a settings alternative.

**Architecture:** `ReminderAlertStyle` on `MotionSettings`; shared `pulsing` set; Jump draws away-axis offset, Pulse keeps glow; hover settles jump then peeks.

**Tech Stack:** Rust / iced, existing settings + bar_strip patterns.

## Task 1: Settings enum + persistence

- [ ] Add `ReminderAlertStyle`, field on `MotionSettings`, `from_json`, tests
- [ ] Panel segmented control + `Message::ReminderAlertChosen`
- [ ] README Motion row

## Task 2: Jump math + draw

- [ ] `jump_amount`, `jump_translation`, `BarStrip.jump`
- [ ] `bar_jump` / settle in `app.rs`; Tick / hover wiring
- [ ] Tests for jump, hover settle, pulse path
