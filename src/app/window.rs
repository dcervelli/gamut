//! What the window is called, and how large it opens.

use std::path::Path;

use winit::dpi::{LogicalSize, PhysicalSize};
use winit::event_loop::ActiveEventLoop;
use winit::window::WindowAttributes;

use crate::ui;
use crate::ui::chrome::{BAR_HEIGHT, SIDE_WIDTH};
use crate::{APP_ID, PROGRAM};

/// Fraction of a monitor's room a freshly opened window may occupy.
const MAX_WINDOW_FRACTION: f64 = 0.66;

/// What the panels take out of the window, in the logical pixels they are
/// laid out in: a side on each edge and a bar top and bottom.
const CHROME: [f64; 2] = [2.0 * SIDE_WIDTH as f64, 2.0 * BAR_HEIGHT as f64];

/// The smallest a window opens at. Below this the chrome has all of it and
/// there is nowhere left for a picture, so a size asked for beneath it is
/// taken as far as it goes and no further.
const MIN_WINDOW: [u32; 2] = [320, 240];

/// A logical pixel of slack on [`PANELS_WINDOW`].
///
/// A window is laid out in logical pixels and sized in device ones, so the
/// size that comes back is the size asked for rounded to the device grid — a
/// 392-pixel window on a monitor at 1.6 is 627 device pixels and 391.875
/// logical ones. Asking for exactly the room the panels need therefore leaves
/// them out about as often as not; asking for a pixel more never does, the
/// rounding being half a device pixel at worst.
const FLOOR_SLACK: f64 = 1.0;

/// The window that has room for both floating panels at once: the content
/// area [`ui::PANELS_ROOM`] asks for, the chrome around it, and
/// [`FLOOR_SLACK`].
///
/// The floor a window opens at instead of [`MIN_WINDOW`], where the monitor
/// has the room to spare. A window that opens too small for its own interface
/// has the histogram and the information toggles dead in it from the first
/// frame, and nothing the viewer did asked for that — where the picture is
/// small, the window is better a little larger than the picture. It is only a
/// floor: `--size` is not held to it, and neither is a monitor that cannot
/// take it.
const PANELS_WINDOW: [f64; 2] = [
    ui::PANELS_ROOM[0] as f64 + CHROME[0] + FLOOR_SLACK,
    ui::PANELS_ROOM[1] as f64 + CHROME[1] + FLOOR_SLACK,
];

/// What a window opens at when the first file's header will not say how large
/// its image is. Every format read here does say, so this is a fallback for a
/// decoder added later without a header probe rather than a size anything
/// reaches today.
const DEFAULT_IMAGE: [f32; 2] = [960.0, 640.0];

/// The most characters of a name to keep in the title and status bar. A name
/// is laid out unwrapped and reshaped every frame, so a pathological one — the
/// `path.display()` fallback can be as long as the command line allows — would
/// cost real time. Ordinary names are far shorter than this.
const MAX_LABEL_CHARS: usize = 256;

pub(super) fn file_label(path: &Path) -> String {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string());
    match name.char_indices().nth(MAX_LABEL_CHARS) {
        Some((cut, _)) => format!("{}\u{2026}", &name[..cut]),
        None => name,
    }
}

pub(super) fn window_title(path: &Path) -> String {
    format!("{} — {PROGRAM}", file_label(path))
}

/// Before there is anything to look at, the title carries the file being read.
/// Titling an empty window with a file it is not yet showing would be saying
/// something untrue, and the title is the only place the name can go.
pub(super) fn loading_title(path: &Path) -> String {
    format!("loading {} — {PROGRAM}", file_label(path))
}

/// Give the window an identity before it opens.
///
/// A title alone tells a compositor what to write in the bar and nothing more.
/// Pairing the window with its desktop entry — for the icon a taskbar shows,
/// for the entry a file manager launches, and for a `class:` a Hyprland rule
/// can match — needs the application's name as well. Wayland calls it the
/// `app_id` and X11 the class half of `WM_CLASS`; winit spells both `with_name`
/// on a per-backend extension trait, so both are set and whichever backend is
/// in use reads its own. The name is [`APP_ID`], the reverse-DNS one the
/// desktop entry is filed under, not the word the binary is called by.
///
/// The instance name is left empty: it exists to tell several windows of one
/// application apart, and there is only ever the one here.
pub(super) fn with_app_id(attributes: WindowAttributes) -> WindowAttributes {
    #[cfg(any(
        target_os = "linux",
        target_os = "dragonfly",
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "openbsd"
    ))]
    {
        use winit::platform::wayland::WindowAttributesExtWayland;
        use winit::platform::x11::WindowAttributesExtX11;

        let attributes = WindowAttributesExtWayland::with_name(attributes, APP_ID, "");
        WindowAttributesExtX11::with_name(attributes, APP_ID, "")
    }
    #[cfg(not(any(
        target_os = "linux",
        target_os = "dragonfly",
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "openbsd"
    )))]
    attributes
}

