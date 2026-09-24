//! The export dialog: a box over the window with a name for the new file, a
//! choice of JPG or PNG, the size the picture is written at, what the file
//! will not hold, and Export and Cancel. `Ctrl+E` or the file menu opens
//! it; `Enter` or Export writes the file, `Esc`, Cancel or a click outside
//! puts it away.
//!
//! What is written is the picture as the screen shows it — turned, cropped
//! to the region where one is up, and through the window, exposure, curve
//! and false color — and the dialog says what the new file loses that the
//! screen would not: [`warnings`] reads the [`Facts`] the application hands
//! over as the dialog opens, one line each. None of them refuse; only the
//! name and the size can.
//!
//! A modal for the reason the rename dialog is one, and drawn with its
//! pieces — see `ui::rename`. The name is judged by [`judge`] against a
//! closure the application hands in, so that a name already taken is
//! refused rather than written over, and the judgment can be tested against
//! nothing but strings. The format follows an extension typed in the field,
//! and the field's extension follows a format chosen by its button, so the
//! two never disagree about what is written.
//!
//! The size is three boxes — a percentage, a width and a height — that
//! say one thing between them, since the aspect is locked: [`Resize`]
//! holds what each says, and typing in any one rewrites the other two.
//! A box that does not parse, or asks for a side under one pixel or over
//! [`SIDE_MAX`], is outlined and said under the row, and Export goes dead
//! until it is put right; the last size that would do is kept, so the
//! warnings still speak of something. The picture is enlarged bicubic
//! whatever the screen's own filter, and the dialog warns where that is
//! not what the screen shows.

use std::path::Path;

use egui::{Key, Label, Modifiers, RichText, Sense, vec2};

use super::chrome::Pass;
use super::control::{Command, Control};
use super::rename::{self, FIELD_HEIGHT, GAP, NameField, Tone, WIDTH_MIN};
use super::slider::{self, Line};
use super::style::MENU_PADDING;
use super::{PADDING, Rect, TEXT_SIZE, icon, menu};
use crate::image::encode;
use crate::render::Upscale;

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
/// How wide each of the size's boxes is: room for five digits, or a
/// percentage to two decimals and its sign.
const BOX: f32 = 76.0;
/// The slider's word, the size row's, and the heading's.
const QUALITY: &str = "Quality";
const SIZE: &str = "Size";
const WARNINGS: &str = "Warnings";
/// The largest side the picture is written at: `MAX_TEXTURE_DIMENSION`,
/// the largest this program would show, and past what any encoder here
/// is worth asking for.
pub const SIDE_MAX: u32 = 1 << 15;

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

/// One of the three boxes the size is typed in, in the order they stand.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dimension {
    Percent,
    Width,
    Height,
}

impl Dimension {
    pub const ALL: [Dimension; 3] = [Dimension::Percent, Dimension::Width, Dimension::Height];

    /// Which of [`Resize::typed`] this box's text is.
    fn index(self) -> usize {
        match self {
            Dimension::Percent => 0,
            Dimension::Width => 1,
            Dimension::Height => 2,
        }
    }

    /// The box's name in egui's memory, and in the accessibility tree.
    fn name(self) -> &'static str {
        match self {
            Dimension::Percent => "percent",
            Dimension::Width => "width",
            Dimension::Height => "height",
        }
    }
}

/// What is wrong with a size box.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SizeVerdict {
    NotANumber,
    /// A side under one pixel, or a percentage of nothing.
    TooSmall,
    /// A side over [`SIDE_MAX`] — the one typed, or the one it would make
    /// of the other.
    TooLarge,
}

impl SizeVerdict {
    pub fn message(self) -> (String, Tone) {
        let words = match self {
            SizeVerdict::NotANumber => "Type a number.".to_string(),
            SizeVerdict::TooSmall => "A side is at least 1 pixel.".to_string(),
            SizeVerdict::TooLarge => format!("A side is at most {SIDE_MAX} pixels."),
        };
        (words, Tone::Refusal)
    }
}

