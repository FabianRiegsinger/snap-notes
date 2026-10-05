# Visual Refresh — Design Spec

## Overview

Make Snap Notes look deliberate and consistent on every platform. Today, sizes, radii, colors and icons are scattered literals:
- emoji icons (🗑 ✕ 🖼 🔗) that render differently per OS
- the title font changes per OS
- one flat shadow
- fully saturated paper
- a hard-coded dark theme

This refresh introduces a small design system (tokens, a light and dark theme, a bundled typeface and an icon font) and applies it to every surface. It also adds motion and polish.

The user asked for all nine suggestions, on a feature branch, with every choice made by the implementer. The decisions below record those choices.

## Decisions

| # | Topic | Decision |
|---|---|---|
| D1 | Dark mode | Follows the system appearance automatically: `iced::system::theme()` at boot and `iced::system::theme_changes()` while running. No setting. |
| D2 | Typeface | **Inter 4.1** (SIL OFL 1.1), bundled with `application.font(..)` and set as the default font. Subset to Latin, Latin Extended-A/B, Greek, Cyrillic, general punctuation, arrows and currency. Five styles: Regular, Italic, SemiBold, Bold, BoldItalic. Code stays on the system monospace. |
| D3 | Icons | **Lucide** (ISC), its icon font subset to the glyphs the app uses (list below). Every interface icon is a Lucide glyph at one size per context. No emoji in the interface. |
| D4 | Type scale | Interface sizes: `XS 12`, `SM 14`, `MD 16`, `LG 20`, `XL 24`. Body line height 1.45. Note titles are MD SemiBold. Rendered headings: H1 = XL, H2 = LG, H3 = MD (they were 22/18/16). The toolbar's content sizes (12/14/20/28) are note content, not interface, and stay as they are. |
| D5 | Tokens | A `src/theme.rs` module owns spacing (`space(n) = 4·n`), radii (`RADIUS_BAR 3`, `RADIUS_CONTROL 4`, `RADIUS_SURFACE 10`), the type scale, shadows and all colors. Literal radii, interface font sizes and colors outside `theme.rs` are replaced by tokens. |
| D6 | Paper | Two shadows: a contact shadow (offset 0,1, blur 2) and an ambient shadow (offset 0,8, blur 24). A 3 % top-to-bottom lightness gradient. A 1 px top highlight. A faint adhesive band across the top 16 px (the grip area). Applies to the open note, the peek and the settings card. |
| D7 | Palette | Two tones per note color. The **bar** keeps the saturated note color. The **paper** is derived from it: light mode mixes toward white and desaturates, dark mode mixes toward a warm near-black. The paper-tint setting (−30 %…+30 %) still applies on top. Ink switches with the mode. Contrast between ink and every paper (all 20 palette colors and all 60 presets, at tint −0.3, 0 and +0.3) must be at least 4.5:1 (WCAG AA), in both modes. |
| D8 | Strip | Bars get a vertical gradient (+6 % lightness at the top) and a 1 px top highlight. While a note is open, its bar is drawn 1.5× wider with the note's ambient shadow, so it reads as the note's tab. |
| D9 | Motion | (a) **Buttons:** a pressed state on every button, darker plus a 1 px downward shift. (b) **Deleting:** after a note (or its peek) folds away, its bar collapses over 180 ms before it is removed. (c) **Toolbar:** fades and slides in (6 px) over 120 ms on entering edit mode. (d) **Mode switch:** the body view that appears (formatted or editor) fades in over 120 ms. All durations scale with the Motion speed setting. |
| D10 | Polish | (a) **Focus:** a 1.5 px focus ring (ink 25 %) on the focused title and editor. (b) **Empty state:** an empty note shows "Start typing… Markdown works." (c) **Scrollbars:** 3 px while idle, 6 px on hover, color-matched to the ink. (d) **Peek arrow:** dropped. The peek already grows out of its bar, so an arrow adds nothing, and iced quads cannot draw triangles without a canvas. |

## Lucide glyphs used

Mapped to their uses in the app:
- **Header and confirmations:** `trash-2` for delete, `x` for close.
- **Toolbar:** `bold`, `italic`, `strikethrough`, `code`, `palette` (text color), `highlighter`, `a-large-small` (size), `link`, `image`.
- **Strip:** `plus` for add, and `sliders-horizontal` for the settings slot. The settings slot stays drawn from quads, because it animates its reveal; the icon is used only in the tray.
- **Spare:** `check` and `square` are subset in, but task checkboxes keep ☐/☑. Those are note content, not interface.

The codepoints come from lucide-static's `font/info.json` at the pinned version, and are recorded in `src/icons.rs`.

## Architecture

### `src/theme.rs` (new)
- **`Mode` enum:** `Light` or `Dark`, mapped from `iced::theme::Mode`, with `Light` as the fallback.
- **`Theme` value:** built from the mode. `App` owns it.
  - **Colors:** `ink(alpha)`, `paper(color, tint)`, `bar(color)`, `danger`, `focus_ring`, `scrollbar(active)` and `surface_card` (the settings card).
  - **Effects:** `shadows() -> (Shadow, Shadow)` and `bar_gradient(color) -> Gradient`.