/// Open at the image's own size, shrunk to fit comfortably on the monitors —
/// or at the size `--size` asked for, where it asked for one.
///
/// The monitors are the only thing here that has to be asked of the event
/// loop; what is done with their answer is [`window_size`], which is testable.
/// Every monitor is collected, not just one: `primary_monitor` is `None` on
/// Wayland by definition, and nothing before the surface is mapped says which
/// monitor the compositor will open the window on.
pub(super) fn initial_window_size(
    event_loop: &ActiveEventLoop,
    image: Option<[f32; 2]>,
    asked: Option<[u32; 2]>,
) -> LogicalSize<u32> {
    let monitors: Vec<_> = event_loop
        .available_monitors()
        .map(|monitor| (monitor.size(), monitor.scale_factor()))
        .collect();
    window_size(&monitors, image, asked)
}

/// A monitor's room in the logical pixels a window is laid out in, or `None`
/// for one that has not said what mode it is in — a Wayland output reports
/// `0 × 0` until its mode arrives.
fn monitor_room(size: PhysicalSize<u32>, scale: f64) -> Option<[f64; 2]> {
    (size.width > 0 && size.height > 0 && scale > 0.0).then(|| {
        [
            f64::from(size.width) / scale,
            f64::from(size.height) / scale,
        ]
    })
}

/// The window this monitor would want: the image at its own pixels, held
/// inside [`MAX_WINDOW_FRACTION`] of the monitor's room, plus the chrome.
///
/// The panels take their room out of the image rather than lying over it, so
/// the window asks for the image *plus* the chrome around it — otherwise a
/// picture that would open at 100% would open slightly reduced. The fraction
/// still applies to the image itself.
///
/// The image's pixels are physical and the window's are logical, so the
/// monitor's scale converts between them. On Wayland that scale is the
/// output's integer one — 2 where the compositor is really running 1.6 — and
/// the true fractional scale does not arrive until the surface is mapped. The
/// error is in the safe direction: a window that opens a little smaller than
/// 100% rather than one that overruns the screen.
///
/// A picture smaller than the floor is given the floor: [`PANELS_WINDOW`]
/// where this monitor can take it, and [`MIN_WINDOW`] where it cannot. The
/// floor is measured against the monitor's whole room rather than against
/// [`MAX_WINDOW_FRACTION`] of it — the fraction is about leaving the desktop
/// its share of a large window, and this is about a small one being usable at
/// all, so it may exceed the fraction on a monitor with little to spare.
fn wanted_window(room: [f64; 2], scale: f64, image: [f32; 2]) -> [f64; 2] {
    let mut width = f64::from(image[0]) / scale;
    let mut height = f64::from(image[1]) / scale;

    let max_width = room[0] * MAX_WINDOW_FRACTION - CHROME[0];
    let max_height = room[1] * MAX_WINDOW_FRACTION - CHROME[1];
    if max_width > 1.0 && max_height > 1.0 {
        let shrink = (max_width / width).min(max_height / height).min(1.0);
        width *= shrink;
        height *= shrink;
    }

    let floor = floor_for(room);
    [
        (width + CHROME[0]).max(floor[0]),
        (height + CHROME[1]).max(floor[1]),
    ]
}

/// The smallest window this monitor should be given: one with room for both
/// panels where the monitor can hold it, and [`MIN_WINDOW`] where it cannot —
/// a floor that did not fit the screen would be a window off the edge of it,
/// which is the thing the rest of this is for.
fn floor_for(room: [f64; 2]) -> [f64; 2] {
    let least = [f64::from(MIN_WINDOW[0]), f64::from(MIN_WINDOW[1])];
    if PANELS_WINDOW[0] <= room[0] && PANELS_WINDOW[1] <= room[1] {
        [
            least[0].max(PANELS_WINDOW[0]),
            least[1].max(PANELS_WINDOW[1]),
        ]
    } else {
        least
    }
}

