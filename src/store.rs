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

    pub fn add_note(&mut self) -> Uuid {
        let order = self.notes.len();
        let mut note = Note::new(NoteColor::random());
        note.order = order;
        let id = note.id;
        self.notes.push(note);
        id
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
        let id = store.add_note();
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
        let a = store.add_note();
        let b = store.add_note();
        let _c = store.add_note();
        store.reorder(0, 2);
        assert_eq!(store.notes()[2].id, a);
        assert_eq!(store.notes()[0].id, b);
    }

    #[test]
    fn debounce_timing() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = NoteStore::load(dir.path().join("n.json"));
        store.add_note();
        store.mark_dirty();
        assert!(!store.should_save());
    }
}
