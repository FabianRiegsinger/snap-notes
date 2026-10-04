# Visual Refresh Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give Snap Notes a consistent design system (tokens, light/dark theme following the system, bundled Inter and Lucide) and apply it, with motion and polish, to every surface.

**Architecture:** A pure `theme.rs` owns tokens, color math and the light/dark `Theme`; `icons.rs` maps a Lucide subset; fonts are subset, committed under `assets/fonts/` and compiled in. `App` holds the `Theme` (from `iced::system`), passes it to every view through existing props, and drives new motion with `Morph`.

**Tech Stack:** Rust 2021, iced 0.14 (`system::theme`, `system::theme_changes`, gradients, `application.font`), fonttools `pyftsubset` (asset build only), `ttf-parser` (dev-dependency, tests only).

**Spec:** `docs/superpowers/specs/2026-10-04-visual-refresh-design.md`

## Global Constraints

- Dark mode follows the system only; no setting. Unknown or failed detection → Light.
- Inter 4.1 (SIL OFL 1.1), styles Regular, Italic, SemiBold, Bold, BoldItalic; subset ranges `U+0000-024F,U+0370-03FF,U+0400-04FF,U+2000-206F,U+20A0-20CF,U+2190-21FF`. Code stays on the system monospace.
- Lucide (ISC) subset to exactly: `trash-2, x, bold, italic, strikethrough, code, palette, highlighter, a-large-small, link, image, plus, sliders-horizontal, check, square`.
- Type scale: XS 12, SM 14, MD 16, LG 20, XL 24; body line height 1.45; note titles MD SemiBold; rendered H1 24, H2 20, H3 16. Toolbar content sizes 12/14/20/28 unchanged.
- Tokens: `space(n) = 4·n`; radii BAR 3, CONTROL 4, SURFACE 10.
- Paper: contact shadow (0,1) blur 2 + ambient shadow (0,8) blur 24; 3 % top-to-bottom lightness gradient; 1 px top highlight; adhesive band over the top 16 px.
- Ink/paper contrast ≥ 4.5:1 for all 20 `PALETTE` and 60 `PRESETS` colors at tint −0.3, 0, +0.3, in both modes.
- Bars: +6 % lightness gradient at the top and a 1 px top highlight; the open note's bar is 1.5× wide with the ambient shadow.
- Motion: pressed state darker + 1 px down on every button; bar collapse 180 ms after delete; toolbar fade + 6 px slide 120 ms; body fade-in 120 ms on mode switch; all scaled by the Motion speed setting.
- Polish: 1.5 px focus ring (ink 25 %) on focused title/editor; empty note placeholder `Start typing… Markdown works.`; scrollbar 3 px idle, 6 px hovered, ink-colored.
- No emoji in interface chrome after Task 4–7 (☐/☑ task markers are note content and stay).
- CI: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test --bin snap-notes` green after every task.
- Comments and naming follow the surrounding code; no section banners.

## Review Focus

1. Theme switching live while a note is open or mid-morph → every surface recolors on the next frame, no stale colors. Test in Task 2 (`theme_change_updates_app_theme`).
2. A user palette color (custom preset) that is very dark or very light → paper still meets 4.5:1. Covered by Task 2's exhaustive contrast test over `PRESETS`.
3. Deleting a second note while the first bar is still collapsing → first collapse finishes immediately, both notes end up deleted, no index mix-up. Test in Task 8 (`second_delete_finishes_running_collapse`).
4. Switching mode repeatedly faster than 120 ms → fade restarts cleanly, never stuck invisible. Test in Task 8 (`mode_switch_fade_reaches_full_opacity`).
5. Text outside the Inter subset (CJK, emoji in notes) → still renders through system fallback; nothing missing or crashing. Manual check in Task 9 (note with `漢字 😀`).

---

### Task 1: Fonts and icons

**Files:**
- Create: `assets/fonts/subset.sh`, `assets/fonts/Inter-{Regular,Italic,SemiBold,Bold,BoldItalic}.subset.ttf`, `assets/fonts/lucide.subset.ttf`, `assets/fonts/OFL.txt`, `assets/fonts/LUCIDE-LICENSE.txt`, `src/icons.rs`
- Modify: `src/main.rs` (`mod icons;`, register fonts, `.default_font(..)`), `Cargo.toml` (`[dev-dependencies] ttf-parser = "0.25"`)

**Interfaces:**
- Produces:
  - `pub const BODY_FONT: Font = Font::with_name("Inter")`, `pub const TITLE_FONT: Font` (Inter, `Weight::Semibold`), `pub const ICON_FONT: Font = Font::with_name("lucide")` — in `src/icons.rs` (Task 2 re-exports the text fonts from `theme`).
  - `pub enum Icon { Trash, Close, Bold, Italic, Strike, Code, Palette, Highlight, Size, Link, Image, Plus, Sliders, Check, Square }` with `pub fn codepoint(self) -> char` and `pub const ALL: [Icon; 15]`.
  - `pub fn icon<'a>(icon: Icon, size: f32) -> iced::widget::Text<'a>` (Lucide glyph text, `ICON_FONT`, given size).
  - `pub static FONTS: [&[u8]; 6]` (the six `include_bytes!`) used by `main.rs`.

- [ ] **Step 1: Fetch sources and subset.** Write `subset.sh`:
  - It downloads `https://github.com/rsms/inter/releases/download/v4.1/Inter-4.1.zip` and `lucide-static` (pin the newest version and write it into the script) `font/lucide.ttf`, `font/info.json` and `LICENSE` from unpkg.
  - It runs `pyftsubset` with the Global Constraints ranges for each Inter style (from `extras/ttf/` in the zip), and with `--unicodes` equal to the 15 Lucide codepoints from `info.json` for the icon font.
  - It copies `LICENSE.txt` from the zip as `OFL.txt`.
  - Run it once and commit its outputs. Record the resulting sizes in the report.
