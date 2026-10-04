# Rich Text Notes — Design Spec

## Overview

Let notes carry formatting: emphasis, structure, color, highlight, font size, clickable links and inline images. Today a note body is a plain `String` edited with iced 0.14's `text_editor` (`src/note_panel.rs`). iced has no rich text *editor*, so this feature stores the body as Markdown extended with a few custom tags, shows it **rendered** when the note is open, and switches to the existing plain-text editor while the user types.

## Decisions

- **Formatting scope:** bold, italic, strikethrough, inline code, headings, bullet and numbered lists, task checkboxes, block quotes, code blocks, horizontal rules, text color, background highlight, font size, links and images.
- **Editing model:** rendered by default, raw Markdown while editing. Clicking the rendered body enters edit mode; blur or Esc returns to the rendered view. A second Esc folds the note as today. No WYSIWYG editing.
- **Storage:** `Note.content` stays a `String` and now holds Markdown. No schema change and no migration.
- **Color and size syntax:** short custom tags (`{coral}…{/}`, `{bg:amber}…{/}`, `{size:20}…{/}`) plus `==highlight==`.
- **Images:** copied into an `images/` folder next to `notes.json` and referenced by relative path.
- **Peek:** shows plain text with the markup stripped; it does not render formatting.
- **Out of scope:** tables, syntax highlighting inside code blocks, export formats, WYSIWYG editing, remote images.

## Syntax

Standard CommonMark plus pulldown-cmark's strikethrough and task-list extensions:

