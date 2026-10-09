//! The Output section: what goes out to the screen. The picture as the
//! display starts from it, every value pushed through the display's
//! response and binned again on the curve the screen is driven with — sRGB's
//! — from black to as far above white as the surface goes, so that what the
//! surface clips piles into the end it is clipped at. The shares clipped at
//! either end are written in its corners.

use std::sync::Arc;

use super::plot::{band, bars, corners, white_line};
use super::*;
use crate::image::Transfer;
use crate::image::display::{Display, Headroom, ToneMap};
use crate::image::stats::Fine;
use crate::ui::FrameInput;

/// The curve the output's axis is spaced on: the one the screen is driven
/// with, so that a display doing nothing to an sRGB file plots the file's
/// own codes where its own plot does.
const ENCODING: Transfer = Transfer::Srgb;

/// The plot of what goes out, and the room above white it runs up to: the
/// display's input rebinned through its response, held between passes for
/// as long as nothing it was worked out from has changed.
pub(super) struct Binned {
    key: Key,
    /// The fine bins it was rebinned from, held so that the key's address
    /// for them cannot be another's while this is kept.
    fine: Arc<Fine>,
    pub plot: Plot,
    /// How far above white the plot's axis runs, as a ratio to white: the
    /// surface's room, or, on a surface whose room is not known, as far as
    /// the response goes.
    pub room: f32,
}

/// What a plot of the output is worked out from: the input's fine bins, by
/// where they are held, and everything the response reads.
#[derive(Clone, Copy, PartialEq, Debug)]
struct Key {
    fine: usize,
    offset: u32,
    gain: u32,
    tone_map: ToneMap,
    false_colored: bool,
    headroom: Headroom,
    room: u32,
}

impl Key {
    fn of(current: &Current, input: &FrameInput) -> Self {
        let (offset, gain) = current.display.transform();
        Self {
            fine: Arc::as_ptr(&current.stats.fine) as usize,
            offset: offset.to_bits(),
            gain: gain.to_bits(),
            tone_map: current.display.tone_map(),
            false_colored: current.display.false_colored(current.image.is_gray()),
            headroom: input.headroom,
            room: input.room.to_bits(),
        }
    }
}

/// The plot of what goes out for `current`, from the pass before where
/// nothing it reads has changed since, and worked out again otherwise.
///
/// Rebinned from the fine bins the input was counted in, never scanned
/// again: a drag of a handle changes the response on every frame, and four
/// thousand bins through it is nothing where two million pixels through it
/// would be a frame each. Kept where egui keeps a pass's data, as the
/// information panel's contents are, since only the panel reads it.
pub(super) fn binned(ui: &egui::Ui, current: &Current, input: &FrameInput) -> Arc<Binned> {
    let id = egui::Id::new("histogram output");
    let key = Key::of(current, input);
    if let Some(held) = ui.data(|data| data.get_temp::<Arc<Binned>>(id))
        && held.key == key
        && Arc::ptr_eq(&held.fine, &current.stats.fine)
    {
        return held;
    }
    let made = Arc::new(rebin(current, input.headroom, input.room, key));
    ui.data_mut(|data| data.insert_temp(id, made.clone()));
    made
}

/// The plot of what goes out for `current`, worked out afresh: what a test
/// holds the trace against, with no pass to keep it in.
#[cfg(test)]
pub(super) fn binned_for(current: &Current, headroom: Headroom, room: f32) -> Binned {
    let input_key = Key {
        fine: Arc::as_ptr(&current.stats.fine) as usize,
        offset: 0,
        gain: 0,
        tone_map: current.display.tone_map(),
        false_colored: false,
        headroom,
        room: room.to_bits(),
    };
    rebin(current, headroom, room, input_key)
}

/// What the display makes of one value on the input's axis, as a ratio to
/// white, never below black.
fn response(display: &Display, transfer: Transfer, gray: bool, headroom: Headroom, v: f32) -> f32 {
    display
        .response(transfer.to_linear(v), gray, headroom)
        .max(0.0)
}

/// The room a plot of the output runs up to: the surface's, where it is
/// known, and otherwise as far as the response takes any value the input
/// holds, rounded up to a whole number of stops above white, and never
/// below white. An HDR surface whose monitor has not said how far it goes
/// has its room put where the picture needs it rather than nowhere.
fn resolved_room(fine: &Fine, room: f32, through: impl Fn(f32) -> f32) -> f32 {
    if room.is_finite() {
        return room.max(1.0);
    }
    let span = fine.max - fine.min;
    let planes = std::iter::once(&fine.luma).chain(fine.color.iter().flatten());
    let mut highest = 1.0f32;
    for plane in planes {
        for (index, &count) in plane.iter().enumerate() {
            if count > 0 {
                let value = fine.min + index as f32 / (plane.len() - 1) as f32 * span;
                let out = through(value);
                if out.is_finite() {
                    highest = highest.max(out);
                }
            }
        }
    }
    highest.log2().ceil().exp2().max(1.0)
}