/// The size to open at, given what the monitors are.
///
/// Each monitor is asked what window it would want, and the largest of those
/// answers that fits on *every* monitor is taken. Which monitor the window
/// lands on is the compositor's to decide and is not known until it has
/// decided it, so the size that opens is one that no monitor would have to
/// overrun. Where none of them fits everywhere — a small monitor beside a
/// large one, and [`MIN_WINDOW`] below the small one's room — the smallest is
/// taken, as the least bad of them.
fn window_size(
    monitors: &[(PhysicalSize<u32>, f64)],
    image: Option<[f32; 2]>,
    asked: Option<[u32; 2]>,
) -> LogicalSize<u32> {
    // A size that was asked for is the window itself, chrome and all, and in
    // the logical pixels a compositor lays windows out in. Nothing else has a
    // say in it: neither the image's shape nor the monitors' room, a window
    // larger than the screen being something a compositor is asked for on
    // purpose.
    if let Some([width, height]) = asked {
        return LogicalSize::new(width.max(MIN_WINDOW[0]), height.max(MIN_WINDOW[1]));
    }

    // Only a file whose header would not say how large it is arrives here
    // with nothing, and then a plain rectangle is the best that can be done.
    let image = image.unwrap_or(DEFAULT_IMAGE);

    let rooms: Vec<([f64; 2], f64)> = monitors
        .iter()
        .filter_map(|&(size, scale)| monitor_room(size, scale).map(|room| (room, scale)))
        .collect();

    // Nothing said what any monitor is: the image's own pixels, taken as
    // logical ones, is all that is left to open at.
    if rooms.is_empty() {
        return logical(wanted_window([f64::INFINITY; 2], 1.0, image));
    }

    let wanted: Vec<[f64; 2]> = rooms
        .iter()
        .map(|&(room, scale)| wanted_window(room, scale, image))
        .collect();

    let fits = |size: &[f64; 2]| {
        rooms
            .iter()
            .all(|(room, _)| size[0] <= room[0] && size[1] <= room[1])
    };
    let area = |size: &[f64; 2]| size[0] * size[1];

    let chosen = wanted
        .iter()
        .filter(|size| fits(size))
        .max_by(|a, b| area(a).total_cmp(&area(b)))
        .or_else(|| wanted.iter().min_by(|a, b| area(a).total_cmp(&area(b))))
        .copied()
        .unwrap_or(wanted[0]);

    logical(chosen)
}

