//! The desktop's faces on Linux, as fontconfig resolves them.
//!
//! Its library resolves `sans-serif` the way every other program on the
//! desktop has it resolved, through the whole of its configuration — the
//! desktop's own rules included, which on Omarchy are what name the face.
//! The library is opened at run time, so a desktop without it still gets a
//! window, from fontdb's rougher reading.

use fontconfig::{
    FC_FAMILY, FC_PROPORTIONAL, FC_SPACING, FC_WEIGHT, FC_WEIGHT_DEMIBOLD, FC_WEIGHT_NORMAL,
    Fontconfig, Pattern,
};

use super::{Face, Want};

/// fontconfig's library, where it could be loaded.
pub(super) struct Desktop(Option<Fontconfig>);

impl Desktop {
    pub(super) fn new() -> Self {
        Self(Fontconfig::new())
    }

    /// The face the desktop names for `want`, or `None` where it names none
    /// or there is no library to ask.
    pub(super) fn find(&self, want: Want) -> Option<Face> {
        self.0.as_ref().and_then(|fc| resolve(fc, want))
    }
}

/// The face fontconfig resolves `want` to, as `fc-match` would print it.
///
/// fontconfig always answers with its nearest face, so the answer is checked
/// against the question: a bold that came back regular, or a monospace that
/// came back proportional, is no answer, and the family falls back to the
/// sans as if the desktop had named nothing.
fn resolve(fc: &Fontconfig, want: Want) -> Option<Face> {
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
    Some(Face {
        bytes,
        index,
        coords: Vec::new(),
    })
}
