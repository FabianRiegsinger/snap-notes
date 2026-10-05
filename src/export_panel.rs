//! The export card: every note with a checkbox, the format switch and the
//! Export button. It takes the Settings' place, morphing out of the gear.

use crate::app::Message;
use crate::export::ExportFormat;
use crate::icons::{icon, Icon, ICON_FONT};
use crate::note::Note;
use crate::note_panel::pressable;
use crate::settings_panel::{paper_card, text_button};
use crate::theme::{self, space, RADIUS_CONTROL, TEXT_MD, TEXT_SM, TEXT_XS};

use iced::widget::{button, checkbox, column, row, scrollable, text, Space};
use iced::{border, Border, Element, Fill, Padding, Size, Theme};
use std::collections::HashSet;
use std::time::Instant;
use uuid::Uuid;

/// How a failed write's message starts; the reason follows.
const SAVE_ERROR: &str = "Couldn't save: ";

/// The open export panel's choices and its last result.
pub struct ExportState {
    pub selected: HashSet<Uuid>,
    pub format: ExportFormat,
    /// The last export's one-line result and when it happened.
    pub status: Option<(String, Instant)>,
}

impl ExportState {
    /// Every one of `notes` selected, as Markdown.
    pub fn new(notes: &[Note]) -> Self {
        Self {
            selected: notes.iter().map(|n| n.id).collect(),
            format: ExportFormat::Markdown,
            status: None,
        }
    }

    /// When the last export succeeded, if it did: the panel closes a moment
    /// after. A failure stays until the next action.
    pub fn succeeded_at(&self) -> Option<Instant> {
        self.status
            .as_ref()
            .filter(|(message, _)| !message.starts_with(SAVE_ERROR))
            .map(|(_, at)| *at)
    }
}

/// "Exported N notes", singular for one.
pub fn exported_message(count: usize) -> String {
    if count == 1 {
        "Exported 1 note".to_string()
    } else {
        format!("Exported {count} notes")
    }
}

pub fn save_error_message(error: &std::io::Error) -> String {
    format!("{SAVE_ERROR}{error}")
}

pub struct ExportView<'a> {
    pub theme: theme::Theme,
    pub notes: &'a [Note],
    pub state: &'a ExportState,
    pub size: Size,
    pub morph_progress: f32,
    pub content_alpha: f32,
}

fn note_row<'a>(note: &Note, checked: bool, theme: theme::Theme, a: f32) -> Element<'a, Message> {
    let title = note.title.trim();
    let title = if title.is_empty() { "Untitled" } else { title };
    let id = note.id;
    checkbox(checked)
        .label(title.to_string())
        .on_toggle(move |_| Message::ExportToggleNote(id))
        .size(TEXT_SM + 2.0)
        .text_size(TEXT_SM)
        .spacing(space(2))
        .width(Fill)
        .icon(checkbox::Icon {
            font: ICON_FONT,
            code_point: Icon::Check.codepoint(),
            size: Some(TEXT_XS.into()),
            line_height: iced::widget::text::LineHeight::default(),
            shaping: iced::widget::text::Shaping::Basic,
        })
        .style(move |_theme: &Theme, status| {
            let (checked, hovered) = match status {
                checkbox::Status::Active { is_checked } => (is_checked, false),
                checkbox::Status::Hovered { is_checked } => (is_checked, true),
                checkbox::Status::Disabled { is_checked } => (is_checked, false),
            };
            let fill = match (checked, hovered) {
                (true, false) => 0.8,
                (true, true) => 0.9,
                (false, false) => 0.06,
                (false, true) => 0.12,
            };
            checkbox::Style {
                background: theme.ink(fill * a).into(),
                icon_color: theme.card().scale_alpha(a),
                border: Border {
                    color: theme.ink(0.35 * a),
                    width: if checked { 0.0 } else { 1.0 },
                    radius: RADIUS_CONTROL.into(),
                },
                text_color: Some(theme.ink(0.85 * a)),
            }
        })
        .into()
}

/// One side of the .md/.txt switch: an ink wash marks the chosen format.
fn format_button<'a>(
    format: ExportFormat,
    chosen: ExportFormat,
    theme: theme::Theme,
    a: f32,
) -> Element<'a, Message> {
    let selected = format == chosen;
    pressable(
        text(format!(".{}", format.extension())).size(TEXT_XS),
        Padding::new(2.0).left(space(2)).right(space(2)),
        Message::ExportFormatChosen(format),
    )
    .style(move |_theme: &Theme, status| button::Style {
        background: match status {
            button::Status::Pressed => Some(theme.ink(0.24 * a).into()),
            button::Status::Hovered => {
                Some(theme.ink(if selected { 0.18 } else { 0.1 } * a).into())
            }
            _ if selected => Some(theme.ink(0.14 * a).into()),
            _ => None,
        },
        text_color: theme.ink(if selected { 0.9 } else { 0.6 } * a),
        border: border::rounded(RADIUS_CONTROL),
        ..Default::default()
    })
    .into()
}

