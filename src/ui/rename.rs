//! The rename dialog: a box over the window with the file's name in a field,
//! what is wrong with the name as it is being typed, and OK and Cancel.
//! `F2` or the file menu opens it; `Enter` or OK renames, `Esc`, Cancel or a
//! click outside puts it away.
//!
//! A modal rather than a popup: the five menus and the chooser are things
//! the picture can be looked at around, where a rename is a question being
//! asked, and nothing else in the window answers until it is. egui's
//! `Modal` dims everything behind it and takes the pointer; its state is
//! the application's rather than egui's, handed in as [`Input`] on every
//! frame it is up and handed back as commands, since what is in the field
//! and what is wrong with it are the application's to say.
//!
//! What is wrong is worked out by [`judge`] as each key lands, and said
//! under the field as it is found: a name already taken, a slash — the
//! dialog renames and does not move — and, in the caution color rather than
//! the refusing one, an extension that changes. The field's outline turns
//! the warning color with a name that will not do, and OK goes dead with it,
//! so that what `Enter` would do is never in doubt.
//!
//! The words are here; whether a name is taken is the filesystem's answer,
//! which the application asks and hands to [`judge`] as a closure, so that
//! the judgment itself can be tested against nothing but strings.

use std::path::Path;

use egui::{Align, Button, Frame, Key, Modifiers, RichText, Sense, TextEdit, vec2};

use super::chrome::Pass;
use super::control::{Command, Control};
use super::style::{MENU_PADDING, MENU_RADIUS, TOGGLE_RADIUS};
use super::{PADDING, TEXT_SIZE, help, menu};

/// The modal's id in egui's memory.
pub fn id() -> egui::Id {
    egui::Id::new("rename")
}

/// The dialog's width, and the least a window has to be for it to open
/// at all: narrower than this the field could not show a name. Cut for
/// a name rather than a path: it is the one thing the field holds.
const WIDTH: f32 = 280.0;
const WIDTH_MIN: f32 = 200.0;
/// The field, and the room under it kept for the line about the name —
/// kept whether or not there is one, so that the dialog does not grow and
/// shrink under the hand as the name is typed.
const FIELD_HEIGHT: f32 = 28.0;
const MESSAGE_HEIGHT: f32 = 18.0;
/// The gap between the field and that line, and between it and the
/// buttons.
const GAP: f32 = 8.0;
/// The field's text is inset this far from its edge, and the hairline
/// around it.
const FIELD_INSET: f32 = 8.0;
const HAIRLINE: f32 = 1.0;
/// The two buttons at the foot, each this wide.
const BUTTON: [f32; 2] = [60.0, 24.0];

/// What the dialog is drawn from, on each frame it is up.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Input {
    /// The name the file has now, which the dialog is headed with.
    pub current: String,
    /// What the field says.
    pub name: String,
    /// What is wrong with it, or nothing.
    pub verdict: Verdict,
    /// Whether this is the first frame the dialog is up, on which the field
    /// takes the keyboard with the name's stem selected — the part that is
    /// usually the one being changed.
    pub opened: bool,
}

/// What can be made of the name in the field.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// The file's name as it is: nothing to do.
    Unchanged,
    Empty,
    /// The dialog renames; it does not move.
    Slash,
    /// `.`, `..`, or a name a filesystem refuses outright.
    NotAName,
    /// A file of that name is already in the directory.
    Taken,
    /// Can be done — with the extension changed, where it is.
    Fine(Option<ExtensionChange>),
}

/// The extension changing, which is not refused but is worth a word: a
/// file called `.png` that holds a JPEG opens in fewer places than it did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtensionChange {
    /// The extension as it was, and as it will be, each without its dot
    /// and each `None` for no extension at all.
    pub from: Option<String>,
    pub to: Option<String>,
}

/// Which ink a line about the name is set in.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tone {
    /// The name will not do.
    Refusal,
    /// It will, but here is what it changes.
    Caution,
}

