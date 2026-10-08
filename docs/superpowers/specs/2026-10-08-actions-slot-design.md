# Actions slot + brand icon

## Goal

Replace the three strip slots (New, Search, Settings) with one **Actions** control that expands into those three side by side. Ship a shared brand mark for the strip control, tray, window, and Windows `.exe` icon.

## Strip interaction

- One Actions slot under the bars on every platform (including macOS).
- **Collapsed:** hollow slot with the brand mark; tooltip “Actions”.
- **Expanded:** horizontal row of New (+) / Search / Settings, right-aligned, short gap between them; same hollow/magnify language as today.
- Hover Actions → expand. Leave the row → collapse. Click does not toggle expand.
- Click an action while expanded → run it. Alt/Option on + still means clipboard note.
- Keyboard shortcuts and tray menu items still open each action directly.
- Panel mutual exclusion (search vs settings vs note) unchanged.

## Layout

- `compute_layout` takes `actions_expanded` instead of `settings_slot`.
- Collapsed content height: bars + gap + one slot.
- Expanded: one row of three slots; panel morph anchors use the matching child, or the Actions slot when collapsed.

## Brand mark

- Sticky note with a short adhesive band and a thin vertical bar on the right (strip cue).
- Peach / warm paper fill for color surfaces (Windows tray, `.exe`, window icon).
- Monochrome silhouette for macOS template tray icon.
- Strip Actions glyph: simplified mark drawn with quads (no new Lucide codepoint required).

## Out of scope

- Changing tray menu structure.
- Remembering expanded state across sessions.