- [ ] **Step 2: Write the failing tests** in `src/icons.rs`:
  - `bundled_inter_fonts_parse_with_expected_styles`: each Inter file parses with `ttf_parser::Face::parse`. Family "Inter". Weight 400/400/600/700/700, italic false/true/false/false/true.
  - `every_icon_is_in_the_icon_font`: for each `Icon::ALL`, `face.glyph_index(icon.codepoint()).is_some()`.
  - `icon_font_holds_only_the_listed_icons`: the cmap's mapped codepoints in the Private Use Area equal the `Icon::ALL` set.
  - `icons_are_distinct`.
- [ ] **Step 3: Run them.** `cargo test --bin snap-notes icons::` → compile errors.
- [ ] **Step 4: Implement** `icons.rs` (codepoints from the pinned `info.json`). In `main.rs`, chain `.font(bytes)` for every `FONTS` entry and add `.default_font(icons::BODY_FONT)`.
- [ ] **Step 5: Verify.** `cargo test --bin snap-notes && cargo clippy --all-targets -- -D warnings && cargo fmt --check` → green.
- [ ] **Step 6: Commit.** `feat: bundle Inter and a Lucide icon subset`

### Task 2: Theme, tokens and live dark mode

**Files:**
- Create: `src/theme.rs`
- Modify: `src/main.rs` (`mod theme;`, `theme` fn), `src/app.rs` (field, message, boot task, subscription), `src/animation.rs` (`Morph::with_secs`)

