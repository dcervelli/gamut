use super::*;
use crate::image::{AlphaMode, Channels, ColorSpace, DecodedImage, Samples};
use crate::ui::side;

/// The histogram in the side panel at its narrowest, on a display of one.
fn full_panel() -> Rect {
    panel(Rect::new(0.0, 0.0, side::WIDTH_MIN, 600.0), 1.0)
}

/// `image`, on screen.
fn shown(image: DecodedImage) -> Current {
    Current::of(image, "file")
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
}

/// The pointer reads a bin of the plot and nothing outside it — not the
/// panel around it, and not the line the axis labels are set on.
#[test]
fn only_the_plot_itself_answers_the_pointer() {
    let bars = bars(full_panel());

    assert_eq!(hovered_bin(bars, None), None, "no pointer, no mark");
    assert_eq!(hovered_bin(bars, Some([bars.x - 1.0, bars.y + 1.0])), None);
    assert_eq!(
        hovered_bin(bars, Some([bars.x + 1.0, bars.y - 1.0])),
        None,
        "the labels' line is above the plot, not part of it"
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
    let bars = bars(full_panel());
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

/// The narrowest side panel holds the histogram exactly, at a logical
/// pixel to the bin.
#[test]
fn the_narrowest_side_panel_is_the_histogram_at_its_own_size() {
    let side = Rect::new(40.0, 30.0, side::WIDTH_MIN, 600.0);
    for scale in [1.0, 1.25, 1.5, 2.0] {
        let panel = panel(side, scale);
        assert_eq!(
            (panel.x, panel.y, panel.width, panel.height),
            (side.x, side.y, SIZE[0], SIZE[1]),
            "at {scale}"
        );
    }
}

/// A wider side panel widens the plot only by whole device pixels to every
/// bin, so that no bar comes out fatter than its neighbors; between those
/// steps it stands in the middle of the panel, which it never overhangs.
#[test]
fn the_plot_widens_by_whole_device_pixels_to_the_bin() {
    let bins = BINS as f32;
    let at = |width: f32, scale: f32| {
        let side = Rect::new(100.0, 0.0, width, 600.0);
        let panel = panel(side, scale);
        (side, panel, panel.width - TOOLBAR_WIDTH - 2.0 * PANEL_INSET)
    };
    assert_eq!(at(side::WIDTH_MAX, 1.0).2, 2.0 * bins);
    assert_eq!(at(side::WIDTH_MAX - 1.0, 1.0).2, bins);
    assert_eq!(at(side::WIDTH_MIN + bins / 2.0, 2.0).2, 1.5 * bins);
    let (side, panel, _) = at(side::WIDTH_MIN + 100.0, 1.0);
    assert_eq!(panel.x, side.x + 50.0, "in the middle between the steps");

    for scale in [1.0, 1.25, 1.5, 1.75, 2.0, 3.0] {
        let mut width = side::WIDTH_MIN;
        while width <= side::WIDTH_MAX {
            let (side, panel, plot) = at(width, scale);
            let per_bin = plot * scale / bins;
            assert!(
                plot == bins || (per_bin - per_bin.round()).abs() < 1e-3,
                "{plot} at {scale} in {width}"
            );
            assert!(plot >= bins);
            assert!(panel.x >= side.x && panel.right() <= side.right() + 1e-3);
            width += 7.0;
        }
    }
}

/// A bin is one logical pixel, which is what keeps the bars from landing
/// astride a pixel boundary, whatever stands beside the plot. Nor does
/// the plot move down for the rows under it, which is what the panel
/// grows by.
#[test]
fn the_plot_keeps_one_pixel_to_the_bin_whatever_grows_around_it() {
    let panel = full_panel();
    for gray in [true, false] {
        let bars = plot_area(panel, gray);
        assert_eq!(bars.width, BINS as f32, "gray {gray}");

        // Everything the panel holds is inside it, and clear of the
        // plot.
        let last = toolbar(gray).len() - 1;
        let button = toolbar_button(panel, gray, last);
        assert!(button.right() <= bars.x, "the strip clears the plot");
        assert!(toolbar_button(panel, gray, 0).y >= panel.y);
        assert!(button.bottom() <= panel.bottom(), "{button:?}");
        // And it ends above the band of color, which is what the
        // room under the plot is for: a button beside the ramp would
        // read as belonging to it rather than to the plot it acts on.
        assert!(
            button.bottom() <= ramp(bars).y,
            "gray {gray}: {button:?} against the band at {:?}",
            ramp(bars)
        );
        assert!(ramp(bars).y >= bars.bottom(), "the band is under the plot");
        assert!(ramp(bars).bottom() <= panel.bottom());
    }
}

/// The button that marks the clipped pixels stands beside the band, in
/// the strip's own column and centered on the band, in the room between
/// the stack above and the rows below — clear of both, whatever the
/// stack holds, and clear of the black handle's grip beside it.
#[test]
fn the_marks_button_stands_beside_the_band() {
    let panel = full_panel();
    let button = marks_button(panel);
    let band = ramp(bars(panel));

    assert_eq!(button.x, toolbar_button(panel, false, 0).x, "in the strip");
    assert_eq!(
        button.y + button.height / 2.0,
        band.y + band.height / 2.0,
        "centered on the band"
    );
    for gray in [true, false] {
        let last = toolbar_button(panel, gray, toolbar(gray).len() - 1);
        assert!(
            button.y >= last.bottom() + TOOLBAR_GAP,
            "gray {gray}: {button:?} against the stack ending at {last:?}"
        );
    }
    assert!(button.bottom() <= Rows::new(panel).exposure.y, "{button:?}");
    assert!(
        button.right() <= grip(band, band.x).x,
        "{button:?} against the handle at {:?}",
        grip(band, band.x)
    );
}

/// The false colors take their room off the plot rather than off the
/// panel, so that the information panel below does not shift about from
/// one file to the next.
#[test]
fn the_panel_is_one_height_with_the_false_colors_and_without() {
    let panel = full_panel();
    let (with, without) = (plot_area(panel, true), plot_area(panel, false));
    assert!(with.height < without.height, "{with:?} {without:?}");
    assert_eq!(with.y, without.y, "both start under the same label");

    let last = swatch_button(with, Colormap::ALL.len() - 1);
    assert!(
        last.y >= ramp(with).bottom(),
        "the ramps sit under the band"
    );
    assert!(last.bottom() <= panel.bottom(), "{last:?} in {panel:?}");
    assert!(last.right() <= with.right() + 0.01, "{last:?}");
    assert!(
        ramp(without).bottom() <= panel.bottom(),
        "and without them the band still clears the panel's edge"
    );
}

/// The handles stand on the band and reach past it, and the room that
/// takes the pointer is wider than the mark: a mark five pixels wide is
/// not something a hand lands on.
#[test]
fn a_handle_is_easier_to_take_hold_of_than_it_is_wide() {
    let band = ramp(bars(full_panel()));
    let at = band.x + 100.0;
    let grip = grip(band, at);
    assert!(grip.width > HANDLE_WIDTH);
    assert_eq!(
        grip.x + grip.width / 2.0,
        at,
        "centered on the value it marks"
    );
    assert!(grip.y < band.y && grip.bottom() > band.bottom());
    // Above the band it stays clear of the plot's ground, and below it
    // clear of whatever comes next: the false colors, or the rows.
    let plot = bars(full_panel());
    assert!(
        grip.y >= plot.bottom() + PLOT_INSET,
        "{grip:?} over {plot:?}"
    );
    assert!(grip.bottom() <= swatch_button(plot, 0).y);
    assert!(grip.bottom() <= Rows::new(full_panel()).exposure.y);
}

/// A control the display would ignore is not on the panel at all, and the
/// ones that remain close the gap up rather than leaving a hole where it
/// would have been.
#[test]
fn a_control_that_could_do_nothing_is_not_there() {
    let panel = full_panel();

    assert_eq!(toolbar(true), [Control::Luma, Control::Log, Control::Reset]);
    assert_eq!(
        toolbar(false),
        [Control::Luma, Control::Planes, Control::Log, Control::Reset],
        "three channels have planes to toggle"
    );
    // The reset moves up into the room the planes toggle is not taking.
    assert_eq!(
        toolbar_button(panel, true, 1).y,
        toolbar_button(panel, false, 1).y,
        "and the slot it leaves is filled, not left empty"
    );

    // The row of false colors takes its room off the plot on an image
    // that has them, under the band, and the plot is taller without.
    let swatch = swatch_button(plot_area(panel, true), 2);
    assert!(swatch.y >= ramp(plot_area(panel, true)).bottom());
    assert!(plot_area(panel, false).height > plot_area(panel, true).height);
}

/// The rows sit under the band, inside the panel, and land in the same
/// place whether or not the image has a row of false colors — the band
/// and the swatches under it end on the same line, so the controls
/// below them do not shift about from one file to the next. The last
/// of the three rows ends the panel.
#[test]
fn the_rows_sit_under_the_band_whatever_it_ends_in() {
    let panel = full_panel();
    let rows = Rows::new(panel);
    let inside = panel.inset(PANEL_INSET, PANEL_INSET);

    let lowest = swatch_button(plot_area(panel, true), 0).bottom();
    assert_eq!(
        lowest,
        ramp(bars(panel)).bottom(),
        "the false colors end where the band alone would have"
    );
    for (widget, rect) in row_buttons(panel) {
        assert!(rect.y >= lowest, "{widget:?} clears the band: {rect:?}");
        assert!(rect.bottom() <= inside.bottom(), "{widget:?} {rect:?}");
        assert!(rect.x >= inside.x + ROW_LABEL, "{widget:?} clears its word");
        assert!(rect.right() <= inside.right() + 0.01, "{widget:?} {rect:?}");
    }
    // The last row is the last thing on the panel, and what it
    // leaves under it is the panel's own inset and nothing more.
    assert_eq!(rows.bottom(), inside.bottom());
    let labels: Vec<&str> = rows.labels().map(|(word, _)| word).collect();
    assert_eq!(labels, ["Exposure", "Window", "Curve"]);
}

/// The windows and the curves cover everything there is to choose, and
/// the exposure's row is a slider up to its reading, which ends where
/// the rows' last buttons do.
#[test]
fn the_rows_offer_every_choice_there_is() {
    let panel = full_panel();
    let widgets: Vec<Control> = row_buttons(panel).map(|(widget, _)| widget).collect();
    assert_eq!(widgets.len(), WINDOWS.len() + ToneMap::ALL.len());

    let rows = Rows::new(panel);
    let inside = panel.inset(PANEL_INSET, PANEL_INSET);
    assert_eq!(rows.slider().x, rows.exposure.x);
    assert!(rows.slider().right() < rows.stops().x);
    assert_eq!(rows.stops().right(), inside.right());
    assert!(
        rows.slider().width - HANDLE_GRIP > 3.0 * 2.0 * SLIDER_STOPS / EV_STEP,
        "a quarter stop is wider than a few pixels"
    );

    // The three rules, and no fourth for the image's own, which is
    // always one of these.
    let named: Vec<AutoWindow> = WINDOWS.iter().map(|(_, window)| *window).collect();
    assert_eq!(
        named,
        [AutoWindow::Off, AutoWindow::MinMax, AutoWindow::Percentile]
    );
    assert_eq!(ToneMap::ALL.len(), 2, "one button to a choice");
}

/// The readout is set in the middle of the line, and the ends of the axis
/// keep their corners until it actually reaches them — at which point
/// both go, rather than one.
#[test]
fn the_axis_ends_give_the_line_up_to_the_readout_and_not_before() {
    let bars = bars(full_panel());
    let centered = |width: f32| bars.x + (bars.width - width) / 2.0;

    let (x, fits) = readout_placement(bars, 80.0, [40.0, 40.0]);
    assert_eq!(x, centered(80.0), "centered on the plot, not on the panel");
    assert!(fits, "80 in the middle and 40 either side of 256 is room");

    let room = (bars.width - 80.0) / 2.0 - LABEL_GAP;
    assert!(
        readout_placement(bars, 80.0, [room, room]).1,
        "exactly room"
    );
    assert!(
        !readout_placement(bars, 80.0, [room + 0.5, room]).1,
        "and a hair less is not — the left end alone decides for both"
    );
    assert!(!readout_placement(bars, 80.0, [room, room + 0.5]).1);
    assert!(
        !readout_placement(bars, bars.width + 1.0, [0.0, 0.0]).1,
        "a readout wider than the plot leaves no line to share"
    );
}
