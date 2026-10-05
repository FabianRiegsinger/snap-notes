use crate::note::NoteColor;
use iced::gradient::Linear;
use iced::{Color, Gradient, Shadow, Vector};

pub use crate::icons::{BODY_FONT, TITLE_FONT};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    #[default]
    Light,
    Dark,
}

impl From<iced::theme::Mode> for Mode {
    fn from(mode: iced::theme::Mode) -> Self {
        match mode {
            iced::theme::Mode::Dark => Mode::Dark,
            _ => Mode::Light,
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Theme {
    pub mode: Mode,
}

pub const RADIUS_BAR: f32 = 3.0;
pub const RADIUS_CONTROL: f32 = 4.0;
pub const RADIUS_SURFACE: f32 = 10.0;

pub const TEXT_XS: f32 = 12.0;
pub const TEXT_SM: f32 = 14.0;
pub const TEXT_MD: f32 = 16.0;
pub const TEXT_LG: f32 = 20.0;
pub const TEXT_XL: f32 = 24.0;
pub const BODY_LINE_HEIGHT: f32 = 1.45;

/// Spacing scale: `n` steps of 4 px.
pub const fn space(n: u16) -> f32 {
    4.0 * n as f32
}

const LIGHT_INK: Color = Color::from_rgb(0.13, 0.12, 0.10);
const DARK_INK: Color = Color::from_rgb(0.93, 0.92, 0.89);
const DARK_BASE: Color = Color::from_rgb(0.11, 0.105, 0.10);

impl Theme {
    pub fn new(mode: Mode) -> Self {
        Self { mode }
    }

    fn is_dark(&self) -> bool {
        self.mode == Mode::Dark
    }

    pub fn ink(&self, alpha: f32) -> Color {
        let base = if self.is_dark() { DARK_INK } else { LIGHT_INK };
        Color { a: alpha, ..base }
    }

    /// Text on a colored `bg`: whichever ink, dark or light, contrasts more
    /// with it, so a highlight stays readable in both modes.
    pub fn text_on(&self, bg: Color, alpha: f32) -> Color {
        let ink = if contrast(LIGHT_INK, bg) >= contrast(DARK_INK, bg) {
            LIGHT_INK
        } else {
            DARK_INK
        };
        Color { a: alpha, ..ink }
    }

    /// The note's paper color: softened in light mode, deepened in dark
    /// mode, shaded by `tint` and always readable under the ink.
    pub fn paper(&self, color: NoteColor, tint: f32) -> Color {
        let [r, g, b, _] = color.rgba;
        let base = Color::from_rgb(r, g, b);
        let mut p = if self.is_dark() {
            mix(base, DARK_BASE, 0.78)
        } else {
            desaturate(mix(base, Color::WHITE, 0.55), 0.25)
        };
        p = if tint >= 0.0 {
            mix(p, Color::BLACK, tint)
        } else {
            mix(p, Color::WHITE, -tint)
        };
        let ink = self.ink(1.0);
        let toward = if self.is_dark() {
            Color::BLACK
        } else {
            Color::WHITE
        };
        for _ in 0..50 {
            if contrast(ink, p) >= 4.5 {
                break;
            }
            p = mix(p, toward, 0.02);
        }
        p
    }

    /// A docked bar's fill: the note color, 6 % lighter at the top.
    pub fn bar_gradient(&self, color: NoteColor, alpha: f32) -> Gradient {
        let [r, g, b, _] = color.rgba;
        let bottom = Color::from_rgba(r, g, b, alpha);
        let top = Color {
            a: alpha,
            ..mix(bottom, Color::WHITE, 0.06)
        };
        vertical(top, bottom)
    }

    pub fn danger(&self, alpha: f32) -> Color {
        Color::from_rgba(0.75, 0.18, 0.18, alpha)
    }

    pub fn focus_ring(&self) -> Color {
        self.ink(0.25)
    }

    pub fn scrollbar(&self, active: bool) -> Color {
        self.ink(if active { 0.45 } else { 0.25 })
    }

    pub fn card(&self) -> Color {
        if self.is_dark() {
            Color::from_rgb(0.16, 0.155, 0.15)
        } else {
            Color::from_rgb(0.97, 0.96, 0.94)
        }
    }

    pub fn highlight(&self) -> Color {
        Color::from_rgba(1.0, 1.0, 1.0, if self.is_dark() { 0.06 } else { 0.30 })
    }

    pub fn band(&self) -> Color {
        self.ink(0.04)
    }

    /// Contact and ambient shadow, scaled by `strength`.
    pub fn shadows(&self, strength: f32) -> [Shadow; 2] {
        let (contact, ambient) = if self.is_dark() {
            (0.40, 0.35)
        } else {
            (0.18, 0.12)
        };
        [
            Shadow {
                color: Color::from_rgba(0.0, 0.0, 0.0, contact * strength),
                offset: Vector::new(0.0, 1.0),
                blur_radius: 2.0,
            },
            Shadow {
                color: Color::from_rgba(0.0, 0.0, 0.0, ambient * strength),
                offset: Vector::new(0.0, 8.0),
                blur_radius: 24.0,
            },
        ]
    }

    /// Paper with 3 % more light at the top.
    pub fn paper_gradient(&self, paper: Color, alpha: f32) -> Gradient {
        let bottom = Color { a: alpha, ..paper };
        let top = Color {
            a: alpha,
            ..mix(paper, Color::WHITE, 0.03)
        };
        vertical(top, bottom)
    }
}

fn vertical(top: Color, bottom: Color) -> Gradient {
    Gradient::Linear(
        Linear::new(std::f32::consts::PI)
            .add_stop(0.0, top)
            .add_stop(1.0, bottom),
    )
}

pub fn mix(a: Color, b: Color, t: f32) -> Color {
    Color::from_rgba(
        a.r + (b.r - a.r) * t,
        a.g + (b.g - a.g) * t,
        a.b + (b.b - a.b) * t,
        a.a + (b.a - a.a) * t,
    )
}

/// `fg` composited over `bg` (alpha blending); the result is opaque when
/// `bg` is.
pub fn over(fg: Color, bg: Color) -> Color {
    let a = fg.a + bg.a * (1.0 - fg.a);
    if a <= 0.0 {
        return Color::TRANSPARENT;
    }
    let ch = |f: f32, b: f32| (f * fg.a + b * bg.a * (1.0 - fg.a)) / a;
    Color::from_rgba(ch(fg.r, bg.r), ch(fg.g, bg.g), ch(fg.b, bg.b), a)
}

/// Moves `c` toward its own gray by `amount` (0 keeps it, 1 is fully gray).
pub fn desaturate(c: Color, amount: f32) -> Color {
    let gray = 0.2126 * c.r + 0.7152 * c.g + 0.0722 * c.b;
    mix(c, Color::from_rgba(gray, gray, gray, c.a), amount)
}

/// WCAG 2.x relative luminance of the sRGB color.
pub fn relative_luminance(c: Color) -> f32 {
    let lin = |v: f32| {
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * lin(c.r) + 0.7152 * lin(c.g) + 0.0722 * lin(c.b)
}

/// WCAG 2.x contrast ratio between two colors.
pub fn contrast(a: Color, b: Color) -> f32 {
    let (la, lb) = (relative_luminance(a), relative_luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::note::PALETTE;
    use crate::settings::PRESETS;

    const MODES: [Mode; 2] = [Mode::Light, Mode::Dark];

    #[test]
    fn contrast_matches_wcag_reference() {
        assert!((contrast(Color::BLACK, Color::WHITE) - 21.0).abs() < 0.01);
        let grey = Color::from_rgb8(0x77, 0x77, 0x77);
        assert!((contrast(grey, Color::WHITE) - 4.48).abs() < 0.02);
    }

    #[test]
    fn paper_always_meets_aa_contrast() {
        for mode in MODES {
            let theme = Theme::new(mode);
            for c in PALETTE.iter().chain(PRESETS.iter()) {
                for tint in [-0.3, 0.0, 0.3] {
                    let ratio = contrast(theme.ink(1.0), theme.paper(*c, tint));
                    assert!(ratio >= 4.5, "{mode:?} {:?} tint {tint}: {ratio}", c.rgba);
                }
            }
        }
    }

    #[test]
    fn text_on_backgrounds_meets_aa() {
        for mode in MODES {
            let theme = Theme::new(mode);
            for c in PALETTE {
                let [r, g, b, _] = c.rgba;
                let bg = Color::from_rgb(r, g, b);
                let ratio = contrast(theme.text_on(bg, 1.0), bg);
                assert!(ratio >= 4.5, "{mode:?} {:?}: {ratio}", c.rgba);
            }
        }
    }

    #[test]
    fn light_paper_is_softer_than_its_bar() {
        let theme = Theme::new(Mode::Light);
        for c in PALETTE {
            let [r, g, b, _] = c.rgba;
            let bar = relative_luminance(Color::from_rgb(r, g, b));
            assert!(
                relative_luminance(theme.paper(c, 0.0)) > bar,
                "{:?}",
                c.rgba
            );
        }
    }

    #[test]
    fn dark_paper_is_dark() {
        let theme = Theme::new(Mode::Dark);
        for c in PALETTE {
            assert!(
                relative_luminance(theme.paper(c, 0.0)) < 0.2,
                "{:?}",
                c.rgba
            );
        }
    }

    #[test]
    fn tokens() {
        assert_eq!(space(3), 12.0);
        assert_eq!(
            (RADIUS_BAR, RADIUS_CONTROL, RADIUS_SURFACE),
            (3.0, 4.0, 10.0)
        );
        assert_eq!(
            [TEXT_XS, TEXT_SM, TEXT_MD, TEXT_LG, TEXT_XL],
            [12.0, 14.0, 16.0, 20.0, 24.0]
        );
        assert_eq!(BODY_LINE_HEIGHT, 1.45);
    }

    #[test]
    fn mode_from_iced_defaults_to_light() {
        assert_eq!(Mode::from(iced::theme::Mode::Dark), Mode::Dark);
        assert_eq!(Mode::from(iced::theme::Mode::Light), Mode::Light);
        assert_eq!(Mode::from(iced::theme::Mode::None), Mode::Light);
        assert_eq!(Theme::default().mode, Mode::Light);
    }
}
