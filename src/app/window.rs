//! What the window is called, and how large it opens.

use std::path::Path;

use winit::dpi::{LogicalSize, PhysicalSize, Size};
use winit::event_loop::ActiveEventLoop;
use winit::window::WindowAttributes;

use crate::{APP_ID, PROGRAM};
use crate::ui::chrome::{BAR_HEIGHT, SIDE_WIDTH};

/// Fraction of the monitor a freshly opened window may occupy.
const MAX_WINDOW_FRACTION: f64 = 0.85;

/// The smallest a window opens at. Below this the chrome has all of it and
/// there is nowhere left for a picture, so a size asked for beneath it is
/// taken as far as it goes and no further.
const MIN_WINDOW: [u32; 2] = [320, 240];

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

/// Open at the image's own size, shrunk to fit comfortably on the monitor —
/// or at the size `--size` asked for, where it asked for one.
///
/// The monitor is the only thing here that has to be asked of the event loop;
/// what is done with its answer is [`window_size`], which is testable.
pub(super) fn initial_window_size(
    event_loop: &ActiveEventLoop,
    image: Option<[f32; 2]>,
    asked: Option<[u32; 2]>,
) -> Size {
    let monitor = event_loop
        .primary_monitor()
        .or_else(|| event_loop.available_monitors().next())
        .map(|monitor| (monitor.size(), monitor.scale_factor()));
    window_size(monitor, image, asked)
}

/// The size to open at, given what the monitor is.
///
/// The panels take their room out of the image rather than lying over it, so
/// the window asks for the image *plus* the chrome around it — otherwise a
/// picture that used to open at 100% would open slightly reduced. The monitor
/// fraction still applies to the image itself.
fn window_size(
    monitor: Option<(PhysicalSize<u32>, f64)>,
    image: Option<[f32; 2]>,
    asked: Option<[u32; 2]>,
) -> Size {
    // A size that was asked for is the window itself, chrome and all, and in
    // the logical pixels a compositor lays windows out in rather than the
    // physical ones the image is placed in. Nothing else has a say in it:
    // neither the image's shape nor the monitor's room, a window larger than
    // the screen being something a compositor is asked for on purpose.
    if let Some([width, height]) = asked {
        return LogicalSize::new(width.max(MIN_WINDOW[0]), height.max(MIN_WINDOW[1])).into();
    }

    let scale = monitor.map_or(1.0, |(_, scale)| scale);
    let chrome = [
        2.0 * SIDE_WIDTH as f64 * scale,
        2.0 * BAR_HEIGHT as f64 * scale,
    ];

    // Only a file whose header would not say how large it is arrives here
    // with nothing, and then a plain rectangle is the best that can be done.
    let image = image.unwrap_or(DEFAULT_IMAGE);
    let (mut width, mut height) = (image[0] as f64, image[1] as f64);

    if let Some((available, _)) = monitor {
        let max_width = available.width as f64 * MAX_WINDOW_FRACTION - chrome[0];
        let max_height = available.height as f64 * MAX_WINDOW_FRACTION - chrome[1];
        if max_width > 1.0 && max_height > 1.0 {
            let shrink = (max_width / width).min(max_height / height).min(1.0);
            width *= shrink;
            height *= shrink;
        }
    }

    PhysicalSize::new(
        ((width + chrome[0]).round() as u32).max(MIN_WINDOW[0]),
        ((height + chrome[1]).round() as u32).max(MIN_WINDOW[1]),
    )
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    const MONITOR: Option<(PhysicalSize<u32>, f64)> = Some((PhysicalSize::new(2560, 1440), 1.0));

    /// A size asked for is given as it was asked for, in logical pixels: the
    /// chrome is not added to it, and the monitor does not shrink it.
    #[test]
    fn an_asked_size_is_the_window() {
        let size = window_size(MONITOR, Some([100.0, 100.0]), Some([800, 500]));
        assert_eq!(size, Size::Logical(LogicalSize::new(800.0, 500.0)));

        let huge = window_size(MONITOR, None, Some([9000, 9000]));
        assert_eq!(huge, Size::Logical(LogicalSize::new(9000.0, 9000.0)));
    }

    /// Below the floor there is nowhere left for a picture, so a smaller ask
    /// opens at the floor rather than at nothing.
    #[test]
    fn an_asked_size_stops_at_the_smallest_window() {
        let size = window_size(MONITOR, None, Some([1, 1]));
        assert_eq!(
            size,
            Size::Logical(LogicalSize::new(
                f64::from(MIN_WINDOW[0]),
                f64::from(MIN_WINDOW[1])
            ))
        );
    }

    /// Without one, the image decides: its own size plus the chrome around
    /// it, held inside the monitor.
    #[test]
    fn without_one_the_image_decides() {
        let Size::Physical(small) = window_size(MONITOR, Some([640.0, 480.0]), None) else {
            panic!("the image path is in physical pixels");
        };
        assert_eq!(small.width, 640 + 2 * SIDE_WIDTH as u32);
        assert_eq!(small.height, 480 + 2 * BAR_HEIGHT as u32);

        let Size::Physical(large) = window_size(MONITOR, Some([8000.0, 6000.0]), None) else {
            panic!("the image path is in physical pixels");
        };
        assert!(large.width <= 2560 && large.height <= 1440);
    }
}
