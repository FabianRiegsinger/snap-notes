use crate::app::Message;
use crate::color_picker::swatch;
use crate::note::NoteColor;
use crate::note_panel::{icon_button, ink};
use crate::rich::Format;

use iced::widget::{column, container, pick_list, row};
use iced::{Border, Element, Padding, Theme};
use std::fmt;

/// A font size offered by the size list.
#[derive(Debug, Clone, Copy, PartialEq)]
struct SizeChoice(&'static str, f32);

const SIZES: [SizeChoice; 4] = [
    SizeChoice("small", 12.0),
    SizeChoice("normal", 14.0),
    SizeChoice("large", 20.0),
    SizeChoice("huge", 28.0),
];

impl fmt::Display for SizeChoice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// The formatting row shown above the editor, with the text color grid
/// below it while `color_open`. `alpha` fades it like the header controls.
pub fn toolbar<'a>(palette: &'a [NoteColor], color_open: bool, alpha: f32) -> Element<'a, Message> {
    let apply = |label: &'a str, format| icon_button(label, Message::FormatApplied(format), alpha);
    let sizes = pick_list(SIZES, None::<SizeChoice>, |choice: SizeChoice| {
        Message::FormatApplied(Format::Size(choice.1))
    })
    .placeholder("size")
    .text_size(13)
    .padding(Padding::new(2.0).left(6).right(6))
    .style(move |_theme: &Theme, _status| pick_list::Style {
        text_color: ink(0.65 * alpha),
        placeholder_color: ink(0.65 * alpha),
        handle_color: ink(0.5 * alpha),
        background: ink(0.06 * alpha).into(),
        border: Border {
            radius: 4.0.into(),
            ..Default::default()
        },
    });

    let buttons = row![
        apply("B", Format::Bold),
        apply("I", Format::Italic),
        apply("S", Format::Strike),
        apply("</>", Format::Code),
        icon_button("🎨", Message::ToggleTextColorPicker, alpha),
        apply("H", Format::Highlight),
        sizes,
        apply("🔗", Format::Link),
        icon_button("🖼", Message::PickImage, alpha),
    ]
    .spacing(2)
    .align_y(iced::Alignment::Center)
    // Narrow notes wrap the row, so the link and image buttons stay visible.
    .wrap()
    .vertical_spacing(2);

    let mut col = column![buttons].spacing(4);
    if color_open {
        let rows: Vec<Element<'a, Message>> = palette
            .chunks(10)
            .enumerate()
            .map(|(r, chunk)| {
                row(chunk.iter().enumerate().map(|(i, color)| {
                    swatch(
                        *color,
                        18.0,
                        false,
                        Message::FormatApplied(Format::Color(r * 10 + i)),
                    )
                }))
                .spacing(2)
                .into()
            })
            .collect();
        col = col.push(container(column(rows).spacing(2)));
    }
    container(col)
        .padding(Padding::new(4.0).left(14).right(14))
        .into()
}
