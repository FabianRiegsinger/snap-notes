//! Pure auto-hide state machine (no iced or OS dependency).
// Task 2 wires this into app.rs and removes this allow.
#![allow(dead_code)]

use std::time::{Duration, Instant};

use crate::animation::ease_out_cubic;

/// The cursor must rest in the edge zone this long before the strip reveals.
pub const REVEAL_DWELL: Duration = Duration::from_millis(150);
/// The strip hides this long after it was last in use.
pub const HIDE_GRACE: Duration = Duration::from_millis(800);
/// Slide length at motion speed 1.
pub const SLIDE_SECS: f32 = 0.2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Shown,
    Hiding,
    Hidden,
    Revealing,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Inputs {
    pub enabled: bool,
    /// Cursor is in the edge zone.
    pub in_edge: bool,
    /// Cursor is over the strip column, the peek or the toast.
    pub in_use_area: bool,
    /// Something is in use (search, drag, pulse, ...) and prevents hiding.
    pub blocked: bool,
    /// Reveal at once, skipping the dwell.
    pub force_reveal: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct AutoHide {
    phase: Phase,
    /// Linear slide progress: 0 = shown, 1 = hidden.
    offset: f32,
    edge_since: Option<Instant>,
    idle_since: Option<Instant>,
}

impl Default for AutoHide {
    fn default() -> Self {
        Self {
            phase: Phase::Shown,
            offset: 0.0,
            edge_since: None,
            idle_since: None,
        }
    }
}

impl AutoHide {
    /// Advances the machine. Returns true while it needs frames or has a
    /// pending dwell/grace deadline.
    pub fn step(&mut self, inputs: Inputs, now: Instant, dt: f32, speed: f32) -> bool {
        if !inputs.enabled {
            *self = Self::default();
            return false;
        }

        if inputs.in_use_area || inputs.blocked {
            self.idle_since = None;
        } else if self.idle_since.is_none() {
            self.idle_since = Some(now);
        }

        if inputs.in_edge && self.phase == Phase::Hidden {
            self.edge_since.get_or_insert(now);
        } else {
            self.edge_since = None;
        }

        let reveal_now = matches!(self.phase, Phase::Hidden | Phase::Hiding)
            && (inputs.force_reveal || inputs.blocked);
        let dwelled = self
            .edge_since
            .is_some_and(|since| now.saturating_duration_since(since) >= REVEAL_DWELL);
        if reveal_now || dwelled {
            self.phase = Phase::Revealing;
            self.edge_since = None;
        } else if self.phase == Phase::Shown
            && self
                .idle_since
                .is_some_and(|since| now.saturating_duration_since(since) >= HIDE_GRACE)
        {
            self.phase = Phase::Hiding;
            self.idle_since = None;
        }

        let delta = dt / (SLIDE_SECS / speed.max(f32::EPSILON));
        match self.phase {
            Phase::Hiding => {
                self.offset = (self.offset + delta).min(1.0);
                if self.offset >= 1.0 {
                    self.phase = Phase::Hidden;
                }
            }
            Phase::Revealing => {
                self.offset = (self.offset - delta).max(0.0);
                if self.offset <= 0.0 {
                    self.phase = Phase::Shown;
                }
            }
            Phase::Shown | Phase::Hidden => {}
        }

        matches!(self.phase, Phase::Hiding | Phase::Revealing) || self.next_deadline().is_some()
    }

    /// Eased offset, 0 (shown) to 1 (hidden). Hides ease in and reveals ease
    /// out; for cubics these are the same curve, so a mid-slide reversal
    /// continues from the current position without a jump.
    pub fn offset(&self) -> f32 {
        1.0 - ease_out_cubic(1.0 - self.offset)
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }

    pub fn is_hidden(&self) -> bool {
        self.phase == Phase::Hidden
    }

    /// The dwell or grace deadline still pending, for scheduling a wake-up.
    pub fn next_deadline(&self) -> Option<Instant> {
        match self.phase {
            Phase::Hidden => self.edge_since.map(|t| t + REVEAL_DWELL),
            Phase::Shown => self.idle_since.map(|t| t + HIDE_GRACE),
            Phase::Hiding | Phase::Revealing => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    const MS: fn(u64) -> Duration = Duration::from_millis;

    fn idle() -> Inputs {
        Inputs {
            enabled: true,
            ..Inputs::default()
        }
    }

    fn edge() -> Inputs {
        Inputs {
            in_edge: true,
            ..idle()
        }
    }

    fn in_use() -> Inputs {
        Inputs {
            in_use_area: true,
            ..idle()
        }
    }

    /// A machine that is fully Hidden.
    fn hidden(t0: Instant) -> (AutoHide, Instant) {
        let mut m = AutoHide::default();
        m.step(idle(), t0, 0.0, 1.0);
        let t = t0 + HIDE_GRACE;
        m.step(idle(), t, 0.0, 1.0);
        assert_eq!(m.phase(), Phase::Hiding);
        m.step(idle(), t, 1.0, 1.0);
        assert_eq!(m.phase(), Phase::Hidden);
        (m, t)
    }

    #[test]
    fn disabled_is_always_shown() {
        let t0 = Instant::now();
        let mut m = AutoHide::default();
        let off = Inputs::default();
        for i in 0..20 {
            m.step(off, t0 + MS(i * 200), 0.016, 1.0);
        }
        assert_eq!(m.phase(), Phase::Shown);
        assert_eq!(m.offset(), 0.0);
        assert!(!m.is_hidden());
        assert!(!m.step(off, t0, 0.016, 1.0));
    }

    #[test]
    fn hides_after_grace_when_idle() {
        let t0 = Instant::now();
        let mut m = AutoHide::default();
        m.step(idle(), t0, 0.0, 1.0);
        m.step(idle(), t0 + MS(799), 0.0, 1.0);
        assert_eq!(m.phase(), Phase::Shown);
        m.step(idle(), t0 + MS(800), 0.0, 1.0);
        assert_eq!(m.phase(), Phase::Hiding);
    }

    #[test]
    fn reentry_resets_grace() {
        let t0 = Instant::now();
        let mut m = AutoHide::default();
        m.step(idle(), t0, 0.0, 1.0);
        m.step(in_use(), t0 + MS(700), 0.0, 1.0);
        m.step(idle(), t0 + MS(750), 0.0, 1.0);
        m.step(idle(), t0 + MS(1500), 0.0, 1.0);
        assert_eq!(m.phase(), Phase::Shown);
        m.step(idle(), t0 + MS(1550), 0.0, 1.0);
        assert_eq!(m.phase(), Phase::Hiding);
    }

    #[test]
    fn dwell_reveals_after_150ms() {
        let (mut m, t) = hidden(Instant::now());
        m.step(edge(), t, 0.0, 1.0);
        m.step(edge(), t + MS(149), 0.0, 1.0);
        assert_eq!(m.phase(), Phase::Hidden);
        m.step(edge(), t + MS(150), 0.0, 1.0);
        assert_eq!(m.phase(), Phase::Revealing);
    }

    #[test]
    fn leaving_edge_resets_dwell() {
        let (mut m, t) = hidden(Instant::now());
        m.step(edge(), t, 0.0, 1.0);
        m.step(idle(), t + MS(100), 0.0, 1.0);
        m.step(edge(), t + MS(120), 0.0, 1.0);
        m.step(edge(), t + MS(250), 0.0, 1.0);
        assert_eq!(m.phase(), Phase::Hidden);
        m.step(edge(), t + MS(270), 0.0, 1.0);
        assert_eq!(m.phase(), Phase::Revealing);
    }

    #[test]
    fn blocked_prevents_and_reverses_hide() {
        let t0 = Instant::now();
        let blocked = Inputs {
            blocked: true,
            ..idle()
        };
        let mut m = AutoHide::default();
        m.step(blocked, t0, 0.0, 1.0);
        m.step(blocked, t0 + MS(5000), 0.0, 1.0);
        assert_eq!(m.phase(), Phase::Shown);

        // A running hide reverses when something blocks.
        let mut m = AutoHide::default();
        m.step(idle(), t0, 0.0, 1.0);
        m.step(idle(), t0 + HIDE_GRACE, 0.0, 1.0);
        m.step(idle(), t0 + HIDE_GRACE, SLIDE_SECS / 2.0, 1.0);
        assert_eq!(m.phase(), Phase::Hiding);
        m.step(blocked, t0 + HIDE_GRACE, 0.0, 1.0);
        assert_eq!(m.phase(), Phase::Revealing);
        while m.step(blocked, t0 + HIDE_GRACE, 0.016, 1.0) {}
        assert_eq!(m.phase(), Phase::Shown);
        assert_eq!(m.offset(), 0.0);

        // Fully hidden also comes back.
        let (mut m, t) = hidden(t0);
        m.step(blocked, t, 0.0, 1.0);
        assert_eq!(m.phase(), Phase::Revealing);
    }

    #[test]
    fn force_reveal_skips_dwell() {
        let (mut m, t) = hidden(Instant::now());
        let force = Inputs {
            force_reveal: true,
            ..idle()
        };
        m.step(force, t, 0.0, 1.0);
        assert_eq!(m.phase(), Phase::Revealing);
    }

    #[test]
    fn slide_offset_is_monotonic_and_reaches_bounds() {
        let t0 = Instant::now();
        let mut m = AutoHide::default();
        m.step(idle(), t0, 0.0, 1.0);
        let t = t0 + HIDE_GRACE;
        m.step(idle(), t, 0.0, 1.0);
        let mut last = m.offset();
        let mut frames = 0;
        while m.step(idle(), t, 1.0 / 60.0, 1.0) {
            assert!(m.offset() >= last);
            last = m.offset();
            frames += 1;
            assert!(frames < 100);
        }
        assert_eq!(m.offset(), 1.0);
        assert!(m.is_hidden());
        assert!((11..=13).contains(&frames), "{frames}");

        m.step(edge(), t, 0.0, 1.0);
        m.step(edge(), t + REVEAL_DWELL, 0.0, 1.0);
        assert_eq!(m.phase(), Phase::Revealing);
        while m.phase() == Phase::Revealing {
            m.step(idle(), t + REVEAL_DWELL, 1.0 / 60.0, 1.0);
            assert!(m.offset() <= last);
            last = m.offset();
        }
        assert_eq!(m.offset(), 0.0);
        assert_eq!(m.phase(), Phase::Shown);
    }

    #[test]
    fn speed_divides_slide_length() {
        let (mut m, t) = hidden(Instant::now());
        let force = Inputs {
            force_reveal: true,
            ..idle()
        };
        m.step(force, t, 0.0, 2.0);
        m.step(force, t, SLIDE_SECS / 2.0 + 1e-4, 2.0);
        assert_eq!(m.phase(), Phase::Shown);
    }

    #[test]
    fn mid_slide_reversal_continues_from_current_offset() {
        let t0 = Instant::now();
        let mut m = AutoHide::default();
        m.step(idle(), t0, 0.0, 1.0);
        let t = t0 + HIDE_GRACE;
        m.step(idle(), t, 0.0, 1.0);
        m.step(idle(), t, SLIDE_SECS * 0.6, 1.0);
        assert_eq!(m.phase(), Phase::Hiding);
        let before = m.offset();
        assert!(before > 0.0 && before < 1.0);
        let force = Inputs {
            force_reveal: true,
            ..idle()
        };
        m.step(force, t, 0.0, 1.0);
        assert_eq!(m.phase(), Phase::Revealing);
        assert!((m.offset() - before).abs() < 1e-6);
        m.step(force, t, SLIDE_SECS * 0.1, 1.0);
        assert!(m.offset() < before);
    }

    #[test]
    fn disabling_reveals_instantly() {
        let (mut m, t) = hidden(Instant::now());
        assert!(!m.step(Inputs::default(), t, 0.0, 1.0));
        assert_eq!(m.phase(), Phase::Shown);
        assert_eq!(m.offset(), 0.0);

        // Also mid-slide, even with a blocker set.
        let t0 = Instant::now();
        let mut m = AutoHide::default();
        m.step(idle(), t0, 0.0, 1.0);
        m.step(idle(), t0 + HIDE_GRACE, 0.0, 1.0);
        m.step(idle(), t0 + HIDE_GRACE, 0.1, 1.0);
        let off = Inputs {
            blocked: true,
            ..Inputs::default()
        };
        m.step(off, t0 + HIDE_GRACE, 0.0, 1.0);
        assert_eq!(m.phase(), Phase::Shown);
        assert_eq!(m.offset(), 0.0);
    }

    #[test]
    fn next_deadline_reports_pending_timer() {
        let t0 = Instant::now();
        let mut m = AutoHide::default();
        assert_eq!(m.next_deadline(), None);
        assert!(m.step(idle(), t0, 0.0, 1.0));
        assert_eq!(m.next_deadline(), Some(t0 + HIDE_GRACE));
        m.step(in_use(), t0 + MS(100), 0.0, 1.0);
        assert_eq!(m.next_deadline(), None);

        let (mut m, t) = hidden(t0);
        assert_eq!(m.next_deadline(), None);
        assert!(!m.step(idle(), t, 0.0, 1.0));
        assert!(m.step(edge(), t, 0.0, 1.0));
        assert_eq!(m.next_deadline(), Some(t + REVEAL_DWELL));
        m.step(idle(), t + MS(10), 0.0, 1.0);
        assert_eq!(m.next_deadline(), None);
    }
}