/// The size the picture is written at, as the three boxes hold it.
///
/// The aspect is locked to the source's: whichever box is typed in, the
/// other two are worked out from it and rewritten, while the box being
/// typed in keeps its text as typed, so that a `.` on its way to a decimal
/// is not taken away. What is typed is read as a whole: a side is a
/// whole number of pixels from one to [`SIDE_MAX`], a percentage any
/// positive number, and a side that comes out under a pixel is one pixel
/// rather than none. A box that will not do keeps its text, is named in
/// `refused`, and leaves `size` at the last size that would.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Resize {
    /// What the picture is — the region, where one is up — in the turned
    /// picture: what the percentage is of.
    pub source: [u32; 2],
    /// What each box says, by [`Dimension::index`].
    pub typed: [String; 3],
    /// The size the export comes out at: the last that would do.
    pub size: [u32; 2],
    /// Which box will not do, and why.
    pub refused: Option<(Dimension, SizeVerdict)>,
}

impl Resize {
    /// The picture at its own size: 100%, and its width and height.
    pub fn new(source: [u32; 2]) -> Self {
        Resize {
            source,
            typed: [
                percent_text(100.0),
                source[0].to_string(),
                source[1].to_string(),
            ],
            size: source,
            refused: None,
        }
    }

    /// `dimension`'s box now says `text`: the size follows it where it
    /// can, and the other two boxes follow the size.
    pub fn edit(&mut self, dimension: Dimension, text: String) {
        self.typed[dimension.index()] = text;
        let typed = self.typed[dimension.index()].trim();
        let [width, height] = self.source.map(f64::from);
        let outcome = match dimension {
            Dimension::Percent => percent(typed)
                .and_then(|percent| sized([width * percent / 100.0, height * percent / 100.0])),
            Dimension::Width => {
                side(typed).and_then(|w| sized([f64::from(w), f64::from(w) * height / width]))
            }
            Dimension::Height => {
                side(typed).and_then(|h| sized([f64::from(h) * width / height, f64::from(h)]))
            }
        };
        match outcome {
            Ok(size) => {
                self.size = size;
                self.refused = None;
                for other in Dimension::ALL {
                    if other != dimension {
                        self.typed[other.index()] = match other {
                            Dimension::Percent => percent_text(f64::from(size[0]) / width * 100.0),
                            Dimension::Width => size[0].to_string(),
                            Dimension::Height => size[1].to_string(),
                        };
                    }
                }
            }
            Err(verdict) => self.refused = Some((dimension, verdict)),
        }
    }

    /// Whether Export does anything, as far as the size is concerned.
    pub fn allows(&self) -> bool {
        self.refused.is_none()
    }

    /// Whether `dimension`'s box is the one that will not do.
    pub fn refuses(&self, dimension: Dimension) -> bool {
        self.refused
            .is_some_and(|(refused, _)| refused == dimension)
    }

    /// The line under the row — `None` for nothing to say.
    pub fn message(&self) -> Option<(String, Tone)> {
        self.refused.map(|(_, verdict)| verdict.message())
    }

    /// Whether the picture grows along either side: what is written is
    /// then enlarged bicubic, whatever the screen does.
    pub fn enlarges(&self) -> bool {
        self.size[0] > self.source[0] || self.size[1] > self.source[1]
    }
}

/// A percentage as typed, with or without its sign: any positive number.
fn percent(typed: &str) -> Result<f64, SizeVerdict> {
    let number = typed.strip_suffix('%').map_or(typed, str::trim_end);
    let percent: f64 = number.parse().map_err(|_| SizeVerdict::NotANumber)?;
    if !percent.is_finite() {
        return Err(SizeVerdict::NotANumber);
    }
    if percent <= 0.0 {
        return Err(SizeVerdict::TooSmall);
    }
    Ok(percent)
}

