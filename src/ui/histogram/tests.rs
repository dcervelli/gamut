use super::display::{Rows, plot_area, row_buttons, swatch_button};
use super::*;
use crate::image::display::{Colormap, Headroom, ToneMap};
use crate::image::gain_map::{GainMap, Lift};
use crate::image::{AlphaMode, Channels, ColorSpace, DecodedImage, Samples};
use crate::ui::side;
use crate::ui::slider::{HANDLE_GRIP, HANDLE_WIDTH};

/// `image`, on screen.
fn shown(image: DecodedImage) -> Current {
    Current::of(image, "file")
}

/// An 8-bit sRGB picture, color or gray, with a ramp across it.
fn picture(gray: bool) -> Current {
    let channels = if gray { Channels::Gray } else { Channels::Rgb };
    let count = channels.count();
    let data = (0..64u32)
        .flat_map(|x| std::iter::repeat_n((x * 4) as u8, count))
        .collect();
    shown(DecodedImage::new(
        64,
        1,
        Samples::U8 { channels, data },
        ColorSpace::SRGB,
        AlphaMode::Opaque,
    ))
}

/// The same color picture with a gray gain map beside it, lifting its right
/// half by two stops.
fn gain_mapped() -> Current {
    let mut image = picture(false).image.as_ref().clone();
    image.gain_map = Some(std::sync::Arc::new(GainMap {
        width: 2,
        height: 1,
        channels: 1,
        data: vec![0, 255],
        lift: Lift::Apple { headroom: 4.0 },
    }));
    shown(image)
}

/// The column laid out from the top of a scroll area `height` tall.
fn laid_out(current: &Current, height: f32) -> Vec<(Section, Rect)> {
    section::layout(Rect::new(0.0, 0.0, COLUMN_WIDTH, height), current)
}

/// Where `section` was laid out.
fn rect_of(layout: &[(Section, Rect)], section: Section) -> Rect {
    layout
        .iter()
        .find(|(laid, _)| *laid == section)
        .map(|(_, rect)| *rect)
        .unwrap_or_else(|| panic!("{section:?} is laid out"))
}

/// The exposure is stepped in quarter stops, so it is written in
/// quarters: a tenth of a stop cannot say what one press is worth, and
/// would read one step as `0.2` and the next but one as `0.8`. Only a
/// value from `--exposure` can be anything else, and it is written back
/// as it was asked for.
#[test]
fn an_exposure_is_written_in_the_quarters_it_is_stepped_in() {
    assert_eq!(stops_label(0.0), "0");
    assert_eq!(stops_label(EV_STEP), "+\u{00bc}");
    assert_eq!(stops_label(-EV_STEP), "-\u{00bc}");
    assert_eq!(stops_label(0.5), "+\u{00bd}");
    assert_eq!(stops_label(-0.75), "-\u{00be}");
    assert_eq!(stops_label(1.0), "+1");
    assert_eq!(stops_label(-1.25), "-1\u{00bc}");
    assert_eq!(
        stops_label(-16.0),
        "-16",
        "the far end of what a key reaches"
    );
    assert_eq!(stops_label(0.1), "+0.10");
}

/// A share of the picture is written so that a small one is still
/// something and a large one is not four digits: nothing at all for
/// none, a floor under the least that is not none, tenths while they
/// tell the reader something and whole percents once they do not.
#[test]
fn a_clipped_share_is_written_to_be_read_at_a_glance() {
    assert_eq!(share_words(0.0), None);
    assert_eq!(share_words(-1.0), None);
    assert_eq!(share_words(f32::NAN), None);
    assert_eq!(share_words(0.0001).as_deref(), Some("<0.1%"));
    assert_eq!(share_words(0.001).as_deref(), Some("0.1%"));
    assert_eq!(share_words(0.0234).as_deref(), Some("2.3%"));
    assert_eq!(share_words(0.1).as_deref(), Some("10%"));
    assert_eq!(share_words(0.5).as_deref(), Some("50%"));
    assert_eq!(share_words(1.0).as_deref(), Some("100%"));
}

