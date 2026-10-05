use crate::note::Note;
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
}