**Interfaces:**
- Consumes: `icons::{BODY_FONT, TITLE_FONT}`, `note::{NoteColor, PALETTE}`, `settings::PRESETS`.
- Produces (all in `theme`):
  - `pub enum Mode { Light, Dark }` with `impl From<iced::theme::Mode> for Mode`, where anything that isn't `Dark` gives `Light`.
  - `#[derive(Clone, Copy)] pub struct Theme { pub mode: Mode }`, with `Theme::new(mode)` and `Default` (Light).
  - Color methods:
    - `ink(&self, alpha: f32) -> Color`: light ink `(0.13, 0.12, 0.10)`, dark ink `(0.93, 0.92, 0.89)`.
    - `paper(&self, color: NoteColor, tint: f32) -> Color` (algorithm below).
    - `bar_gradient(&self, color: NoteColor, alpha: f32) -> Gradient`.
    - `danger(&self, alpha: f32) -> Color`: `(0.75, 0.18, 0.18)`.
    - `focus_ring(&self) -> Color`, which is `ink(0.25)`.
    - `scrollbar(&self, active: bool) -> Color`: ink 0.45 when active, 0.25 when idle.
    - `card(&self) -> Color`: the settings card, light `(0.97, 0.96, 0.94)`, dark `(0.16, 0.155, 0.15)`.
    - `highlight(&self) -> Color`: white at 0.30 in light, 0.06 in dark.
    - `band(&self) -> Color`, which is `ink(0.04)`.
  - `shadows(&self, strength: f32) -> [Shadow; 2]`: contact `{rgba(0,0,0, c·s), (0,1), 2}` and ambient `{rgba(0,0,0, a·s), (0,8), 24}`, with c/a = 0.18/0.12 in light and 0.40/0.35 in dark.
  - `paper_gradient(&self, paper: Color, alpha: f32) -> Gradient`: top = `mix(paper, WHITE, 0.03)`, bottom = paper.
  - Free functions: `pub const fn space(n: u16) -> f32`.
  - Constants:
    - `RADIUS_BAR = 3.0`, `RADIUS_CONTROL = 4.0`, `RADIUS_SURFACE = 10.0`;
    - `TEXT_XS..TEXT_XL` = 12/14/16/20/24;
    - `BODY_LINE_HEIGHT = 1.45`;
    - `pub use crate::icons::{BODY_FONT, TITLE_FONT}`.
  - Color math (pure): `mix(a, b, t)`, `desaturate(c, amount)`, `relative_luminance(c) -> f32` (sRGB per WCAG 2.x), `contrast(a, b) -> f32`.
- `Morph::with_secs(secs: f32, speed: f32) -> Self` in `animation.rs`: open and close both take `secs` at speed 1.
- `Message::ThemeChanged(theme::Mode)` and `App.theme: theme::Theme` (`pub(crate)`).

**Paper algorithm:**
1. Light: `p = desaturate(mix(color, WHITE, 0.55), 0.25)`. Dark: `p = mix(color, (0.11, 0.105, 0.10), 0.78)`.
2. Apply tint like today's `shade`: positive tint mixes toward black, negative toward white.
3. Enforce contrast. While `contrast(ink, p) < 4.5`, nudge `p` by 0.02, toward white in light mode or toward black in dark mode. Stop after at most 50 steps.

- [ ] **Step 1: Write the failing tests** in `theme.rs`:
  - `contrast_matches_wcag_reference`: black on white is 21.0 (±0.01), and `#777777` on white is 4.48 (±0.02).
  - `paper_always_meets_aa_contrast`: for both modes, every color in `PALETTE` and `PRESETS`, and tint in [−0.3, 0.0, 0.3], `contrast(theme.ink(1.0), theme.paper(c, t)) >= 4.5`.
  - `light_paper_is_softer_than_its_bar`: for every `PALETTE` color, the paper's luminance is above the bar's.
  - `dark_paper_is_dark`: in dark mode, every paper's luminance is below 0.2.
  - `tokens`: `space(3) == 12.0`, the radii, the type sizes, and `BODY_LINE_HEIGHT`.
  - `mode_from_iced_defaults_to_light`.
  - `animation.rs`: `with_secs_finishes_in_its_duration`. 0.18 s at speed 1 completes after 12 ticks of 1/60 s, and at speed 2 after 6.
  - `app.rs`: `theme_change_updates_app_theme`. `update(ThemeChanged(Mode::Dark))` gives `app.theme.mode == Mode::Dark`.
