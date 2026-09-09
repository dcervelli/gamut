//! The faces the interface is set in: the desktop's own sans and monospace,
//! so that the window wears what the rest of the desktop is wearing. Nothing
//! is shipped with the binary.
//!
//! Which face that is, is fontconfig's to say, and it is asked directly: its
//! library resolves `sans-serif` the way every other program on the desktop
//! has it resolved, through the whole of its configuration — the desktop's
//! own rules included, which on Omarchy are what name the face. Where there
//! is no fontconfig library to ask, fontdb reads the same font directories
//! itself and answers as best it can from the aliases alone.
//!
//! egui takes fonts as bytes, so each face is read whole once at startup and
//! handed over; a face is a few hundred kilobytes, and the same bytes serve
//! every size the interface asks for. With the bytes goes one number worked
//! out from them: how far the face's glyphs have to be moved for its
//! capitals to sit at the middle of the row egui gives them, since egui
//! centers a row by its full height and a face can put that height where it
//! likes around the letters.

use std::borrow::Cow;
use std::sync::Arc;

use anyhow::{Result, anyhow};
use egui::{FontData, FontDefinitions, FontFamily, FontTweak};
use fontconfig::{
    FC_FAMILY, FC_PROPORTIONAL, FC_SPACING, FC_WEIGHT, FC_WEIGHT_DEMIBOLD, FC_WEIGHT_NORMAL,
    Fontconfig, Pattern,
};
use skrifa::instance::{LocationRef, Size};
use skrifa::{FontRef, MetadataProvider};

/// The bold sans, for the one thing in the window set bold: the file's
/// name. egui's own families are the proportional and the monospace face,
/// so a weight is a family of its own, named here.
pub const BOLD: &str = "sans-bold";

/// Loads the system's sans, bold sans and monospace faces into a set of
/// font definitions egui can be given.
///
/// A desktop with no sans face at all is an error rather than a window with
/// no words in it; the monospace and the bold face each fall back to the
/// sans where the desktop names none.
pub fn system() -> Result<FontDefinitions> {
    let mut finder = Finder::new();
    let sans = finder
        .find(Want::Sans)
        .ok_or_else(|| anyhow!("no sans-serif font is installed; the interface cannot be set"))?;
    let bold = finder.find(Want::SansBold);
    let mono = finder.find(Want::Mono);

    let mut fonts = FontDefinitions::empty();
    fonts
        .font_data
        .insert("sans".to_owned(), Arc::new(sans.into()));
    let mut proportional = vec!["sans".to_owned()];
    let mut monospace = Vec::new();
    let mut bold_family = Vec::new();
    if let Some(bold) = bold {
        fonts
            .font_data
            .insert(BOLD.to_owned(), Arc::new(bold.into()));
        bold_family.push(BOLD.to_owned());
    }
    if let Some(mono) = mono {
        fonts
            .font_data
            .insert("mono".to_owned(), Arc::new(mono.into()));
        monospace.push("mono".to_owned());
        // A glyph the sans lacks may be in the monospace face.
        proportional.push("mono".to_owned());
    }
    // Every family ends in the sans, so that no glyph the sans has is
    // missing from a run set in another.
    monospace.push("sans".to_owned());
    bold_family.push("sans".to_owned());

    fonts
        .families
        .insert(FontFamily::Proportional, proportional);
    fonts.families.insert(FontFamily::Monospace, monospace);
    fonts
        .families
        .insert(FontFamily::Name(BOLD.into()), bold_family);
    Ok(fonts)
}

/// One of the three faces the interface is set in.
#[derive(Clone, Copy)]
enum Want {
    Sans,
    SansBold,
    Mono,
}

/// A face as it came off the disk: the bytes of the file holding it, and
/// which face in the file it is.
struct Face {
    bytes: Vec<u8>,
    index: u32,
}

impl From<Face> for FontData {
    fn from(face: Face) -> Self {
        let tweak = FontTweak {
            y_offset_factor: centering(&face.bytes, face.index),
            ..Default::default()
        };
        FontData {
            font: Cow::Owned(face.bytes),
            index: face.index,
            tweak,
        }
    }
}

/// Where a face is asked for: fontconfig first, and fontdb's own reading of
/// the font directories only where fontconfig's library cannot be loaded.
/// The fallback scans every installed face, so it is not opened unless it
/// is needed.
struct Finder {
    fontconfig: Option<Fontconfig>,
    fontdb: Option<fontdb::Database>,
}

impl Finder {
    fn new() -> Self {
        Self {
            fontconfig: Fontconfig::new(),
            fontdb: None,
        }
    }

    fn find(&mut self, want: Want) -> Option<Face> {
        self.fontconfig
            .as_ref()
            .and_then(|fc| desktop(fc, want))
            .or_else(|| listed(self.fontdb(), want))
    }

    fn fontdb(&mut self) -> &fontdb::Database {
        self.fontdb.get_or_insert_with(|| {
            let mut db = fontdb::Database::new();
            db.load_system_fonts();
            db
        })
    }
}

