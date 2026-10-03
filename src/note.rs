use chrono::{DateTime, Utc};
use rand::seq::IndexedRandom;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use uuid::Uuid;

#[derive(Debug, Clone, Copy)]
pub struct NoteColor {
    pub rgba: [f32; 4],
}

impl NoteColor {
    pub const fn new(r: f32, g: f32, b: f32) -> Self {
        Self {
            rgba: [r, g, b, 1.0],
        }
    }

    /// Parses `#RRGGBB` (the `#` is optional); `None` for anything else.
    pub fn parse_hex(hex: &str) -> Option<Self> {
        let hex = hex.strip_prefix('#').unwrap_or(hex);
        if hex.len() != 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        let channel = |i: usize| {
            u8::from_str_radix(&hex[i..i + 2], 16)
                .ok()
                .map(|v| v as f32 / 255.0)
        };
        Some(Self::new(channel(0)?, channel(2)?, channel(4)?))
    }

    pub fn to_hex(self) -> String {
        let r = (self.rgba[0] * 255.0).round() as u8;
        let g = (self.rgba[1] * 255.0).round() as u8;
        let b = (self.rgba[2] * 255.0).round() as u8;
        format!("#{:02X}{:02X}{:02X}", r, g, b)
    }

    /// A random color from `palette`, or the first default color if it is empty.
    pub fn random_from(palette: &[NoteColor]) -> Self {
        palette
            .choose(&mut rand::rng())
            .copied()
            .unwrap_or(PALETTE[0])
    }
}

/// Colors are stored as hex, so two colors are equal when their hex is.
impl PartialEq for NoteColor {
    fn eq(&self, other: &Self) -> bool {
        self.to_hex() == other.to_hex()
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
        // A hand-edited, invalid color must not make the whole file unreadable.
        Ok(Self::parse_hex(&s).unwrap_or(PALETTE[0]))
    }
}

pub const PALETTE: [NoteColor; 20] = [
    NoteColor::new(1.0, 0.420, 0.420),   // Coral #FF6B6B
    NoteColor::new(0.969, 0.561, 0.702), // Rose #F78FB3
    NoteColor::new(1.0, 0.710, 0.761),   // Blush #FFB5C2
    NoteColor::new(1.0, 0.627, 0.478),   // Peach #FFA07A
    NoteColor::new(1.0, 0.624, 0.263),   // Tangerine #FF9F43
    NoteColor::new(1.0, 0.851, 0.239),   // Amber #FFD93D
    NoteColor::new(1.0, 0.949, 0.459),   // Lemon #FFF275
    NoteColor::new(0.871, 0.722, 0.529), // Sand #DEB887
    NoteColor::new(0.773, 0.910, 0.424), // Lime #C5E86C
    NoteColor::new(0.529, 0.682, 0.451), // Sage #87AE73
    NoteColor::new(0.420, 0.796, 0.467), // Mint #6BCB77
    NoteColor::new(0.306, 0.804, 0.769), // Teal #4ECDC4
    NoteColor::new(0.498, 0.859, 0.855), // Aqua #7FDBDA
    NoteColor::new(0.271, 0.718, 0.820), // Sky #45B7D1
    NoteColor::new(0.416, 0.612, 0.969), // Cornflower #6A9CF7
    NoteColor::new(0.486, 0.514, 0.992), // Periwinkle #7C83FD
    NoteColor::new(0.725, 0.514, 1.0),   // Lavender #B983FF
    NoteColor::new(0.898, 0.561, 0.878), // Orchid #E58FE0
    NoteColor::new(0.690, 0.537, 0.408), // Mocha #B08968
    NoteColor::new(0.467, 0.533, 0.600), // Slate #778899
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Note {
    pub id: Uuid,
    pub color: NoteColor,
    #[serde(default)]
    pub title: String,
    pub content: String,
    /// Where the user last dragged the open note (top-left, logical pixels
    /// relative to the screen-covering window). `None` opens it by the dock.
    #[serde(default)]
    pub position: Option<[f32; 2]>,
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
            position: None,
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
        let color = NoteColor::parse_hex("#FF6B6B").unwrap();
        assert_eq!(color.to_hex(), "#FF6B6B");
        let json = serde_json::to_string(&color).unwrap();
        let back: NoteColor = serde_json::from_str(&json).unwrap();
        assert_eq!(back.to_hex(), "#FF6B6B");
    }

    #[test]
    fn invalid_hex_is_rejected_without_panicking() {
        for bad in ["#FFF", "red", "#GGGGGG", "", "#FF6B6Bé"] {
            assert_eq!(NoteColor::parse_hex(bad), None, "{bad}");
        }
        assert_eq!(NoteColor::parse_hex("#ff6b6b"), Some(PALETTE[0]));
    }

    #[test]
    fn note_with_invalid_color_still_loads() {
        let json = r##"{"id":"6f1c2c1e-6c1a-4a8e-9a43-2a1f0e0f9b11","color":"#FFF","content":"hi","order":0,"created_at":"2026-10-01T00:00:00Z","updated_at":"2026-10-01T00:00:00Z"}"##;
        let note: Note = serde_json::from_str(json).unwrap();
        assert_eq!(note.content, "hi");
        assert_eq!(note.color, PALETTE[0]);
    }

    #[test]
    fn note_without_title_deserializes() {
        let json = r##"{"id":"6f1c2c1e-6c1a-4a8e-9a43-2a1f0e0f9b11","color":"#FF6B6B","content":"hi","order":0,"created_at":"2026-10-01T00:00:00Z","updated_at":"2026-10-01T00:00:00Z"}"##;
        let note: Note = serde_json::from_str(json).unwrap();
        assert_eq!(note.title, "");
        assert_eq!(note.position, None);
        assert_eq!(note.content, "hi");
    }

    #[test]
    fn palette_has_20_distinct_colors() {
        assert_eq!(PALETTE.len(), 20);
        let hexes: HashSet<_> = PALETTE.iter().map(|c| c.to_hex()).collect();
        assert_eq!(hexes.len(), 20);
    }

    #[test]
    fn palette_hex_comments_match_values() {
        let source = include_str!("note.rs");
        for color in PALETTE {
            assert!(
                source.contains(&color.to_hex()),
                "{} not documented",
                color.to_hex()
            );
        }
    }

    #[test]
    fn random_from_picks_from_given_palette() {
        let palette = [PALETTE[0], PALETTE[5]];
        for _ in 0..50 {
            assert!(palette.contains(&NoteColor::random_from(&palette)));
        }
    }
}
