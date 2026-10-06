use crate::note::Note;
use crate::rich;
use std::collections::HashMap;
use uuid::Uuid;

/// One bar in the strip: a top-level note and the notes stacked under it,
/// as indices into the store's notes.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub top: usize,
    pub members: Vec<usize>,
}

impl Entry {
    /// Whether the note at `index` is this entry's top or one of its members.
    pub fn holds(&self, index: usize) -> bool {
        self.top == index || self.members.contains(&index)
    }

    /// The top and its members, in store order.
    pub fn notes(&self) -> impl Iterator<Item = usize> + '_ {
        std::iter::once(self.top).chain(self.members.iter().copied())
    }
}

/// Per entry, `(done, total)` task items summed over its top and members;
/// `None` when none of them has tasks.
pub fn progress(notes: &[Note], entries: &[Entry]) -> Vec<Option<(usize, usize)>> {
    entries
        .iter()
        .map(|entry| {
            entry
                .notes()
                .filter_map(|i| rich::task_progress(&notes[i].content))
                .reduce(|(d, t), (d2, t2)| (d + d2, t + t2))
        })
        .collect()
}

/// One entry per top-level note, in strip (store) order; members are listed
/// in store order. A note whose `stack` points at a missing note, or at a
/// note that is itself stacked, counts as top-level.
pub fn entries(notes: &[Note]) -> Vec<Entry> {
    let index: HashMap<Uuid, usize> = notes.iter().enumerate().map(|(i, n)| (n.id, i)).collect();
    let top_of = |note: &Note| {
        note.stack
            .and_then(|top| index.get(&top).copied())
            .filter(|&top| notes[top].stack.is_none())
    };
    let mut entries = Vec::new();
    let mut entry_of = HashMap::new();
    for (i, note) in notes.iter().enumerate() {
        if top_of(note).is_none() {
            entry_of.insert(i, entries.len());
            entries.push(Entry {
                top: i,
                members: Vec::new(),
            });
        }
    }
    for (i, note) in notes.iter().enumerate() {
        if let Some(top) = top_of(note) {
            entries[entry_of[&top]].members.push(i);
        }
    }
    entries
}

/// The gap (0..=bars) a bar dragged from `dragged` drops into at
/// `cursor_y`: before the first bar whose center is below the cursor. Like
/// `NoteStore::reorder`, it stays within the dragged bar's pin group, given
/// each bar's pin state (pinned bars come first).
pub fn insertion_slot(centers: &[f32], cursor_y: f32, pinned: &[bool], dragged: usize) -> usize {
    let slot = centers
        .iter()
        .position(|&center| cursor_y < center)
        .unwrap_or(centers.len());
    let boundary = pinned.iter().take_while(|&&p| p).count();
    if pinned.get(dragged).copied().unwrap_or(false) {
        slot.min(boundary)
    } else {
        slot.max(boundary)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::note::PALETTE;

    fn notes(n: usize) -> Vec<Note> {
        (0..n).map(|_| Note::new(PALETTE[0])).collect()
    }

    fn entry(top: usize, members: &[usize]) -> Entry {
        Entry {
            top,
            members: members.to_vec(),
        }
    }

    #[test]
    fn entries_without_stacks_are_one_per_note() {
        assert_eq!(entries(&[]), Vec::new());
        assert_eq!(
            entries(&notes(3)),
            vec![entry(0, &[]), entry(1, &[]), entry(2, &[])]
        );
    }

    #[test]
    fn stack_groups_members() {
        let mut notes = notes(5);
        let top = notes[1].id;
        notes[2].stack = Some(top);
        notes[3].stack = Some(top);
        let entries = entries(&notes);
        assert_eq!(
            entries,
            vec![entry(0, &[]), entry(1, &[2, 3]), entry(4, &[])]
        );
        assert!(entries[1].holds(3) && !entries[1].holds(4));
        assert_eq!(entries[1].notes().collect::<Vec<_>>(), vec![1, 2, 3]);
    }

    #[test]
    fn orphan_member_is_top_level() {
        let mut notes = notes(4);
        // Points at a missing note.
        notes[1].stack = Some(Uuid::new_v4());
        // Points at a note that is itself a member.
        let top = notes[0].id;
        notes[2].stack = Some(top);
        notes[3].stack = Some(notes[2].id);
        assert_eq!(
            entries(&notes),
            vec![entry(0, &[2]), entry(1, &[]), entry(3, &[])]
        );
    }

    #[test]
    fn stack_progress_sums_members() {
        let mut ns = notes(3);
        ns[0].content = "- [x] a\n- [ ] b".into();
        ns[1].content = "- [x] c".into();
        let entries = [entry(0, &[1]), entry(2, &[])];
        assert_eq!(progress(&ns, &entries), [Some((2, 3)), None]);
    }

    #[test]
    fn insertion_slot_stays_in_the_pin_group() {
        let centers = [10.0, 20.0, 30.0, 40.0];
        let pinned = [true, true, false, false];
        // Unclamped: the first bar whose center is below the cursor.
        assert_eq!(insertion_slot(&centers, 25.0, &pinned, 3), 2);
        assert_eq!(insertion_slot(&centers, 35.0, &pinned, 0), 2);
        assert_eq!(insertion_slot(&centers, 15.0, &pinned, 0), 1);
        assert_eq!(insertion_slot(&centers, 50.0, &pinned, 2), 4);
        // An unpinned bar can't go above the pinned ones, nor a pinned one
        // below them.
        assert_eq!(insertion_slot(&centers, 0.0, &pinned, 3), 2);
        assert_eq!(insertion_slot(&centers, 50.0, &pinned, 1), 2);
        // Without pins nothing is clamped.
        let none = [false; 4];
        assert_eq!(insertion_slot(&centers, 0.0, &none, 3), 0);
        assert_eq!(insertion_slot(&centers, 50.0, &none, 0), 4);
    }
}
