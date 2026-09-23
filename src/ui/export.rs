//! The export dialog: a box over the window with a name for the new file, a
//! choice of JPG or PNG, what the file will not hold, and Export and Cancel.
//! `Ctrl+E` or the file menu opens it; `Enter` or Export writes the
//! file, `Esc`, Cancel or a click outside puts it away.
//!
//! What is written is the picture as the screen shows it — turned, cropped
//! to the region where one is up, and through the window, exposure, curve
//! and false color — and the dialog says what the new file loses that the
//! screen would not: [`warnings`] reads the [`Facts`] the application hands
//! over as the dialog opens, one line each. None of them refuse; only the
//! name can.
//!
//! A modal for the reason the rename dialog is one, and drawn with its
//! pieces — see `ui::rename`. The name is judged by [`judge`] against a
//! closure the application hands in, so that a name already taken is
//! refused rather than written over, and the judgment can be tested against
//! nothing but strings. The format follows an extension typed in the field,
//! and the field's extension follows a format chosen by its button, so the
//! two never disagree about what is written.

use std::path::Path;

use egui::{Key, Label, Modifiers, RichText, Sense, vec2};

use super::chrome::Pass;
use super::control::{Command, Control};
use super::rename::{self, GAP, NameField, Tone, WIDTH_MIN};
use super::slider::{self, Line};
use super::style::MENU_PADDING;
use super::{PADDING, Rect, TEXT_SIZE, icon, menu};
use crate::image::encode;

/// The modal's id in egui's memory.
pub fn id() -> egui::Id {
    egui::Id::new("export")
}

/// Wider than the rename dialog's: the warnings are sentences.
const WIDTH: f32 = 360.0;
/// How far JPG's quality is set in under it.
const INDENT: f32 = 20.0;
/// How wide the quality's slider is.
const SLIDER: f32 = 140.0;
/// The height of a format's row, and of the warnings' heading.
const ROW: f32 = 20.0;
/// What the warning mark before the heading is sized to.
const ICON: f32 = 14.0;
/// The heading's rule.
const HAIRLINE: f32 = 1.0;
/// The slider's word, and the heading's.
const QUALITY: &str = "Quality";
const WARNINGS: &str = "Warnings";

/// What the file is written as.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Format {
    Png,
    Jpeg,
}

impl Format {
    /// The two, in the order their buttons stand.
    pub const ALL: [Format; 2] = [Format::Png, Format::Jpeg];

    pub fn label(self) -> &'static str {
        match self {
            Format::Png => "PNG",
            Format::Jpeg => "JPG",
        }
    }

    /// The extension a file of this format is given, without its dot.
    pub fn extension(self) -> &'static str {
        match self {
            Format::Png => "png",
            Format::Jpeg => "jpg",
        }
    }

    /// The format an extension, without its dot, names, in any case.
    pub fn of_extension(extension: &str) -> Option<Format> {
        match extension.to_ascii_lowercase().as_str() {
            "png" => Some(Format::Png),
            "jpg" | "jpeg" => Some(Format::Jpeg),
            _ => None,
        }
    }
}

/// The format `name`'s extension names, if it names one of the two.
pub fn format_of(name: &str) -> Option<Format> {
    Path::new(name)
        .extension()
        .and_then(|extension| Format::of_extension(&extension.to_string_lossy()))
}

/// `name` with its extension made `format`'s: the last one replaced, or
/// one added to a name without any.
pub fn with_extension(name: &str, format: Format) -> String {
    Path::new(name)
        .with_extension(format.extension())
        .to_string_lossy()
        .into_owned()
}

/// What a file exported from one called `stem` is called by default: the stem
/// with `-edited` after it, counted up past any name `exists` says is taken.
pub fn default_name(stem: &str, format: Format, exists: impl Fn(&str) -> bool) -> String {
    let extension = format.extension();
    let mut name = format!("{stem}-edited.{extension}");
    let mut count = 2;
    while exists(&name) {
        name = format!("{stem}-edited-{count}.{extension}");
        count += 1;
    }
    name
}

/// What can be made of the name in the field.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Verdict {
    Empty,
    /// The dialog writes beside the file on screen; it does not choose a
    /// directory.
    Slash,
    /// `.`, `..`, or a name a filesystem refuses outright.
    NotAName,
    /// A file of that name is already in the directory, and is not written
    /// over.
    Taken,
    Fine,
}

impl Verdict {
    /// Whether Export does anything.
    pub fn allows(&self) -> bool {
        *self == Verdict::Fine
    }

