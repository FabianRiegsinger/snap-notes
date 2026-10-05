use crate::note::Note;
use crate::rich;
use std::ffi::OsString;
use std::io;
use std::path::Path;

/// File format of an export.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExportFormat {
    Markdown,
    Text,
}

impl ExportFormat {
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

/// Makes the note's single line breaks CommonMark hard breaks, so other
/// Markdown apps show them as the note does: a line followed by another
/// non-empty line ends in two spaces. Code blocks (fenced or indented) and
/// lines that already end in a hard break are left alone.
fn hard_breaks(body: &str) -> String {
    let lines: Vec<&str> = body.split('\n').collect();
    let mut out = String::with_capacity(body.len() + lines.len() * 2);
    // The open fence's character (`` ` `` or `~`).
    let mut fence: Option<char> = None;
    let mut indented_code = false;
    let mut prev_blank = true;
    for (i, line) in lines.iter().enumerate() {
        let content = line.strip_suffix('\r').unwrap_or(line);
        let cr = &line[content.len()..];
        let trimmed = content.trim_start_matches([' ', '\t']);
        // In columns: a tab counts as four.
        let indent: usize = content[..content.len() - trimmed.len()]
            .chars()
            .map(|c| if c == '\t' { 4 } else { 1 })
            .sum();
        let blank = trimmed.is_empty();
        let fence_char = (indent <= 3
            && (trimmed.starts_with("```") || trimmed.starts_with("~~~")))
        .then(|| trimmed.chars().next())
        .flatten();
        let code = match (fence, fence_char) {
            (Some(open), Some(c)) if open == c => {
                fence = None;
                true
            }
            (Some(_), _) => true,
            (None, Some(c)) => {
                fence = Some(c);
                true
            }
            (None, None) => {
                // Indented code can't interrupt a paragraph.
                if !blank {
                    indented_code = indent >= 4 && (prev_blank || indented_code);
                }
                indented_code
            }
        };
        let next_has_text = lines.get(i + 1).is_some_and(|next| !next.trim().is_empty());
        let breaks = !code
            && !blank
            && next_has_text
            && !content.ends_with("  ")
            && !content.ends_with('\\');
        out.push_str(content);
        if breaks {
            out.push_str("  ");
        }
        out.push_str(cr);
        if i + 1 < lines.len() {
            out.push('\n');
        }
        prev_blank = blank;
    }
    out
}

/// The notes as one document, in order: each a title and its body, separated
/// by one blank line, ending in a single line break.
pub fn render(notes: &[&Note], format: ExportFormat) -> String {
    let entries: Vec<String> = notes
        .iter()
        .map(|note| {
            let title = title_or_untitled(note);
            let (heading, body) = match format {
                ExportFormat::Markdown => (
                    format!("# {title}"),
                    hard_breaks(&rich::strip_tags(&note.content)),
                ),
                ExportFormat::Text => (
                    format!("{title}\n{}", "=".repeat(title.chars().count())),
                    rich::plain_text(&note.content),
                ),
            };
            // Blank lines go, but not the first line's indentation.
            let body = body.trim_start_matches(['\n', '\r']).trim_end();
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
pub fn suggested_name(date: chrono::NaiveDate, format: ExportFormat) -> String {
    format!(
        "snap-notes-{}.{}",
        date.format("%Y-%m-%d"),
        format.extension()
    )
}

/// Writes `contents` to `path` through `<path>.tmp`, so a failure never
/// leaves a half-written file behind.
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
    fn export_keeps_leading_indentation() {
        let a = note("T", "\n    code\n");
        assert_eq!(render(&[&a], ExportFormat::Markdown), "# T\n\n    code\n");
    }

    #[test]
    fn md_export_keeps_single_line_breaks() {
        let a = note("T", "milk\neggs");
        assert_eq!(
            render(&[&a], ExportFormat::Markdown),
            "# T\n\nmilk  \neggs\n"
        );
        // Not before or after a blank line, nor twice, nor after a `\\` break.
        let b = note("T", "a\n\nb  \nc\\\nd");
        assert_eq!(
            render(&[&b], ExportFormat::Markdown),
            "# T\n\na\n\nb  \nc\\\nd\n"
        );
    }

    #[test]
    fn md_export_leaves_code_fences_alone() {
        let a = note("T", "x\nz\n```\nlet a;\nlet b;\n```\ny\nw");
        assert_eq!(
            render(&[&a], ExportFormat::Markdown),
            "# T\n\nx  \nz  \n```\nlet a;\nlet b;\n```\ny  \nw\n"
        );
        // Nor in an indented code block.
        let b = note("T", "p\n\n    let a;\n    let b;\n\n\tc;\n\td;");
        assert_eq!(
            render(&[&b], ExportFormat::Markdown),
            "# T\n\np\n\n    let a;\n    let b;\n\n\tc;\n\td;\n"
        );
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
