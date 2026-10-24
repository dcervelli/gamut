//! A command the configuration file defines, as the interface knows it:
//! what it needs before it can run, the inputs its dialog asks for, and
//! the dialog itself.
//!
//! The commands themselves are the application's — see `commands.rs` for
//! the grammar and the run, and `app/commands.rs` for the run in flight.
//! What is here is what a frame is drawn from: the menu's items, each
//! carrying what the command needs so that the tooltip can say why one is
//! dead, and the rows of the dialog.

use std::fmt;

use egui::{Key, Modifiers, Sense, vec2};

use super::chrome::Pass;
use super::control::{Command, Control};
use super::rename::{self, Boxed, FIELD_HEIGHT, GAP, Tone};
use super::slider::{self, Line};
use super::{Rect, TEXT_SIZE};

/// What a command needs before it can run; the menu item and the key are
/// dead without it.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Needs {
    /// A file on screen: what most commands act on.
    #[default]
    File,
    /// A region marked out on the picture.
    Region,
    /// The pointer on the picture, which a press from the menu never has.
    Pointer,
    /// A file that says where it was taken.
    Location,
    /// A file whose pixels are placed on the ground.
    Georeference,
    /// Nothing at all: a command that runs from the empty window too.
    Nothing,
}

impl Needs {
    /// Every need, in the order the configuration file's words list them.
    pub const ALL: [Needs; 6] = [
        Needs::File,
        Needs::Region,
        Needs::Pointer,
        Needs::Location,
        Needs::Georeference,
        Needs::Nothing,
    ];

    /// The word the configuration file writes it as.
    pub fn word(self) -> &'static str {
        match self {
            Needs::File => "file",
            Needs::Region => "region",
            Needs::Pointer => "pointer",
            Needs::Location => "location",
            Needs::Georeference => "georeference",
            Needs::Nothing => "none",
        }
    }

    /// The need the configuration file's `word` names.
    pub fn read(word: &str) -> Result<Needs, String> {
        Needs::ALL
            .into_iter()
            .find(|needs| needs.word() == word)
            .ok_or_else(|| {
                let words: Vec<&str> = Needs::ALL.iter().map(|needs| needs.word()).collect();
                format!("needs one of {}, not `{word}`", words.join(", "))
            })
    }
}

/// A command as a control carries it: its place in the set the frame was
/// drawn from, and what it needs, which is what makes it dead.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Slot {
    pub index: usize,
    pub needs: Needs,
}

#[cfg(test)]
impl Slot {
    /// The one [`super::Control::ALL`] holds.
    pub const SAMPLE: Slot = Slot {
        index: 0,
        needs: Needs::File,
    };
}

/// One command on the file menu: the words on its item, whether it opens a
/// dialog before it runs, and the control a press on it is.
#[derive(Clone, PartialEq, Debug)]
pub struct Item {
    pub label: String,
    pub asks: bool,
    pub slot: Slot,
    /// Whether a press on it would be taken: what it needs is there, and
    /// no other command is running.
    pub enabled: bool,
}

/// One input of the dialog as a frame draws it: its words, what kind it
/// is, what it holds and what is wrong with that.
#[derive(Clone, PartialEq, Debug)]
pub struct Row {
    pub label: String,
    pub kind: Kind,
    pub value: Value,
    pub refused: Option<Refusal>,
}

/// The dialog as a frame draws it.
#[derive(Clone, PartialEq, Debug)]
pub struct Input {
    /// The command's label, which heads it.
    pub title: String,
    pub rows: Vec<Row>,
    /// Whether OK takes a press: every input holds something that will do,
    /// and no other command is running.
    pub allowed: bool,
    /// Whether another command is running, which is said under the rows.
    pub busy: bool,
    /// Whether this is its first frame, when its first field takes the
    /// keyboard.
    pub opened: bool,
}