/// The ends of an axis and the value under the pointer are written with
/// no more digits than they need: `1`, not `1.0000`.
#[test]
fn a_decimal_is_written_without_the_zeros_it_does_not_need() {
    assert_eq!(trimmed("1.000".into()), "1");
    assert_eq!(trimmed("0.500".into()), "0.5");
    assert_eq!(trimmed("0.059".into()), "0.059");
    assert_eq!(trimmed("0.000".into()), "0");
    assert_eq!(trimmed("-0.000".into()), "0");
    assert_eq!(trimmed("12".into()), "12");

    let graded = shown(DecodedImage::new(
        2,
        1,
        Samples::U8 {
            channels: Channels::Gray,
            data: vec![0, 255],
        },
        ColorSpace::SRGB,
        AlphaMode::Opaque,
    ));
    assert_eq!(axis_words(&graded, 1.0), "1");
    assert_eq!(axis_words(&graded, 0.0), "0");
    assert!(
        axis_ends(&graded, &graded.stats.plot).is_none(),
        "a graded file's axis goes without saying"
    );

    // Linear integer data reads back as the counts it was stored in.
    let counts = shown(DecodedImage::new(
        2,
        1,
        Samples::U16 {
            channels: Channels::Gray,
            data: vec![0, 4095],
        },
        ColorSpace::LINEAR_BT709,
        AlphaMode::Opaque,
    ));
    assert_eq!(axis_words(&counts, 4095.0 / 65535.0), "4095");
    assert_eq!(
        axis_ends(&counts, &counts.stats.plot),
        Some(["0".to_string(), "4095".to_string()])
    );
}

/// The panel is the whole of the side, and every section in it is laid out
/// at its own height and the column's width, whatever the side's height: on
/// a side shorter than the column the column runs on past it, to be
/// scrolled to, rather than any section being shrunk or left out.
#[test]
fn every_section_is_laid_out_at_its_own_height_whatever_the_room() {
    assert_eq!(side::WIDTH, PANEL_WIDTH);
    assert_eq!(
        PANEL_WIDTH,
        COLUMN_WIDTH + crate::ui::style::SCROLLBAR_GUTTER + 2.0 * PANEL_INSET,
        "the column, its scrollbar's gutter and the panel's insets"
    );
    let side = Rect::new(40.0, 30.0, side::WIDTH, 300.0);
    let whole = panel(side);
    assert_eq!(
        (whole.x, whole.y, whole.width, whole.height),
        (side.x, side.y, PANEL_WIDTH, side.height)
    );

    let current = gain_mapped();
    let column = Rect::new(50.0, 70.0, COLUMN_WIDTH, 240.0);
    let layout = section::layout(column, &current);
    let sections: Vec<Section> = layout.iter().map(|(section, _)| *section).collect();
    assert_eq!(sections, Section::ALL, "in the order the data flows");
    assert_eq!(layout[0].1.y, column.y, "the first opens the column");
    for (section, rect) in &layout {
        assert_eq!(rect.height, section.height(), "{section:?}");
        assert_eq!((rect.x, rect.width), (column.x, COLUMN_WIDTH));
    }
    for pair in layout.windows(2) {
        assert_eq!(
            pair[1].1.y,
            pair[0].1.bottom() + SECTION_GAP,
            "{:?} under {:?}",
            pair[1].0,
            pair[0].0
        );
    }
    let bottom = layout.last().expect("sections").1.bottom();
    assert_eq!(bottom - column.y, section::height(&current));
    assert!(
        bottom - column.y > 2.0 * column.height,
        "the column runs on past a short side: {}",
        bottom - column.y
    );
    assert_eq!(
        MIN_HEIGHT,
        2.0 * PANEL_INSET + TOOLBAR_HEIGHT + HEADER_GAP + Section::Output.height(),
        "the row of buttons and the head section are the least the panel needs"
    );
}