/// A size settled on, rounded to the whole logical pixels a window is asked
/// for in and held at the floor the rounding could otherwise drop it below.
fn logical(size: [f64; 2]) -> LogicalSize<u32> {
    LogicalSize::new(
        (size[0].round() as u32).max(MIN_WINDOW[0]),
        (size[1].round() as u32).max(MIN_WINDOW[1]),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::chrome::content_area;

    /// One ordinary monitor, unscaled.
    const MONITOR: [(PhysicalSize<u32>, f64); 1] = [(PhysicalSize::new(2560, 1440), 1.0)];

    /// A size asked for is given as it was asked for, in logical pixels: the
    /// chrome is not added to it, and the monitors do not shrink it.
    #[test]
    fn an_asked_size_is_the_window() {
        let size = window_size(&MONITOR, Some([100.0, 100.0]), Some([800, 500]));
        assert_eq!(size, LogicalSize::new(800, 500));

        let huge = window_size(&MONITOR, None, Some([9000, 9000]));
        assert_eq!(huge, LogicalSize::new(9000, 9000));
    }

    /// Below the floor there is nowhere left for a picture, so a smaller ask
    /// opens at the floor rather than at nothing.
    #[test]
    fn an_asked_size_stops_at_the_smallest_window() {
        let size = window_size(&MONITOR, None, Some([1, 1]));
        assert_eq!(size, LogicalSize::new(MIN_WINDOW[0], MIN_WINDOW[1]));
    }

    /// Without one, the image decides: its own size plus the chrome around
    /// it, held inside the monitor.
    #[test]
    fn without_one_the_image_decides() {
        let small = window_size(&MONITOR, Some([640.0, 480.0]), None);
        assert_eq!(small.width, 640 + 2 * SIDE_WIDTH as u32);
        assert_eq!(small.height, 480 + 2 * BAR_HEIGHT as u32);

        let large = window_size(&MONITOR, Some([8000.0, 6000.0]), None);
        assert!(large.width <= 2560 && large.height <= 1440);
    }

    /// A picture smaller than the interface still opens a window the
    /// interface fits in: both panels can be opened in the window a tiny
    /// image gets, so neither toggle is dead in it from the first frame.
    ///
    /// Measured against `ui::PANELS_ROOM`, which
    /// `ui::tests::the_panels_room_is_room_for_both` holds to what the panels
    /// actually do with it.
    #[test]
    fn a_small_picture_still_opens_a_window_the_panels_fit_in() {
        let size = window_size(&MONITOR, Some([32.0, 24.0]), None);
        let content = content_area([size.width as f32, size.height as f32], true, false);
        assert!(content.width >= ui::PANELS_ROOM[0], "{content:?}");
        assert!(content.height >= ui::PANELS_ROOM[1], "{content:?}");

        // And with a device pixel's rounding taken off it, which is what a
        // compositor hands back: the window is laid out in logical pixels
        // and sized in device ones. Half a device pixel is the worst of it,
        // and the coarsest grid is a monitor at 1:1.
        let rounded = content_area(
            [size.width as f32 - 0.5, size.height as f32 - 0.5],
            true,
            false,
        );
        assert!(rounded.width >= ui::PANELS_ROOM[0], "{rounded:?}");
        assert!(rounded.height >= ui::PANELS_ROOM[1], "{rounded:?}");
    }

    /// And a monitor with no room for such a window is not made to hold one.
    /// The floor is a floor, not a size asked for: a window that would not
    /// fit the screen is the thing the rest of this is here to prevent.
    #[test]
    fn a_monitor_too_small_for_the_panels_keeps_the_smallest_window() {
        let cramped = [(PhysicalSize::new(500, 400), 1.0)];
        let size = window_size(&cramped, Some([32.0, 24.0]), None);
        assert_eq!(size, LogicalSize::new(MIN_WINDOW[0], MIN_WINDOW[1]));
    }

    /// The window is asked for in logical pixels, so a scaled monitor's room
    /// is its own logical room — not the device pixels it has. Asking for the
    /// device count would open a window `scale` times too large, which on a
    /// 4K monitor at 2x is well past the screen.
    #[test]
    fn a_scaled_monitor_is_measured_in_logical_pixels() {
        let monitors = [(PhysicalSize::new(3840, 2160), 2.0)];
        let size = window_size(&monitors, Some([3000.0, 2000.0]), None);
        assert!(size.width <= 1920 && size.height <= 1080, "{size:?}");
    }

    /// The largest window that fits everywhere is taken, because which
    /// monitor the compositor opens on is not known yet: the big monitor's
    /// own answer would overrun the small one, so the small one's answer —
    /// which fits on both — is the one used.
    #[test]
    fn the_largest_that_fits_on_every_monitor_wins() {
        let big = (PhysicalSize::new(3840, 2160), 1.0);
        let small = (PhysicalSize::new(1280, 800), 1.0);

        let alone = window_size(&[big], Some([3000.0, 2000.0]), None);
        let together = window_size(&[big, small], Some([3000.0, 2000.0]), None);

        assert!(alone.width > 1280, "the big monitor alone wants more");
        assert_eq!(
            together,
            window_size(&[small], Some([3000.0, 2000.0]), None)
        );
        assert!(
            together.width <= 1280 && together.height <= 800,
            "{together:?}"
        );
    }

    /// A monitor with no room for even the smallest window cannot be fitted,
    /// and then the smallest of the answers is the least bad of them.
    #[test]
    fn where_nothing_fits_the_smallest_is_taken() {
        let tiny = (PhysicalSize::new(200, 150), 1.0);
        let big = (PhysicalSize::new(3840, 2160), 1.0);
        let size = window_size(&[big, tiny], Some([3000.0, 2000.0]), None);
        assert_eq!(size, LogicalSize::new(MIN_WINDOW[0], MIN_WINDOW[1]));
    }

    /// An output that has not said what mode it is in reports nothing, and
    /// counting its zero as room would shrink every window to the floor.
    #[test]
    fn a_monitor_that_says_nothing_is_ignored() {
        let unknown = (PhysicalSize::new(0, 0), 1.0);
        let size = window_size(&[MONITOR[0], unknown], Some([640.0, 480.0]), None);
        assert_eq!(size, window_size(&MONITOR, Some([640.0, 480.0]), None));
    }

    /// With no monitors at all there is nothing to hold the window inside, so
    /// the image's own pixels are taken as logical ones and the chrome added.
    #[test]
    fn with_no_monitors_the_image_is_the_window() {
        let size = window_size(&[], Some([640.0, 480.0]), None);
        assert_eq!(size.width, 640 + 2 * SIDE_WIDTH as u32);
        assert_eq!(size.height, 480 + 2 * BAR_HEIGHT as u32);
    }
}