/// What an input of the dialog is, with its default.
#[derive(Clone, PartialEq, Debug)]
pub enum Kind {
    /// Words, as typed.
    Text {
        default: String,
    },
    /// A number typed, judged as it is typed: finite, and inside the range
    /// where there is one.
    Number {
        range: Option<(f64, f64)>,
        default: f64,
    },
    Checkbox {
        default: bool,
    },
    /// One of a few words.
    Radio {
        choices: Vec<String>,
        default: usize,
    },
    Slider {
        low: f64,
        high: f64,
        default: f64,
        step: f64,
    },
}

/// What an input holds as the dialog is filled in.
#[derive(Clone, PartialEq, Debug)]
pub enum Value {
    Text(String),
    /// As typed, which may not be a number yet.
    Number(String),
    Checkbox(bool),
    /// The choice's place among the input's choices.
    Radio(usize),
    Slider(f64),
}

/// A number as the environment and the dialog write it: as few places as
/// say it, and no exponent.
pub fn decimal(value: f64) -> String {
    let written = format!("{value}");
    if written == "-0" {
        "0".to_string()
    } else {
        written
    }
}

impl Kind {
    /// An input as the configuration file writes it: its kind's word, and
    /// what that kind takes after it, whitespace between.
    pub fn read(value: &str) -> Result<Kind, String> {
        let value = value.trim();
        let (word, rest) = value
            .split_once(char::is_whitespace)
            .map_or((value, ""), |(word, rest)| (word, rest.trim()));
        let tokens: Vec<&str> = rest.split_whitespace().collect();
        let number = |token: &str| -> Result<f64, String> {
            token
                .parse::<f64>()
                .ok()
                .filter(|number| number.is_finite())
                .ok_or_else(|| format!("`{token}` is not a number"))
        };
        match word {
            "string" => Ok(Kind::Text {
                default: rest.to_string(),
            }),
            "number" => {
                let numbers = tokens
                    .iter()
                    .map(|token| number(token))
                    .collect::<Result<Vec<f64>, String>>()?;
                let (range, default) = match numbers[..] {
                    [] => (None, 0.0),
                    [default] => (None, default),
                    [low, high] => (Some((low, high)), low),
                    [low, high, default] => (Some((low, high)), default),
                    _ => return Err("a number takes at most a least, a most and a default".into()),
                };
                if let Some((low, high)) = range {
                    if low >= high {
                        return Err(format!(
                            "{} is not less than {}",
                            decimal(low),
                            decimal(high)
                        ));
                    }
                    if !(low..=high).contains(&default) {
                        return Err(format!(
                            "the default {} is not between {} and {}",
                            decimal(default),
                            decimal(low),
                            decimal(high)
                        ));
                    }
                }
                Ok(Kind::Number { range, default })
            }
            "checkbox" => match tokens[..] {
                [] | ["false"] => Ok(Kind::Checkbox { default: false }),
                ["true"] => Ok(Kind::Checkbox { default: true }),
                _ => Err(format!("a checkbox is true or false, not `{rest}`")),
            },
            "radio" => {
                let (choices, default) = match rest.split_once('=') {
                    Some((choices, default)) => (choices, Some(default.trim())),
                    None => (rest, None),
                };
                let choices: Vec<String> = choices.split_whitespace().map(str::to_string).collect();
                if choices.len() < 2 {
                    return Err("a radio needs at least two choices".into());
                }
                for (at, choice) in choices.iter().enumerate() {
                    if choices[..at].contains(choice) {
                        return Err(format!("the choice `{choice}` is there twice"));
                    }
                }
                let default = match default {
                    None => 0,
                    Some(default) => choices
                        .iter()
                        .position(|choice| choice == default)
                        .ok_or_else(|| format!("the default `{default}` is not a choice"))?,
                };
                Ok(Kind::Radio { choices, default })
            }
            "slider" => {
                let numbers = tokens
                    .iter()
                    .map(|token| number(token))
                    .collect::<Result<Vec<f64>, String>>()?;
                let (low, high, default, step) = match numbers[..] {
                    [low, high] => (low, high, None, None),
                    [low, high, default] => (low, high, Some(default), None),
                    [low, high, default, step] => (low, high, Some(default), Some(step)),
                    _ => {
                        return Err(
                            "a slider takes a least and a most, then a default and a step".into(),
                        );
                    }
                };
                if low >= high {
                    return Err(format!(
                        "{} is not less than {}",
                        decimal(low),
                        decimal(high)
                    ));
                }
                let default = default.unwrap_or(low);
                if !(low..=high).contains(&default) {
                    return Err(format!(
                        "the default {} is not between {} and {}",
                        decimal(default),
                        decimal(low),
                        decimal(high)
                    ));
                }
                let whole = low.fract() == 0.0 && high.fract() == 0.0;
                let step = step.unwrap_or(if whole { 1.0 } else { (high - low) / 100.0 });
                if step <= 0.0 {
                    return Err(format!("the step {} is not more than 0", decimal(step)));
                }
                Ok(Kind::Slider {
                    low,
                    high,
                    default,
                    step,
                })
            }
            "" => Err("an input needs a kind: string, number, checkbox, radio or slider".into()),
            other => Err(format!(
                "`{other}` is not a kind of input: string, number, checkbox, radio or slider"
            )),
        }
    }