impl Verdict {
    /// Whether OK does anything.
    pub fn allows(&self) -> bool {
        matches!(self, Verdict::Fine(_))
    }

    /// The line under the field, and how it is set — `None` for nothing
    /// to say.
    pub fn message(&self) -> Option<(String, Tone)> {
        let refused = |words: &str| Some((words.to_string(), Tone::Refusal));
        match self {
            Verdict::Unchanged | Verdict::Fine(None) => None,
            Verdict::Empty => refused("Type a name."),
            Verdict::Slash => refused("A name cannot hold a slash: the file stays where it is."),
            Verdict::NotAName => refused("That is not a name a file can have."),
            Verdict::Taken => refused("A file of that name is already there."),
            Verdict::Fine(Some(change)) => {
                let dotted = |extension: &str| format!(".{extension}");
                let words = match (&change.from, &change.to) {
                    (Some(from), Some(to)) => format!(
                        "The extension changes from {} to {}.",
                        dotted(from),
                        dotted(to)
                    ),
                    (Some(from), None) => format!("The name loses its {} extension.", dotted(from)),
                    (None, Some(to)) => format!("The name gains a {} extension.", dotted(to)),
                    (None, None) => return None,
                };
                Some((words, Tone::Caution))
            }
        }
    }
}

/// What can be made of `typed` as a new name for the file called `current`,
/// `exists` saying whether a file of a given name is already in its
/// directory.
///
/// In order of what would stop the rename first: a name unchanged is
/// nothing to do whatever else is true of it, and a name that is not a
/// name is not worth asking the directory about.
pub fn judge(current: &str, typed: &str, exists: impl FnOnce(&str) -> bool) -> Verdict {
    if typed == current {
        return Verdict::Unchanged;
    }
    if typed.is_empty() {
        return Verdict::Empty;
    }
    if typed.contains('/') {
        return Verdict::Slash;
    }
    if typed == "." || typed == ".." || typed.contains('\0') {
        return Verdict::NotAName;
    }
    if exists(typed) {
        return Verdict::Taken;
    }
    let extension = |name: &str| {
        Path::new(name)
            .extension()
            .map(|extension| extension.to_string_lossy().into_owned())
    };
    let (from, to) = (extension(current), extension(typed));
    let same = match (&from, &to) {
        (Some(from), Some(to)) => from.eq_ignore_ascii_case(to),
        (None, None) => true,
        _ => false,
    };
    Verdict::Fine((!same).then_some(ExtensionChange { from, to }))
}

/// How many chars of `name` a fresh dialog selects: the stem, up to the
/// last dot — the whole name for one with no extension, or a dot-file.
pub fn stem_chars(name: &str) -> usize {
    match name.rfind('.') {
        Some(dot) if dot > 0 => name[..dot].chars().count(),
        _ => name.chars().count(),
    }
}

/// Draws the dialog, and reads what was pressed in it.
pub(super) fn show(pass: &mut Pass, ui: &mut egui::Ui, input: &Input) {
    let width = WIDTH.min(pass.input.logical[0] - 2.0 * PADDING);
    if width < WIDTH_MIN {
        return;
    }
    let theme = pass.theme;
    let frame = Frame::NONE
        .fill(theme.menu_background.into())
        .stroke(egui::Stroke::new(help::HAIRLINE, theme.border))
        .corner_radius(MENU_RADIUS)
        .inner_margin(MENU_PADDING);
    let inside = width - 2.0 * MENU_PADDING;
    let response = egui::Modal::new(id()).frame(frame).show(ui.ctx(), |ui| {
        ui.set_width(inside);
        ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
        keys(pass, ui, input);
        // Headed with the file's name, so that the question says which
        // file it is about.
        let title = format!("{} {}", Control::Rename.label(), input.current);
        menu::titled(pass, ui, &title, |pass, ui| {
            field(pass, ui, input, inside);
            ui.add_space(GAP);
            message(pass, ui, input, inside);
            ui.add_space(GAP);
            buttons(pass, ui, input);
        });
    });
    if response.should_close() {
        pass.press(Control::CancelRename);
    }
}