    /// The line under the field — `None` for nothing to say.
    pub fn message(&self) -> Option<(String, Tone)> {
        let words = match self {
            Verdict::Fine => return None,
            Verdict::Empty => "Type a name.",
            Verdict::Slash => "A name cannot hold a slash: the file goes beside this one.",
            Verdict::NotAName => "That is not a name a file can have.",
            Verdict::Taken => rename::TAKEN,
        };
        Some((words.to_string(), Tone::Refusal))
    }
}

/// What can be made of `typed` as the new file's name, `exists` saying
/// whether a file of that name is already in the directory. A name that is
/// not a name is not worth asking the directory about.
pub fn judge(typed: &str, exists: impl FnOnce(&str) -> bool) -> Verdict {
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
    Verdict::Fine
}

/// What the application knows about the picture being exported, gathered once
/// as the dialog opens.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Facts {
    /// More than 8 bits a channel, or a gain map's lift above white: more
    /// than an 8-bit file holds.
    pub deeper_than_8_bit: bool,
    pub alpha: bool,
    pub frames: Frames,
    /// Whether the file carries metadata, which the new one will not.
    pub metadata: bool,
    /// The region's size, where one is up and only it is written.
    pub region: Option<[u32; 2]>,
    /// The format the file on screen is in, where it is one of the two.
    pub source: Option<Format>,
}

/// How many pictures the file on screen holds.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Frames {
    Still,
    /// An animation, on this frame, counted from zero.
    Animation {
        frame: usize,
    },
    /// A paged file, on this page, counted from zero.
    Pages {
        page: usize,
    },
}

/// One line the dialog says about what is written. Only what the new file
/// loses that a look at the screen would not tell: that the export looks
/// like the screen — its turn, its window and exposure, its curve and false
/// color — is what an export is, and a JPG's loss is what its quality
/// slider is for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Warning {
    Flattened,
    AlphaDropped,
    /// Only this frame is written, counted from one.
    OneFrame(usize),
    /// Only this page is written, counted from one.
    OnePage(usize),
    MetadataDropped,
    RegionOnly([u32; 2]),
}

impl Warning {
    pub fn words(self) -> String {
        match self {
            Warning::Flattened => "Export, 8-bit SDR, losing precision.".into(),
            Warning::AlphaDropped => "JPG doesn't support alpha.".into(),
            Warning::OneFrame(frame) => format!("Exporting frame {frame}"),
            Warning::OnePage(page) => format!("Exporting page {page}"),
            Warning::MetadataDropped => "Metadata are not exported.".into(),
            Warning::RegionOnly([width, height]) => {
                format!("Exporting a {width} \u{00d7} {height} crop.")
            }
        }
    }
}

/// What the dialog says about writing a picture of `facts` as `format`,
/// in the order it is said: what is lost from every pixel first, then
/// what is left out.
pub fn warnings(facts: Facts, format: Format) -> Vec<Warning> {
    let mut said = Vec::new();
    if facts.deeper_than_8_bit {
        said.push(Warning::Flattened);
    }
    if facts.alpha && format == Format::Jpeg {
        said.push(Warning::AlphaDropped);
    }
    match facts.frames {
        Frames::Still => {}
        Frames::Animation { frame } => said.push(Warning::OneFrame(frame + 1)),
        Frames::Pages { page } => said.push(Warning::OnePage(page + 1)),
    }
    if facts.metadata {
        said.push(Warning::MetadataDropped);
    }
    if let Some(size) = facts.region {
        said.push(Warning::RegionOnly(size));
    }
    said
}

/// What the dialog is drawn from, on each frame it is up.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Input {
    /// The name of the file on screen, which the dialog is headed with.
    pub source: String,
    /// What the field says.
    pub name: String,
    pub format: Format,
    /// What a JPEG is written at, from `encode::JPEG_QUALITY_MIN` to 100.
    pub quality: u8,
    pub verdict: Verdict,
    pub warnings: Vec<Warning>,
    /// Whether this is the first frame the dialog is up — see
    /// `rename::Input::opened`.
    pub opened: bool,
}