    /// What the input holds before anything is done to it.
    pub fn default_value(&self) -> Value {
        match self {
            Kind::Text { default } => Value::Text(default.clone()),
            Kind::Number { default, .. } => Value::Number(decimal(*default)),
            Kind::Checkbox { default } => Value::Checkbox(*default),
            Kind::Radio { default, .. } => Value::Radio(*default),
            Kind::Slider { default, .. } => Value::Slider(*default),
        }
    }

    /// Whether `value` is one this kind holds: what decides whether a
    /// value remembered from before the file was edited still fits.
    pub fn holds(&self, value: &Value) -> bool {
        match (self, value) {
            (Kind::Text { .. }, Value::Text(_)) | (Kind::Number { .. }, Value::Number(_)) => true,
            (Kind::Checkbox { .. }, Value::Checkbox(_)) => true,
            (Kind::Radio { choices, .. }, Value::Radio(at)) => *at < choices.len(),
            (Kind::Slider { low, high, .. }, Value::Slider(value)) => {
                (*low..=*high).contains(value)
            }
            _ => false,
        }
    }

    /// What is wrong with `value` as this input's, if anything: only a
    /// number typed can be wrong.
    pub fn judge(&self, value: &Value) -> Option<Refusal> {
        let (Kind::Number { range, .. }, Value::Number(typed)) = (self, value) else {
            return None;
        };
        let Some(number) = typed
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|number| number.is_finite())
        else {
            return Some(Refusal::NotANumber);
        };
        match range {
            Some((low, high)) if !(*low..=*high).contains(&number) => Some(Refusal::Outside {
                low: *low,
                high: *high,
            }),
            _ => None,
        }
    }

    /// `value` as the environment hands it to the script.
    pub fn environment(&self, value: &Value) -> String {
        match (self, value) {
            (_, Value::Text(text)) => text.clone(),
            (_, Value::Number(typed)) => typed
                .trim()
                .parse::<f64>()
                .map_or_else(|_| typed.trim().to_string(), decimal),
            (_, Value::Checkbox(on)) => on.to_string(),
            (Kind::Radio { choices, .. }, Value::Radio(at)) => {
                choices.get(*at).cloned().unwrap_or_default()
            }
            (_, Value::Radio(at)) => at.to_string(),
            (_, Value::Slider(value)) => decimal(*value),
        }
    }
}

/// What is wrong with a number typed into the dialog.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Refusal {
    NotANumber,
    Outside { low: f64, high: f64 },
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Refusal::NotANumber => write!(f, "Type a number."),
            Refusal::Outside { low, high } => {
                write!(f, "Between {} and {}.", decimal(*low), decimal(*high))
            }
        }
    }
}

/// The dialog's id in egui's memory.
pub fn id() -> egui::Id {
    egui::Id::new("command")
}

