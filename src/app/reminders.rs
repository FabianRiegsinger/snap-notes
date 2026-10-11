//! Reminders: firing them, their alerting bars and the reminder picker.

use super::*;

impl App {
    pub(super) fn handle_reminders(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::ReminderTick => return self.fire_reminders(chrono::Local::now()),
            Message::ToggleReminderPicker => {
                // The picker works on the committed title.
                self.commit_title();
                if self.reminder_picker.is_some() {
                    self.reminder_picker = None;
                } else {
                    self.open_reminder_picker();
                }
            }
            Message::OpenReminderPicker => {
                self.commit_title();
                self.open_reminder_picker();
            }
            Message::ReminderPickerDay(date) => {
                if let Some(draft) = &mut self.reminder_picker {
                    draft.date = date;
                    draft.view_month =
                        chrono::NaiveDate::from_ymd_opt(date.year(), date.month(), 1)
                            .expect("valid month");
                }
            }
            Message::ReminderPickerMonth(delta) => {
                if let Some(draft) = &mut self.reminder_picker {
                    crate::reminder_picker::shift_month(draft, delta);
                }
            }
            Message::ReminderPickerHour(delta) => {
                if let Some(draft) = &mut self.reminder_picker {
                    crate::reminder_picker::adjust_time(draft, delta, 0);
                }
            }
            Message::ReminderPickerMinute(delta) => {
                if let Some(draft) = &mut self.reminder_picker {
                    crate::reminder_picker::adjust_time(draft, 0, delta);
                }
            }
            Message::ReminderPickerDone => {
                self.commit_title();
                self.apply_reminder_picker();
            }
            Message::ReminderPickerClear => {
                self.commit_title();
                let Some(id) = self.active_note else {
                    self.reminder_picker = None;
                    return Task::none();
                };
                self.reminder_picker = None;
                let title = self
                    .store
                    .notes()
                    .iter()
                    .find(|n| n.id == id)
                    .map(|n| reminder::without_reminder(&n.title))
                    .unwrap_or_default();
                if let Some(note) = self.store.note_mut(id) {
                    reminder::retitle(note, title, chrono::Local::now());
                    note.reminder_fired = None;
                    note.updated_at = chrono::Utc::now();
                }
                self.pulsing.remove(&id);
                self.store.mark_dirty();
            }
            Message::ReminderToCalendar => {
                let Some((note, at)) = self.share_reminder() else {
                    return Task::none();
                };
                let dir = std::env::temp_dir().join("snap-notes");
                match crate::ics::calendar_file(&dir, &note, at, chrono::Utc::now()) {
                    Ok(path) => self.open_external(&path.to_string_lossy()),
                    Err(e) => eprintln!("could not write the calendar file: {e}"),
                }
            }
            Message::ReminderByEmail => {
                let Some((note, at)) = self.share_reminder() else {
                    return Task::none();
                };
                self.open_external(&crate::share::mailto(&note, &reminder::export_label(at)));
            }
            _ => unreachable!("not a reminders message"),
        }
        Task::none()
    }

    /// Hands a file or link to the system's default app. Tests only record
    /// it.
    fn open_external(&mut self, target: &str) {
        #[cfg(not(test))]
        platform::open_url(target);
        #[cfg(test)]
        self.opened.push(target.to_string());
    }

    /// Closes the reminder picker, setting the active note's reminder to
    /// the picked time (as Done does).
    fn apply_reminder_picker(&mut self) {
        let Some(draft) = self.reminder_picker.take() else {
            return;
        };
        let (Some(at), Some(id)) = (draft.at(), self.active_note) else {
            return;
        };
        let title = self
            .store
            .notes()
            .iter()
            .find(|n| n.id == id)
            .map(|n| reminder::with_reminder(&n.title, at))
            .unwrap_or_else(|| reminder::with_reminder("", at));
        if let Some(note) = self.store.note_mut(id) {
            reminder::retitle(note, title, chrono::Local::now());
            note.reminder_fired = None;
            note.updated_at = chrono::Utc::now();
        }
        self.pulsing.remove(&id);
        self.store.mark_dirty();
    }

    /// For the picker's share buttons: commits the title and the picked
    /// time, then returns the active note and its reminder time, if any.
    fn share_reminder(&mut self) -> Option<(crate::note::Note, chrono::DateTime<chrono::Local>)> {
        self.commit_title();
        self.apply_reminder_picker();
        let id = self.active_note?;
        let note = self.store.notes().iter().find(|n| n.id == id)?;
        Some((note.clone(), reminder::at(note)?))
    }

    /// Opens the reminder calendar for the active note, prefilling from its
    /// current trigger or the next whole hour.
    pub(super) fn open_reminder_picker(&mut self) {
        let Some(id) = self.active_note else {
            return;
        };
        let now = chrono::Local::now();
        let draft = self
            .store
            .notes()
            .iter()
            .find(|n| n.id == id)
            .and_then(|n| reminder::at(n).map(ReminderDraft::from_at))
            .unwrap_or_else(|| ReminderDraft::from_now(now));
        self.reminder_picker = Some(draft);
        self.color_picker_open = false;
        self.text_color_picker_open = false;
        self.color_bubble_rect = None;
    }

    /// Fires every reminder due at `now`: a notification, an alerting bar,
    /// and the fired time saved at once so it never fires twice. Alerting
    /// bars stay visible while the strip may remain auto-hidden.
    pub(super) fn fire_reminders(&mut self, now: chrono::DateTime<chrono::Local>) -> Task<Message> {
        // A title being typed may parse as a time on its way to another.
        let editing = self.title_edit.as_ref().map(|e| e.id);
        let due: Vec<_> = self
            .store
            .notes()
            .iter()
            .filter(|note| Some(note.id) != editing)
            .filter_map(|note| Some((note.id, reminder::due(note, now)?)))
            .collect();
        if due.is_empty() {
            return Task::none();
        }
        let had_alert = !self.pulsing.is_empty();
        for (id, time) in due {
            let Some(note) = self.store.note_mut(id) else {
                continue;
            };
            note.reminder_fired = Some(time);
            notify::reminder(&reminder::display(&note.title));
            // The open note is already in view.
            if !(self.active_note == Some(id) && self.morph.is_opening()) {
                self.begin_alert(id);
            }
        }
        let _ = self.store.save();
        self.store.did_save();
        self.animating |= self.pulse_running();
        // Widen off the edge sliver so alerting bars have room to draw.
        if !had_alert && !self.pulsing.is_empty() {
            return self.dock_window();
        }
        Task::none()
    }

    /// Starts (or joins) the alert animation for `id`.
    pub(super) fn begin_alert(&mut self, id: Uuid) {
        if self.pulsing.is_empty() {
            // Jump starts mid-rise so the hop is visible immediately;
            // Pulse's cosine is already lit at phase 0.
            self.pulse_phase = match self.settings.settings().motion.reminder_alert {
                crate::settings::ReminderAlertStyle::Jump => JUMP_SECS * 0.25,
                crate::settings::ReminderAlertStyle::Pulse => 0.0,
            };
        }
        self.pulsing.insert(id);
    }

    /// Some bar alerts and the notes are on screen.
    pub(super) fn pulse_running(&self) -> bool {
        self.visible && !self.pulsing.is_empty()
    }

    /// Some note has a reminder that hasn't fired yet.
    pub(super) fn reminders_pending(&self) -> bool {
        self.store
            .notes()
            .iter()
            .any(|note| reminder::pending(note).is_some())
    }

    /// How strongly each bar pulses (0..=1); empty while nothing pulses or
    /// when the alert style is Jump. A stacked note's reminder pulses its
    /// stack's bar.
    pub(super) fn bar_pulse(&self) -> Vec<f32> {
        use crate::settings::ReminderAlertStyle;
        if self.pulsing.is_empty()
            || self.settings.settings().motion.reminder_alert != ReminderAlertStyle::Pulse
        {
            return Vec::new();
        }
        let wave = 0.5 - 0.5 * (std::f32::consts::TAU * self.pulse_phase / PULSE_SECS).cos();
        let amount = 0.25 + 0.75 * wave;
        let notes = self.store.notes();
        self.entries()
            .iter()
            .map(|entry| {
                let pulses = std::iter::once(entry.top)
                    .chain(entry.members.iter().copied())
                    .any(|i| self.pulsing.contains(&notes[i].id));
                if pulses {
                    amount
                } else {
                    0.0
                }
            })
            .collect()
    }

    /// Which bars have an uncleared fired reminder (for alert-only draw/hit
    /// while the strip is auto-hidden).
    pub(super) fn bar_alert(&self) -> Vec<bool> {
        if self.pulsing.is_empty() {
            return Vec::new();
        }
        let notes = self.store.notes();
        self.entries()
            .iter()
            .map(|entry| {
                std::iter::once(entry.top)
                    .chain(entry.members.iter().copied())
                    .any(|i| self.pulsing.contains(&notes[i].id))
            })
            .collect()
    }

    /// Away-from-edge jump offset per bar; empty while nothing alerts or when
    /// the alert style is Pulse. Hover settles that bar to rest.
    pub(super) fn bar_jump(&self) -> Vec<f32> {
        use crate::settings::ReminderAlertStyle;
        if self.pulsing.is_empty()
            || self.settings.settings().motion.reminder_alert != ReminderAlertStyle::Jump
        {
            return Vec::new();
        }
        let amount = jump_amount(self.pulse_phase);
        let notes = self.store.notes();
        let hover_id = self.hover_bar.map(|(id, _)| id);
        self.entries()
            .iter()
            .map(|entry| {
                let ids: Vec<Uuid> = std::iter::once(entry.top)
                    .chain(entry.members.iter().copied())
                    .map(|i| notes[i].id)
                    .collect();
                let jumps = ids.iter().any(|id| self.pulsing.contains(id));
                if !jumps {
                    return 0.0;
                }
                if let Some((sid, since, from)) = self.jump_settle {
                    if ids.contains(&sid) {
                        return jump_settle_offset(from, since.elapsed().as_secs_f32());
                    }
                }
                if hover_id.is_some_and(|id| ids.contains(&id) && self.pulsing.contains(&id)) {
                    return 0.0;
                }
                amount
            })
            .collect()
    }

    /// Pauses a jumping alert when the cursor enters its bar: settle to rest
    /// then hold; clearing hover resumes the hop.
    pub(super) fn sync_jump_settle(&mut self, hovered: Option<Uuid>) {
        use crate::settings::ReminderAlertStyle;
        if self.settings.settings().motion.reminder_alert != ReminderAlertStyle::Jump {
            self.jump_settle = None;
            return;
        }
        match hovered {
            Some(id) if self.pulsing.contains(&id) => {
                let from = jump_amount(self.pulse_phase);
                self.jump_settle = Some((id, Instant::now(), from));
            }
            _ => self.jump_settle = None,
        }
    }

    pub(super) fn jump_settle_active(&self) -> bool {
        self.jump_settle.as_ref().is_some_and(|(_, since, from)| {
            *from != 0.0 && since.elapsed().as_secs_f32() < JUMP_SETTLE_SECS
        })
    }
}