/// Draws the dialog, and reads what was pressed in it.
pub(super) fn show(pass: &mut Pass, ui: &mut egui::Ui, input: &Input) {
    let width = WIDTH.min(pass.input.logical[0] - 2.0 * PADDING);
    if width < WIDTH_MIN {
        return;
    }
    let frame = rename::dialog_frame(pass);
    let inside = width - 2.0 * MENU_PADDING;
    let response = egui::Modal::new(id()).frame(frame).show(ui.ctx(), |ui| {
        ui.set_width(inside);
        ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
        keys(pass, ui, input);
        let title = format!("{}\u{2026}", Control::Export.label());
        menu::titled(pass, ui, &title, |pass, ui| {
            rename::name_field(
                pass,
                ui,
                NameField {
                    id: id().with("name"),
                    name: &input.name,
                    refused: !input.verdict.allows(),
                    opened: input.opened,
                    width: inside,
                },
                Command::ExportName,
            );
            ui.add_space(GAP);
            // Room for what is wrong with the name only while something
            // is: the formats sit right under the field otherwise.
            if let Some(said) = input.verdict.message() {
                rename::line(pass, ui, inside, Some(said));
                ui.add_space(GAP);
            }
            formats(pass, ui, input.format, input.quality);
            ui.add_space(GAP);
            if !input.warnings.is_empty() {
                heading(pass, ui, inside);
                ui.add_space(GAP / 2.0);
                said(pass, ui, &input.warnings, inside);
                ui.add_space(GAP);
            }
            rename::ok_cancel(
                pass,
                ui,
                Control::ExportTo,
                Control::CancelExport,
                input.verdict.allows(),
            );
        });
    });
    if response.should_close() {
        pass.press(Control::CancelExport);
    }
}

/// `Enter`, taken before the field can see it: the export where the name
/// will do, nothing where it will not. `Esc` is the modal's own.
fn keys(pass: &mut Pass, ui: &mut egui::Ui, input: &Input) {
    let entered = ui.input_mut(|keys| keys.consume_key(Modifiers::NONE, Key::Enter));
    if entered && input.verdict.allows() {
        pass.press(Control::ExportTo);
    }
}

/// The two formats as radio buttons, one under the other, and under JPG
/// its quality: the word, the interface's slider (see `ui::slider`) and the
/// number, set in by [`INDENT`] so that it reads as JPG's. Live only while
/// JPG is the format, since it is JPG's alone.
fn formats(pass: &mut Pass, ui: &mut egui::Ui, format: Format, quality: u8) {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing = vec2(GAP, GAP / 2.0);
        for choice in Format::ALL {
            ui.horizontal(|ui| {
                ui.set_min_height(ROW);
                let radio = ui.radio(choice == format, choice.label());
                if radio.clicked() {
                    pass.press(Control::ExportAs(choice));
                }
            });
            if choice == Format::Jpeg {
                ui.horizontal(|ui| {
                    ui.set_min_height(ROW);
                    ui.add_space(INDENT);
                    let live = format == Format::Jpeg;
                    ui.add_enabled(live, Label::new(QUALITY));
                    let (room, _) = ui.allocate_exact_size(vec2(SLIDER, ROW), Sense::HOVER);
                    let ground = pass.theme.menu_background;
                    let asked = slider::show(
                        pass,
                        ui,
                        Line {
                            name: QUALITY,
                            value: f32::from(quality),
                            low: f32::from(encode::JPEG_QUALITY_MIN),
                            high: 100.0,
                            step: 1.0,
                            origin: f32::from(encode::JPEG_QUALITY_MIN),
                            tip: None,
                            live,
                            ground,
                        },
                        Rect::new(room.left(), room.top(), room.width(), room.height()),
                    );
                    if let Some(asked) = asked {
                        pass.commands.push(Command::ExportQuality(asked as u8));
                    }
                    ui.add_enabled(live, Label::new(quality.to_string()));
                });
            }
        }
    });
}

/// The heading over the warnings: the warning mark and the word in the
/// ordinary ink, and a hairline running on to the dialog's edge.
fn heading(pass: &mut Pass, ui: &mut egui::Ui, width: f32) {
    let theme = pass.theme;
    let (rect, _) = ui.allocate_exact_size(vec2(width, ROW), Sense::HOVER);
    let mark = icon::square(
        pass.grid,
        egui::Rect::from_min_size(rect.left_top(), vec2(ROW, ROW)),
        ICON,
    );
    icon::paint(
        ui.painter(),
        icon::TRIANGLE_ALERT,
        mark,
        theme.text_primary.into(),
        theme.menu_background.into(),
    );
    let galley = ui.painter().layout_no_wrap(
        WARNINGS.to_string(),
        egui::FontId::proportional(TEXT_SIZE),
        egui::Color32::PLACEHOLDER,
    );
    let words = egui::pos2(
        mark.right() + GAP / 2.0,
        rect.center().y - galley.size().y / 2.0,
    );
    let rule_from = words.x + galley.size().x + GAP;
    ui.painter()
        .galley(words, galley, theme.text_primary.into());
    // On the device's grid, as every rule is: a hairline that straddled two
    // rows of pixels would come out as two gray ones.
    let thickness = pass.grid.line_width(HAIRLINE);
    let y = pass.grid.snap(rect.center().y - thickness / 2.0);
    if rule_from < rect.right() {
        ui.painter().rect_filled(
            egui::Rect::from_x_y_ranges(rule_from..=rect.right(), y..=y + thickness),
            0.0,
            theme.border,
        );
    }
}