/// The dialog's width, given the room.
const WIDTH: f32 = 380.0;
/// The share of the width inside it that the inputs' words take, the rest
/// going to the inputs.
const LABEL_SHARE: f32 = 0.38;
/// The room after a slider for the number it is at.
const SLIDER_VALUE: f32 = 52.0;
/// How tall a row of choices or a checkbox is.
const ROW: f32 = 22.0;

/// What a dialog with nothing to ask says: it is there to be sure.
pub const CONFIRM: &str = "Run this command?";

/// What the dialog says while another command is running, under its rows.
pub const BUSY: &str = "Another command is still running.";

/// Draws the dialog, and reads what was done in it: headed with the
/// command's label, a row to an input — its words at the left, the input
/// at the right, and under a number what is wrong with it — and OK and
/// Cancel at the foot.
pub(super) fn show(pass: &mut Pass, ui: &mut egui::Ui, input: &Input) {
    rename::modal(
        pass,
        ui,
        rename::Dialog {
            id: id(),
            title: &input.title,
            width: WIDTH,
            cancel: Control::CommandCancel,
        },
        |pass, ui| keys(pass, ui, input),
        |pass, ui, inside| {
            let words = (inside * LABEL_SHARE).round();
            let field = inside - words - GAP;
            // The first field typed into takes the keyboard as the dialog
            // opens, and whenever nothing else has it.
            let first = input
                .rows
                .iter()
                .position(|row| matches!(row.value, Value::Text(_) | Value::Number(_)));
            if input.rows.is_empty() {
                rename::line(pass, ui, inside, Some((CONFIRM.to_string(), Tone::Caution)));
                ui.add_space(GAP);
            }
            for (index, row) in input.rows.iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.set_min_height(FIELD_HEIGHT);
                    ui.spacing_mut().item_spacing = vec2(GAP, 0.0);
                    label(pass, ui, &row.label, words);
                    let primary = first == Some(index);
                    widget(
                        pass,
                        ui,
                        index,
                        row,
                        field,
                        primary,
                        primary && input.opened,
                    );
                });
                if let Some(refused) = row.refused {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing = vec2(GAP, 0.0);
                        ui.add_space(words);
                        rename::line(pass, ui, field, Some((refused.to_string(), Tone::Refusal)));
                    });
                }
                ui.add_space(GAP / 2.0);
            }
            ui.add_space(GAP / 2.0);
            if input.busy {
                rename::line(pass, ui, inside, Some((BUSY.to_string(), Tone::Caution)));
                ui.add_space(GAP);
            }
            rename::ok_cancel(
                pass,
                ui,
                Control::CommandOk,
                Control::CommandCancel,
                input.allowed,
            );
        },
    );
}

/// `Enter`, taken before the fields can see it: the command run where
/// every input will do, nothing where one will not. `Esc` is the modal's.
fn keys(pass: &mut Pass, ui: &mut egui::Ui, input: &Input) {
    let entered = ui.input_mut(|keys| keys.consume_key(Modifiers::NONE, Key::Enter));
    if entered && input.allowed {
        pass.press(Control::CommandOk);
    }
}

/// An input's words, `width` across, cut where they run past it.
fn label(pass: &Pass, ui: &mut egui::Ui, words: &str, width: f32) {
    let (rect, _) = ui.allocate_exact_size(vec2(width, FIELD_HEIGHT), Sense::HOVER);
    let galley = ui.ctx().fonts_mut(|fonts| {
        fonts.layout_job({
            let mut job = egui::text::LayoutJob::simple_singleline(
                words.to_string(),
                egui::FontId::proportional(TEXT_SIZE),
                egui::Color32::PLACEHOLDER,
            );
            job.wrap.max_width = width;
            job.wrap.max_rows = 1;
            job.wrap.break_anywhere = true;
            job
        })
    });
    ui.painter().galley(
        egui::pos2(rect.left(), rect.center().y - galley.size().y / 2.0),
        galley,
        pass.theme.text_primary.into(),
    );
}

