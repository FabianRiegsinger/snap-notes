//! The animation frame tick: steps every morph, then finishes what folded
//! away and hands over pending focus.

use super::*;

impl App {
    /// Steps one animation frame at `now`.
    pub(super) fn tick(&mut self, now: Instant) -> Task<Message> {
        let dt = self
            .last_tick
            .map_or(1.0 / 60.0, |t| now.duration_since(t).as_secs_f32())
            .clamp(0.0, 1.0 / 30.0);
        self.last_tick = Some(now);

        let centers = self.magnification_centers();
        let mag_active =
            self.magnification
                .update(self.cursor_y, &centers, dt, &self.settings.settings().hover);
        let actions_folding = !self.actions_morph.is_closed();
        let morph_active = self.actions_morph.tick(dt)
            | self.morph.tick(dt)
            | self.peek.tick(dt)
            | self.settings_panel.morph.tick(dt)
            | self.search_panel.morph.tick(dt)
            | self.export_panel.morph.tick(dt)
            | self.mode_fade.tick(dt);
        let collapse_active = self.collapsing.as_mut().is_some_and(|(_, m)| m.tick(dt));
        let collapsed = if self.collapsing.is_some() && !collapse_active {
            self.finish_collapse()
        } else {
            Task::none()
        };
        if self.peek.is_closed() {
            self.peek_note = None;
        }
        let settings_closed = self.settings_panel.finish_closing();
        if settings_closed {
            self.palette_slot = None;
        }
        let search_closed = self.search_panel.finish_closing();
        let export_closed = self.export_panel.finish_closing();
        if export_closed {
            self.export = None;
        }
        // Hidden notes don't alert; they resume once shown.
        let pulse_active = self.pulse_running();
        if pulse_active {
            let period = match self.settings.settings().motion.reminder_alert {
                crate::settings::ReminderAlertStyle::Jump => JUMP_SECS,
                crate::settings::ReminderAlertStyle::Pulse => PULSE_SECS,
            };
            self.pulse_phase = (self.pulse_phase + dt) % period;
        }
        let settle_active = self.jump_settle_active();
        let shake_active = self.shake_secs(now).is_some();
        let auto_hide = self.drive_auto_hide(now, dt);
        let slide_active = matches!(
            self.auto_hide.phase(),
            autohide::Phase::Hiding | autohide::Phase::Revealing
        );
        self.animating = mag_active
            || morph_active
            || collapse_active
            || pulse_active
            || settle_active
            || shake_active
            || slide_active;
        if !self.animating {
            self.last_tick = None;
        }

        let focus = Task::batch([collapsed, self.take_pending_focus(), auto_hide]);
        if self.active_note.is_some() && self.morph.is_closed() {
            return Task::batch([focus, self.finish_close()]);
        }
        let actions_folded = actions_folding && self.actions_morph.is_closed();
        if settings_closed || search_closed || export_closed || actions_folded {
            // Without passthrough the window shrinks back to the strip.
            return Task::batch([focus, self.dock_window()]);
        }
        focus
    }

    /// Fades the toolbar and body in again after an edit/render switch; a
    /// switch mid-fade starts over from transparent.
    pub(super) fn restart_mode_fade(&mut self) {
        self.mode_fade.restart();
        self.animating = true;
    }

    /// A mode fade that has already finished, so a note opens fully visible.
    pub(super) fn settled_fade(speed: f32) -> Morph {
        let mut fade = Morph::with_secs(MODE_FADE_SECS, speed);
        fade.open();
        // Any step at least as long as the fade lands exactly on 1.
        fade.tick(2.0 * MODE_FADE_SECS / speed.max(0.01));
        fade
    }

    /// The focus waiting for its widget, once the widget is drawn.
    pub(super) fn take_pending_focus(&mut self) -> Task<Message> {
        let shown = |frame: Option<MorphFrame>| frame.is_some_and(|f| f.content_alpha >= 0.01);
        match self.pending_focus {
            Some(PendingFocus::SearchField) if shown(self.search_frame()) => {
                self.pending_focus = None;
                focus_field()
            }
            Some(PendingFocus::Body) if !self.editing || self.active_note.is_none() => {
                self.pending_focus = None;
                Task::none()
            }
            Some(PendingFocus::Body) if shown(self.note_frame()) => {
                self.pending_focus = None;
                focus_body()
            }
            _ => Task::none(),
        }
    }
}
