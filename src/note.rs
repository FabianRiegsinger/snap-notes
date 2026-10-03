use chrono::{DateTime, Utc};
use rand::seq::IndexedRandom;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NoteColor {
    pub rgba: [f32; 4],
}

impl NoteColor {
    pub const fn new(r: f32, g: f32, b: f32) -> Self {
        Self {
            rgba: [r, g, b, 1.0],
        }
    }

    pub fn from_hex(hex: &str) -> Self {
        let hex = hex.trim_start_matches('#');
        let r = u8::from_str_radix(&hex[0..2], 16).unwrap() as f32 / 255.0;
        let g = u8::from_str_radix(&hex[2..4], 16).unwrap() as f32 / 255.0;
        let b = u8::from_str_radix(&hex[4..6], 16).unwrap() as f32 / 255.0;
        Self {
            rgba: [r, g, b, 1.0],
        }
    }

    pub fn to_hex(self) -> String {
        let r = (self.rgba[0] * 255.0).round() as u8;
        let g = (self.rgba[1] * 255.0).round() as u8;
        let b = (self.rgba[2] * 255.0).round() as u8;
        format!("#{:02X}{:02X}{:02X}", r, g, b)
    }

    pub fn random() -> Self {
        let mut rng = rand::rng();
        *PALETTE.choose(&mut rng).unwrap()
    }
}

impl Serialize for NoteColor {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_hex())
    }
}

impl<'de> Deserialize<'de> for NoteColor {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Ok(Self::from_hex(&s))
    }
}

pub const PALETTE: [NoteColor; 12] = [
    NoteColor::new(1.0, 0.420, 0.420),   // Coral #FF6B6B
    NoteColor::new(1.0, 0.627, 0.478),   // Peach #FFA07A
    NoteColor::new(1.0, 0.851, 0.239),   // Amber #FFD93D
    NoteColor::new(0.420, 0.796, 0.467), // Mint #6BCB77
    NoteColor::new(0.306, 0.804, 0.769), // Teal #4ECDC4
    NoteColor::new(0.271, 0.718, 0.820), // Sky #45B7D1
    NoteColor::new(0.486, 0.514, 0.992), // Periwinkle #7C83FD
    NoteColor::new(0.725, 0.514, 1.0),   // Lavender #B983FF
    NoteColor::new(0.969, 0.561, 0.702), // Rose #F78FB3
    NoteColor::new(0.467, 0.533, 0.600), // Slate #778899
    NoteColor::new(0.871, 0.722, 0.529), // Sand #DEB887
    NoteColor::new(0.529, 0.682, 0.451), // Sage #87AE73
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Note {
    pub id: Uuid,
    pub color: NoteColor,
    #[serde(default)]
    pub title: String,
    pub content: String,
    pub order: usize,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Note {
    pub fn new(color: NoteColor) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            color,
            title: String::new(),
            content: String::new(),
            order: 0,
            created_at: now,
            updated_at: now,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn note_color_roundtrip_hex() {
        let color = NoteColor::from_hex("#FF6B6B");
        assert_eq!(color.to_hex(), "#FF6B6B");
        let json = serde_json::to_string(&color).unwrap();
        let back: NoteColor = serde_json::from_str(&json).unwrap();
        assert_eq!(back.to_hex(), "#FF6B6B");
    }

    #[test]
    fn note_without_title_deserializes() {
        let json = r##"{"id":"6f1c2c1e-6c1a-4a8e-9a43-2a1f0e0f9b11","color":"#FF6B6B","content":"hi","order":0,"created_at":"2026-10-01T00:00:00Z","updated_at":"2026-10-01T00:00:00Z"}"##;
        let note: Note = serde_json::from_str(json).unwrap();
        assert_eq!(note.title, "");
        assert_eq!(note.content, "hi");
    }

    #[test]
    fn palette_has_12_distinct_colors() {
        assert_eq!(PALETTE.len(), 12);
        let hexes: HashSet<_> = PALETTE.iter().map(|c| c.to_hex()).collect();
        assert_eq!(hexes.len(), 12);
    }

    #[test]
    fn random_color_is_from_palette() {
        for _ in 0..50 {
            let c = NoteColor::random();
            assert!(PALETTE.iter().any(|p| p.to_hex() == c.to_hex()));
        }
    }
}
