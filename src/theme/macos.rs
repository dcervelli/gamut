//! The Mac's own colors: AppKit's semantic `NSColor`s, resolved under the
//! appearance the application is drawn in.
//!
//! A semantic color is not one color but one per appearance — light, dark,
//! and the raised-contrast twin of each — and it resolves to whichever the
//! current drawing appearance is. So the appearance is made current for the
//! reads, each color is converted to sRGB, and what comes back is
//! [`System`]: the Mac's answer to the questions a palette answers on
//! Omarchy, which [`Theme::from_system`] turns into the interface's roles.
//! The user's accent color comes with it, since `controlAccentColor` is
//! the one the System Settings pane chose.
//!
//! `NSApp`'s appearance is the main thread's to read, and the event loop is
//! the main thread; asked from anywhere else, this answers `None` and the
//! caller falls back as it would on a desktop with no palette.

use std::cell::Cell;

use block2::RcBlock;
use objc2::MainThreadMarker;
use objc2::rc::{Retained, autoreleasepool};
use objc2_app_kit::{
    NSAppearanceNameAqua, NSAppearanceNameDarkAqua, NSApplication, NSColor, NSColorSpace,
};
use objc2_foundation::NSArray;

use super::{Mode, System, Theme};
use crate::render::Color;

/// The theme the Mac's appearance resolves to now, or `None` off the main
/// thread.
pub fn theme() -> Option<Theme> {
    system().map(|system| Theme::from_system(&system))
}

/// Every color [`Theme::from_system`] reads, as the appearance in force
/// resolves them.
fn system() -> Option<System> {
    let main = MainThreadMarker::new()?;
    autoreleasepool(|_| {
        let appearance = NSApplication::sharedApplication(main).effectiveAppearance();
        // Aqua or Dark Aqua, whatever else the appearance is: the
        // raised-contrast appearances match their own side, and the colors
        // below are read under the appearance itself, so they keep the
        // contrast it raised.
        let names = NSArray::from_slice(&[unsafe { NSAppearanceNameAqua }, unsafe {
            NSAppearanceNameDarkAqua
        }]);
        let mode = match appearance.bestMatchFromAppearancesWithNames(&names) {
            Some(name) if &*name == unsafe { NSAppearanceNameDarkAqua } => Mode::Dark,
            _ => Mode::Light,
        };

        let read: Cell<Option<System>> = Cell::new(None);
        let block = RcBlock::new(|| {
            read.set(Some(System {
                mode,
                window: srgb(&NSColor::windowBackgroundColor()),
                label: srgb(&NSColor::labelColor()),
                secondary_label: srgb(&NSColor::secondaryLabelColor()),
                text: srgb(&NSColor::textColor()),
                separator: srgb(&NSColor::separatorColor()),
                accent: srgb(&NSColor::controlAccentColor()),
                blue: srgb(&NSColor::systemBlueColor()),
                red: srgb(&NSColor::systemRedColor()),
                yellow: srgb(&NSColor::systemYellowColor()),
                orange: srgb(&NSColor::systemOrangeColor()),
            }));
        });
        appearance.performAsCurrentDrawingAppearance(&block);
        read.take()
    })
}

/// `color` as 8-bit sRGB with its alpha, which several of the label and
/// separator colors carry: they are drawn over what is under them, and
/// [`Theme::from_system`] lays them over the window to find what they come
/// to. A color that will not convert — a pattern, which none of these is —
/// reads as transparent rather than as a guess.
fn srgb(color: &NSColor) -> Color {
    let space = NSColorSpace::sRGBColorSpace();
    let Some(converted): Option<Retained<NSColor>> = color.colorUsingColorSpace(&space) else {
        return Color::rgba(0, 0, 0, 0);
    };
    let channel = |value: f64| (value * 255.0).round().clamp(0.0, 255.0) as u8;
    Color::rgba(
        channel(converted.redComponent()),
        channel(converted.greenComponent()),
        channel(converted.blueComponent()),
        channel(converted.alphaComponent()),
    )
}