/// The gain map's section is there only for a picture that carries one.
#[test]
fn the_gain_maps_section_is_there_only_with_a_map() {
    let plain: Vec<Section> = laid_out(&picture(false), 600.0)
        .into_iter()
        .map(|(section, _)| section)
        .collect();
    assert_eq!(plain, [Section::Output, Section::Display, Section::File]);
    let mapped = laid_out(&gain_mapped(), 600.0);
    assert!(
        mapped
            .iter()
            .any(|(section, _)| *section == Section::GainMap)
    );
}

/// Every plot keeps a point to the bin, which is what the pointer reads it
/// by, and sits inside its section, clear of the header above it.
#[test]
fn every_plot_keeps_one_pixel_to_the_bin() {
    for gray in [true, false] {
        let current = if gray { picture(true) } else { gain_mapped() };
        for (section, rect) in laid_out(&current, 600.0) {
            let bars = section.plot(rect, gray);
            assert_eq!(bars.width, BINS as f32, "{section:?}, gray {gray}");
            assert!(
                bars.y - PLOT_INSET >= section::header_line(rect).bottom(),
                "{section:?}"
            );
            assert!(bars.bottom() + PLOT_INSET <= rect.bottom(), "{section:?}");
        }
    }
}

/// The pointer reads a bin of a plot and nothing outside it — not the
/// section around it, and not the line its header is set on.
#[test]
fn only_the_plot_itself_answers_the_pointer() {
    let layout = laid_out(&picture(false), 600.0);
    let bars = Section::Output.plot(rect_of(&layout, Section::Output), false);

    assert_eq!(hovered_bin(bars, None), None, "no pointer, no mark");
    assert_eq!(hovered_bin(bars, Some([bars.x - 1.0, bars.y + 1.0])), None);
    assert_eq!(
        hovered_bin(bars, Some([bars.x + 1.0, bars.y - 1.0])),
        None,
        "the header is above the plot, not part of it"
    );
    assert_eq!(
        hovered_bin(bars, Some([bars.right(), bars.y + 1.0])),
        None,
        "half-open at the far edge, as every other hit test here is"
    );
    assert_eq!(hovered_bin(bars, Some([bars.x + 1.0, bars.bottom()])), None);
}

/// And it reads the bin the bar under it was drawn from, so that the rule
/// and the bar it stands on cannot part company.
#[test]
fn the_pointer_marks_the_bar_it_is_over() {
    let layout = laid_out(&picture(false), 600.0);
    let bars = Section::File.plot(rect_of(&layout, Section::File), false);
    let at = |x: f32| hovered_bin(bars, Some([bars.x + x, bars.y + 1.0]));

    assert_eq!(at(0.0), Some(0), "the first pixel of the plot is bin zero");
    assert_eq!(
        at(bars.width - 0.5),
        Some(BINS - 1),
        "and the last of it is the last bin"
    );
    assert_eq!(at(3.1), at(3.9), "one pixel of pointer, one bin");
    assert_ne!(at(3.1), at(4.1), "the next pixel is the next bin");

    // Every bin the pointer can name has a bar drawn inside the plot for
    // it to stand on, the two ends included.
    for bin in [0, 1, BINS / 2, BINS - 2, BINS - 1] {
        let across = bin_across(bin);
        assert!((0.0..=1.0).contains(&across), "bin {bin} at {across}");
    }
    assert_eq!(bin_across(0), 0.0);
    assert_eq!(bin_across(BINS - 1), 1.0);
}

/// The row of buttons at the head of the panel: the plot toggles from the
/// left, the clipped pixels' toggle set apart after them, and the reset at
/// the far end, all inside the row and none on another. A control that
/// could do nothing is not there at all, and the ones that remain close up.
#[test]
fn the_row_of_buttons_heads_the_panel() {
    let row = Rect::new(10.0, 10.0, COLUMN_WIDTH, TOOLBAR_HEIGHT);
    for gray in [true, false] {
        let buttons = toolbar(row, gray);
        let controls: Vec<Control> = buttons.iter().map(|(control, _)| *control).collect();
        let expected: &[Control] = if gray {
            &[Control::Luma, Control::Log, Control::Marks, Control::Reset]
        } else {
            &[
                Control::Luma,
                Control::Planes,
                Control::Log,
                Control::Marks,
                Control::Reset,
            ]
        };
        assert_eq!(controls, expected, "gray {gray}");
        for pair in buttons.windows(2) {
            assert!(pair[0].1.right() < pair[1].1.x, "{pair:?}");
        }
        for (control, rect) in &buttons {
            assert!(
                rect.x >= row.x && rect.right() <= row.right(),
                "{control:?}"
            );
            assert_eq!((rect.y, rect.height), (row.y, row.height), "{control:?}");
        }
        let [.., (_, log), (_, marks), (_, reset)] = buttons.as_slice() else {
            panic!("{buttons:?}");
        };
        assert!(
            marks.x - log.right() > TOOLBAR_GAP,
            "the marks are set apart from the plot toggles"
        );
        assert_eq!(reset.right(), row.right(), "the reset at the far end");
    }
}