/// A side as typed: a whole number of pixels, at least one and at most
/// [`SIDE_MAX`].
fn side(typed: &str) -> Result<u32, SizeVerdict> {
    let side: u64 = typed.parse().map_err(|_| SizeVerdict::NotANumber)?;
    if side == 0 {
        return Err(SizeVerdict::TooSmall);
    }
    if side > u64::from(SIDE_MAX) {
        return Err(SizeVerdict::TooLarge);
    }
    Ok(side as u32)
}

/// The size a pair of sides worked out in proportion comes to: each
/// rounded to a pixel, never under one, and refused over [`SIDE_MAX`].
fn sized(sides: [f64; 2]) -> Result<[u32; 2], SizeVerdict> {
    let mut size = [0; 2];
    for (slot, side) in size.iter_mut().zip(sides) {
        let rounded = side.round().max(1.0);
        if rounded > f64::from(SIDE_MAX) {
            return Err(SizeVerdict::TooLarge);
        }
        *slot = rounded as u32;
    }
    Ok(size)
}

/// A percentage as the box writes it: to two decimals, the trailing zeros
/// and a bare point left off, so that 100 is `100` and a third is `33.33`.
fn percent_text(percent: f64) -> String {
    let text = format!("{percent:.2}");
    let trimmed = text.trim_end_matches('0').trim_end_matches('.');
    trimmed.to_string()
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
    /// How the screen enlarges the picture, which an export enlarged
    /// bicubic may not match.
    pub upscale: Upscale,
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
    /// The picture is enlarged, bicubic, while the screen shows nearest.
    Bicubic,
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
            Warning::Bicubic => "Enlarged bicubic, where the screen shows nearest.".into(),
        }
    }
}

/// What the dialog says about writing a picture of `facts` as `format` at
/// the size `resize` holds, in the order it is said: what is lost from
/// every pixel first, then what is left out, then what is made up.
pub fn warnings(facts: Facts, format: Format, resize: &Resize) -> Vec<Warning> {
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
    if resize.enlarges() && facts.upscale == Upscale::Nearest {
        said.push(Warning::Bicubic);
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
    /// What the size boxes say, and what they come to.
    pub resize: Resize,
    pub verdict: Verdict,
    pub warnings: Vec<Warning>,
    /// Whether this is the first frame the dialog is up — see
    /// `rename::Input::opened`.
    pub opened: bool,
}

/// Whether Export does anything: the name will do, and so will the size.
fn allowed(input: &Input) -> bool {
    input.verdict.allows() && input.resize.allows()
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
                    suffix: None,
                    primary: true,
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
            ui.add_space(GAP / 2.0);
            rule(pass, ui, inside);
            ui.add_space(GAP / 2.0);
            size_row(pass, ui, &input.resize);
            ui.add_space(GAP);
            if let Some(said) = input.resize.message() {
                rename::line(pass, ui, inside, Some(said));
                ui.add_space(GAP);
            }
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
                allowed(input),
            );
        });
    });
    if response.should_close() {
        pass.press(Control::CancelExport);
    }
}

/// `Enter`, taken before the fields can see it: the export where the name
/// and the size will do, nothing where they will not. `Esc` is the
/// modal's own.
fn keys(pass: &mut Pass, ui: &mut egui::Ui, input: &Input) {
    let entered = ui.input_mut(|keys| keys.consume_key(Modifiers::NONE, Key::Enter));
    if entered && allowed(input) {
        pass.press(Control::ExportTo);
    }
}

