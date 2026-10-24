//! The info panel's Output tab: what the last command run on the file on
//! screen printed, line by line, under a line saying which command it was,
//! how long it took and how it ended.
//!
//! The output is shown, never obeyed — what a command asks of the window
//! comes back another way; see `commands.rs`. The tab is the one place a
//! script's free text goes: the text found in a picture, the tags a tool
//! read, what went wrong.

use std::sync::Arc;
use std::time::Duration;

use egui::{Align, Sense, WidgetInfo, WidgetType, pos2, vec2};

use super::TEXT_SIZE;
use super::chrome::{ICON_SIDE, Pass};
use super::control::Control;
use super::info::LABEL_SIZE;
use super::style::{SCROLLBAR_GUTTER, SCROLLBAR_WIDTH, TOGGLE_RADIUS};
use super::tooltip::Tip;

/// One line of a run's output.
#[derive(Clone, PartialEq, Debug)]
pub struct Line {
    pub text: String,
    /// Said on the error stream, or by the window about the run, rather
    /// than printed: set in the warning's ink.
    pub error: bool,
}

/// How a run ended, as the tab's head line says it.
#[derive(Clone, PartialEq, Debug)]
pub enum Ended {
    /// It exited with this code; zero is success.
    Exited(i32),
    /// A signal ended it.
    Killed,
    /// It never began.
    Unstarted,
}

/// One run's output, kept for the file it ran on.
#[derive(Clone, PartialEq, Debug)]
pub struct Run {
    pub label: String,
    pub took: Duration,
    pub ended: Ended,
    pub lines: Vec<Line>,
    /// Whether its output was kept on disk, for the editor to open.
    pub logged: bool,
}

impl Run {
    /// The head line: the command, how long it took and how it ended.
    pub fn header(&self) -> String {
        let ended = match self.ended {
            Ended::Exited(0) => "ok".to_string(),
            Ended::Exited(code) => format!("exit {code}"),
            Ended::Killed => "stopped".to_string(),
            Ended::Unstarted => "did not start".to_string(),
        };
        format!("{} \u{b7} {} \u{b7} {ended}", self.label, took(self.took))
    }

    /// The whole of the output, as the tab's copy takes it.
    pub fn text(&self) -> String {
        self.lines
            .iter()
            .map(|line| format!("{}\n", line.text))
            .collect()
    }
}

/// How long a run took, in the unit that says it in the fewest figures.
pub fn took(duration: Duration) -> String {
    let millis = duration.as_millis();
    if millis < 1000 {
        format!("{millis} ms")
    } else if millis < 60_000 {
        format!("{:.1} s", duration.as_secs_f64())
    } else {
        let seconds = duration.as_secs();
        format!("{}:{:02}", seconds / 60, seconds % 60)
    }
}

/// What the tab shows.
#[derive(Clone, PartialEq, Debug)]
pub enum State {
    /// No command has run on this file.
    Nothing,
    /// A command is running on it, by its label.
    Running(String),
    /// The last run on it.
    Shown(Arc<Run>),
}

/// What a frame draws the tab from.
#[derive(Clone, PartialEq, Debug)]
pub struct Input {
    /// The file on screen, which the scroll is kept for.
    pub file: String,
    pub state: State,
}

/// What the tab says with nothing to show.
pub const NOTHING: &str = "No command has run on this file.";

/// What the button on the head line says, which opens the log.
const LOG: &str = "Open log";

/// How tall the head line is, and each line of the output.
const HEAD_HEIGHT: f32 = 22.0;
const LINE_HEIGHT: f32 = 18.0;
/// The room inside a line, either side of its words.
const LINE_PADDING: f32 = 4.0;

/// Draws the tab's body, `width` across: the head line, and under it the
/// output, a line to a row, each cut to the panel's width and copied whole
/// by a click. The rows are laid out only where they are in view, since a
/// run can print thousands of lines.
pub(super) fn show(pass: &mut Pass, ui: &mut egui::Ui, input: &Input, width: f32) {
    let rest = ui.available_rect_before_wrap();
    match &input.state {
        State::Nothing => said(pass, ui, rest, NOTHING),
        State::Running(_) => {
            let spinner = egui::Rect::from_center_size(rest.center(), egui::Vec2::splat(ICON_SIDE));
            ui.put(
                spinner,
                egui::Spinner::new()
                    .size(ICON_SIDE)
                    .color(pass.theme.text_dim),
            );
        }
        State::Shown(run) => shown(pass, ui, input, run, width),
    }
}

/// The button at the right of the head line that opens the run's log in
/// the editor: its words, outlined, washed under the pointer. Gives back
/// the room it took.
fn log_button(pass: &mut Pass, ui: &mut egui::Ui, head: egui::Rect) -> f32 {
    let theme = pass.theme;
    let galley = ui.painter().layout_no_wrap(
        LOG.to_string(),
        egui::FontId::proportional(LABEL_SIZE),
        egui::Color32::PLACEHOLDER,
    );
    let size = vec2(
        (galley.size().x + 2.0 * LINE_PADDING * 2.0).round(),
        (HEAD_HEIGHT - 2.0 * LINE_PADDING / 2.0).round(),
    );
    let rect = egui::Rect::from_min_size(
        pos2(
            head.right() - SCROLLBAR_GUTTER - size.x,
            head.center().y - size.y / 2.0,
        ),
        size,
    );
    let control = Control::OutputEdit;
    let response = ui.interact(rect, ui.id().with("log"), Sense::CLICK);
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, control.label()));
    let (edge, ink) = if response.hovered() {
        (theme.accent, theme.text_primary)
    } else {
        (theme.border, theme.text_dim)
    };
    let painter = ui.painter();
    painter.rect_filled(rect, TOGGLE_RADIUS, theme.bar_background);
    painter.rect_stroke(
        rect,
        TOGGLE_RADIUS,
        egui::Stroke::new(super::HAIRLINE, edge),
        egui::StrokeKind::Inside,
    );
    let at = rect.center() - galley.size() / 2.0;
    painter.galley(at, galley, ink.into());
    let response = pass.tooltip(response, Tip::Control(control));
    if response.clicked() {
        pass.press(control);
    }
    size.x + SCROLLBAR_GUTTER + LINE_PADDING
}

