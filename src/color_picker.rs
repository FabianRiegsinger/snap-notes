use crate::app::Message;
use crate::note::{NoteColor, PALETTE};

use iced::widget::{button, column, container, row, text, Space};
use iced::{Color, Element, Theme};

pub fn color_picker(current: &NoteColor) -> Element<'_, Message> {
    let rows: Vec<Element<'_, Message>> = PALETTE
        .chunks(5)
        .map(|chunk| {
            let btns: Vec<Element<'_, Message>> = chunk
                .iter()
                .map(|color| {
                    let is_current = color.to_hex() == current.to_hex();
                    let c = *color;
                    let r = c.rgba[0];
                    let g = c.rgba[1];
                    let b = c.rgba[2];

                    let swatch_content: Element<'_, Message> = if is_current {
                        container(text("✓").size(12))
                            .width(24)
                            .height(24)
                            .align_x(iced::Alignment::Center)
                            .align_y(iced::Alignment::Center)
                            .style(move |_theme: &Theme| container::Style {
                                background: Some(Color::from_rgb(r, g, b).into()),
                                border: iced::Border {
                                    radius: 12.0.into(),
                                    width: 2.0,
                                    color: Color::WHITE,
                                },
                                ..Default::default()
                            })
                            .into()
                    } else {
                        container(Space::new().width(24).height(24))
                            .style(move |_theme: &Theme| container::Style {
                                background: Some(Color::from_rgb(r, g, b).into()),
                                border: iced::Border {
                                    radius: 12.0.into(),
                                    ..Default::default()
                                },
                                ..Default::default()
                            })
                            .into()
                    };

                    button(swatch_content)
                        .on_press(Message::ColorChosen(c))
                        .padding(2)
                        .style(|_theme: &Theme, _status| button::Style {
                            background: None,
                            ..Default::default()
                        })
                        .into()
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