/// The input of `row`, the `index`th, `width` across: a box to type in, a
/// checkbox, a choice of words or a slider. What it is changed to goes back
/// as [`Command::DialogInput`]. A `primary` box takes the keyboard back
/// whenever nothing has it, and on the dialog's first frame, `opened`, it
/// takes it with all it holds selected.
fn widget(
    pass: &mut Pass,
    ui: &mut egui::Ui,
    index: usize,
    row: &Row,
    width: f32,
    primary: bool,
    opened: bool,
) {
    match (&row.kind, &row.value) {
        (_, Value::Text(text) | Value::Number(text)) => {
            let id = id().with(("input", index));
            let (output, typed) = rename::boxed_field(
                pass,
                ui,
                Boxed {
                    id,
                    text,
                    refused: row.refused.is_some(),
                    size: vec2(width, FIELD_HEIGHT),
                    suffix: None,
                },
            );
            if let Some(typed) = typed {
                let value = match row.value {
                    Value::Number(_) => Value::Number(typed),
                    _ => Value::Text(typed),
                };
                pass.commands.push(Command::DialogInput(index, value));
            }
            if opened {
                let mut state = output.state;
                state
                    .cursor
                    .set_char_range(Some(egui::text::CCursorRange::two(
                        egui::text::CCursor::new(0),
                        egui::text::CCursor::new(text.chars().count()),
                    )));
                state.store(ui.ctx(), id);
                output.response.request_focus();
            } else if primary && ui.memory(|memory| memory.focused().is_none()) {
                output.response.request_focus();
            }
        }
        (_, Value::Checkbox(on)) => {
            let mut checked = *on;
            ui.allocate_ui(vec2(width, ROW), |ui| {
                if ui.checkbox(&mut checked, "").changed() {
                    pass.commands
                        .push(Command::DialogInput(index, Value::Checkbox(checked)));
                }
            });
        }
        (Kind::Radio { choices, .. }, Value::Radio(chosen)) => {
            ui.allocate_ui(vec2(width, ROW), |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = vec2(GAP, GAP / 2.0);
                    for (at, choice) in choices.iter().enumerate() {
                        if ui.radio(at == *chosen, choice).clicked() && at != *chosen {
                            pass.commands
                                .push(Command::DialogInput(index, Value::Radio(at)));
                        }
                    }
                });
            });
        }
        (
            Kind::Slider {
                low, high, step, ..
            },
            Value::Slider(value),
        ) => {
            let (room, _) =
                ui.allocate_exact_size(vec2(width - SLIDER_VALUE - GAP, ROW), Sense::HOVER);
            let ground = pass.theme.menu_background;
            let asked = slider::show(
                pass,
                ui,
                Line {
                    name: &row.label,
                    value: *value as f32,
                    low: *low as f32,
                    high: *high as f32,
                    step: *step as f32,
                    origin: *low as f32,
                    tip: None,
                    live: true,
                    ground,
                },
                Rect::new(room.left(), room.top(), room.width(), room.height()),
            );
            if let Some(asked) = asked {
                let snapped = snap(f64::from(asked), *low, *high, *step);
                pass.commands
                    .push(Command::DialogInput(index, Value::Slider(snapped)));
            }
            ui.add_sized(
                vec2(SLIDER_VALUE, ROW),
                egui::Label::new(decimal(*value)).truncate(),
            );
        }
        // A value of another kind than its input is never handed over.
        _ => {}
    }
}

