use iced::font::Weight;
use iced::widget::{text, Text};
use iced::Font;

/// Default text face.
#[allow(dead_code)] // used from Task 3 on
pub const BODY_FONT: Font = Font::with_name("Inter");

/// Semibold face for note titles and headings.
#[allow(dead_code)] // used from Task 3 on
pub const TITLE_FONT: Font = Font {
    weight: Weight::Semibold,
    ..Font::with_name("Inter")
};

/// Lucide icon font.
pub const ICON_FONT: Font = Font::with_name("lucide");

/// Bundled font files: Inter Regular, Italic, SemiBold, Bold, BoldItalic, then Lucide.
pub static FONTS: [&[u8]; 6] = [
    include_bytes!("../assets/fonts/Inter-Regular.subset.ttf"),
    include_bytes!("../assets/fonts/Inter-Italic.subset.ttf"),
    include_bytes!("../assets/fonts/Inter-SemiBold.subset.ttf"),
    include_bytes!("../assets/fonts/Inter-Bold.subset.ttf"),
    include_bytes!("../assets/fonts/Inter-BoldItalic.subset.ttf"),
    include_bytes!("../assets/fonts/lucide.subset.ttf"),
];

/// Icons available in the bundled Lucide subset.
#[allow(dead_code)] // used from Task 3 on
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Trash,
    Close,
    Bold,
    Italic,
    Strike,
    Code,
    Palette,
    Highlight,
    Size,
    Link,
    Image,
    Plus,
    Sliders,
    Check,
    Square,
}

impl Icon {
    /// Every icon in the subset.
    #[allow(dead_code)] // used from Task 3 on
    pub const ALL: [Icon; 15] = [
        Icon::Trash,
        Icon::Close,
        Icon::Bold,
        Icon::Italic,
        Icon::Strike,
        Icon::Code,
        Icon::Palette,
        Icon::Highlight,
        Icon::Size,
        Icon::Link,
        Icon::Image,
        Icon::Plus,
        Icon::Sliders,
        Icon::Check,
        Icon::Square,
    ];

    /// The icon's Private Use Area codepoint in the Lucide font.
    pub fn codepoint(self) -> char {
        match self {
            Icon::Trash => '\u{e18e}',
            Icon::Close => '\u{e1b2}',
            Icon::Bold => '\u{e05d}',
            Icon::Italic => '\u{e0fb}',
            Icon::Strike => '\u{e177}',
            Icon::Code => '\u{e093}',
            Icon::Palette => '\u{e1dd}',
            Icon::Highlight => '\u{e0f4}',
            Icon::Size => '\u{e587}',
            Icon::Link => '\u{e102}',
            Icon::Image => '\u{e0f6}',
            Icon::Plus => '\u{e13d}',
            Icon::Sliders => '\u{e29a}',
            Icon::Check => '\u{e06c}',
            Icon::Square => '\u{e167}',
        }
    }
}

/// A Lucide glyph as text at the given size.
#[allow(dead_code)] // used from Task 3 on
pub fn icon<'a>(icon: Icon, size: f32) -> Text<'a> {
    text(icon.codepoint().to_string())
        .font(ICON_FONT)
        .size(size)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn bundled_inter_fonts_parse_with_expected_styles() {
        let expected = [
            (400, false),
            (400, true),
            (600, false),
            (700, false),
            (700, true),
        ];
        for (bytes, (weight, italic)) in FONTS[..5].iter().zip(expected) {
            let face = ttf_parser::Face::parse(bytes, 0).expect("Inter parses");
            let name = |id| {
                face.names()
                    .into_iter()
                    .find(|n| n.name_id == id)
                    .and_then(|n| n.to_string())
            };
            let family = name(ttf_parser::name_id::TYPOGRAPHIC_FAMILY)
                .or_else(|| name(ttf_parser::name_id::FAMILY));
            assert_eq!(family.as_deref(), Some("Inter"));
            assert_eq!(face.weight().to_number(), weight);
            assert_eq!(face.is_italic(), italic);
        }
    }

    #[test]
    fn every_icon_is_in_the_icon_font() {
        let face = ttf_parser::Face::parse(FONTS[5], 0).expect("icon font parses");
        for icon in Icon::ALL {
            assert!(face.glyph_index(icon.codepoint()).is_some(), "{icon:?}");
        }
    }

    #[test]
    fn icon_font_holds_only_the_listed_icons() {
        let face = ttf_parser::Face::parse(FONTS[5], 0).expect("icon font parses");
        let mut mapped = HashSet::new();
        for table in face.tables().cmap.expect("cmap").subtables {
            table.codepoints(|c| {
                if (0xE000..=0xF8FF).contains(&c) {
                    mapped.insert(c);
                }
            });
        }
        let listed: HashSet<u32> = Icon::ALL.iter().map(|i| i.codepoint() as u32).collect();
        assert_eq!(mapped, listed);
    }

    #[test]
    fn icons_are_distinct() {
        let set: HashSet<char> = Icon::ALL.iter().map(|i| i.codepoint()).collect();
        assert_eq!(set.len(), Icon::ALL.len());
    }
}