/// `words`, dim, in the middle of `rect`.
fn said(pass: &Pass, ui: &mut egui::Ui, rect: egui::Rect, words: &str) {
    let ink: egui::Color32 = pass.theme.text_dim.into();
    let mut job = egui::text::LayoutJob::simple(
        words.to_string(),
        egui::FontId::proportional(LABEL_SIZE),
        ink,
        rect.width(),
    );
    job.halign = Align::Center;
    let galley = ui.ctx().fonts_mut(|fonts| fonts.layout_job(job));
    let top = (rect.center().y - galley.size().y / 2.0).round();
    ui.painter().galley(pos2(rect.center().x, top), galley, ink);
}

/// One line laid out to be drawn in `ink` at most `width` across, cut with
/// an ellipsis where it runs past.
fn one_line(ui: &egui::Ui, text: &str, font: egui::FontId, width: f32) -> Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::single_section(
        text.to_string(),
        egui::TextFormat::simple(font, egui::Color32::PLACEHOLDER),
    );
    job.wrap = egui::text::TextWrapping {
        max_width: width.max(0.0),
        max_rows: 1,
        break_anywhere: true,
        overflow_character: Some('\u{2026}'),
    };
    ui.ctx().fonts_mut(|fonts| fonts.layout_job(job))
}

fn shown(pass: &mut Pass, ui: &mut egui::Ui, input: &Input, run: &Run, width: f32) {
    let theme = pass.theme;
    let (head, _) = ui.allocate_exact_size(vec2(width, HEAD_HEIGHT), Sense::HOVER);
    let button = if run.logged {
        log_button(pass, ui, head)
    } else {
        0.0
    };
    let galley = one_line(
        ui,
        &run.header(),
        egui::FontId::proportional(LABEL_SIZE),
        width - 2.0 * LINE_PADDING - button,
    );
    let ink = match run.ended {
        Ended::Exited(0) => theme.text_dim,
        _ => theme.warning,
    };
    ui.painter().galley(
        pos2(
            head.left() + LINE_PADDING,
            head.center().y - galley.size().y / 2.0,
        ),
        galley,
        ink.into(),
    );
    if run.lines.is_empty() {
        let rest = ui.available_rect_before_wrap();
        said(pass, ui, rest, "Printed nothing.");
        return;
    }
    let font = egui::FontId::monospace(TEXT_SIZE);
    ui.spacing_mut().scroll.bar_inner_margin = SCROLLBAR_GUTTER - SCROLLBAR_WIDTH;
    egui::ScrollArea::vertical()
        .id_salt(("output", input.file.as_str()))
        .auto_shrink(false)
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
        .show_rows(ui, LINE_HEIGHT, run.lines.len(), |ui, range| {
            let across = width - SCROLLBAR_GUTTER;
            for index in range {
                let line = &run.lines[index];
                let control = Control::OutputRow(index);
                let (rect, response) =
                    ui.allocate_exact_size(vec2(across, LINE_HEIGHT), Sense::CLICK);
                response
                    .widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, control.label()));
                if response.hovered() {
                    ui.painter()
                        .rect_filled(rect, TOGGLE_RADIUS, theme.button_hover);
                }
                let galley = one_line(ui, &line.text, font.clone(), across - 2.0 * LINE_PADDING);
                let ink = if line.error {
                    theme.warning
                } else {
                    theme.text_primary
                };
                ui.painter().galley(
                    pos2(
                        rect.left() + LINE_PADDING,
                        rect.center().y - galley.size().y / 2.0,
                    ),
                    galley,
                    ink.into(),
                );
                let response = pass.tooltip(response, Tip::Control(control));
                if response.clicked() {
                    pass.press(control);
                }
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_run_says_how_long_it_took_and_how_it_ended() {
        let run = Run {
            label: "Redact".into(),
            took: Duration::from_millis(340),
            ended: Ended::Exited(0),
            lines: vec![
                Line {
                    text: "one".into(),
                    error: false,
                },
                Line {
                    text: "two".into(),
                    error: true,
                },
            ],
            logged: true,
        };
        assert_eq!(run.header(), "Redact \u{b7} 340 ms \u{b7} ok");
        assert_eq!(run.text(), "one\ntwo\n");
        assert_eq!(took(Duration::from_millis(1234)), "1.2 s");
        assert_eq!(took(Duration::from_secs(75)), "1:15");
        let failed = Run {
            ended: Ended::Exited(3),
            ..run
        };
        assert!(failed.header().ends_with("exit 3"));
    }
}