/// The Export button: ink on paper, faded and inert while nothing is
/// selected.
fn export_button<'a>(enabled: bool, theme: theme::Theme, a: f32) -> Element<'a, Message> {
    let label = row![icon(Icon::Download, TEXT_SM), text("Export").size(TEXT_SM)]
        .spacing(space(2))
        .align_y(iced::Alignment::Center);
    pressable(
        label,
        Padding::new(space(1)).left(space(3)).right(space(3)),
        Message::ExportRequested,
    )
    .on_press_maybe(enabled.then_some(Message::ExportRequested))
    .style(move |_theme: &Theme, status| {
        let fill = match status {
            button::Status::Hovered => 0.9,
            button::Status::Pressed => 1.0,
            button::Status::Disabled => 0.25,
            button::Status::Active => 0.8,
        };
        button::Style {
            background: Some(theme.ink(fill * a).into()),
            text_color: theme.card().scale_alpha(a),
            border: border::rounded(RADIUS_CONTROL),
            ..Default::default()
        }
    })
    .into()
}

pub fn export_panel(v: ExportView<'_>) -> Element<'_, Message> {
    let ExportView {
        theme,
        notes,
        state,
        size,
        morph_progress: t,
        content_alpha: a,
    } = v;

    let inner: Element<'_, Message> = if a < 0.01 {
        Space::new().width(Fill).height(Fill).into()
    } else {
        let header = row![
            text("Export")
                .size(TEXT_MD)
                .font(theme::TITLE_FONT)
                .color(theme.ink(a)),
            Space::new().width(Fill),
            text_button(icon(Icon::Close, TEXT_SM), Message::ToggleExport, theme, a),
        ]
        .align_y(iced::Alignment::Center);

        let controls = row![
            text_button(
                text("All").size(TEXT_XS),
                Message::ExportAll(true),
                theme,
                a
            ),
            text_button(
                text("None").size(TEXT_XS),
                Message::ExportAll(false),
                theme,
                a
            ),
            Space::new().width(Fill),
            format_button(ExportFormat::Markdown, state.format, theme, a),
            format_button(ExportFormat::Text, state.format, theme, a),
        ]
        .spacing(space(1))
        .align_y(iced::Alignment::Center);

        let mut list = column![]
            .spacing(space(2))
            .padding(Padding::ZERO.right(space(3)));
        for note in notes {
            list = list.push(note_row(note, state.selected.contains(&note.id), theme, a));
        }

        let status: Element<'_, Message> = match &state.status {
            Some((message, _)) => {
                let color = if state.succeeded_at().is_some() {
                    theme.ink(0.65 * a)
                } else {
                    theme.danger(a)
                };
                text(message.as_str())
                    .size(TEXT_XS)
                    .color(color)
                    .wrapping(iced::widget::text::Wrapping::None)
                    .width(Fill)
                    .into()
            }
            None => Space::new().width(Fill).into(),
        };
        let footer = row![status, export_button(!state.selected.is_empty(), theme, a)]
            .spacing(space(2))
            .padding(Padding::ZERO.right(space(3)))
            .align_y(iced::Alignment::Center);

        column![header, controls, scrollable(list).height(Fill), footer]
            .spacing(space(3))
            .padding(Padding::new(space(4)).right(space(1)))
            .into()
    };
    paper_card(inner, size, theme, t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exported_message_counts() {
        assert_eq!(exported_message(1), "Exported 1 note");
        assert_eq!(exported_message(2), "Exported 2 notes");
        assert_eq!(exported_message(0), "Exported 0 notes");
    }

    #[test]
    fn only_success_closes_the_panel() {
        let mut state = ExportState::new(&[]);
        assert!(state.succeeded_at().is_none());
        let at = Instant::now();
        state.status = Some((exported_message(2), at));
        assert_eq!(state.succeeded_at(), Some(at));
        let error = std::io::Error::other("disk full");
        state.status = Some((save_error_message(&error), at));
        assert_eq!(state.status.as_ref().unwrap().0, "Couldn't save: disk full");
        assert!(state.succeeded_at().is_none());
    }
}