- **Free functions:** `space`, the radii, the type sizes and `body_line_height`.
- **Color math:** pure helpers (`mix`, `desaturate`, `lightness_shift`, `relative_luminance`, `contrast`), unit-tested.

### `src/icons.rs` (new)
- **`Icon` enum:** one variant per glyph, with `codepoint()`.
- **Drawing helpers:** `icon(Icon) -> Text` for widgets, plus a way to draw an icon from `fill_text` for the canvas-drawn peek.
- **Font:** `ICON_FONT: Font` (the font family named "lucide").

### `assets/fonts/` (new)
- **Font files:** `Inter-{Regular,Italic,SemiBold,Bold,BoldItalic}.subset.ttf` and `lucide.subset.ttf`.
- **Licenses:** `OFL.txt` (Inter) and `LUCIDE-LICENSE.txt`.
- **Script:** `subset.sh` documents the exact `pyftsubset` commands and the source versions, so the subsets are reproducible. The fonts are embedded with `include_bytes!`.

### Theme plumbing
- **`main.rs`:**
  - registers the fonts and sets `.default_font(theme::BODY_FONT)`;
  - the `theme` fn returns an iced `Theme` matching the mode, so default widget colors fit.
- **`App`:** gains a `theme: theme::Theme` field.
  - Boot runs `system::theme()`, which sends `Message::ThemeChanged(Mode)`.
  - The subscription listens to `system::theme_changes()`.
- **Views:** every view takes `&theme::Theme`, or the derived colors, through its existing props structs (`PostIt`, `SettingsView`, `BarStrip`), the toolbar and rich_view. No global state.

### Surfaces
- **Open note (`note_panel.rs`):**
  - paper from `theme.paper`;
  - the layered shadow, from two nested containers that each carry one shadow;
  - a gradient background, the top highlight and the adhesive band;
  - icons for the header buttons;
  - the focus ring;
  - the empty-state placeholder;
  - the scrollbar sizes.
- **Peek (`peek.rs`):** the same paper treatment, drawn with quads (two shadow quads, a gradient fill, a highlight quad). The trash icon from the icon font. Sizes and line height from tokens.
- **Strip (`bar_strip.rs`):** the bar gradient and highlight, the open note's tab bar (D8), and bar collapse (D9b).
- **Toolbar (`toolbar.rs`):** Lucide icons, pressed states, fade and slide-in (D9c).
- **Formatted view (`rich_view.rs`):** heading sizes from the type scale, colors from the theme, the fade-in (D9d).
- **Settings (`settings_panel.rs`, `color_picker.rs`):** the card color and text from the theme (light and dark), tokens for sizes and radii, and the paper treatment for the card.

### Motion state
- **Bar collapse:** `App` keeps `collapsing: Option<(Uuid, Morph)>`. The strip layout scales that bar's height and gap by `1 − t`, and the note is removed from the store when the morph ends. A note being deleted keeps its data until then. Deleting another note meanwhile finishes the current collapse first.
- **Toolbar and mode switch:** `App` keeps one `Morph` for the edit-mode switch, driving the toolbar alpha and offset and the body fade-in. It is restarted on every switch between formatted and editor view.
- **Ticks:** existing frame-tick handling drives all of it (the `animating` flag).

## Error handling
- **Font registration** can't fail at runtime, because the bytes are compiled in. If a glyph is missing, cosmic-text falls back to system fonts.
- **Theme detection:** if it fails or returns an unknown mode, light is used.

## Testing
- **`theme.rs`:**
  - color math (mix, luminance and contrast against known WCAG values);
  - the contrast guarantee of D7 over all palette and preset colors, at three tints, in both modes;
  - the radius and type-scale tokens.
- **`icons.rs`:** every `Icon` codepoint exists in the bundled icon font (read the font's cmap with `ttf-parser` in a test — a dev-dependency), and that font contains no extra glyphs beyond those listed.
- **Fonts:** a test that every bundled Inter file parses and has the expected family and style names.
- **App motion:**
  - deleting a note starts a collapse;
  - the note stays until the collapse ends, then is gone;
  - a peek delete also collapses;
  - the edit-mode morph restarts on each switch.
- **Strip layout:** a collapsing bar's height scales with `1 − t`.
- **Manual (with the run skill):** both modes and switching live, every surface and icon, the focus ring, the empty state, the scrollbars, deleting a note, and toolbar and mode-switch motion.

## README
- **Under the hood:** mention Inter and Lucide (with licenses), and dark mode.
- **Settings table:** add a "Dark mode follows your system" note.
- **Install:** the binary-size note gets the new size.

## Out of scope
- A manual theme setting (D1).
- A peek arrow (D10d).
- Custom content fonts.
- Animations beyond D9.
