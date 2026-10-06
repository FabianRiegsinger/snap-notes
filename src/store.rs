use crate::note::{Note, NoteColor};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use uuid::Uuid;

pub(crate) const DEBOUNCE: Duration = Duration::from_millis(500);

#[derive(Serialize, Deserialize)]
struct StoreFile {
    notes: Vec<Note>,
}

/// A deleted note and where it was, so the delete can be undone.
#[derive(Clone, Debug)]
pub struct Deleted {
    pub note: Note,
    pub index: usize,
    /// The member that became its stack's top, if it was a top.
    pub promoted: Option<Uuid>,
}

pub struct NoteStore {
    notes: Vec<Note>,
    path: PathBuf,
    dirty: bool,
    last_mark: Option<Instant>,
    load_failed: bool,
    /// Bumped on every change (or mutable access) to the notes, so derived
    /// state such as search results knows when to recompute.
    revision: u64,
}

impl NoteStore {
    pub fn load(path: PathBuf) -> Self {
        let (notes, load_failed) = match fs::read_to_string(&path) {
            Ok(s) => match serde_json::from_str::<StoreFile>(&s) {
                Ok(f) => (f.notes, false),
                Err(_) => (Vec::new(), true),
            },
            Err(e) => (Vec::new(), e.kind() != io::ErrorKind::NotFound),
        };
        let (mut notes, regrouped) = normalize_stacks(notes);
        // Older tagged notes get a fixed reminder anchor. Repairs made here
        // are saved soon after.
        let mut changed = regrouped;
        for note in &mut notes {
            changed |= crate::reminder::freeze_anchor(note);
        }
        Self {
            notes,
            path,
            dirty: changed,
            last_mark: changed.then(Instant::now),
            load_failed,
            revision: 0,
        }
    }

    /// The notes file exists but could not be read or parsed, so the store
    /// started empty.
    pub fn load_failed(&self) -> bool {
        self.load_failed
    }

    pub fn save(&self) -> io::Result<()> {
        let file = StoreFile {
            notes: self.notes.clone(),
        };
        let json = serde_json::to_string_pretty(&file)?;
        let tmp = self.path.with_extension("json.tmp");
        fs::write(&tmp, &json)?;
        fs::rename(&tmp, &self.path)?;
        Ok(())
    }

    /// The folder holding the notes file (and the note images).
    pub fn dir(&self) -> &Path {
        match self.path.parent() {
            Some(dir) if !dir.as_os_str().is_empty() => dir,
            _ => Path::new("."),
        }
    }

    pub fn notes(&self) -> &[Note] {
        &self.notes
    }