/// `Enter`, taken before the field can see it: the rename where the name
/// will do, nothing where it will not, and the dialog put away where the
/// name is the one the file already has. `Esc` is the modal's own.
fn keys(pass: &mut Pass, ui: &mut egui::Ui, input: &Input) {
    let entered = ui.input_mut(|keys| keys.consume_key(Modifiers::NONE, Key::Enter));
    if !entered {
        return;
    }
    match input.verdict {
        Verdict::Fine(_) => pass.press(Control::RenameTo),
        Verdict::Unchanged => pass.press(Control::CancelRename),
        _ => {}
    }
}

/// The field, in a hand-drawn box as the chooser's is: outlined in the
/// warning color while the name will not do, and in the hairline otherwise.
/// On the frame the dialog opens, the stem is selected and the field takes
/// the keyboard; and it takes the keyboard again whenever nothing has it,
/// so that a click on the dialog's own frame does not leave the keys going
/// to the window.
fn field(pass: &mut Pass, ui: &mut egui::Ui, input: &Input, width: f32) {
    let theme = pass.theme;
    let (rect, _) = ui.allocate_exact_size(vec2(width, FIELD_HEIGHT), Sense::HOVER);
    let outline = match input.verdict.message() {
        Some((_, Tone::Refusal)) => theme.warning,
        _ => theme.border,
    };
    let painter = ui.painter();
    painter.rect_filled(rect, TOGGLE_RADIUS, theme.bar_background);
    painter.rect_stroke(
        rect,
        TOGGLE_RADIUS,
        egui::Stroke::new(HAIRLINE, outline),
        egui::StrokeKind::Inside,
    );
    let field_id = id().with("name");
    let mut text = input.name.clone();
    let edit = TextEdit::singleline(&mut text)
        .id(field_id)
        .return_key(None)
        .frame(Frame::NONE)
        .font(egui::FontId::proportional(TEXT_SIZE))
        .text_color(theme.text_primary.into())
        .desired_width(f32::INFINITY)
        .vertical_align(Align::Center)
        .margin(egui::Margin::ZERO);
    let inner = rect.shrink2(vec2(FIELD_INSET, 0.0));
    // Laid out as `Ui::put` would lay it, filling the box with the words
    // centered in it, but through `show` so that the field's state — its
    // selection — comes back with the response.
    let output = ui
        .scope_builder(
            egui::UiBuilder::new()
                .max_rect(inner)
                .layout(egui::Layout::centered_and_justified(
                    egui::Direction::TopDown,
                )),
            |ui| edit.show(ui),
        )
        .inner;
    if output.response.changed() {
        pass.commands.push(Command::Name(text));
    }
    if input.opened {
        let mut state = output.state;
        state
            .cursor
            .set_char_range(Some(egui::text::CCursorRange::two(
                egui::text::CCursor::new(0),
                egui::text::CCursor::new(stem_chars(&input.name)),
            )));
        state.store(ui.ctx(), field_id);
        output.response.request_focus();
    } else if ui.memory(|memory| memory.focused().is_none()) {
        output.response.request_focus();
    }
}

