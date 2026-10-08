use crate::app::Message;
use crate::note::NoteColor;

use crate::color_wheel::{color_wheel, to_hsv, with_value, MIN_VALUE};
use crate::icons::{icon, Icon};
use crate::theme::{self, space, RADIUS_CONTROL, TEXT_SM, TEXT_XS};

use iced::widget::{button, column, container, row, slider, text, text_input, Space};
use iced::{border, Border, Color, Element, Fill, Padding, Theme};

/// Side of a preset swatch.
const PRESET: f32 = 24.0;
/// Diameter of the color wheel.
const WHEEL: f32 = 100.0;
/// Width of the presets and the wheel side by side: three swatches (with
/// their 2 px button padding) and the gaps; the hex field spans it.
const WIDTH: f32 = 3.0 * (PRESET + 4.0) + 2.0 * space(1) + space(3) + WHEEL;

/// The color bubble's content: the palette as presets in rows of three (the
/// current color checked) beside the color wheel, and the brightness slider
/// and hex field below.
pub fn color_picker<'a>(
    current: NoteColor,
    palette: &'a [NoteColor],
    hex: &'a str,
    theme: theme::Theme,
) -> Element<'a, Message> {
    let presets = column(palette.chunks(3).map(|chunk| {
        row(chunk.iter().map(|color| {
            swatch(
                *color,
                PRESET,
                *color == current,
                Message::ColorChosen(*color),
                theme,
            )
        }))
        .spacing(space(1))
        .into()
    }))
    .spacing(space(1));
    let wheel = color_wheel(current, WHEEL, Message::ColorAdjusted);

    let (_, _, value) = to_hsv(current);
    let brightness = column![
        row![
            text("Brightness").size(TEXT_XS).color(theme.ink(0.75)),
            Space::new().width(Fill),
            text(brightness_label(value))
                .size(TEXT_XS)
                .color(theme.ink(0.55)),
        ],
        slider(MIN_VALUE..=1.0, value, move |v| {
            Message::ColorAdjusted(with_value(current, v))
        })
        .step(0.01),
    ]
    .spacing(space(1))
    .width(WIDTH);

    let valid = parse_hex_field(hex).is_some();
    let field = text_input("#RRGGBB", hex)
        .on_input(Message::ColorHexEdited)
        .on_submit(Message::ColorHexSubmitted)
        .size(TEXT_SM)
        .padding(Padding::new(space(1)).left(space(2)).right(space(2)))
        .width(WIDTH)
        .style(move |_theme: &Theme, status| text_input::Style {
            background: theme.band().into(),
            border: Border {
                radius: RADIUS_CONTROL.into(),
                width: 1.0,
                color: if !valid {
                    theme.danger(0.5)
                } else if matches!(status, text_input::Status::Focused { .. }) {
                    theme.focus_ring()
                } else {
                    theme.ink(0.15)
                },
            },
            icon: theme.ink(0.65),
            placeholder: theme.ink(0.35),
            value: theme.ink(0.9),
            selection: theme.ink(0.18),
        });

    column![
        row![presets, wheel]
            .spacing(space(3))
            .align_y(iced::Alignment::Center),
        brightness,
        field,
    ]
    .spacing(space(2))
    .into()
}

/// A color button; `selected` adds an ink ring and a check mark.
pub(crate) fn swatch<'a>(
    color: NoteColor,
    size: f32,
    selected: bool,
    on_press: Message,
    theme: theme::Theme,
) -> Element<'a, Message> {
    let [r, g, b, _] = color.rgba;
    let fill = Color::from_rgb(r, g, b);
    let mark: Element<'a, Message> = if selected {
        let on_fill = if theme::relative_luminance(fill) > 0.4 {
            Color::BLACK
        } else {
            Color::WHITE
        };
        icon(Icon::Check, size / 2.0).color(on_fill).into()
    } else {
        Space::new().into()
    };
    let face = container(mark)
        .width(size)
        .height(size)
        .align_x(iced::Alignment::Center)
        .align_y(iced::Alignment::Center)
        .style(move |_theme: &Theme| container::Style {
            background: Some(fill.into()),
            border: iced::Border {
                width: if selected { 2.0 } else { 0.0 },
                color: theme.ink(1.0),
                ..border::rounded(RADIUS_CONTROL)
            },
            ..Default::default()
        });
    button(face)
        .on_press(on_press)
        .padding(2)
        .style(|_theme: &Theme, _status| button::Style {
            background: None,
            ..Default::default()
        })
        .into()
}

/// The brightness slider's readout: the value as a whole percentage.
fn brightness_label(value: f32) -> String {
    format!("{:.0}%", value * 100.0)
}

/// The color the bubble's hex field holds: `#RRGGBB`, the `#` optional and
/// surrounding spaces ignored; `None` while it is anything else.
pub fn parse_hex_field(text: &str) -> Option<NoteColor> {
    NoteColor::parse_hex(text.trim())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_field_takes_six_digits_with_or_without_hash() {
        let coral = Some(NoteColor::new(1.0, 0.420, 0.420));
        assert_eq!(parse_hex_field("#FF6B6B"), coral);
        assert_eq!(parse_hex_field("ff6b6b"), coral);
        assert_eq!(parse_hex_field(" #ff6b6b "), coral);
    }

    #[test]
    fn brightness_reads_as_a_percentage() {
        assert_eq!(brightness_label(1.0), "100%");
        assert_eq!(brightness_label(0.424), "42%");
        assert_eq!(brightness_label(MIN_VALUE), "10%");
    }

    #[test]
    fn hex_field_rejects_anything_else() {
        for bad in [
            "", "#", "#FF6B6", "#FF6B6B0", "FFF", "#GG6B6B", "coral", "##FF6B6B",
        ] {
            assert_eq!(parse_hex_field(bad), None, "{bad}");
        }
    }
}
