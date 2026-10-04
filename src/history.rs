//! Undo/redo history for the open note's body. iced's text editor keeps
//! none, so the app records a snapshot before every change.

use std::time::{Duration, Instant};

/// Most steps kept; the oldest are dropped first.
const MAX_STEPS: usize = 200;
/// Typing after a longer pause starts a new step.
const TYPING_PAUSE: Duration = Duration::from_secs(1);

pub struct History<T> {
    undo: Vec<T>,
    redo: Vec<T>,
    /// When the current typing step last grew, if one is open.
    typing_since: Option<Instant>,
}

impl<T> Default for History<T> {
    fn default() -> Self {
        Self {
            undo: Vec::new(),
            redo: Vec::new(),
            typing_since: None,
        }
    }
}

impl<T> History<T> {
    /// Records `before`, the state ahead of a change. Consecutive `typing`
    /// changes less than a second apart share one step.
    pub fn record(&mut self, before: T, typing: bool, now: Instant) {
        let joins = typing
            && self
                .typing_since
                .is_some_and(|last| now.duration_since(last) < TYPING_PAUSE);
        self.typing_since = typing.then_some(now);
        self.redo.clear();
        if joins {
            return;
        }
        self.undo.push(before);
        if self.undo.len() > MAX_STEPS {
            self.undo.remove(0);
        }
    }

    /// Ends the open typing step, e.g. after the cursor moved.
    pub fn break_step(&mut self) {
        self.typing_since = None;
    }

    /// The state to go back to, given the `current` one.
    pub fn undo(&mut self, current: T) -> Option<T> {
        let previous = self.undo.pop()?;
        self.redo.push(current);
        self.typing_since = None;
        Some(previous)
    }

    /// The state to go forward to, given the `current` one.
    pub fn redo(&mut self, current: T) -> Option<T> {
        let next = self.redo.pop()?;
        self.undo.push(current);
        self.typing_since = None;
        Some(next)
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(ms: u64) -> Instant {
        // A fixed base keeps the arithmetic exact.
        thread_local!(static BASE: Instant = Instant::now());
        BASE.with(|b| *b + Duration::from_millis(ms))
    }

    #[test]
    fn undo_and_redo_walk_the_steps() {
        let mut h = History::default();
        h.record("a", false, at(0));
        h.record("ab", false, at(10));
        assert_eq!(h.undo("abc"), Some("ab"));
        assert_eq!(h.undo("ab"), Some("a"));
        assert_eq!(h.undo("a"), None);
        assert_eq!(h.redo("a"), Some("ab"));
        assert_eq!(h.redo("ab"), Some("abc"));
        assert_eq!(h.redo("abc"), None);
    }

    #[test]
    fn quick_typing_is_one_step() {
        let mut h = History::default();
        h.record("", true, at(0));
        h.record("h", true, at(200));
        h.record("hi", true, at(400));
        assert_eq!(h.undo("hi!"), Some(""));
        assert_eq!(h.undo(""), None);
    }

    #[test]
    fn pause_or_other_change_starts_a_new_step() {
        let mut h = History::default();
        h.record("", true, at(0));
        h.record("a", true, at(1500));
        h.record("ab", false, at(1600));
        h.record("ab ", true, at(1700));
        assert_eq!(h.undo("ab c"), Some("ab "));
        assert_eq!(h.undo("ab "), Some("ab"));
        assert_eq!(h.undo("ab"), Some("a"));
        assert_eq!(h.undo("a"), Some(""));
    }

    #[test]
    fn break_step_ends_typing_step() {
        let mut h = History::default();
        h.record("", true, at(0));
        h.break_step();
        h.record("a", true, at(100));
        assert_eq!(h.undo("ab"), Some("a"));
    }

    #[test]
    fn new_change_clears_redo() {
        let mut h = History::default();
        h.record("a", false, at(0));
        assert_eq!(h.undo("ab"), Some("a"));
        h.record("a", false, at(10));
        assert_eq!(h.redo("ax"), None);
    }

    #[test]
    fn keeps_at_most_max_steps() {
        let mut h = History::default();
        for i in 0..MAX_STEPS + 5 {
            h.record(i, false, at(0));
        }
        let mut undone = 0;
        let mut current = usize::MAX;
        while let Some(previous) = h.undo(current) {
            current = previous;
            undone += 1;
        }
        assert_eq!(undone, MAX_STEPS);
        assert_eq!(current, 5);
    }
}