| Effect | Syntax |
|---|---|
| Bold / italic / strikethrough | `**bold**`, `*italic*`, `~~struck~~` |
| Inline code | `` `code` `` |
| Heading | `# H1` … `### H3` (H4–H6 render as H3) |
| Lists | `- item`, `1. item`, nested by indentation |
| Task | `- [ ] open`, `- [x] done` |
| Quote / rule / code block | `> quote`, `---`, fenced ```` ``` ```` |
| Link | `[text](https://example.com)` or a bare `<https://example.com>` |
| Image | `![alt](images/<uuid>.png)` |
| Text color | `{coral}text{/}` or `{#FF0000}text{/}` |
| Highlight | `{bg:amber}text{/}` or `==text==` (= `{bg:amber}`) |
| Font size | `{size:20}text{/}` |
| Literal brace | `\{` |

### Color names

The palette in `settings.json` stores 20 colors without names, so tag names are **fixed slot names** taken from the default palette, in slot order: `coral, rose, blush, peach, tangerine, amber, lemon, sand, lime, sage, mint, teal, aqua, sky, cornflower, periwinkle, lavender, orchid, mocha, slate`. `{coral}` resolves to whatever color slot 0 currently holds, so tagged text follows palette edits exactly like note bars do. Names are matched case-insensitively. A hex tag (`{#RRGGBB}`, parsed with `NoteColor::parse_hex`) is a fixed color that never follows the palette.

### Tag rules

- `{name}`, `{#hex}`, `{bg:name|#hex}` and `{size:N}` push a style; `{/}` pops the innermost.
- Tags nest: `{coral}{size:20}big red{/} red{/}`.
- An unclosed tag ends at the end of its block; styles never leak across paragraphs, list items or headings.
- A stray `{/}` with nothing open is dropped.
- An unknown tag (`{foo}`, `{size:abc}`) renders as literal text.
- Size is clamped to 8–48.
- Tags are only recognized in text, not inside inline code or code blocks.
- The parser never panics on any input.

Existing notes are already valid Markdown. The one visible change: a line in an old note that starts with `#`, `-`, `1.` or `>` now renders as a heading, list item or quote.

## Modules

### `src/rich.rs` (new)

Pure parsing and text helpers, no iced widgets.

```rust
pub struct Doc { pub blocks: Vec<Block> }

pub enum BlockKind {
    Paragraph,
    Heading(u8),                     // 1..=3
    ListItem { ordered: Option<u64>, depth: usize, task: Option<bool> },
    Quote,
    CodeBlock,
    Image { path: String, alt: String },
    Rule,
}

pub struct Block {
    pub kind: BlockKind,
    pub spans: Vec<Span>,
    pub source_line: usize,          // 0-based line in `content` where the block starts
}

pub struct Span {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    pub strike: bool,
    pub code: bool,
    pub color: Option<NoteColor>,
    pub background: Option<NoteColor>,
    pub size: Option<f32>,
    pub link: Option<String>,
}

pub fn parse(content: &str, palette: &[NoteColor]) -> Doc;
pub fn plain_text(content: &str) -> String;           // for peek
pub fn toggle_task(content: &str, line: usize) -> Option<String>;
pub fn wrap_selection(content: &str, selection: Range<usize>, format: Format) -> (String, Range<usize>);
```

- Uses `pulldown-cmark` directly (`Options::ENABLE_STRIKETHROUGH | ENABLE_TASKLISTS`) with offset iteration to compute `source_line`.
- A small tag scanner runs over each `Text` event and keeps a style stack per block.
- `plain_text` drops tags and Markdown markers, turns images into `🖼` and tasks into `☐` / `☑`.
- `toggle_task` flips `[ ]` / `[x]` on the given line if it is a task item (any indentation, `-`, `*`, `+` or `N.` marker); otherwise `None`.
- `Format` covers `Bold, Italic, Strike, Code, Color(slot), Highlight, Size(f32), Link`. With an empty selection, `wrap_selection` inserts the marker pair and puts the cursor between them.

### `src/images.rs` (new)

```rust
pub fn import(dir: &Path, src: &Path) -> io::Result<String>;      // returns "images/<uuid>.<ext>"
pub fn import_png(dir: &Path, rgba: &[u8], w: u32, h: u32) -> io::Result<String>;
pub fn resolve(dir: &Path, rel: &str) -> Option<PathBuf>;
pub fn sweep(dir: &Path, notes: &[Note]) -> io::Result<usize>;  // files removed
```

`dir` is the directory that holds `notes.json`.

- **Import:** accepts `png`, `jpg`, `jpeg`, `gif`, `webp` (case-insensitive) up to 20 MB. It creates `images/` on demand, copies to a temp file and renames to `images/<uuid>.<ext>`, the same atomic pattern as `store.rs`.
- **Clipboard:** a clipboard image is encoded to PNG by `import_png`.
- **Resolve:** returns a path only for `images/<file>` where `<file>` has no separators, no `..` and is not absolute. URLs and every other path return `None`. This keeps a hand-edited or pasted note from reading arbitrary files and keeps the sweep inside `images/`.
- **Sweep:** runs once at startup. It removes files in `images/` whose name is `<uuid>.<ext>` and that no note's content references. Files with other names are never touched. Deleting a note or cutting an image line therefore keeps the file until the next launch, so text cut and pasted back within a session still works.

### `src/note_panel.rs`

- Panel state gains `editing: bool` and a cached `Doc`.
- **Rendered mode:** a `column` of blocks inside the existing `scrollable`, with the same padding as the editor so text does not jump when switching. Each block is a `mouse_area` around iced `rich_text`, built from `Span`s using `text::Span` size, color, font, highlight and link. Headings scale from the 14 px body size (H1 22, H2 18, H3 16). Lists indent 16 px per depth. Images render with `image` at most the body width, never upscaled; a missing or undecodable file shows a small "image not found" placeholder.
- **Edit mode:** the existing `text_editor` with a toolbar row above it: B, I, S, code, color (palette dropdown reusing `color_picker.rs`), highlight, size (small/normal/large/huge = 12/14/20/28), link, 🖼.
- An empty note opens directly in edit mode, so creating a note works as today.

### `src/app.rs`

New messages:

| Message | Effect |
|---|---|
| `BodyClicked(Option<usize>)` | Enter edit mode, focus the editor, cursor to the start of that source line (`None` = end of text). |
| `EditorBlurred` | Leave edit mode, re-parse. Sent when the title, header or anything outside the note is clicked. |
| `ToggleTask(usize)` | `rich::toggle_task`, mark the store dirty, re-parse. Stays in rendered mode. |
| `LinkClicked(String)` | Open `http`, `https` or `mailto` URLs in the default browser (`open` / `cmd /c start` / `xdg-open`). Other schemes do nothing. |
| `FormatApplied(Format)` | `rich::wrap_selection` on the editor content. |
| `ImageDropped(PathBuf)` | From `window::Event::FileDropped` while a note is open: import, then insert `![](…)` at the cursor (edit mode) or append to the end (rendered mode). |
| `ImagePasted` | On Cmd/Ctrl+V in edit mode: if `arboard` has an image, import it and insert it; otherwise let the editor paste text as usual. |
| `ImagePicked(Option<PathBuf>)` | Result of the 🖼 button's `rfd` file dialog; imports as for a drop. |

Esc handling: in edit mode Esc sends `EditorBlurred`; in rendered mode it folds the note as today. The parse cache is rebuilt on note open, on leaving edit mode and after `ToggleTask`, never per frame.

### `src/peek.rs`

`peek_text` builds its lines from `rich::plain_text(&note.content)` instead of raw `content.lines()`.

## Dependencies

- iced feature `image` (decode and display)
- `pulldown-cmark` (parsing)
- `arboard` (clipboard images)
- `rfd` (file dialog)

The implementation plan measures the release binary before and after. It also checks the Linux CI job for any extra system packages `arboard` or `rfd` need, and updates `.github/workflows/build.yml` and the README accordingly.

## Error Handling

- A failed import (I/O error, unsupported type, over 20 MB) inserts nothing and logs to stderr, matching how save errors are handled today.
- An undecodable or missing image renders the placeholder; the rest of the note renders normally.
- Malformed tags render as literal text.
- A failed browser launch is ignored.

## Testing

TDD; unit tests live next to the code as in the existing modules.

- **`rich.rs`:**
  - every tag type, nesting, unclosed tags, stray `{/}`, unknown tags, `\{`, `==highlight==`
  - slot-name and hex colors, slot names resolving against a modified palette, size clamping
  - tags ignored inside code
  - `source_line` per block type, `toggle_task` on indented, numbered and non-task lines
  - `wrap_selection` with and without a selection, `plain_text`
  - plain old notes parse to the same text
  - a set of hostile inputs that must not panic
- **`images.rs` (with `tempfile`):**
  - import copies under a uuid name, rejects bad extensions and oversized files
  - `resolve` rejects `..`, absolute paths, nested paths and URLs
  - `sweep` removes only unreferenced uuid-named files
- **`peek.rs`:** existing tests updated; new tests for stripped markup.
- **Manual (with the `run` skill):** drop an image, paste an image, use the 🖼 dialog, click a link, tick a checkbox, switch edit to rendered and back, fold and reopen, change a palette slot and see tagged text follow it.

## README

- Add a "Formatting" section with the syntax table.
- Add `images/` to "Your data".
- The portability line becomes "copy the executable, both JSON files and the `images/` folder".
- Add any new Linux build packages.
