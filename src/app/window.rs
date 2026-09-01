//! What the window is called, and how large it opens.

use std::path::Path;

use winit::dpi::PhysicalSize;
use winit::event_loop::ActiveEventLoop;
use winit::window::WindowAttributes;

use crate::APP_ID;
use crate::ui::chrome::{BAR_HEIGHT, SIDE_WIDTH};

/// Fraction of the monitor a freshly opened window may occupy.
const MAX_WINDOW_FRACTION: f64 = 0.85;

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
    format!("{} — {APP_ID}", file_label(path))
}

/// Before there is anything to look at, the title carries the file being read.
/// Titling an empty window with a file it is not yet showing would be saying
/// something untrue, and the title is the only place the name can go.
pub(super) fn loading_title(path: &Path) -> String {
    format!("loading {} — {APP_ID}", file_label(path))
}

/// Give the window an identity before it opens.
///
/// A title alone tells a compositor what to write in the bar and nothing more.
/// Pairing the window with its desktop entry — for the icon a taskbar shows,
/// for the entry a file manager launches, and for a `class:` a Hyprland rule
/// can match — needs the application's name as well. Wayland calls it the
/// `app_id` and X11 the class half of `WM_CLASS`; winit spells both `with_name`
/// on a per-backend extension trait, so both are set and whichever backend is
/// in use reads its own.
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

/// Open at the image's own size, shrunk to fit comfortably on the monitor.
///
/// The panels take their room out of the image rather than lying over it, so
/// the window asks for the image *plus* the chrome around it — otherwise a
/// picture that used to open at 100% would open slightly reduced. The monitor
/// fraction still applies to the image itself.
pub(super) fn initial_window_size(
    event_loop: &ActiveEventLoop,
    image: Option<[f32; 2]>,
) -> PhysicalSize<u32> {
    let monitor = event_loop
        .primary_monitor()
        .or_else(|| event_loop.available_monitors().next());
    let scale = monitor
        .as_ref()
        .map_or(1.0, |monitor| monitor.scale_factor());
    let chrome = [
        2.0 * SIDE_WIDTH as f64 * scale,
        2.0 * BAR_HEIGHT as f64 * scale,
    ];

    // Only a file whose header would not say how large it is arrives here
    // with nothing, and then a plain rectangle is the best that can be done.
    let image = image.unwrap_or(DEFAULT_IMAGE);
    let (mut width, mut height) = (image[0] as f64, image[1] as f64);

    if let Some(monitor) = monitor {
        let available = monitor.size();
        let max_width = available.width as f64 * MAX_WINDOW_FRACTION - chrome[0];
        let max_height = available.height as f64 * MAX_WINDOW_FRACTION - chrome[1];
        if max_width > 1.0 && max_height > 1.0 {
            let shrink = (max_width / width).min(max_height / height).min(1.0);
            width *= shrink;
            height *= shrink;
        }
    }

    PhysicalSize::new(
        ((width + chrome[0]).round() as u32).max(320),
        ((height + chrome[1]).round() as u32).max(240),
    )
}