    /// Changes whenever the notes may have changed.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn note_mut(&mut self, id: Uuid) -> Option<&mut Note> {
        self.revision += 1;
        self.notes.iter_mut().find(|n| n.id == id)
    }

    pub fn add_note(&mut self, palette: &[NoteColor]) -> Uuid {
        let order = self.notes.len();
        let mut note = Note::new(NoteColor::random_from(palette));
        note.order = order;
        let id = note.id;
        self.notes.push(note);
        self.revision += 1;
        id
    }

    pub fn seed_templates(&mut self, palette: &[NoteColor]) {
        use rand::seq::IndexedRandom;
        if !self.notes.is_empty() {
            return;
        }
        for (order, color) in palette.choose_multiple(&mut rand::rng(), 3).enumerate() {
            let mut note = Note::new(*color);
            note.order = order;
            self.notes.push(note);
        }
        self.revision += 1;
    }

    /// Gives every note colored `old` the color `new`. Returns whether any
    /// note changed; the caller marks the store dirty.
    pub fn recolor(&mut self, old: NoteColor, new: NoteColor) -> bool {
        self.recolor_many(&[(old, new)])
    }

    /// Applies several `(old, new)` recolors at once: each note moves at most
    /// once, by the first pair matching its original color, so chained pairs
    /// (`a -> b`, `b -> c`) don't feed into each other.
    pub fn recolor_many(&mut self, changes: &[(NoteColor, NoteColor)]) -> bool {
        let mut changed = false;
        self.revision += 1;
        for note in &mut self.notes {
            if let Some((_, new)) = changes.iter().find(|(old, _)| *old == note.color) {
                note.color = *new;
                changed = true;
            }
        }
        changed
    }

    /// Index of the note, and how many members follow it in its stack.
    fn group(&self, id: Uuid) -> Option<(usize, usize)> {
        let at = self.notes.iter().position(|n| n.id == id)?;
        let members = self.notes[at + 1..]
            .iter()
            .take_while(|n| n.stack == Some(id))
            .count();
        Some((at, members))
    }

    fn renumber(&mut self) {
        self.revision += 1;
        for (i, note) in self.notes.iter_mut().enumerate() {
            note.order = i;
        }
    }

    /// Deleting a stack's top promotes its first member to be the new top.
    /// Returns what `restore` needs to undo it, or `None` for an unknown id.
    pub fn delete_note(&mut self, id: Uuid) -> Option<Deleted> {
        let (at, members) = self.group(id)?;
        let mut promoted = None;
        if members > 0 {
            let new_top = self.notes[at + 1].id;
            self.notes[at + 1].stack = None;
            for member in &mut self.notes[at + 2..at + 1 + members] {
                member.stack = Some(new_top);
            }
            promoted = Some(new_top);
        }
        let note = self.notes.remove(at);
        self.renumber();
        Some(Deleted {
            note,
            index: at,
            promoted,
        })
    }

    /// Puts a deleted note back at its index (clamped) and, if it was a stack
    /// top, the promoted note and its members back under it. The store's
    /// invariants are restored if other changes got in the way. The caller
    /// saves.
    pub fn restore(&mut self, deleted: Deleted) {
        let Deleted {
            note,
            index,
            promoted,
        } = deleted;
        let id = note.id;
        if let Some(top) = promoted {
            // Only while the promoted note is still a top of its own.
            if self.notes.iter().any(|n| n.id == top && n.stack.is_none()) {
                for n in &mut self.notes {
                    if n.id == top || n.stack == Some(top) {
                        n.stack = Some(id);
                    }
                }
            }
        }
        self.notes.insert(index.min(self.notes.len()), note);
        let (notes, _) = normalize_stacks(std::mem::take(&mut self.notes));
        self.notes = notes;
        self.renumber();
    }

    /// Pins or unpins a note, keeping pinned notes first: pinning moves it to
    /// the end of the pinned group, unpinning to the start of the unpinned
    /// group. A top note carries its members along (they take its pin state);
    /// a stack member is ignored, as it follows its top.
    pub fn set_pinned(&mut self, id: Uuid, pinned: bool) {
        let Some((at, members)) = self.group(id) else {
            return;
        };
        if self.notes[at].stack.is_some() || self.notes[at].pinned == pinned {
            return;
        }
        let mut group: Vec<Note> = self.notes.drain(at..=at + members).collect();
        for note in &mut group {
            note.pinned = pinned;
        }
        // Both moves land on the boundary between the two groups.
        let boundary = self.notes.iter().take_while(|n| n.pinned).count();
        self.notes.splice(boundary..boundary, group);
        self.renumber();
    }

    /// Moves the note at `from_index` (with its members when it is a stack
    /// top) so it lands at `to_index` of the remaining notes. The target is
    /// clamped to the note's pin group and never lands inside a stack. A stack
    /// member does not move on its own.
    pub fn reorder(&mut self, from_index: usize, to_index: usize) {
        let id = self.notes[from_index].id;
        let Some((at, members)) = self.group(id) else {
            return;
        };
        if self.notes[at].stack.is_some() {
            return;
        }
        let pinned = self.notes[at].pinned;
        let group: Vec<Note> = self.notes.drain(at..=at + members).collect();
        let boundary = self.notes.iter().take_while(|n| n.pinned).count();
        let (lo, hi) = if pinned {
            (0, boundary)
        } else {
            (boundary, self.notes.len())
        };
        let mut to = to_index.clamp(lo, hi);
        // Inside a stack, snap to its edge in the direction of travel.
        while to > lo && to < hi && self.notes[to].stack.is_some() {
            if to_index > from_index {
                to += 1;
            } else {
                to -= 1;
            }
        }
        self.notes.splice(to..to, group);
        self.renumber();
    }

    /// Puts `id` under the stack whose top is `onto_top`, right after the
    /// stack's last member. Stacking a top that has members moves the whole
    /// group (nested stacks are not supported). The note takes the stack's pin
    /// state. Returns false (and changes nothing) for a missing note, a note
    /// stacked on itself, or a target that is itself a stack member.
    pub fn stack(&mut self, id: Uuid, onto_top: Uuid) -> bool {
        if id == onto_top {
            return false;
        }
        let (Some((at, members)), Some((onto_at, _))) = (self.group(id), self.group(onto_top))
        else {
            return false;
        };
        if self.notes[onto_at].stack.is_some() {
            return false;
        }
        let pinned = self.notes[onto_at].pinned;
        let mut group: Vec<Note> = self.notes.drain(at..=at + members).collect();
        for note in &mut group {
            note.pinned = pinned;
            note.stack = Some(onto_top);
        }
        let (onto_at, onto_members) = self.group(onto_top).expect("target is still there");
        let end = onto_at + 1 + onto_members;
        self.notes.splice(end..end, group);
        self.renumber();
        true
    }

    /// Dissolves the stack under `top`: its members stay right after it, now
    /// as top-level notes.
    pub fn unstack(&mut self, top: Uuid) {
        for note in &mut self.notes {
            if note.stack == Some(top) {
                note.stack = None;
            }
        }
        self.revision += 1;
    }

    pub fn mark_dirty(&mut self) {
        self.revision += 1;
        self.dirty = true;
        self.last_mark = Some(Instant::now());
    }

    pub fn should_save(&self) -> bool {
        self.dirty
            && self
                .last_mark
                .map(|t| t.elapsed() >= DEBOUNCE)
                .unwrap_or(false)
    }

    #[cfg(test)]
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn did_save(&mut self) {
        self.dirty = false;
        self.last_mark = None;
    }
}

