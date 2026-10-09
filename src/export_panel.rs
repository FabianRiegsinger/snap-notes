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
use std::path::PathBuf;
use std::time::Instant;
use uuid::Uuid;

/// How the last export went.
#[derive(Debug, Clone, PartialEq)]
pub enum ExportStatus {
    /// This many notes were written.
    Exported(usize),
    /// The write failed for this reason.
    Failed(String),
}

impl ExportStatus {
    /// The panel's one-line message.
    pub fn message(&self) -> String {
        match self {
            ExportStatus::Exported(1) => "Exported 1 note".to_string(),
            ExportStatus::Exported(count) => format!("Exported {count} notes"),
            ExportStatus::Failed(reason) => format!("Couldn't save: {reason}"),
        }
    }
}

/// An export the save dialog returned a path for: the notes (in strip
/// order) and the format chosen when the dialog opened, and the
/// [`ExportState::generation`] of the panel that opened it.
#[derive(Debug, Clone)]
pub struct ExportJob {
    pub generation: u64,
    pub path: PathBuf,
    pub notes: Vec<Uuid>,
    pub format: ExportFormat,
}

/// The open export panel's choices and its last result.
pub struct ExportState {
    pub selected: HashSet<Uuid>,
    pub format: ExportFormat,
    /// The last export's result and when it happened.
    pub status: Option<(ExportStatus, Instant)>,
    /// Tells this opening of the panel from earlier ones, so a dialog an
    /// earlier one started doesn't report into it.
    pub generation: u64,
}

impl ExportState {
    /// Every one of `notes` selected, as Markdown.
    pub fn new(notes: &[Note], generation: u64) -> Self {
        Self {
            selected: notes.iter().map(|n| n.id).collect(),
            format: ExportFormat::Markdown,
            status: None,
            generation,
        }
    }

    /// When the last export succeeded, if it did: the panel closes a moment
    /// after. A failure stays until the next action.
    pub fn succeeded_at(&self) -> Option<Instant> {
        match &self.status {
            Some((ExportStatus::Exported(_), at)) => Some(*at),
            _ => None,
        }
    }

    /// The ids of `notes` that are selected, in their order.
    pub fn selected_in(&self, notes: &[Note]) -> Vec<Uuid> {
        notes
            .iter()
            .map(|note| note.id)
            .filter(|id| self.selected.contains(id))
            .collect()
    }

    /// Whether Export can start, as far as the panel goes: at least one
    /// selected note still exists. No save dialog may be open either.
    pub fn can_export(&self, notes: &[Note]) -> bool {
        notes.iter().any(|note| self.selected.contains(&note.id))
    }
}

pub struct ExportView<'a> {
    pub theme: theme::Theme,
    pub notes: &'a [Note],
    pub state: &'a ExportState,
    /// The Export button can start a dialog.
    pub can_export: bool,
    pub size: Size,
    pub morph_progress: f32,
    pub content_alpha: f32,
}

/// A note's row in the list: its title without the reminder tag.
pub(crate) fn row_label(note: &Note) -> String {
    let title = crate::reminder::display(&note.title);
    if title.is_empty() {
        "Untitled".to_string()
    } else {
        title
    }
}

fn note_row<'a>(note: &Note, checked: bool, theme: theme::Theme, a: f32) -> Element<'a, Message> {
    let id = note.id;
    checkbox(checked)
        .label(row_label(note))
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
        can_export,
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
            Some((status, _)) => {
                let color = match status {
                    ExportStatus::Exported(_) => theme.ink(0.65 * a),
                    ExportStatus::Failed(_) => theme.danger(a),
                };
                text(status.message())
                    .size(TEXT_XS)
                    .color(color)
                    .wrapping(iced::widget::text::Wrapping::None)
                    .width(Fill)
                    .into()
            }
            None => Space::new().width(Fill).into(),
        };
        let footer = row![status, export_button(can_export, theme, a)]
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
    fn status_messages() {
        assert_eq!(ExportStatus::Exported(1).message(), "Exported 1 note");
        assert_eq!(ExportStatus::Exported(2).message(), "Exported 2 notes");
        assert_eq!(ExportStatus::Exported(0).message(), "Exported 0 notes");
        assert_eq!(
            ExportStatus::Failed("disk full".into()).message(),
            "Couldn't save: disk full"
        );
    }

    #[test]
    fn only_success_closes_the_panel() {
        let mut state = ExportState::new(&[], 0);
        assert!(state.succeeded_at().is_none());
        let at = Instant::now();
        state.status = Some((ExportStatus::Exported(2), at));
        assert_eq!(state.succeeded_at(), Some(at));
        state.status = Some((ExportStatus::Failed("disk full".into()), at));
        assert!(state.succeeded_at().is_none());
    }
}