/// The face fontconfig resolves `want` to, as `fc-match` would print it.
///
/// fontconfig always answers with its nearest face, so the answer is checked
/// against the question: a bold that came back regular, or a monospace that
/// came back proportional, is no answer, and the family falls back to the
/// sans as if the desktop had named nothing.
fn desktop(fc: &Fontconfig, want: Want) -> Option<Face> {
    let (family, weight) = match want {
        Want::Sans => (c"sans-serif", FC_WEIGHT_NORMAL),
        Want::SansBold => (c"sans-serif", FC_WEIGHT_DEMIBOLD),
        Want::Mono => (c"monospace", FC_WEIGHT_NORMAL),
    };
    let mut pattern = Pattern::new(fc).ok()?;
    pattern.add_string(FC_FAMILY, family).ok()?;
    pattern.add_integer(FC_WEIGHT, weight).ok()?;
    let found = pattern.font_match().ok()?;
    match want {
        Want::Sans => {}
        Want::SansBold => {
            if found.weight().ok()? < FC_WEIGHT_DEMIBOLD {
                return None;
            }
        }
        Want::Mono => {
            if found.get_int(FC_SPACING).ok()? == FC_PROPORTIONAL {
                return None;
            }
        }
    }
    let bytes = std::fs::read(found.filename().ok()?).ok()?;
    let index = found.face_index().unwrap_or(0).try_into().ok()?;
    Some(Face { bytes, index })
}

/// The face fontdb's own reading of the font directories names for `want`,
/// where there is no fontconfig to ask.
fn listed(db: &fontdb::Database, want: Want) -> Option<Face> {
    let (family, weight) = match want {
        Want::Sans => (fontdb::Family::SansSerif, fontdb::Weight::NORMAL),
        Want::SansBold => (fontdb::Family::SansSerif, fontdb::Weight::BOLD),
        Want::Mono => (fontdb::Family::Monospace, fontdb::Weight::NORMAL),
    };
    let id = db.query(&fontdb::Query {
        families: &[family],
        weight,
        ..Default::default()
    })?;
    db.with_face_data(id, |bytes, index| Face {
        bytes: bytes.to_vec(),
        index,
    })
}

/// How far down a face's glyphs go, as a fraction of the font size, for its
/// capitals to sit at the middle of the row: the number egui's tweak takes.
///
/// A face that cannot be read, or that says nothing about the height of its
/// capitals and has no `H` to measure, is left where egui puts it.
fn centering(bytes: &[u8], index: u32) -> f32 {
    let Ok(face) = FontRef::from_index(bytes, index) else {
        return 0.0;
    };
    let unscaled = (Size::unscaled(), LocationRef::default());
    let read = face.metrics(unscaled.0, unscaled.1);
    let cap_height = read.cap_height.or_else(|| {
        let h = face.charmap().map('H')?;
        Some(face.glyph_metrics(unscaled.0, unscaled.1).bounds(h)?.y_max)
    });
    let Some(cap_height) = cap_height else {
        return 0.0;
    };
    Metrics {
        ascent: read.ascent,
        descent: read.descent,
        line_gap: read.leading,
        cap_height,
        units_per_em: f32::from(read.units_per_em),
    }
    .centering()
}

/// The vertical metrics of a face, in its own units: what egui builds a row
/// from, and where the capitals are within it.
struct Metrics {
    ascent: f32,
    /// Below the baseline, so negative, as the font has it.
    descent: f32,
    line_gap: f32,
    cap_height: f32,
    units_per_em: f32,
}

impl Metrics {
    /// egui makes a row `ascent + |descent| + line_gap` tall with the
    /// baseline `ascent` down from its top, and centers that box. The
    /// capitals are centered half their height above the baseline, and this
    /// is how far that is above the box's own middle: a face with its line
    /// gap all below the baseline, or its ascent no higher than its
    /// capitals, has its capitals riding high, and is moved down by this
    /// much. Positive is downward, as egui's tweak has it.
    fn centering(&self) -> f32 {
        let row = self.ascent - self.descent + self.line_gap;
        let cap_center = self.ascent - self.cap_height / 2.0;
        (row / 2.0 - cap_center) / self.units_per_em
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_face_with_its_gap_below_the_baseline_is_moved_down() {
        // Nimbus Sans Narrow: capitals as tall as the ascent, a fifth of an
        // em of gap under the descent.
        let face = Metrics {
            ascent: 718.0,
            descent: -282.0,
            line_gap: 200.0,
            cap_height: 718.0,
            units_per_em: 1000.0,
        };
        assert!((face.centering() - 0.241).abs() < 1e-6);
    }

    #[test]
    fn a_face_whose_box_is_already_centered_on_its_capitals_is_left_alone() {
        // Adwaita Sans.
        let face = Metrics {
            ascent: 1984.0,
            descent: -494.0,
            line_gap: 0.0,
            cap_height: 1490.0,
            units_per_em: 2048.0,
        };
        assert_eq!(face.centering(), 0.0);
    }

    #[test]
    fn a_face_with_headroom_above_its_capitals_is_moved_up() {
        // More ascent than the capitals use, and nothing under the descent:
        // the capitals sit low, so the offset goes the other way.
        let face = Metrics {
            ascent: 1000.0,
            descent: -200.0,
            line_gap: 0.0,
            cap_height: 600.0,
            units_per_em: 1000.0,
        };
        assert!(face.centering() < 0.0);
    }
}
