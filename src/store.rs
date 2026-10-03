use crate::note::{Note, NoteColor};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use uuid::Uuid;

const DEBOUNCE: Duration = Duration::from_millis(500);

#[derive(Serialize, Deserialize)]
struct StoreFile {
    notes: Vec<Note>,
}

pub struct NoteStore {
    notes: Vec<Note>,
    path: PathBuf,
    dirty: bool,
    last_mark: Option<Instant>,
}

impl NoteStore {
    pub fn load(path: PathBuf) -> Self {
        let notes = fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str::<StoreFile>(&s).ok())
            .map(|f| f.notes)
            .unwrap_or_default();
        Self {
            notes,
            path,
            dirty: false,
            last_mark: None,
        }
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

    pub fn notes(&self) -> &[Note] {
        &self.notes
    }

    pub fn note_mut(&mut self, id: Uuid) -> Option<&mut Note> {
        self.notes.iter_mut().find(|n| n.id == id)
    }

    pub fn add_note(&mut self, palette: &[NoteColor]) -> Uuid {
        let order = self.notes.len();
        let mut note = Note::new(NoteColor::random_from(palette));
        note.order = order;
        let id = note.id;
        self.notes.push(note);
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
    }

    /// Gives every note colored `old` the color `new`. Returns whether any
    /// note changed; the caller marks the store dirty.
    #[allow(dead_code)] // TEMP: used by the settings panel in a later commit.
    pub fn recolor(&mut self, old: NoteColor, new: NoteColor) -> bool {
        let mut changed = false;
        for note in self.notes.iter_mut().filter(|n| n.color == old) {
            note.color = new;
            changed = true;
        }
        changed
    }

    pub fn delete_note(&mut self, id: Uuid) {
        self.notes.retain(|n| n.id != id);
        for (i, note) in self.notes.iter_mut().enumerate() {
            note.order = i;
        }
    }

    pub fn reorder(&mut self, from_index: usize, to_index: usize) {
        let note = self.notes.remove(from_index);
        self.notes.insert(to_index, note);
        for (i, note) in self.notes.iter_mut().enumerate() {
            note.order = i;
        }
    }

    pub fn mark_dirty(&mut self) {
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

    pub fn did_save(&mut self) {
        self.dirty = false;
        self.last_mark = None;
    }
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
    fn debounce_timing() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = NoteStore::load(dir.path().join("n.json"));
        store.add_note(&PALETTE);
        store.mark_dirty();
        assert!(!store.should_save());
    }
}
