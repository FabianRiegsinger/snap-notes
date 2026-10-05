//! User-adjustable visual settings, persisted next to the notes.

use crate::app::{NOTE_GAP, NOTE_MARGIN};
use crate::bar_strip::STRIP_WIDTH;
use crate::note::{NoteColor, PALETTE};
use crate::resize::MAX_NOTE_WIDTH;

use serde::Serialize;
use std::fs;
use std::io;
use std::ops::RangeInclusive;
use std::path::PathBuf;
use std::time::{Duration, Instant};

const DEBOUNCE: Duration = Duration::from_millis(500);

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BarSettings {
    pub width: f32,
    pub height: f32,
    pub gap: f32,
}

impl Default for BarSettings {
    fn default() -> Self {
        Self {
            width: 6.0,
            height: 30.0,
            gap: 12.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HoverSettings {
    pub magnification: f32,
    pub spread: f32,
    pub peek_delay_secs: f32,
}

impl Default for HoverSettings {
    fn default() -> Self {
        Self {
            magnification: 4.0,
            spread: 60.0,
            peek_delay_secs: 1.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NoteSettings {
    pub size: f32,
    /// How much lighter (negative) or darker than its bar a note's paper is.
    pub paper_tint: f32,
    /// Header controls stay this faint until the note is hovered.
    pub idle_control_alpha: f32,
}

impl Default for NoteSettings {
    fn default() -> Self {
        Self {
            size: 420.0,
            paper_tint: -0.12,
            idle_control_alpha: 0.3,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MotionSettings {
    /// Divides the open/close/peek durations: 2 is twice as fast.
    pub speed: f32,
}

impl Default for MotionSettings {
    fn default() -> Self {
        Self { speed: 1.0 }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct WindowSettings {
    /// Share of the screen height the bar strip may use (centered).
    pub height_fraction: f32,
}

impl Default for WindowSettings {
    fn default() -> Self {
        Self {
            height_fraction: 0.9,
        }
    }
}

/// Where the app shows up besides its notes. At least one stays visible so
/// the app can always be reached.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AppSettings {
    pub show_menu_bar_icon: bool,
    pub show_dock_icon: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            show_menu_bar_icon: true,
            show_dock_icon: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Settings {
    pub bars: BarSettings,
    pub hover: HoverSettings,
    pub notes: NoteSettings,
    pub motion: MotionSettings,
    pub window: WindowSettings,
    pub palette: Vec<NoteColor>,
    pub app: AppSettings,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            bars: BarSettings::default(),
            hover: HoverSettings::default(),
            notes: NoteSettings::default(),
            motion: MotionSettings::default(),
            window: WindowSettings::default(),
            palette: PALETTE.to_vec(),
            app: AppSettings::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsGroup {
    Bars,
    Hover,
    Notes,
    Motion,
    Window,
    Palette,
    App,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingToggle {
    MenuBarIcon,
    DockIcon,
}

impl SettingToggle {
    #[cfg(any(windows, target_os = "macos"))]
    pub const ALL: [SettingToggle; 2] = [SettingToggle::MenuBarIcon, SettingToggle::DockIcon];

    fn other(self) -> SettingToggle {
        match self {
            SettingToggle::MenuBarIcon => SettingToggle::DockIcon,
            SettingToggle::DockIcon => SettingToggle::MenuBarIcon,
        }
    }
}

impl SettingsGroup {
    pub const SLIDERS: [SettingsGroup; 5] = [
        SettingsGroup::Bars,
        SettingsGroup::Hover,
        SettingsGroup::Notes,
        SettingsGroup::Motion,
        SettingsGroup::Window,
    ];

    pub fn label(self) -> &'static str {
        match self {
            SettingsGroup::Bars => "Bars",
            SettingsGroup::Hover => "Hover",
            SettingsGroup::Notes => "Notes",
            SettingsGroup::Motion => "Motion",
            SettingsGroup::Window => "Window",
            SettingsGroup::Palette => "Palette",
            SettingsGroup::App => "App",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingKey {
    BarWidth,
    BarHeight,
    BarGap,
    Magnification,
    Spread,
    PeekDelay,
    NoteSize,
    PaperTint,
    IdleControlAlpha,
    Speed,
    HeightFraction,
}

impl SettingKey {
    pub const ALL: [SettingKey; 11] = [
        SettingKey::BarWidth,
        SettingKey::BarHeight,
        SettingKey::BarGap,
        SettingKey::Magnification,
        SettingKey::Spread,
        SettingKey::PeekDelay,
        SettingKey::NoteSize,
        SettingKey::PaperTint,
        SettingKey::IdleControlAlpha,
        SettingKey::Speed,
        SettingKey::HeightFraction,
    ];

    pub fn range(self) -> RangeInclusive<f32> {
        match self {
            SettingKey::BarWidth => 3.0..=12.0,
            SettingKey::BarHeight => 16.0..=60.0,
            SettingKey::BarGap => 0.0..=24.0,
            SettingKey::Magnification => 0.0..=5.0,
            SettingKey::Spread => 20.0..=150.0,
            SettingKey::PeekDelay => 0.0..=3.0,
            SettingKey::NoteSize => 240.0..=440.0,
            SettingKey::PaperTint => -0.3..=0.3,
            SettingKey::IdleControlAlpha => 0.0..=1.0,
            SettingKey::Speed => 0.25..=3.0,
            SettingKey::HeightFraction => 0.5..=1.0,
        }
    }

    /// Group and field name of this setting in `settings.json`.
    fn json_path(self) -> (&'static str, &'static str) {
        match self {
            SettingKey::BarWidth => ("bars", "width"),
            SettingKey::BarHeight => ("bars", "height"),
            SettingKey::BarGap => ("bars", "gap"),
            SettingKey::Magnification => ("hover", "magnification"),
            SettingKey::Spread => ("hover", "spread"),
            SettingKey::PeekDelay => ("hover", "peek_delay_secs"),
            SettingKey::NoteSize => ("notes", "size"),
            SettingKey::PaperTint => ("notes", "paper_tint"),
            SettingKey::IdleControlAlpha => ("notes", "idle_control_alpha"),
            SettingKey::Speed => ("motion", "speed"),
            SettingKey::HeightFraction => ("window", "height_fraction"),
        }
    }

    /// Slider step size.
    pub fn step(self) -> f32 {
        match self {
            SettingKey::PaperTint
            | SettingKey::IdleControlAlpha
            | SettingKey::Speed
            | SettingKey::HeightFraction => 0.01,
            SettingKey::Magnification | SettingKey::PeekDelay => 0.1,
            _ => 1.0,
        }
    }

    pub fn group(self) -> SettingsGroup {
        match self {
            SettingKey::BarWidth | SettingKey::BarHeight | SettingKey::BarGap => {
                SettingsGroup::Bars
            }
            SettingKey::Magnification | SettingKey::Spread | SettingKey::PeekDelay => {
                SettingsGroup::Hover
            }
            SettingKey::NoteSize | SettingKey::PaperTint | SettingKey::IdleControlAlpha => {
                SettingsGroup::Notes
            }
            SettingKey::Speed => SettingsGroup::Motion,
            SettingKey::HeightFraction => SettingsGroup::Window,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            SettingKey::BarWidth => "Width",
            SettingKey::BarHeight => "Height",
            SettingKey::BarGap => "Gap",
            SettingKey::Magnification => "Magnification",
            SettingKey::Spread => "Spread",
            SettingKey::PeekDelay => "Peek delay",
            SettingKey::NoteSize => "Default size",
            SettingKey::PaperTint => "Paper tint",
            SettingKey::IdleControlAlpha => "Idle control opacity",
            SettingKey::Speed => "Animation speed",
            SettingKey::HeightFraction => "Strip height",
        }
    }

    pub fn format(self, v: f32) -> String {
        match self {
            SettingKey::BarWidth
            | SettingKey::BarHeight
            | SettingKey::BarGap
            | SettingKey::NoteSize
            | SettingKey::Spread => format!("{v:.0} px"),
            // Stored as the extra size on top of the bar's own (scale 1 + v).
            SettingKey::Magnification => format!("{:.1}×", 1.0 + v),
            SettingKey::Speed => format!("{v:.2}×"),
            SettingKey::PeekDelay => format!("{v:.1} s"),
            SettingKey::PaperTint => {
                let percent = (v.abs() * 100.0).round();
                if percent == 0.0 {
                    "same as bar".to_string()
                } else if v < 0.0 {
                    format!("{percent:.0} % lighter")
                } else {
                    format!("{percent:.0} % darker")
                }
            }
            SettingKey::IdleControlAlpha | SettingKey::HeightFraction => {
                format!("{:.0} %", v * 100.0)
            }
        }
    }
}

impl Settings {
    /// Reads settings leniently: every valid value is kept, and anything
    /// missing, mistyped or out of range falls back to (or is clamped
    /// toward) its default, so one bad entry never discards the rest.
    pub fn from_json(json: &str) -> Settings {
        let mut settings = Settings::default();
        let Ok(value) = serde_json::from_str::<serde_json::Value>(json) else {
            return settings;
        };
        for key in SettingKey::ALL {
            let (group, field) = key.json_path();
            if let Some(v) = value
                .get(group)
                .and_then(|g| g.get(field))
                .and_then(|v| v.as_f64())
            {
                settings.set(key, v as f32);
            }
        }
        if let Some(entries) = value.get("palette").and_then(|p| p.as_array()) {
            if entries.len() == PALETTE.len() {
                settings.palette = entries
                    .iter()
                    .zip(PALETTE)
                    .map(|(entry, default)| {
                        entry
                            .as_str()
                            .and_then(NoteColor::parse_hex)
                            .unwrap_or(default)
                    })
                    .collect();
            }
        }
        let app = value.get("app");
        for (field, target) in [
            ("show_menu_bar_icon", &mut settings.app.show_menu_bar_icon),
            ("show_dock_icon", &mut settings.app.show_dock_icon),
        ] {
            if let Some(v) = app.and_then(|a| a.get(field)).and_then(|v| v.as_bool()) {
                *target = v;
            }
        }
        if !settings.app.show_menu_bar_icon && !settings.app.show_dock_icon {
            settings.app.show_menu_bar_icon = true;
        }
        settings
    }

    pub fn is_on(&self, toggle: SettingToggle) -> bool {
        match toggle {
            SettingToggle::MenuBarIcon => self.app.show_menu_bar_icon,
            SettingToggle::DockIcon => self.app.show_dock_icon,
        }
    }

    /// Whether `toggle` may flip: turning it off is refused while the other
    /// icon is already hidden.
    pub fn can_toggle(&self, toggle: SettingToggle) -> bool {
        !self.is_on(toggle) || self.is_on(toggle.other())
    }

    /// Flips `toggle` if allowed. Returns whether anything changed.
    pub fn toggle(&mut self, toggle: SettingToggle) -> bool {
        if !self.can_toggle(toggle) {
            return false;
        }
        let value = match toggle {
            SettingToggle::MenuBarIcon => &mut self.app.show_menu_bar_icon,
            SettingToggle::DockIcon => &mut self.app.show_dock_icon,
        };
        *value = !*value;
        true
    }

    fn field(&mut self, key: SettingKey) -> &mut f32 {
        match key {
            SettingKey::BarWidth => &mut self.bars.width,
            SettingKey::BarHeight => &mut self.bars.height,
            SettingKey::BarGap => &mut self.bars.gap,
            SettingKey::Magnification => &mut self.hover.magnification,
            SettingKey::Spread => &mut self.hover.spread,
            SettingKey::PeekDelay => &mut self.hover.peek_delay_secs,
            SettingKey::NoteSize => &mut self.notes.size,
            SettingKey::PaperTint => &mut self.notes.paper_tint,
            SettingKey::IdleControlAlpha => &mut self.notes.idle_control_alpha,
            SettingKey::Speed => &mut self.motion.speed,
            SettingKey::HeightFraction => &mut self.window.height_fraction,
        }
    }

    pub fn get(&self, key: SettingKey) -> f32 {
        match key {
            SettingKey::BarWidth => self.bars.width,
            SettingKey::BarHeight => self.bars.height,
            SettingKey::BarGap => self.bars.gap,
            SettingKey::Magnification => self.hover.magnification,
            SettingKey::Spread => self.hover.spread,
            SettingKey::PeekDelay => self.hover.peek_delay_secs,
            SettingKey::NoteSize => self.notes.size,
            SettingKey::PaperTint => self.notes.paper_tint,
            SettingKey::IdleControlAlpha => self.notes.idle_control_alpha,
            SettingKey::Speed => self.motion.speed,
            SettingKey::HeightFraction => self.window.height_fraction,
        }
    }

    /// Sets `key` clamped to its range; a non-finite value resets it.
    pub fn set(&mut self, key: SettingKey, value: f32) {
        let default = Settings::default().get(key);
        let range = key.range();
        let value = if value.is_finite() { value } else { default };
        *self.field(key) = value.clamp(*range.start(), *range.end());
    }

    pub fn reset(&mut self, group: SettingsGroup) {
        let d = Settings::default();
        match group {
            SettingsGroup::Bars => self.bars = d.bars,
            SettingsGroup::Hover => self.hover = d.hover,
            SettingsGroup::Notes => self.notes = d.notes,
            SettingsGroup::Motion => self.motion = d.motion,
            SettingsGroup::Window => self.window = d.window,
            SettingsGroup::Palette => self.palette = d.palette,
            SettingsGroup::App => self.app = d.app,
        }
    }

    /// Puts `color` into palette slot `slot`. Returns the replaced color when
    /// notes using it should follow to `color`: not when there is no such
    /// slot, and not when another slot still holds that color (its notes
    /// can't be told apart and stay as they are).
    pub fn replace_palette_color(&mut self, slot: usize, color: NoteColor) -> Option<NoteColor> {
        let entry = self.palette.get_mut(slot)?;
        let old = std::mem::replace(entry, color);
        (!self.palette.contains(&old)).then_some(old)
    }

    /// Restores the default palette and returns `(old, default)` for every
    /// slot that changed, so notes can follow. Colors that are also in the
    /// default palette are left out: their notes keep them.
    pub fn reset_palette(&mut self) -> Vec<(NoteColor, NoteColor)> {
        let changes = self
            .palette
            .iter()
            .zip(PALETTE)
            .filter(|(old, _)| !PALETTE.contains(old))
            .map(|(old, new)| (*old, new))
            .collect();
        self.palette = PALETTE.to_vec();
        changes
    }

    /// Window width that fits the strip plus the widest note.
    pub fn open_width(&self) -> f32 {
        STRIP_WIDTH + NOTE_GAP + MAX_NOTE_WIDTH + NOTE_MARGIN
    }
}

pub struct SettingsStore {
    settings: Settings,
    path: PathBuf,
    dirty: bool,
    last_mark: Option<Instant>,
}

impl SettingsStore {
    /// Loads `path`, falling back to defaults when it is missing or invalid.
    pub fn load(path: PathBuf) -> Self {
        let settings = fs::read_to_string(&path)
            .map(|json| Settings::from_json(&json))
            .unwrap_or_default();
        Self {
            settings,
            path,
            dirty: false,
            last_mark: None,
        }
    }

    pub fn save(&self) -> io::Result<()> {
        let json = serde_json::to_string_pretty(&self.settings)?;
        let tmp = self.path.with_extension("json.tmp");
        fs::write(&tmp, &json)?;
        fs::rename(&tmp, &self.path)?;
        Ok(())
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    /// Mutable access; marks the settings for saving.
    pub fn settings_mut(&mut self) -> &mut Settings {
        self.dirty = true;
        self.last_mark = Some(Instant::now());
        &mut self.settings
    }

    pub fn should_save(&self) -> bool {
        self.dirty && self.last_mark.is_some_and(|t| t.elapsed() >= DEBOUNCE)
    }

    #[cfg(test)]
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn did_save(&mut self) {
        self.dirty = false;
        self.last_mark = None;
    }
}

/// Replacement colors offered by the palette editor: 12 hues (every 30°)
/// in 5 tints from deep to pale.
pub const PRESETS: [NoteColor; 60] = [
    NoteColor::new(0.787, 0.113, 0.113),
    NoteColor::new(0.787, 0.45, 0.113),
    NoteColor::new(0.787, 0.787, 0.113),
    NoteColor::new(0.45, 0.787, 0.113),
    NoteColor::new(0.113, 0.787, 0.113),
    NoteColor::new(0.113, 0.787, 0.45),
    NoteColor::new(0.113, 0.787, 0.787),
    NoteColor::new(0.113, 0.45, 0.787),
    NoteColor::new(0.113, 0.113, 0.787),
    NoteColor::new(0.45, 0.113, 0.787),
    NoteColor::new(0.787, 0.113, 0.787),
    NoteColor::new(0.787, 0.113, 0.45),
    NoteColor::new(0.887, 0.213, 0.213),
    NoteColor::new(0.887, 0.55, 0.213),
    NoteColor::new(0.887, 0.887, 0.213),
    NoteColor::new(0.55, 0.887, 0.213),
    NoteColor::new(0.213, 0.887, 0.213),
    NoteColor::new(0.213, 0.887, 0.55),
    NoteColor::new(0.213, 0.887, 0.887),
    NoteColor::new(0.213, 0.55, 0.887),
    NoteColor::new(0.213, 0.213, 0.887),
    NoteColor::new(0.55, 0.213, 0.887),
    NoteColor::new(0.887, 0.213, 0.887),
    NoteColor::new(0.887, 0.213, 0.55),
    NoteColor::new(0.912, 0.388, 0.388),
    NoteColor::new(0.912, 0.65, 0.388),
    NoteColor::new(0.912, 0.912, 0.388),
    NoteColor::new(0.65, 0.912, 0.388),
    NoteColor::new(0.388, 0.912, 0.388),
    NoteColor::new(0.388, 0.912, 0.65),
    NoteColor::new(0.388, 0.912, 0.912),
    NoteColor::new(0.388, 0.65, 0.912),
    NoteColor::new(0.388, 0.388, 0.912),
    NoteColor::new(0.65, 0.388, 0.912),
    NoteColor::new(0.912, 0.388, 0.912),
    NoteColor::new(0.912, 0.388, 0.65),
    NoteColor::new(0.938, 0.562, 0.562),
    NoteColor::new(0.938, 0.75, 0.562),
    NoteColor::new(0.937, 0.938, 0.562),
    NoteColor::new(0.75, 0.938, 0.562),
    NoteColor::new(0.562, 0.938, 0.562),
    NoteColor::new(0.562, 0.938, 0.75),
    NoteColor::new(0.562, 0.937, 0.938),
    NoteColor::new(0.562, 0.75, 0.938),
    NoteColor::new(0.562, 0.562, 0.938),
    NoteColor::new(0.75, 0.562, 0.938),
    NoteColor::new(0.938, 0.562, 0.937),
    NoteColor::new(0.938, 0.562, 0.75),
    NoteColor::new(0.963, 0.737, 0.737),
    NoteColor::new(0.963, 0.85, 0.737),
    NoteColor::new(0.963, 0.963, 0.737),
    NoteColor::new(0.85, 0.963, 0.737),
    NoteColor::new(0.737, 0.963, 0.737),
    NoteColor::new(0.737, 0.963, 0.85),
    NoteColor::new(0.737, 0.963, 0.963),
    NoteColor::new(0.737, 0.85, 0.963),
    NoteColor::new(0.737, 0.737, 0.963),
    NoteColor::new(0.85, 0.737, 0.963),
    NoteColor::new(0.963, 0.737, 0.963),
    NoteColor::new(0.963, 0.737, 0.85),
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::note::PALETTE;

    #[test]
    fn defaults_match_current_constants() {
        let s = Settings::default();
        assert_eq!((s.bars.width, s.bars.height, s.bars.gap), (6.0, 30.0, 12.0));
        assert_eq!(
            (
                s.hover.magnification,
                s.hover.spread,
                s.hover.peek_delay_secs
            ),
            (4.0, 60.0, 1.0)
        );
        assert_eq!(
            (s.notes.size, s.notes.paper_tint, s.notes.idle_control_alpha),
            (420.0, -0.12, 0.3)
        );
        assert_eq!(s.motion.speed, 1.0);
        assert_eq!(s.window.height_fraction, 0.9);
        assert_eq!(s.palette, PALETTE.to_vec());
    }

    #[test]
    fn json_roundtrip() {
        let s = Settings::default();
        let back = Settings::from_json(&serde_json::to_string(&s).unwrap());
        assert_eq!(back, s);
    }

    #[test]
    fn partial_json_fills_defaults() {
        let s = Settings::from_json(r#"{"bars":{"width":9}}"#);
        let expected = Settings {
            bars: BarSettings {
                width: 9.0,
                ..BarSettings::default()
            },
            ..Settings::default()
        };
        assert_eq!(s, expected);
    }

    #[test]
    fn unknown_keys_ignored() {
        let s = Settings::from_json(r#"{"future":1,"bars":{"x":2}}"#);
        assert_eq!(s, Settings::default());
    }

    #[test]
    fn out_of_range_values_clamp() {
        let s = Settings::from_json(r#"{"bars":{"width":100},"hover":{"spread":-5}}"#);
        assert_eq!(s.bars.width, 12.0);
        assert_eq!(s.hover.spread, 20.0);
    }

    #[test]
    fn non_finite_value_resets_to_default() {
        let mut s = Settings::default();
        s.set(SettingKey::Speed, f32::NAN);
        assert_eq!(s.motion.speed, 1.0);
    }

    #[test]
    fn default_note_fits_the_toolbar_on_one_line_with_room_to_spare() {
        let size = Settings::default().notes.size;
        assert!(size >= crate::toolbar::ONE_LINE_WIDTH + 40.0, "{size}");
        assert!(SettingKey::NoteSize.range().contains(&size));
    }

    #[test]
    fn old_expanded_size_is_ignored() {
        let s = Settings::from_json(r#"{"notes":{"size":300,"expanded_size":700}}"#);
        assert_eq!(s.notes.size, 300.0);
        assert!(!SettingKey::ALL.iter().any(|k| k.label() == "Expanded size"));
    }

    #[test]
    fn get_set_every_key() {
        let mut s = Settings::default();
        for key in SettingKey::ALL {
            let end = *key.range().end();
            s.set(key, end);
            assert_eq!(s.get(key), end, "{key:?}");
            s.set(key, end + 1000.0);
            assert_eq!(s.get(key), end, "{key:?} clamps");
        }
    }

    #[test]
    fn reset_group_restores_only_that_group() {
        let mut s = Settings::default();
        s.set(SettingKey::BarWidth, 10.0);
        s.set(SettingKey::Spread, 100.0);
        s.reset(SettingsGroup::Bars);
        assert_eq!(s.bars, BarSettings::default());
        assert_eq!(s.hover.spread, 100.0);
    }

    #[test]
    fn store_corrupt_file_loads_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(&path, "{nope").unwrap();
        assert_eq!(*SettingsStore::load(path).settings(), Settings::default());
    }

    #[test]
    fn store_load_clamps() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(&path, r#"{"bars":{"width":500}}"#).unwrap();
        assert_eq!(SettingsStore::load(path).settings().bars.width, 12.0);
    }

    #[test]
    fn store_save_load_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let mut store = SettingsStore::load(path.clone());
        store.settings_mut().set(SettingKey::BarGap, 20.0);
        assert!(!store.should_save(), "debounced");
        store.save().unwrap();
        assert_eq!(SettingsStore::load(path).settings().bars.gap, 20.0);
    }

    #[test]
    fn replace_palette_color_returns_old() {
        let mut s = Settings::default();
        assert_eq!(s.replace_palette_color(2, PRESETS[0]), Some(PALETTE[2]));
        assert_eq!(s.palette[2], PRESETS[0]);
        assert_eq!(s.replace_palette_color(20, PRESETS[0]), None);
    }

    #[test]
    fn reset_palette_reports_changed_slots() {
        let mut s = Settings::default();
        s.replace_palette_color(1, PRESETS[5]);
        assert_eq!(s.reset_palette(), vec![(PRESETS[5], PALETTE[1])]);
        assert_eq!(s.palette, PALETTE.to_vec());
        assert!(s.reset_palette().is_empty());
    }

    #[test]
    fn one_bad_value_keeps_the_rest() {
        let s = Settings::from_json(r#"{"bars":{"width":"9","gap":20},"hover":null}"#);
        assert_eq!(s.bars.gap, 20.0);
        assert_eq!(s.bars.width, BarSettings::default().width);
        assert_eq!(s.hover, HoverSettings::default());
    }

    #[test]
    fn bad_palette_entry_falls_back_per_slot() {
        let mut hexes: Vec<String> = PRESETS[..20].iter().map(|c| c.to_hex()).collect();
        hexes[3] = "#FFF".into();
        let json = serde_json::json!({ "palette": hexes }).to_string();
        let s = Settings::from_json(&json);
        assert_eq!(s.palette[2], PRESETS[2]);
        assert_eq!(s.palette[3], PALETTE[3]);
        let short = Settings::from_json(r##"{"palette":["#FFF"]}"##);
        assert_eq!(short.palette, PALETTE.to_vec());
    }

    #[test]
    fn invalid_json_loads_defaults() {
        assert_eq!(Settings::from_json("{nope"), Settings::default());
    }

    #[test]
    fn replacing_a_shared_color_leaves_its_notes_alone() {
        let mut s = Settings::default();
        let (a, b) = (PRESETS[0], PRESETS[1]);
        assert_eq!(s.replace_palette_color(1, a), Some(PALETTE[1]));
        assert_eq!(s.replace_palette_color(3, a), Some(PALETTE[3]));
        // Slot 1 still uses `a`, so its notes must keep it.
        assert_eq!(s.replace_palette_color(3, b), None);
        assert_eq!(s.palette[1], a);
        assert_eq!(s.palette[3], b);
    }

    #[test]
    fn reset_palette_skips_colors_still_in_defaults() {
        let mut s = Settings::default();
        s.replace_palette_color(0, PALETTE[5]);
        assert!(s.reset_palette().is_empty());
    }

    #[test]
    fn app_defaults_show_both() {
        let s = Settings::default();
        assert!(s.app.show_menu_bar_icon && s.app.show_dock_icon);
    }

    #[test]
    fn app_settings_load_from_json() {
        let s = Settings::from_json(r#"{"app":{"show_dock_icon":false}}"#);
        assert!(!s.app.show_dock_icon);
        assert!(s.app.show_menu_bar_icon);
    }

    #[test]
    fn app_settings_ignore_non_bool() {
        let s = Settings::from_json(r#"{"app":{"show_dock_icon":"no"}}"#);
        assert!(s.app.show_dock_icon);
    }

    #[test]
    fn both_hidden_in_file_restores_menu_bar_icon() {
        let s =
            Settings::from_json(r#"{"app":{"show_menu_bar_icon":false,"show_dock_icon":false}}"#);
        assert!(s.app.show_menu_bar_icon);
        assert!(!s.app.show_dock_icon);
    }

    #[test]
    fn toggle_refuses_to_hide_last_icon() {
        let mut s = Settings::default();
        assert!(s.toggle(SettingToggle::DockIcon));
        assert!(!s.is_on(SettingToggle::DockIcon));
        assert!(!s.can_toggle(SettingToggle::MenuBarIcon));
        assert!(!s.toggle(SettingToggle::MenuBarIcon));
        assert!(s.is_on(SettingToggle::MenuBarIcon));
        assert!(s.can_toggle(SettingToggle::DockIcon));
        assert!(s.toggle(SettingToggle::DockIcon));
        assert!(s.is_on(SettingToggle::DockIcon));
    }

    #[test]
    fn reset_app_group_shows_both() {
        let mut s = Settings::default();
        s.toggle(SettingToggle::DockIcon);
        s.reset(SettingsGroup::App);
        assert_eq!(s.app, AppSettings::default());
    }

    #[test]
    fn json_roundtrip_keeps_app_settings() {
        let mut s = Settings::default();
        s.toggle(SettingToggle::MenuBarIcon);
        let back = Settings::from_json(&serde_json::to_string(&s).unwrap());
        assert_eq!(back, s);
    }

    #[test]
    fn ranges_allow_off_and_immediate() {
        let mut s = Settings::default();
        for key in [
            SettingKey::PeekDelay,
            SettingKey::Magnification,
            SettingKey::BarGap,
        ] {
            s.set(key, 0.0);
            assert_eq!(s.get(key), 0.0, "{key:?}");
        }
        assert_eq!(SettingKey::Spread.range(), 20.0..=150.0);
        assert_eq!(SettingKey::Speed.range(), 0.25..=3.0);
    }

    #[test]
    fn formats_read_naturally() {
        assert_eq!(SettingKey::Magnification.format(4.0), "5.0×");
        assert_eq!(SettingKey::Magnification.format(0.0), "1.0×");
        assert_eq!(SettingKey::Spread.format(60.0), "60 px");
        assert_eq!(SettingKey::PaperTint.format(-0.12), "12 % lighter");
        assert_eq!(SettingKey::PaperTint.format(0.1), "10 % darker");
        assert_eq!(SettingKey::PaperTint.format(0.0), "same as bar");
        assert_eq!(SettingKey::PeekDelay.format(0.0), "0.0 s");
    }

    #[test]
    fn presets_are_60_distinct() {
        let hexes: std::collections::HashSet<_> = PRESETS.iter().map(|c| c.to_hex()).collect();
        assert_eq!(hexes.len(), 60);
    }

    #[test]
    fn open_width_fits_the_widest_note() {
        let mut s = Settings::default();
        let width = STRIP_WIDTH + NOTE_GAP + MAX_NOTE_WIDTH + NOTE_MARGIN;
        assert_eq!(s.open_width(), width);
        s.set(SettingKey::NoteSize, 440.0);
        assert_eq!(s.open_width(), width);
    }
}