- [ ] **Step 2: Run them.** `cargo test --bin snap-notes theme:: animation:: app::tests::theme_change` → compile errors.
- [ ] **Step 3: Implement.**
  - `theme.rs` and `Morph::with_secs`.
  - App: the `theme` field (default Light). The boot task is batched with the existing one: `iced::system::theme().map(|m| Message::ThemeChanged(m.into()))`. Push `iced::system::theme_changes().map(|m| Message::ThemeChanged(m.into()))` to the subscriptions.
  - `main.rs`'s `theme` fn returns `iced::Theme::Light` or `iced::Theme::Dark` from `app.theme.mode` (expose `pub fn theme_mode(&self)` on `App`).
- [ ] **Step 4: Verify.** Run the full suite, clippy and fmt → green.
- [ ] **Step 5: Commit.** `feat: theme tokens and system dark mode`

### Task 3: Open note and formatted view

**Files:** Modify `src/note_panel.rs`, `src/rich_view.rs`, `src/app.rs` (pass theme), `src/resize.rs` only if it draws colors.

**Interfaces:**
- Consumes: `theme::*`, `icons::{icon, Icon}`.
- Produces:
  - `PostIt` gains `pub theme: theme::Theme` and `pub editor_focused: bool`. The app tracks editor focus with `text_editor::Status` in the style closure, or uses `editing` as a proxy if focus isn't observable. Say which in the report.
  - `note_panel::{ink, shade}` are removed, so callers use `Theme`. `rich_view::view` gains a `theme: theme::Theme` parameter and drops its `ink` closure parameter.

- [ ] **Step 1: Write the failing tests:**
  - `rich_view.rs` — `heading_sizes_follow_type_scale`: `heading_size(1) == theme::TEXT_XL`, `(2) == TEXT_LG`, `(3) == TEXT_MD`.
  - `note_panel.rs`:
    - `empty_placeholder_text`: the placeholder constant equals `"Start typing… Markdown works."`.
    - `scrollbar_widths`: the helper `scroller_width(active: bool) -> f32` gives 6.0 when active and 3.0 when idle.