/// `value` on the slider's steps from `low`, inside its ends, rid of the
/// float's last digits so that a step of a tenth reads as one.
pub fn snap(value: f64, low: f64, high: f64, step: f64) -> f64 {
    let stepped = low + ((value - low) / step).round() * step;
    (stepped.clamp(low, high) * 1e9).round() / 1e9
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each kind reads its own words, and says what is wrong with words it
    /// cannot read.
    #[test]
    fn every_kind_reads_its_grammar_and_refuses_what_it_cannot() {
        assert_eq!(
            Kind::read("string"),
            Ok(Kind::Text {
                default: String::new()
            })
        );
        assert_eq!(
            Kind::read("string  Draft, do not use "),
            Ok(Kind::Text {
                default: "Draft, do not use".into()
            })
        );
        assert_eq!(
            Kind::read("number"),
            Ok(Kind::Number {
                range: None,
                default: 0.0
            })
        );
        assert_eq!(
            Kind::read("number 7"),
            Ok(Kind::Number {
                range: None,
                default: 7.0
            })
        );
        assert_eq!(
            Kind::read("number 1 10"),
            Ok(Kind::Number {
                range: Some((1.0, 10.0)),
                default: 1.0
            })
        );
        assert_eq!(
            Kind::read("number 1 10 4"),
            Ok(Kind::Number {
                range: Some((1.0, 10.0)),
                default: 4.0
            })
        );
        assert_eq!(
            Kind::read("checkbox true"),
            Ok(Kind::Checkbox { default: true })
        );
        assert_eq!(
            Kind::read("checkbox"),
            Ok(Kind::Checkbox { default: false })
        );
        assert_eq!(
            Kind::read("radio png jpg webp = jpg"),
            Ok(Kind::Radio {
                choices: vec!["png".into(), "jpg".into(), "webp".into()],
                default: 1
            })
        );
        assert_eq!(
            Kind::read("radio png jpg"),
            Ok(Kind::Radio {
                choices: vec!["png".into(), "jpg".into()],
                default: 0
            })
        );
        assert_eq!(
            Kind::read("slider 1 50 10"),
            Ok(Kind::Slider {
                low: 1.0,
                high: 50.0,
                default: 10.0,
                step: 1.0
            })
        );
        assert_eq!(
            Kind::read("slider 0 0.5"),
            Ok(Kind::Slider {
                low: 0.0,
                high: 0.5,
                default: 0.0,
                step: 0.005
            })
        );
        for refused in [
            "",
            "colour red",
            "number ten",
            "number 5 1",
            "number 1 5 9",
            "number 1 2 3 4",
            "checkbox maybe",
            "radio png",
            "radio png png",
            "radio png jpg = gif",
            "slider 1",
            "slider 1 5 9",
            "slider 1 5 2 0",
            "slider 1 5 2 1 1",
        ] {
            assert!(Kind::read(refused).is_err(), "{refused}");
        }
    }

    /// A number is judged as typed, and every value is written for the
    /// environment as the script will read it.
    #[test]
    fn a_number_is_judged_and_every_value_written_for_the_script() {
        let number = Kind::read("number 1 10").unwrap();
        assert_eq!(number.judge(&Value::Number("4".into())), None);
        assert_eq!(
            number.judge(&Value::Number("four".into())),
            Some(Refusal::NotANumber)
        );
        assert_eq!(
            number.judge(&Value::Number("11".into())),
            Some(Refusal::Outside {
                low: 1.0,
                high: 10.0
            })
        );
        assert_eq!(
            Refusal::Outside {
                low: 1.0,
                high: 10.0
            }
            .to_string(),
            "Between 1 and 10."
        );
        assert_eq!(number.environment(&Value::Number(" 4.50 ".into())), "4.5");
        let radio = Kind::read("radio png jpg").unwrap();
        assert_eq!(radio.environment(&Value::Radio(1)), "jpg");
        let slider = Kind::read("slider 0 1").unwrap();
        assert_eq!(slider.environment(&Value::Slider(0.25)), "0.25");
        assert!(slider.holds(&Value::Slider(1.0)));
        assert!(!slider.holds(&Value::Slider(2.0)));
        assert!(!slider.holds(&Value::Checkbox(true)));
        let checkbox = Kind::read("checkbox").unwrap();
        assert_eq!(checkbox.environment(&Value::Checkbox(true)), "true");
    }

    #[test]
    fn every_need_is_read_from_its_word() {
        for needs in Needs::ALL {
            assert_eq!(Needs::read(needs.word()), Ok(needs));
        }
        assert!(Needs::read("everything").is_err());
    }
}