/// A section's own button stands at the end of its header, and the
/// readout ends short of it.
#[test]
fn a_sections_button_ends_its_header() {
    let layout = laid_out(&gain_mapped(), 600.0);
    let rect = rect_of(&layout, Section::GainMap);
    let line = section::header_line(rect);
    let button = section::button(rect);
    assert_eq!(button.right(), line.right());
    assert_eq!((button.y, button.height), (line.y, line.height));
    assert!(section::readout_end(line) < button.x);
    assert!(
        line.bottom() <= Section::GainMap.plot(rect, false).y - PLOT_INSET,
        "the header is clear of the plot's ground"
    );
}

/// The false colors take their room off the Display section's plot rather
/// than off the section, so that the sections below do not shift about
/// from one file to the next; they sit under its band, and the rows under
/// them start on the same line either way.
#[test]
fn the_false_colors_are_under_the_display_band_and_nowhere_else() {
    let layout = laid_out(&picture(true), 600.0);
    let rect = rect_of(&layout, Section::Display);
    let (with, without) = (plot_area(rect, true), plot_area(rect, false));
    assert!(with.height < without.height, "{with:?} {without:?}");
    assert_eq!(with.y, without.y, "both start under the same header");

    let last = swatch_button(with, Colormap::ALL.len() - 1);
    assert!(
        last.y >= ramp(with).bottom(),
        "the ramps sit under the band"
    );
    assert!(last.right() <= with.right() + 0.01, "{last:?}");
    assert_eq!(
        swatch_button(with, 0).bottom(),
        ramp(without).bottom(),
        "the false colors end where the band alone would have"
    );
    assert!(swatch_button(with, 0).bottom() <= Rows::new(rect).exposure.y);
}

/// The rows sit under the Display section's band, inside the section, and
/// the last of them ends it.
#[test]
fn the_rows_sit_under_the_display_band() {
    let layout = laid_out(&picture(false), 600.0);
    let rect = rect_of(&layout, Section::Display);
    let rows = Rows::new(rect);
    let inside = rect;
    let band = ramp(plot_area(rect, false));

    for (widget, button) in row_buttons(rect) {
        assert!(button.y >= band.bottom(), "{widget:?} clears the band");
        assert!(button.x >= inside.x + 56.0, "{widget:?} clears its word");
        assert!(
            button.right() <= inside.right() + 0.01,
            "{widget:?} {button:?}"
        );
    }
    assert_eq!(
        rows.bottom(),
        rect.bottom(),
        "the last row ends the section"
    );
    let labels: Vec<&str> = rows.labels().map(|(word, _)| word).collect();
    assert_eq!(labels, ["Exposure", "Window", "Curve"]);

    let widgets: Vec<Control> = row_buttons(rect).map(|(widget, _)| widget).collect();
    assert_eq!(widgets.len(), WINDOWS.len() + ToneMap::ALL.len());
    assert_eq!(rows.slider().x, rows.exposure.x);
    assert!(rows.slider().right() < rows.stops().x);
    assert_eq!(rows.stops().right(), inside.right());
    assert!(
        rows.slider().width - HANDLE_GRIP > 3.0 * 2.0 * SLIDER_STOPS / EV_STEP,
        "a quarter stop is wider than a few pixels"
    );
}

