//! The faces the interface is set in: the desktop's own sans and monospace,
//! found through fontconfig, so that the window wears what the rest of the
//! desktop is wearing. Nothing is shipped with the binary.
//!
//! egui takes fonts as bytes, so each face is read whole once at startup and
//! handed over; a face is a few hundred kilobytes, and the same bytes serve
//! every size the interface asks for.

use std::borrow::Cow;
use std::sync::Arc;

use anyhow::{Result, anyhow};
use egui::{FontData, FontDefinitions, FontFamily};

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
    let mut db = fontdb::Database::new();
    db.load_system_fonts();

    let sans = face(&db, fontdb::Family::SansSerif, fontdb::Weight::NORMAL)
        .ok_or_else(|| anyhow!("no sans-serif font is installed; the interface cannot be set"))?;
    let bold = face(&db, fontdb::Family::SansSerif, fontdb::Weight::BOLD);
    let mono = face(&db, fontdb::Family::Monospace, fontdb::Weight::NORMAL);

    let mut fonts = FontDefinitions::empty();
    fonts.font_data.insert("sans".to_owned(), Arc::new(sans));
    let mut proportional = vec!["sans".to_owned()];
    let mut monospace = Vec::new();
    let mut bold_family = Vec::new();
    if let Some(bold) = bold {
        fonts.font_data.insert(BOLD.to_owned(), Arc::new(bold));
        bold_family.push(BOLD.to_owned());
    }
    if let Some(mono) = mono {
        fonts.font_data.insert("mono".to_owned(), Arc::new(mono));
        monospace.push("mono".to_owned());
        // A glyph the sans lacks may be in the monospace face, as the
        // fallback ran under glyphon.
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

/// The bytes of the face fontconfig names for `family` at `weight`, if it
/// names one, with its index into the collection it came from.
fn face(
    db: &fontdb::Database,
    family: fontdb::Family<'_>,
    weight: fontdb::Weight,
) -> Option<FontData> {
    let id = db.query(&fontdb::Query {
        families: &[family],
        weight,
        ..Default::default()
    })?;
    db.with_face_data(id, |bytes, index| FontData {
        font: Cow::Owned(bytes.to_vec()),
        index,
        tweak: Default::default(),
    })
}