- [ ] **Step 2: Run them** → they fail.
- [ ] **Step 3: Implement.**
  - **Paper:**
    - two nested containers carrying `theme.shadows(morph_progress)[1]` (outer) and `[0]` (inner);
    - background `theme.paper_gradient(theme.paper(note.color, paper_tint · morph_progress), 1.0)`;
    - a 1 px `theme.highlight()` strip at the top;
    - the grip area (top 16 px) on a `theme.band()` background;
    - radius `RADIUS_SURFACE`.
  - **Header:** `Icon::Trash` and `Icon::Close` (size `TEXT_SM`) replace 🗑 and ✕. The title uses `TITLE_FONT`, size `TEXT_MD`.
  - **Pressed style:** every header and confirm button gets a darker background, and the label moves down 1 px via padding `top + 1, bottom − 1`.
  - **Focus ring:** the title input and editor get a 1.5 px border in `theme.focus_ring()` while focused (input status `Focused`, editor status `Focused`).
  - **Placeholder** text and **scrollbar** widths as tested.
  - **Formatted view:** colors come from `theme.ink`. Body size `TEXT_SM`, line height `BODY_LINE_HEIGHT` (applied to the editor and the formatted view's lines and `rich_text`).
  - **Hit-testing:** update `hit_text`'s layout to use the same line height. It must match the drawn `rich_text`.
- [ ] **Step 4: Verify.** Run the full suite, clippy and fmt → green.
- [ ] **Step 5: Commit.** `feat: themed paper, icons and focus ring for the open note`

### Task 4: Toolbar

**Files:** Modify `src/toolbar.rs`, `src/app.rs` (pass theme if needed).

**Interfaces:**
- Consumes: `icons`, `theme`, and Task 3's pressed-style pattern.
- Produces: `toolbar(..)` gains `theme: theme::Theme`.

- [ ] **Step 1: Write the failing test.** `toolbar_buttons_use_icons`: the pure fn `button_icons() -> [Icon; 9]` returns `[Bold, Italic, Strike, Code, Palette, Highlight, Size, Link, Image]` in toolbar order.
- [ ] **Step 2: Run it** → it fails.
- [ ] **Step 3: Implement.** Each toolbar button shows its icon at `TEXT_SM` and uses the pressed style. The size pick list and the color grid use the theme's colors and `RADIUS_CONTROL`. The text labels (B, I, S, `</>`, 🔗, 🖼) are gone.
- [ ] **Step 4: Verify** → green.
- [ ] **Step 5: Commit.** `feat: Lucide toolbar`

### Task 5: Peek

**Files:** Modify `src/peek.rs`, `src/bar_strip.rs` (pass theme).

**Interfaces:**
- Consumes: `theme`, `icons::{ICON_FONT, Icon}`.
- Produces: `draw_peek(..)` gains `theme: &theme::Theme`, and the `BarStrip` field `pub theme: theme::Theme`.

- [ ] **Step 1: Write the failing test.** `peek_body_line_follows_type_scale`: `BODY_LINE == (theme::TEXT_SM * theme::BODY_LINE_HEIGHT).round()`. `TITLE_SIZE == theme::TEXT_MD`.
- [ ] **Step 2: Run it** → it fails.
- [ ] **Step 3: Implement.**
  - **Shadows:** two quads drawn under the paper with `theme.shadows(progress)`.
  - **Fill:** the paper is filled with `theme.paper_gradient(theme.paper(color, tint·t), 1.0)`, plus the highlight quad and the band quad over the header.
  - **Shape:** radius lerps from `RADIUS_BAR` to `RADIUS_SURFACE`.
  - **Icons:** the trash is drawn with `ICON_FONT` and `Icon::Trash`.
  - **Colors:** the confirm buttons use `theme.danger` and `RADIUS_CONTROL`, and all inks come from `theme`.
- [ ] **Step 4: Verify** → green.
- [ ] **Step 5: Commit.** `feat: themed peek`

### Task 6: Strip

**Files:** Modify `src/bar_strip.rs`, `src/app.rs` (pass the open note's index).

**Interfaces:**
- Consumes: `theme`.
- Produces: the `BarStrip` field `pub open_index: Option<usize>`, and `fn bar_rect_for(open: bool, rect: Rectangle) -> Rectangle`. The open bar is 1.5× wide and grows to the left from the right edge.

- [ ] **Step 1: Write the failing test.** `open_bar_is_wider_and_keeps_its_right_edge`: `bar_rect_for(true, r).width == r.width * 1.5`, the right edges are equal, and `bar_rect_for(false, r) == r`.
- [ ] **Step 2: Run it** → it fails.
- [ ] **Step 3: Implement.** Bars get `theme.bar_gradient(color, alpha)`, a 1 px highlight quad and `RADIUS_BAR`. The open note's bar uses `bar_rect_for(true, ..)` and the ambient shadow. The `+` and settings slots use theme inks.
- [ ] **Step 4: Verify** → green.
- [ ] **Step 5: Commit.** `feat: polished strip bars`

### Task 7: Settings and color picker

**Files:** Modify `src/settings_panel.rs`, `src/color_picker.rs`, `src/app.rs` (pass theme).

**Interfaces:** `SettingsView` gains `pub theme: theme::Theme`; `color_picker(..)` gains `theme: theme::Theme`.

- [ ] **Step 1: Write the failing test.** `settings_text_is_readable_in_both_modes`: for both modes, `contrast(theme.ink(1.0), theme.card()) >= 7.0` and `contrast(theme.ink(0.55), theme.card())` is above 3.0 (see the alpha blend helper below).
- [ ] **Step 2: Run it** → it fails.
- [ ] **Step 3: Implement.**
  - The `white(..)` helper and the `SLOT`/`CARD` constants are replaced by `theme.ink(..)` and `theme.card()`.
  - The card gets the paper treatment (shadows, highlight, `RADIUS_SURFACE`). Sizes come from the type scale.
  - Buttons use `RADIUS_CONTROL` and the pressed style.
  - The color picker swatches get `RADIUS_CONTROL` and a theme-colored selection ring.
  - Add `pub fn over(fg: Color, bg: Color) -> Color` (alpha compositing) to `theme`. It's used by the test.
- [ ] **Step 4: Verify** → green.
- [ ] **Step 5: Commit.** `feat: themed settings`

### Task 8: Motion

**Files:** Modify `src/bar_strip.rs` (`compute_layout`), `src/app.rs`, `src/note_panel.rs`, `src/toolbar.rs`.

**Interfaces:**
- Consumes: `Morph::with_secs`.
- Produces:
  - `compute_layout(..)` gains `collapse: Option<(usize, f32)>`. That bar's height and one gap are scaled by `1 − t`.
  - `App` fields:
    - `collapsing: Option<(Uuid, Morph)>`, with `Morph::with_secs(0.18, speed)`;
    - `mode_fade: Morph`, with `Morph::with_secs(0.12, speed)`.
  - `PostIt` gains `pub mode_fade: f32` (0..1).

- [ ] **Step 1: Write the failing tests:**
  - `bar_strip`: `collapsing_bar_shrinks_with_progress`. With 3 bars at scale 1, `collapse = Some((1, 0.5))` makes bar 1's height half the setting height. `Some((1, 1.0))` makes it 0, and the total content height shrinks by height plus gap.
  - `app` — `deleting_collapses_the_bar_before_removing_it`:
    1. Use `app_with_note` with a second note added. Delete note 0 through the open-note path: `DeleteRequested`, then `ConfirmDelete(true)`, then `settle`.
    2. Right after the fold, `collapsing` is `Some` and the note still exists.
    3. After `settle` it's gone and `collapsing` is `None`.
  - `app` — `peek_delete_collapses_too`: the same flow through `PeekDeleteRequested` and `PeekDeleteConfirmed`.
  - `app` — `second_delete_finishes_running_collapse`: start a collapse for note A and delete note B before ticking. A is removed immediately, and B is collapsing.
  - `app` — `mode_switch_fade_reaches_full_opacity`: send `BodyClicked`, `EditorBlurred` and `BodyClicked` within one tick, then `settle`. `mode_fade.progress() == 1.0`.
- [ ] **Step 2: Run them** → they fail.
- [ ] **Step 3: Implement.**
  - Deleting a note (`finish_close`'s pending delete, and `PeekDeleteConfirmed`) starts `collapsing` instead of deleting immediately. Ticks advance it, and at progress 1 the note is deleted, saved, and the scroll is clamped. `animating` stays true while collapsing.
  - `BodyClicked`, `BodyPressed`, `EditorBlurred` and Esc restart `mode_fade` when the mode changes.
  - The toolbar's alpha is `mode_fade` eased, and its y-offset is `6·(1 − eased)`.
  - The new body view's alpha is multiplied by the eased `mode_fade`.
  - `Morph`s get their speed from settings like the others.
- [ ] **Step 4: Verify** → green.
- [ ] **Step 5: Commit.** `feat: collapse deleted bars, fade toolbar and mode switch`

### Task 9: README, size and manual check prep

**Files:** Modify `README.md`.

- [ ] **Step 1: README.**
  - "Under the hood": Inter (SIL OFL 1.1) and Lucide (ISC), with links, plus the `assets/fonts/subset.sh` note.
  - Near the Settings table, a line saying dark mode follows the system.
  - Under Install, the binary-size note with the new size.
- [ ] **Step 2: Measure.** Measure the release binary size before and after: build `main` in a scratch worktree under the session scratch directory and remove it afterwards. Report both sizes.
- [ ] **Step 3: Verify.** Run fmt, clippy and tests → green.
- [ ] **Step 4: Commit.** `docs: visual refresh in the README`