/// The size: its word, then the three boxes — the percentage with its
/// sign inside, and the width and height with a times sign between —
/// each drawn as the name's field is, outlined while it will not do.
fn size_row(pass: &mut Pass, ui: &mut egui::Ui, resize: &Resize) {
    ui.horizontal(|ui| {
        // As tall as the boxes before anything is placed, so that the
        // words are centered on them rather than on the row egui would
        // have started with.
        ui.set_min_height(FIELD_HEIGHT);
        ui.spacing_mut().item_spacing = vec2(GAP, 0.0);
        ui.add(Label::new(SIZE));
        for dimension in Dimension::ALL {
            if dimension == Dimension::Height {
                ui.add(Label::new("\u{00d7}"));
            }
            rename::name_field(
                pass,
                ui,
                NameField {
                    id: id().with(dimension.name()),
                    name: &resize.typed[dimension.index()],
                    refused: resize.refuses(dimension),
                    opened: false,
                    width: BOX,
                    suffix: (dimension == Dimension::Percent).then_some("%"),
                    primary: false,
                },
                move |text| Command::ExportSize(dimension, text),
            );
        }
    });
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
    if rule_from < rect.right() {
        hairline(pass, ui, rule_from..=rect.right(), rect.center().y);
    }
}

/// A rule across the dialog, between the formats and the size: as much
/// room as a gap, with the hairline through the middle of it.
fn rule(pass: &mut Pass, ui: &mut egui::Ui, width: f32) {
    let (rect, _) = ui.allocate_exact_size(vec2(width, GAP), Sense::HOVER);
    hairline(pass, ui, rect.left()..=rect.right(), rect.center().y);
}

