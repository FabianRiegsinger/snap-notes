use crate::note::Note;
use crate::rich;
use std::ffi::OsString;
use std::io;
use std::path::Path;

/// File format of an export.
#[allow(dead_code)] // used from Task 8
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExportFormat {
    Markdown,
    Text,
}

impl ExportFormat {
    #[allow(dead_code)] // used from Task 8
    pub fn extension(self) -> &'static str {
        match self {
            ExportFormat::Markdown => "md",
            ExportFormat::Text => "txt",
        }
    }
}

fn title_or_untitled(note: &Note) -> &str {
    let title = note.title.trim();
    if title.is_empty() {
        "Untitled"
    } else {
        title
    }
}

/// The notes as one document, in order: each a title and its body, separated
/// by one blank line, ending in a single line break.
#[allow(dead_code)] // used from Task 8
pub fn render(notes: &[&Note], format: ExportFormat) -> String {
    let entries: Vec<String> = notes
        .iter()
        .map(|note| {
            let title = title_or_untitled(note);
            let (heading, body) = match format {
                ExportFormat::Markdown => (format!("# {title}"), rich::strip_tags(&note.content)),
                ExportFormat::Text => (
                    format!("{title}\n{}", "=".repeat(title.chars().count())),
                    rich::plain_text(&note.content),
                ),
            };
            let body = body.trim();
            if body.is_empty() {
                heading
            } else {
                format!("{heading}\n\n{body}")
            }
        })
        .collect();
    if entries.is_empty() {
        return String::new();
    }
    format!("{}\n", entries.join("\n\n"))
}

/// `snap-notes-YYYY-MM-DD.<ext>`.
#[allow(dead_code)] // used from Task 8
pub fn suggested_name(date: chrono::NaiveDate, format: ExportFormat) -> String {
    format!(
        "snap-notes-{}.{}",
        date.format("%Y-%m-%d"),
        format.extension()
    )
}

/// Writes `contents` to `path` through `<path>.tmp`, so a failure never
/// leaves a half-written file behind.
#[allow(dead_code)] // used from Task 8
pub fn write(path: &Path, contents: &str) -> io::Result<()> {
    let mut tmp = OsString::from(path);
    tmp.push(".tmp");
    let tmp = Path::new(&tmp);
    let result = std::fs::write(tmp, contents).and_then(|()| std::fs::rename(tmp, path));
    if result.is_err() {
        let _ = std::fs::remove_file(tmp);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::note::PALETTE;

    fn note(title: &str, content: &str) -> Note {
        let mut n = Note::new(PALETTE[0]);
        n.title = title.into();
        n.content = content.into();
        n
    }

    #[test]
    fn markdown_has_heading_per_note() {
        let (a, b) = (note("Groceries", "milk"), note("", "x"));
        assert_eq!(
            render(&[&a, &b], ExportFormat::Markdown),
            "# Groceries\n\nmilk\n\n# Untitled\n\nx\n"
        );
    }

    #[test]
    fn markdown_strips_custom_tags() {
        let a = note("T", "{coral}red{/} and ==hi== **b** [l](https://x)");
        assert_eq!(
            render(&[&a], ExportFormat::Markdown),
            "# T\n\nred and hi **b** [l](https://x)\n"
        );
    }

    #[test]
    fn text_uses_plain_text_and_underlined_titles() {
        let a = note("Groceries", "**milk**\n- [ ] eggs");
        let b = note("", "x");
        assert_eq!(
            render(&[&a, &b], ExportFormat::Text),
            "Groceries\n=========\n\nmilk\n☐ eggs\n\nUntitled\n========\n\nx\n"
        );
    }

    #[test]
    fn export_empty_and_image_only_notes() {
        let empty = note("E", "");
        let image = note("I", "![a](images/a.png)");
        assert_eq!(render(&[&empty], ExportFormat::Markdown), "# E\n");
        assert_eq!(
            render(&[&empty, &image], ExportFormat::Markdown),
            "# E\n\n# I\n\n![a](images/a.png)\n"
        );
        assert_eq!(render(&[&image], ExportFormat::Text), "I\n=\n\nImage\n");
        assert_eq!(render(&[], ExportFormat::Markdown), "");
    }

    #[test]
    fn suggested_name_has_date_and_extension() {
        let date = chrono::NaiveDate::from_ymd_opt(2026, 10, 5).unwrap();
        assert_eq!(
            suggested_name(date, ExportFormat::Markdown),
            "snap-notes-2026-10-05.md"
        );
        assert_eq!(
            suggested_name(date, ExportFormat::Text),
            "snap-notes-2026-10-05.txt"
        );
    }

    #[test]
    fn write_is_atomic() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("out.md");
        write(&path, "hello").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "hello");
        assert!(!dir.path().join("out.md.tmp").exists());
    }
}