/// The handles stand on the band and reach past it, and the room that
/// takes the pointer is wider than the mark — a mark five pixels wide is
/// not something a hand lands on — and clear of the plot above and of the
/// row under it.
#[test]
fn a_handle_is_easier_to_take_hold_of_than_it_is_wide() {
    let layout = laid_out(&picture(true), 600.0);
    let rect = rect_of(&layout, Section::Display);
    for gray in [true, false] {
        let plot = plot_area(rect, gray);
        let band = ramp(plot);
        let at = band.x + 100.0;
        let grip = grip(band, at);
        assert!(grip.width > HANDLE_WIDTH);
        assert_eq!(grip.x + grip.width / 2.0, at, "centered on its value");
        assert!(grip.y < band.y && grip.bottom() > band.bottom());
        assert!(
            grip.y >= plot.bottom() + PLOT_INSET,
            "{grip:?} over {plot:?}"
        );
        assert!(grip.bottom() <= Rows::new(rect).exposure.y);
        if gray {
            assert!(grip.bottom() <= swatch_button(plot, 0).y);
        }
    }
}

/// A pointer on a pixel of the picture is traced down the column: every
/// section marks a bin and writes the pixel's value at its stage. A pointer
/// on one section's plot marks that section alone.
#[test]
fn a_pixel_is_traced_down_every_section() {
    let current = gain_mapped();
    let layout = laid_out(&current, 600.0);
    let binned = output::binned_for(&current, Headroom::None, 1.0);
    let lift = current.image.gain_map.as_ref().unwrap().lift_plot();
    let plots = Plots {
        output: &binned,
        lift: lift.as_ref(),
    };

    let traced = marked(
        &current,
        &layout,
        None,
        Some([60, 0]),
        Headroom::None,
        &plots,
    );
    for section in Section::ALL {
        let mark = traced
            .of(section)
            .unwrap_or_else(|| panic!("{section:?} marks the pixel"));
        assert!(mark.bin.is_some(), "{section:?}: {mark:?}");
        assert!(!mark.words.is_empty(), "{section:?}");
    }
    assert_eq!(
        traced.gain_map.as_ref().unwrap().words,
        "+2.0 stops",
        "the right half is lifted the whole of the lift"
    );

    // On the File section's plot, only it marks anything, and what it
    // says is its bin's value.
    let bars = Section::File.plot(rect_of(&layout, Section::File), false);
    let traced = marked(
        &current,
        &layout,
        Some([bars.x + bars.width / 2.0, bars.y + 1.0]),
        Some([60, 0]),
        Headroom::None,
        &plots,
    );
    assert!(
        traced
            .file
            .as_ref()
            .is_some_and(|mark| mark.bin == Some(BINS / 2))
    );
    assert!(traced.output.is_none());
    assert!(traced.display.is_none());
    assert!(traced.gain_map.is_none());

    // Off the picture and off every plot, nothing is marked.
    let traced = marked(
        &current,
        &layout,
        None,
        Some([999, 0]),
        Headroom::None,
        &plots,
    );
    assert!(
        Section::ALL
            .iter()
            .all(|section| traced.of(*section).is_none())
    );
}

/// On an SDR screen the output stops at white: a stop of exposure piles
/// the top of a ramp into the last bin, which the right corner counts.
#[test]
fn the_output_piles_what_the_screen_clips_at_white() {
    let mut current = picture(true);
    let plain = output::binned_for(&current, Headroom::None, 1.0);
    assert_eq!(plain.room, 1.0);
    current.display.set_exposure(1.0);
    let pushed = output::binned_for(&current, Headroom::None, 1.0);
    let white = crate::image::Transfer::Srgb.to_encoded(1.0);
    let [_, above] = pushed.plot.clipped(0.0, white);
    assert!(above > 0.25, "{above}");
    assert!(pushed.plot.luma[BINS - 1] > plain.plot.luma[BINS - 1]);

    // A room nothing has said goes as far as the response does, in whole
    // stops above white.
    current.display.set_exposure(2.0);
    let unknown = output::binned_for(&current, Headroom::Above, f32::INFINITY);
    assert_eq!(unknown.room, 4.0);
}
