//! The desktop's faces on macOS, as AppKit names them: the system font the
//! menus and the dialogs are set in, its bold, and the system's monospace.
//!
//! San Francisco is one variable file rather than a family of static ones,
//! so the bold is the same file as the regular at another weight, and each
//! face is handed over with the weight it is to be drawn at, which egui sets
//! on the file's weight axis.

use objc2::rc::autoreleasepool;
use objc2_app_kit::{NSFont, NSFontWeightRegular};
use objc2_foundation::{NSString, NSURL};
use skrifa::raw::FileRef;
use skrifa::string::StringId;
use skrifa::{FontRef, MetadataProvider, Tag};

use super::{Face, Want};

/// The weight axis a variable face is set on.
const WEIGHT: Tag = Tag::new(b"wght");

/// The size a font is asked for at. Which file answers does not depend on
/// it; egui sets the size itself.
const SIZE: f64 = 13.0;

/// AppKit, which is always there to ask.
pub(super) struct Desktop;

impl Desktop {
    pub(super) fn new() -> Self {
        Self
    }

    /// The face the system names for `want`, or `None` where the file it
    /// names cannot be read, or has no bold to give where a bold is asked for.
    pub(super) fn find(&self, want: Want) -> Option<Face> {
        autoreleasepool(|_| {
            let (font, weight) = match want {
                Want::Sans => (NSFont::systemFontOfSize(SIZE), 400.0),
                // Linux's bold is fontconfig's demibold; the same here.
                Want::SansBold => (NSFont::boldSystemFontOfSize(SIZE), 600.0),
                Want::Mono => (
                    NSFont::monospacedSystemFontOfSize_weight(SIZE, unsafe { NSFontWeightRegular }),
                    400.0,
                ),
            };
            let path = font
                .fontDescriptor()
                .objectForKey(&NSString::from_str("NSCTFontFileURLAttribute"))?
                .downcast::<NSURL>()
                .ok()?
                .to_file_path()?;
            let bytes = std::fs::read(path).ok()?;
            let index = face_named(&bytes, &font.fontName().to_string())?;
            let variable = FontRef::from_index(&bytes, index)
                .ok()?
                .axes()
                .get_by_tag(WEIGHT)
                .is_some();
            // A static face is the weight it is; asked for a bold and handed
            // the regular file, there is no bold to be had.
            let coords = if variable {
                vec![(*b"wght", weight)]
            } else if matches!(want, Want::SansBold)
                && !font.fontName().to_string().contains("Bold")
            {
                return None;
            } else {
                Vec::new()
            };
            Some(Face {
                bytes,
                index,
                coords,
            })
        })
    }
}

/// Which face of the file `bytes` is the one PostScript calls `name`: the
/// file may be a collection. The first face where none is called that, as
/// a single font's file is read.
fn face_named(bytes: &[u8], name: &str) -> Option<u32> {
    match FileRef::new(bytes).ok()? {
        FileRef::Font(_) => Some(0),
        FileRef::Collection(collection) => Some(
            (0..collection.len())
                .find(|&index| {
                    collection.get(index).is_ok_and(|face| {
                        face.localized_strings(StringId::POSTSCRIPT_NAME)
                            .english_or_first()
                            .is_some_and(|found| found.chars().eq(name.chars()))
                    })
                })
                .unwrap_or(0),
        ),
    }
}
