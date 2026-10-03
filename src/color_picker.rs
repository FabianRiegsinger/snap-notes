use crate::app::Message;
use crate::note::NoteColor;

use iced::widget::{button, column, container, row, text, Space};
use iced::{Color, Element, Theme};

pub fn color_picker<'a>(current: &NoteColor, palette: &'a [NoteColor]) -> Element<'a, Message> {
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
                    )
                })
                .collect();
            row(btns).spacing(4).into()
        })
        .collect();

    container(column(rows).spacing(4))
        .padding(8)
        .style(|_theme: &Theme| container::Style {
            background: Some(Color::from_rgba(0.15, 0.15, 0.15, 0.95).into()),
            border: iced::Border {
                radius: 8.0.into(),
                width: 1.0,
                color: Color::from_rgba(0.4, 0.4, 0.4, 0.5),
            },
            ..Default::default()
        })
        .into()
}

/// A round color button; `selected` adds a white ring and a check mark.
pub(crate) fn swatch<'a>(
    color: NoteColor,
    size: f32,
    selected: bool,
    on_press: Message,
) -> Element<'a, Message> {
    let [r, g, b, _] = color.rgba;
    let mark: Element<'a, Message> = if selected {
        text("✓").size(size / 2.0).into()
    } else {
        Space::new().into()
    };
    let face = container(mark)
        .width(size)
        .height(size)
        .align_x(iced::Alignment::Center)
        .align_y(iced::Alignment::Center)
        .style(move |_theme: &Theme| container::Style {
            background: Some(Color::from_rgb(r, g, b).into()),
            border: iced::Border {
                radius: (size / 2.0).into(),
                width: if selected { 2.0 } else { 0.0 },
                color: Color::WHITE,
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