fn rebin(current: &Current, headroom: Headroom, room: f32, key: Key) -> Binned {
    let fine = &current.stats.fine;
    let display = &current.display;
    let transfer = current.image.color.transfer;
    let gray = current.image.is_gray();
    let room = resolved_room(fine, room, |v| {
        response(display, transfer, gray, headroom, v)
    });
    let plot = fine.rebinned([0.0, ENCODING.to_encoded(room)], |v| {
        ENCODING.to_encoded(response(display, transfer, gray, headroom, v))
    });
    Binned {
        key,
        fine: Arc::clone(fine),
        plot,
        room,
    }
}

/// The output value at the middle of `bin`, as a ratio to white.
fn output_at(binned: &Binned, bin: usize) -> f32 {
    ENCODING.to_linear(bin_value(&binned.plot, bin))
}

/// An output value as the header writes it: a ratio to white, to three
/// places with the zeros it does not need taken off.
fn output_words(value: f32) -> String {
    trimmed(format!("{value:.3}"))
}

/// What the section says with the pointer on its plot at `bin`.
pub(super) fn bin_mark(binned: &Binned, bin: usize) -> Mark {
    Mark {
        bin: Some(bin),
        words: output_words(output_at(binned, bin)),
    }
}

/// What the section says with the pointer on the picture's pixel `(x, y)`:
/// what the screen puts out for it, which is the display's response held
/// to the surface's room.
pub(super) fn pixel_mark(
    current: &Current,
    headroom: Headroom,
    binned: &Binned,
    x: u32,
    y: u32,
) -> Option<Mark> {
    let sample = current.sample(x, y)?;
    let encoded = Plot::value_of(&current.image, &sample)?;
    let out = response(
        &current.display,
        current.image.color.transfer,
        current.image.is_gray(),
        headroom,
        encoded,
    )
    .min(binned.room);
    Some(Mark {
        bin: binned.plot.bin_at(ENCODING.to_encoded(out)),
        words: output_words(out),
    })
}

/// The Output section, laid out at `rect`: the plot of what goes out with
/// white marked where white is not its top, the shares clipped at either
/// end in its corners, and the band of what each output is.
pub(super) fn show(
    pass: &mut Pass,
    ui: &mut egui::Ui,
    current: &Current,
    rect: Rect,
    binned: &Binned,
    mark: Option<&Mark>,
) {
    let theme = pass.theme;
    let gray = current.image.is_gray();
    let plot_rect = Section::Output.plot(rect, gray);
    bars(
        pass,
        ui,
        &binned.plot,
        plot_rect,
        false,
        mark.and_then(|mark| mark.bin),
    );

    let top = ENCODING.to_encoded(binned.room);
    let white = ENCODING.to_encoded(1.0) / top;
    if white < 1.0 - 1e-3 {
        white_line(pass, ui, plot_rect, white);
    }

    // How much of the picture the screen is throwing away, in the corner
    // it is being thrown out of: the share at black in the left corner, and
    // at white or past it in the right — where the surface is actually
    // clipping it, rather than showing it or rolling it off. Only where
    // there is a share to write, so that a corner with a number in it is
    // news.
    //
    // This is the question the panel is most often opened to answer, and a
    // spike against the edge of the plot cannot answer it: the spike says
    // there is clipping and the number says how much.
    let headroom = pass.input.headroom;
    let [below, above] = binned.plot.clipped(0.0, ENCODING.to_encoded(1.0));
    let above = if current.display.clips_white(gray, headroom) {
        above
    } else {
        0.0
    };
    let accent = Color32::from(theme.accent);
    let left = share_words(below).map(|words| (words, accent));
    // The right corner says how far the plot runs where nothing is clipped
    // at white because the surface goes past it: the room, which is then
    // the one end of this axis that is not the same on every screen.
    let right = share_words(above).map(|words| (words, accent)).or_else(|| {
        (binned.room > ABOVE_WHITE).then(|| {
            (
                trimmed(format!("{:.2}", binned.room)),
                Color32::from(theme.text_dim),
            )
        })
    });
    corners(ui, plot_rect, left, right);

    // What each output comes out as, along the foot: gray from black to
    // white, the accent along the top past white where the surface goes
    // there, and under a false color the ramp, read at the output value.
    let display = &current.display;
    let false_colored = display.false_colored(gray);
    band(pass, ui, plot_rect, |t| {
        let out = ENCODING.to_linear(t * top);
        let color = if false_colored {
            display.colormap().color(out.clamp(0.0, 1.0))
        } else {
            [out.clamp(0.0, 1.0); 3]
        };
        (Color::from_linear(color), out > ABOVE_WHITE)
    });

    section::header(
        pass,
        ui,
        Section::Output,
        rect,
        mark.map(|mark| mark.words.as_str()),
    );
}
