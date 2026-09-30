//! What else on the desktop can open the file on screen, and starting one of
//! them: the list the menu under the open button offers. Where the answer is
//! kept is the platform's — `linux.rs` reads the freedesktop desktop entries,
//! and `macos.rs` asks Launch Services. And `edit`, which opens a text file
//! in whatever edits text, for the configuration file: on Linux the editor
//! `VISUAL` or `EDITOR` names, or else the desktop's default for plain text,
//! each in a terminal where its desktop entry asks for one; on a Mac the
//! application that opens plain text. And `browse`, which opens a web
//! address in the browser, for the map of where a picture was taken.

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::{Opener, browse, edit, for_file, open};

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::{Opener, browse, edit, for_file, open};

/// The longest name an item of the menu may wear, in characters.
///
/// A program's name is two or three words and nothing here needs to bound it
/// for its own sake. What does is the menu: its items are as wide as the
/// longest name on them — see `ui::menu::open_items` — so a name that ran on
/// would be a popup wider than the window it opened in.
pub const MAX_NAME: usize = 48;

/// `name` cut to [`MAX_NAME`], with the ellipsis that says it was cut.
pub(super) fn shortened(name: &str) -> String {
    match name.char_indices().nth(MAX_NAME) {
        Some((end, _)) => format!("{}\u{2026}", &name[..end]),
        None => name.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A name is cut to a length a menu can wear, and said to have been cut.
    /// Nothing ordinary is touched.
    #[test]
    fn a_name_too_long_for_a_menu_is_cut() {
        assert_eq!(
            shortened("GNU Image Manipulation Program"),
            "GNU Image Manipulation Program"
        );
        let long = "N".repeat(MAX_NAME + 10);
        let cut = shortened(&long);
        assert_eq!(cut.chars().count(), MAX_NAME + 1);
        assert!(cut.ends_with('\u{2026}'));
        // Cut by characters and not by bytes: a name in another script comes
        // back as text rather than as a panic on a split character.
        let wide = "\u{753b}".repeat(MAX_NAME + 2);
        assert_eq!(shortened(&wide).chars().count(), MAX_NAME + 1);
    }
}
