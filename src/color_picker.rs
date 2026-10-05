use crate::app::Message;
use crate::note::NoteColor;

use crate::icons::{icon, Icon};
use crate::theme::{self, space, RADIUS_CONTROL, RADIUS_SURFACE};

use iced::widget::{button, column, container, row, Space};
use iced::{border, Color, Element, Theme};

pub fn color_picker<'a>(
    current: &NoteColor,
    palette: &'a [NoteColor],
    theme: theme::Theme,
) -> Element<'a, Message> {
    let rows: Vec<Element<'a, Message>> = palette
        .chunks(5)
        .map(|chunk| {
            let btns: Vec<Element<'_, Message>> = chunk
                .iter()
                .map(|color| {
                    swatch(
                        *color,
                        24.0,
                        *color == *current,
                        Message::ColorChosen(*color),
                        theme,
                    )
                })
                .collect();
            row(btns).spacing(space(1)).into()
        })
        .collect();

    container(column(rows).spacing(space(1)))
        .padding(space(2))
        .style(move |_theme: &Theme| container::Style {
            background: Some(theme.card().into()),
            border: iced::Border {
                radius: RADIUS_SURFACE.into(),
                width: 1.0,
                color: theme.ink(0.15),
            },
            ..Default::default()
        })
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