/// The warnings, one to a line and wrapped where a line is long, in the
/// ordinary ink: the heading is what says they are warnings. Labels rather than painted text, so that what the dialog
/// warns of is in the accessibility tree with the rest of it.
fn said(pass: &mut Pass, ui: &mut egui::Ui, warnings: &[Warning], width: f32) {
    let ink = pass.theme.text_primary;
    ui.vertical(|ui| {
        ui.set_width(width);
        ui.spacing_mut().item_spacing.y = GAP / 2.0;
        for warning in warnings {
            ui.add(Label::new(RichText::new(warning.words()).size(TEXT_SIZE).color(ink)).wrap());
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_name_counts_up_past_taken_names() {
        let never = |_: &str| false;
        assert_eq!(default_name("a", Format::Png, never), "a-edited.png");
        assert_eq!(default_name("a", Format::Jpeg, never), "a-edited.jpg");
        let taken = ["a-edited.png", "a-edited-2.png"];
        assert_eq!(
            default_name("a", Format::Png, |name| taken.contains(&name)),
            "a-edited-3.png"
        );
    }

    #[test]
    fn the_format_follows_the_extension_and_the_extension_the_format() {
        assert_eq!(format_of("a.JPEG"), Some(Format::Jpeg));
        assert_eq!(format_of("a.jpg"), Some(Format::Jpeg));
        assert_eq!(format_of("a.Png"), Some(Format::Png));
        assert_eq!(format_of("a.tif"), None);
        assert_eq!(format_of("a"), None);
        assert_eq!(with_extension("a.b.jpg", Format::Png), "a.b.png");
        assert_eq!(with_extension("a", Format::Jpeg), "a.jpg");
        assert_eq!(with_extension("a.png", Format::Jpeg), "a.jpg");
    }

    /// What is asked of a name, in the order it is asked.
    #[test]
    fn a_name_is_judged_before_the_directory_is_asked() {
        let never = |_: &str| false;
        assert_eq!(judge("", never), Verdict::Empty);
        assert_eq!(judge("b/c.png", |_| true), Verdict::Slash);
        assert_eq!(judge("..", |_| true), Verdict::NotAName);
        assert_eq!(judge("b.png", |name| name == "b.png"), Verdict::Taken);
        assert_eq!(judge("b.png", never), Verdict::Fine);
        assert!(Verdict::Fine.allows());
        assert_eq!(Verdict::Fine.message(), None);
        for refused in [
            Verdict::Empty,
            Verdict::Slash,
            Verdict::NotAName,
            Verdict::Taken,
        ] {
            assert!(!refused.allows());
            assert_eq!(refused.message().expect("words").1, Tone::Refusal);
        }
    }

    #[test]
    fn warnings_follow_the_facts() {
        let plain = Facts {
            deeper_than_8_bit: false,
            alpha: false,
            frames: Frames::Still,
            metadata: false,
            region: None,
            source: Some(Format::Png),
        };
        // Nothing lost: nothing to say, whatever the format.
        assert!(warnings(plain, Format::Png).is_empty());
        assert!(warnings(plain, Format::Jpeg).is_empty());

        let everything = Facts {
            deeper_than_8_bit: true,
            alpha: true,
            frames: Frames::Animation { frame: 4 },
            metadata: true,
            region: Some([10, 20]),
            source: None,
        };
        assert_eq!(
            warnings(everything, Format::Jpeg),
            [
                Warning::Flattened,
                Warning::AlphaDropped,
                Warning::OneFrame(5),
                Warning::MetadataDropped,
                Warning::RegionOnly([10, 20]),
            ]
        );
        // PNG keeps alpha.
        assert!(!warnings(everything, Format::Png).contains(&Warning::AlphaDropped));
        let pages = Facts {
            frames: Frames::Pages { page: 2 },
            ..plain
        };
        assert_eq!(
            warnings(pages, Format::Png),
            [Warning::OnePage(3)],
            "counted from one"
        );
        assert_eq!(Warning::OnePage(3).words(), "Exporting page 3");
        assert_eq!(
            Warning::RegionOnly([10, 20]).words(),
            "Exporting a 10 \u{00d7} 20 crop."
        );
    }
}