/// A hairline from side to side of `across`, centered on `y`. On the
/// device's grid, as every rule is: a hairline that straddled two rows
/// of pixels would come out as two gray ones.
fn hairline(pass: &Pass, ui: &egui::Ui, across: std::ops::RangeInclusive<f32>, y: f32) {
    let thickness = pass.grid.line_width(HAIRLINE);
    let top = pass.grid.snap(y - thickness / 2.0);
    ui.painter().rect_filled(
        egui::Rect::from_x_y_ranges(across, top..=top + thickness),
        0.0,
        pass.theme.border,
    );
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
            upscale: Upscale::Nearest,
        };
        let same = Resize::new([10, 20]);
        // Nothing lost: nothing to say, whatever the format.
        assert!(warnings(plain, Format::Png, &same).is_empty());
        assert!(warnings(plain, Format::Jpeg, &same).is_empty());

        let everything = Facts {
            deeper_than_8_bit: true,
            alpha: true,
            frames: Frames::Animation { frame: 4 },
            metadata: true,
            region: Some([10, 20]),
            source: None,
            upscale: Upscale::Nearest,
        };
        let mut bigger = Resize::new([10, 20]);
        bigger.edit(Dimension::Percent, "200".to_string());
        assert_eq!(
            warnings(everything, Format::Jpeg, &bigger),
            [
                Warning::Flattened,
                Warning::AlphaDropped,
                Warning::OneFrame(5),
                Warning::MetadataDropped,
                Warning::RegionOnly([10, 20]),
                Warning::Bicubic,
            ]
        );
        // PNG keeps alpha.
        assert!(!warnings(everything, Format::Png, &bigger).contains(&Warning::AlphaDropped));
        let pages = Facts {
            frames: Frames::Pages { page: 2 },
            ..plain
        };
        assert_eq!(
            warnings(pages, Format::Png, &same),
            [Warning::OnePage(3)],
            "counted from one"
        );
        // The bicubic warning is for a picture enlarged past what the
        // screen would show as nearest: a shrink says nothing, and a
        // screen already bicubic has nothing to be told.
        let mut smaller = Resize::new([10, 20]);
        smaller.edit(Dimension::Percent, "50".to_string());
        assert!(warnings(plain, Format::Png, &smaller).is_empty());
        let bicubic = Facts {
            upscale: Upscale::Bicubic,
            ..plain
        };
        assert!(warnings(bicubic, Format::Png, &bigger).is_empty());
        assert_eq!(warnings(plain, Format::Png, &bigger), [Warning::Bicubic]);
        assert_eq!(Warning::OnePage(3).words(), "Exporting page 3");
        assert_eq!(
            Warning::RegionOnly([10, 20]).words(),
            "Exporting a 10 \u{00d7} 20 crop."
        );
    }

    /// Typing in any one box rewrites the other two in proportion, and the
    /// box typed in keeps its text as typed.
    #[test]
    fn the_three_boxes_say_one_size() {
        let mut resize = Resize::new([800, 600]);
        assert_eq!(resize.typed, ["100", "800", "600"]);
        assert_eq!(resize.size, [800, 600]);
        assert!(resize.allows());

        resize.edit(Dimension::Width, "400".to_string());
        assert_eq!(resize.typed, ["50", "400", "300"]);
        assert_eq!(resize.size, [400, 300]);

        resize.edit(Dimension::Height, "150".to_string());
        assert_eq!(resize.typed, ["25", "200", "150"]);

        resize.edit(Dimension::Percent, "33.3".to_string());
        assert_eq!(resize.typed, ["33.3", "266", "200"], "as typed");
        assert_eq!(resize.size, [266, 200]);
        resize.edit(Dimension::Percent, "33.".to_string());
        assert_eq!(resize.typed[0], "33.", "on its way to a decimal");
        assert_eq!(resize.size, [264, 198]);
        resize.edit(Dimension::Percent, "200%".to_string());
        assert_eq!(resize.size, [1600, 1200]);
        assert!(resize.enlarges());

        // A width that is not a whole share: the percentage is written to
        // two decimals.
        resize.edit(Dimension::Width, "2".to_string());
        assert_eq!(resize.typed, ["0.25", "2", "2"]);
        assert_eq!(percent_text(100.0), "100");
        assert_eq!(percent_text(12.5), "12.5");
        assert_eq!(percent_text(1.0 / 3.0 * 100.0), "33.33");
    }

    /// A box that will not do is named with why, leaves the size at the
    /// last that would, and Export goes dead until it is put right.
    #[test]
    fn a_box_that_will_not_do_is_refused_and_the_size_kept() {
        let mut resize = Resize::new([800, 600]);
        resize.edit(Dimension::Width, "".to_string());
        assert_eq!(
            resize.refused,
            Some((Dimension::Width, SizeVerdict::NotANumber))
        );
        assert!(resize.refuses(Dimension::Width));
        assert!(!resize.refuses(Dimension::Height));
        assert!(!resize.allows());
        assert_eq!(resize.size, [800, 600], "the last size that would do");
        assert_eq!(resize.typed, ["100", "", "600"], "the others stand");
        assert_eq!(
            resize.message(),
            Some(("Type a number.".to_string(), Tone::Refusal))
        );
        resize.edit(Dimension::Width, "0".to_string());
        assert_eq!(
            resize.refused,
            Some((Dimension::Width, SizeVerdict::TooSmall))
        );
        resize.edit(Dimension::Width, "32769".to_string());
        assert_eq!(
            resize.refused,
            Some((Dimension::Width, SizeVerdict::TooLarge))
        );
        resize.edit(Dimension::Width, "32768".to_string());
        assert_eq!(resize.size, [32768, 24576]);
        assert!(resize.allows());
        // The other side is held to the ceiling too.
        resize.edit(Dimension::Height, "32768".to_string());
        assert_eq!(
            resize.refused,
            Some((Dimension::Height, SizeVerdict::TooLarge))
        );
        assert_eq!(resize.size, [32768, 24576]);
        for wrong in ["", "-5", "abc", "1e400", "nan"] {
            resize.edit(Dimension::Percent, wrong.to_string());
            assert!(!resize.allows(), "{wrong:?}");
        }
        resize.edit(Dimension::Percent, "0".to_string());
        assert_eq!(
            resize.refused,
            Some((Dimension::Percent, SizeVerdict::TooSmall))
        );
        resize.edit(Dimension::Percent, "9999".to_string());
        assert_eq!(
            resize.refused,
            Some((Dimension::Percent, SizeVerdict::TooLarge))
        );
        resize.edit(Dimension::Percent, "0.01".to_string());
        assert_eq!(resize.size, [1, 1], "under a pixel is a pixel");
        assert!(resize.allows());
        assert_eq!(
            SizeVerdict::TooLarge.message().0,
            "A side is at most 32768 pixels."
        );
    }
}