/// Makes a hand-edited or damaged file's stacks valid: a note stacked on a
/// missing note, or on a note that is itself stacked, becomes top-level, and
/// each stack's members move right after its top (in their order) and take
/// its pin state. Pinned stacks and notes then come first, in their order.
/// Also returns whether anything changed.
fn normalize_stacks(notes: Vec<Note>) -> (Vec<Note>, bool) {
    let shape = |notes: &[Note]| -> Vec<_> {
        notes
            .iter()
            .map(|n| (n.id, n.stack, n.pinned, n.order))
            .collect()
    };
    let before = shape(&notes);
    let tops: HashSet<Uuid> = notes
        .iter()
        .filter(|n| n.stack.is_none())
        .map(|n| n.id)
        .collect();
    let (mut ordered, mut members): (Vec<Note>, Vec<Note>) = notes
        .into_iter()
        .map(|mut n| {
            if n.stack.is_some_and(|top| !tops.contains(&top)) {
                n.stack = None;
            }
            n
        })
        .partition(|n| n.stack.is_none());
    let regrouped = !members.is_empty();
    let mut i = 0;
    while i < ordered.len() && !members.is_empty() {
        let top = ordered[i].id;
        let (mine, rest): (Vec<Note>, Vec<Note>) =
            members.into_iter().partition(|n| n.stack == Some(top));
        members = rest;
        let count = mine.len();
        ordered.splice(i + 1..i + 1, mine);
        i += 1 + count;
    }
    let mut pinned = false;
    for note in &mut ordered {
        match note.stack {
            None => pinned = note.pinned,
            Some(_) => note.pinned = pinned,
        }
    }
    let pinned_first = ordered.windows(2).all(|w| w[0].pinned || !w[1].pinned);
    if !pinned_first {
        // Stable, so each stack stays together and in order.
        ordered.sort_by_key(|n| !n.pinned);
    }
    if regrouped || !pinned_first {
        for (order, note) in ordered.iter_mut().enumerate() {
            note.order = order;
        }
    }
    let changed = shape(&ordered) != before;
    (ordered, changed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::note::PALETTE;
    use std::path::PathBuf;

    #[test]
    fn load_missing_file_returns_empty_store() {
        let store = NoteStore::load(PathBuf::from("/nonexistent/notes.json"));
        assert!(store.notes().is_empty());
    }

    #[test]
    fn corrupt_file_sets_load_failed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("notes.json");
        fs::write(&path, "{not json").unwrap();
        let store = NoteStore::load(path);
        assert!(store.load_failed());
        assert!(store.notes().is_empty());
    }

    #[test]
    fn missing_file_does_not_set_load_failed() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!NoteStore::load(dir.path().join("notes.json")).load_failed());
    }

    #[test]
    fn add_save_load_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("notes.json");
        let mut store = NoteStore::load(path.clone());
        let id = store.add_note(&PALETTE);
        store.note_mut(id).unwrap().content = "Hello".into();
        store.save().unwrap();
        let loaded = NoteStore::load(path);
        assert_eq!(loaded.notes().len(), 1);
        assert_eq!(loaded.notes()[0].content, "Hello");
    }

    #[test]
    fn orphan_members_are_normalized_on_load() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("notes.json");
        let mut store = NoteStore::load(path.clone());
        let id: Vec<Uuid> = (0..4).map(|_| store.add_note(&PALETTE)).collect();
        store.note_mut(id[1]).unwrap().stack = Some(Uuid::new_v4());
        store.note_mut(id[2]).unwrap().stack = Some(id[0]);
        // Points at a member, not a top.
        store.note_mut(id[3]).unwrap().stack = Some(id[2]);
        store.save().unwrap();
        let loaded = NoteStore::load(path);
        // The member moves right after its top.
        assert_eq!(ids(&loaded), vec![id[0], id[2], id[1], id[3]]);
        let stacks: Vec<_> = loaded.notes().iter().map(|n| n.stack).collect();
        assert_eq!(stacks, vec![None, Some(id[0]), None, None]);
        let orders: Vec<_> = loaded.notes().iter().map(|n| n.order).collect();
        assert_eq!(orders, vec![0, 1, 2, 3]);
        // The repair is saved soon after.
        assert!(loaded.is_dirty() && loaded.last_mark.is_some());
    }

    #[test]
    fn valid_stacks_load_clean() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("notes.json");
        let mut store = NoteStore::load(path.clone());
        let id: Vec<Uuid> = (0..3).map(|_| store.add_note(&PALETTE)).collect();
        assert!(store.stack(id[2], id[0]));
        store.set_pinned(id[0], true);
        store.save().unwrap();
        let loaded = NoteStore::load(path);
        assert_eq!(ids(&loaded), ids(&store));
        assert!(!loaded.is_dirty() && loaded.last_mark.is_none());
    }

    #[test]
    fn load_restores_pin_invariants() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("notes.json");
        let mut store = NoteStore::load(path.clone());
        let id: Vec<Uuid> = (0..4).map(|_| store.add_note(&PALETTE)).collect();
        // A pinned member of an unpinned top, and a pinned stack last.
        store.note_mut(id[1]).unwrap().stack = Some(id[0]);
        store.note_mut(id[1]).unwrap().pinned = true;
        store.note_mut(id[2]).unwrap().pinned = true;
        store.note_mut(id[3]).unwrap().stack = Some(id[2]);
        store.save().unwrap();
        let loaded = NoteStore::load(path);
        assert_eq!(ids(&loaded), vec![id[2], id[3], id[0], id[1]]);
        let pinned: Vec<_> = loaded.notes().iter().map(|n| n.pinned).collect();
        assert_eq!(pinned, vec![true, true, false, false]);
        let orders: Vec<_> = loaded.notes().iter().map(|n| n.order).collect();
        assert_eq!(orders, vec![0, 1, 2, 3]);
        assert!(loaded.is_dirty() && loaded.last_mark.is_some());
    }

    #[test]
    fn reorder_moves_note() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = NoteStore::load(dir.path().join("n.json"));
        let a = store.add_note(&PALETTE);
        let b = store.add_note(&PALETTE);
        let _c = store.add_note(&PALETTE);
        store.reorder(0, 2);
        assert_eq!(store.notes()[2].id, a);
        assert_eq!(store.notes()[0].id, b);
    }

    fn ids(store: &NoteStore) -> Vec<Uuid> {
        store.notes().iter().map(|n| n.id).collect()
    }

    fn store_of(n: usize) -> (NoteStore, Vec<Uuid>, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let mut store = NoteStore::load(dir.path().join("n.json"));
        let ids = (0..n).map(|_| store.add_note(&PALETTE)).collect();
        (store, ids, dir)
    }

    #[test]
    fn pinning_keeps_pinned_first() {
        let (mut store, id, _dir) = store_of(4);
        let rev = store.revision();
        store.set_pinned(id[2], true);
        assert!(store.revision() > rev);
        assert_eq!(ids(&store), vec![id[2], id[0], id[1], id[3]]);
        store.set_pinned(id[3], true);
        assert_eq!(ids(&store), vec![id[2], id[3], id[0], id[1]]);
        store.set_pinned(id[2], false);
        assert_eq!(ids(&store), vec![id[3], id[2], id[0], id[1]]);
        assert!(store.notes()[0].pinned && !store.notes()[1].pinned);
        let orders: Vec<_> = store.notes().iter().map(|n| n.order).collect();
        assert_eq!(orders, vec![0, 1, 2, 3]);
    }

    #[test]
    fn reorder_stays_within_pin_group() {
        let (mut store, id, _dir) = store_of(4);
        store.set_pinned(id[0], true);
        store.set_pinned(id[1], true);
        // pinned: 0, 1; unpinned: 2, 3
        store.reorder(0, 3);
        assert_eq!(ids(&store), vec![id[1], id[0], id[2], id[3]]);
        store.reorder(3, 0);
        assert_eq!(ids(&store), vec![id[1], id[0], id[3], id[2]]);
    }

    #[test]
    fn stack_and_unstack() {
        let (mut store, id, _dir) = store_of(4);
        let rev = store.revision();
        assert!(store.stack(id[3], id[0]));
        assert!(store.revision() > rev);
        assert_eq!(ids(&store), vec![id[0], id[3], id[1], id[2]]);
        assert!(store.stack(id[1], id[0]));
        assert_eq!(ids(&store), vec![id[0], id[3], id[1], id[2]]);
        assert_eq!(store.notes()[1].stack, Some(id[0]));
        assert_eq!(store.notes()[2].stack, Some(id[0]));
        assert!(!store.stack(id[0], id[0]));
        assert!(!store.stack(id[2], id[1]), "target is a member");
        // A top with members moves as a group under another top.
        assert!(store.stack(id[0], id[2]));
        assert_eq!(ids(&store), vec![id[2], id[0], id[3], id[1]]);
        assert!(store.notes().iter().skip(1).all(|n| n.stack == Some(id[2])));
        store.unstack(id[2]);
        assert_eq!(ids(&store), vec![id[2], id[0], id[3], id[1]]);
        assert!(store.notes().iter().all(|n| n.stack.is_none()));
    }

    #[test]
    fn stacking_adopts_pin_and_reorder_skips_stacks() {
        let (mut store, id, _dir) = store_of(4);
        store.set_pinned(id[0], true);
        assert!(store.stack(id[3], id[0]));
        assert!(store.notes()[1].pinned);
        store.stack(id[2], id[1]);
        // [0 (pinned), 3, 1, 2]; moving 0's group never lands inside 1's stack
        store.reorder(2, 3);
        assert_eq!(ids(&store).len(), 4);
        let at = |i: usize| store.notes().iter().position(|n| n.id == id[i]).unwrap();
        assert_eq!(at(2), at(1) + 1);
    }

    #[test]
    fn deleting_stack_top_promotes_member() {
        let (mut store, id, _dir) = store_of(4);
        store.stack(id[1], id[0]);
        store.stack(id[2], id[0]);
        store.delete_note(id[0]);
        assert_eq!(ids(&store), vec![id[1], id[2], id[3]]);
        assert_eq!(store.notes()[0].stack, None);
        assert_eq!(store.notes()[1].stack, Some(id[1]));
        assert_eq!(store.notes()[2].stack, None);
    }

    /// Everything about a note, to compare whole notes.
    fn full(note: &Note) -> impl PartialEq + std::fmt::Debug {
        (
            (note.id, note.stack, note.pinned, note.order),
            (
                note.title.clone(),
                note.content.clone(),
                note.color.to_hex(),
            ),
            (note.position, note.size),
            (note.reminder_fired, note.reminder_set_at),
            (note.created_at, note.updated_at),
        )
    }

    fn snapshot(store: &NoteStore) -> Vec<impl PartialEq + std::fmt::Debug> {
        store.notes().iter().map(full).collect()
    }

    /// Gives every note distinct contents, so a mix-up shows.
    fn fill(store: &mut NoteStore) {
        let ids = ids(store);
        for (i, id) in ids.into_iter().enumerate() {
            let note = store.note_mut(id).unwrap();
            note.title = format!("Note {i} @15:00");
            note.content = format!("body {i}");
            note.size = Some([200.0 + i as f32, 150.0]);
            note.position = Some([i as f32, 0.0]);
            note.reminder_set_at = Some(chrono::Utc::now());
            note.reminder_fired = (i % 2 == 0).then(chrono::Utc::now);
        }
    }

    /// Pinned notes come first and every member follows its top.
    fn assert_invariants(store: &NoteStore) {
        let notes = store.notes();
        assert!(notes.windows(2).all(|w| w[0].pinned || !w[1].pinned));
        assert!(notes.iter().enumerate().all(|(i, n)| n.order == i));
        for (i, note) in notes.iter().enumerate() {
            if let Some(top) = note.stack {
                let prev = &notes[i - 1];
                assert!(prev.id == top || prev.stack == Some(top), "{i}");
                assert!(notes.iter().any(|n| n.id == top && n.stack.is_none()));
            }
        }
    }

    /// Deletes `id` and restores it, expecting every note as it was.
    fn assert_round_trip(store: &mut NoteStore, id: Uuid) -> Deleted {
        let before = snapshot(store);
        let deleted = store.delete_note(id).unwrap();
        assert_invariants(store);
        store.restore(deleted.clone());
        assert_eq!(snapshot(store), before);
        deleted
    }

    #[test]
    fn delete_restore_round_trips_plain_note() {
        let (mut store, id, _dir) = store_of(3);
        fill(&mut store);
        let deleted = assert_round_trip(&mut store, id[1]);
        assert_eq!((deleted.index, deleted.promoted), (1, None));
        assert!(store.delete_note(Uuid::new_v4()).is_none());
    }

    #[test]
    fn delete_restore_round_trips_stack_top() {
        let (mut store, id, _dir) = store_of(4);
        store.stack(id[1], id[0]);
        store.stack(id[2], id[0]);
        fill(&mut store);
        let deleted = assert_round_trip(&mut store, id[0]);
        assert_eq!(deleted.promoted, Some(id[1]));
        let stacks: Vec<_> = store.notes().iter().map(|n| n.stack).collect();
        assert_eq!(stacks, vec![None, Some(id[0]), Some(id[0]), None]);
    }

    #[test]
    fn delete_restore_round_trips_member() {
        let (mut store, id, _dir) = store_of(3);
        store.stack(id[1], id[0]);
        store.stack(id[2], id[0]);
        fill(&mut store);
        let deleted = assert_round_trip(&mut store, id[1]);
        assert_eq!(deleted.promoted, None);
    }

    #[test]
    fn delete_restore_round_trips_pinned() {
        let (mut store, id, _dir) = store_of(3);
        store.set_pinned(id[2], true);
        fill(&mut store);
        let deleted = assert_round_trip(&mut store, id[2]);
        assert_eq!(deleted.index, 0);
        assert_eq!(ids(&store), vec![id[2], id[0], id[1]]);
    }

    #[test]
    fn delete_restore_round_trips_pinned_stack_top() {
        let (mut store, id, _dir) = store_of(4);
        store.stack(id[1], id[0]);
        store.stack(id[2], id[0]);
        store.set_pinned(id[0], true);
        store.set_pinned(id[3], true);
        fill(&mut store);
        let deleted = assert_round_trip(&mut store, id[0]);
        assert_eq!(deleted.promoted, Some(id[1]));
        assert!(store.notes()[..3].iter().all(|n| n.pinned));
    }

    #[test]
    fn restore_after_promoted_note_deleted() {
        let (mut store, id, _dir) = store_of(4);
        store.stack(id[1], id[0]);
        store.stack(id[2], id[0]);
        fill(&mut store);
        let deleted = store.delete_note(id[0]).unwrap();
        // The promoted note goes too; its member is promoted in turn.
        store.delete_note(id[1]).unwrap();
        store.restore(deleted);
        assert_invariants(&store);
        assert_eq!(ids(&store), vec![id[0], id[2], id[3]]);
        let restored = store.notes().iter().find(|n| n.id == id[0]).unwrap();
        assert_eq!(restored.content, "body 0");
    }

    #[test]
    fn restore_after_promoted_note_restacked() {
        let (mut store, id, _dir) = store_of(4);
        store.stack(id[1], id[0]);
        store.stack(id[2], id[0]);
        fill(&mut store);
        let deleted = store.delete_note(id[0]).unwrap();
        // The promoted note and its member move under another stack.
        assert!(store.stack(id[1], id[3]));
        store.restore(deleted);
        assert_invariants(&store);
        assert_eq!(store.notes().len(), 4);
        let at = |i: usize| store.notes().iter().find(|n| n.id == id[i]).unwrap();
        assert_eq!(at(0).stack, None);
        assert_eq!(at(1).stack, Some(id[3]));
        assert_eq!(at(2).stack, Some(id[3]));
    }

    #[test]
    fn restore_after_add_keeps_invariants() {
        let (mut store, id, _dir) = store_of(3);
        store.set_pinned(id[0], true);
        let deleted = store.delete_note(id[0]).unwrap();
        // Another note gets pinned meanwhile, and the list shrinks past index.
        store.set_pinned(id[1], true);
        let extra = store.add_note(&PALETTE);
        store.restore(deleted);
        let notes = store.notes();
        assert!(notes.windows(2).all(|w| w[0].pinned || !w[1].pinned));
        assert_eq!(notes.len(), 4);
        assert!(notes.iter().enumerate().all(|(i, n)| n.order == i));
        assert!(ids(&store).contains(&extra));
        // A stale index past the end is clamped.
        let deleted = store.delete_note(id[2]).unwrap();
        let mut stale = deleted.clone();
        stale.index = 99;
        store.restore(stale);
        assert_eq!(store.notes().len(), 4);
    }

    #[test]
    fn seed_templates_fills_empty_store_with_three_distinct_blank_notes() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = NoteStore::load(dir.path().join("n.json"));
        store.seed_templates(&PALETTE);
        let notes = store.notes();
        assert_eq!(notes.len(), 3);
        assert!(notes.iter().all(|n| n.content.is_empty()));
        assert_eq!(
            notes.iter().map(|n| n.order).collect::<Vec<_>>(),
            vec![0, 1, 2]
        );
        let mut colors: Vec<String> = notes.iter().map(|n| n.color.to_hex()).collect();
        colors.sort();
        colors.dedup();
        assert_eq!(colors.len(), 3);
    }

    #[test]
    fn seed_templates_leaves_existing_notes_alone() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = NoteStore::load(dir.path().join("n.json"));
        let id = store.add_note(&PALETTE);
        store.seed_templates(&PALETTE);
        assert_eq!(store.notes().len(), 1);
        assert_eq!(store.notes()[0].id, id);
    }

    #[test]
    fn recolor_changes_only_matching_notes() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = NoteStore::load(dir.path().join("n.json"));
        let (a, b, c) = (PALETTE[0], PALETTE[1], PALETTE[2]);
        for color in [a, a, b] {
            let id = store.add_note(&PALETTE);
            store.note_mut(id).unwrap().color = color;
        }
        assert!(store.recolor(a, c));
        let colors: Vec<_> = store.notes().iter().map(|n| n.color).collect();
        assert_eq!(colors, vec![c, c, b]);
        assert!(!store.recolor(a, c));
    }

    #[test]
    fn recolor_many_maps_each_note_once() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = NoteStore::load(dir.path().join("n.json"));
        let (x, y, z) = (PALETTE[0], PALETTE[1], PALETTE[2]);
        for color in [x, y] {
            let id = store.add_note(&PALETTE);
            store.note_mut(id).unwrap().color = color;
        }
        assert!(store.recolor_many(&[(x, y), (y, z)]));
        let colors: Vec<_> = store.notes().iter().map(|n| n.color).collect();
        assert_eq!(colors, vec![y, z]);
    }

    #[test]
    fn dir_is_the_notes_file_folder() {
        let dir = tempfile::tempdir().unwrap();
        let store = NoteStore::load(dir.path().join("n.json"));
        assert_eq!(store.dir(), dir.path());
        assert_eq!(
            NoteStore::load(PathBuf::from("n.json")).dir(),
            Path::new(".")
        );
    }

    #[test]
    fn debounce_timing() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = NoteStore::load(dir.path().join("n.json"));
        store.add_note(&PALETTE);
        store.mark_dirty();
        assert!(!store.should_save());
    }
}