/// The line under the field: what is wrong with the name, in the warning
/// ink, or what it changes, in the caution ink — and the room for one
/// either way.
fn message(pass: &mut Pass, ui: &mut egui::Ui, input: &Input, width: f32) {
    let theme = pass.theme;
    let (rect, _) = ui.allocate_exact_size(vec2(width, MESSAGE_HEIGHT), Sense::HOVER);
    let Some((words, tone)) = input.verdict.message() else {
        return;
    };
    let ink = match tone {
        Tone::Refusal => theme.warning,
        Tone::Caution => theme.caution,
    };
    let galley = ui.ctx().fonts_mut(|fonts| {
        fonts.layout_job({
            let mut job = egui::text::LayoutJob::simple_singleline(
                words,
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
        ink.into(),
    );
}

/// Cancel and OK, at the right, OK dead while the name will not do.
fn buttons(pass: &mut Pass, ui: &mut egui::Ui, input: &Input) {
    ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
        ui.spacing_mut().item_spacing = vec2(GAP, 0.0);
        let ok = ui.add_enabled(
            input.verdict.allows(),
            Button::new(RichText::new(Control::RenameTo.label())).min_size(BUTTON.into()),
        );
        if ok.clicked() {
            pass.press(Control::RenameTo);
        }
        let cancel = ui
            .add(Button::new(RichText::new(Control::CancelRename.label())).min_size(BUTTON.into()));
        if cancel.clicked() {
            pass.press(Control::CancelRename);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What is asked of a name, in the order it is asked: the name as it is
    /// is nothing to do, then whether it is a name at all, then whether the
    /// directory has room for it.
    #[test]
    fn a_name_is_judged_before_the_directory_is_asked() {
        let never = |_: &str| false;
        assert_eq!(judge("a.png", "a.png", never), Verdict::Unchanged);
        assert_eq!(judge("a.png", "", never), Verdict::Empty);
        assert_eq!(judge("a.png", "b/c.png", never), Verdict::Slash);
        assert_eq!(judge("a.png", "..", never), Verdict::NotAName);
        assert_eq!(judge("a.png", ".", never), Verdict::NotAName);
        assert_eq!(
            judge("a.png", "b.png", |name| name == "b.png"),
            Verdict::Taken
        );
        // The name as it is is not asked about, even though it is there.
        assert_eq!(judge("a.png", "a.png", |_| true), Verdict::Unchanged);
        assert_eq!(judge("a.png", "b.png", never), Verdict::Fine(None));
    }

    /// An extension that changes is allowed and said; one that changes
    /// only in case is not a change.
    #[test]
    fn a_changed_extension_is_allowed_with_a_word() {
        let never = |_: &str| false;
        let change = |from: Option<&str>, to: Option<&str>| {
            Verdict::Fine(Some(ExtensionChange {
                from: from.map(str::to_string),
                to: to.map(str::to_string),
            }))
        };
        assert_eq!(
            judge("a.jpg", "a.png", never),
            change(Some("jpg"), Some("png"))
        );
        assert_eq!(judge("a.jpg", "a", never), change(Some("jpg"), None));
        assert_eq!(judge("a", "a.jpg", never), change(None, Some("jpg")));
        assert_eq!(judge("a.JPG", "a.jpg", never), Verdict::Fine(None));
        assert_eq!(judge(".hidden", "shown", never), Verdict::Fine(None));

        let said = |verdict: Verdict| verdict.message().expect("something to say");
        assert_eq!(
            said(change(Some("jpg"), Some("png"))),
            (
                "The extension changes from .jpg to .png.".to_string(),
                Tone::Caution
            )
        );
        assert_eq!(
            said(change(Some("jpg"), None)).0,
            "The name loses its .jpg extension."
        );
        assert_eq!(
            said(change(None, Some("png"))).0,
            "The name gains a .png extension."
        );
        assert_eq!(Verdict::Fine(None).message(), None);
        assert_eq!(Verdict::Unchanged.message(), None);
        for refused in [
            Verdict::Empty,
            Verdict::Slash,
            Verdict::NotAName,
            Verdict::Taken,
        ] {
            assert_eq!(said(refused.clone()).1, Tone::Refusal, "{refused:?}");
            assert!(!refused.allows());
        }
        assert!(Verdict::Fine(None).allows());
        assert!(!Verdict::Unchanged.allows());
    }

    /// The stem is what a fresh dialog selects: everything before the last
    /// dot, or the whole of a name without one.
    #[test]
    fn the_stem_is_selected() {
        assert_eq!(stem_chars("sunset.jpg"), 6);
        assert_eq!(stem_chars("archive.tar.gz"), 11);
        assert_eq!(stem_chars("README"), 6);
        assert_eq!(stem_chars(".hidden"), 7);
        assert_eq!(stem_chars("été.png"), 3);
    }
}
