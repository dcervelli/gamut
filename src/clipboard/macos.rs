//! The clipboard on macOS: the general pasteboard. The pasteboard server
//! keeps what was put on it after the process that put it there has gone,
//! so a copy here is one call and nothing is left running to hold it.
//!
//! A pasteboard names its types by uniform type identifier rather than by
//! MIME type, so an [`Offer`] found here carries the identifier, which is
//! what [`receive`] is asked for it under again.

use std::fs::File;
use std::io::Write as _;
use std::path::Path;

use anyhow::{Context, Result, bail};
use objc2::rc::autoreleasepool;
use objc2::runtime::ProtocolObject;
use objc2_app_kit::{
    NSPasteboard, NSPasteboardTypePNG, NSPasteboardTypeString, NSPasteboardWriting,
};
use objc2_foundation::{NSArray, NSData, NSString, NSURL};

use super::{MAX_PASTE_BYTES, Offer, PNG, TEXT, URI_LIST};

/// The picture types this program will take a paste under, by the
/// identifier a pasteboard offers each as, and the extension a file holding
/// it is named by. `public.tiff` is what a screenshot copied to the
/// clipboard, and Preview's copy, put there.
pub(super) const IMAGE_TYPES: &[(&str, &str)] = &[
    ("public.png", "png"),
    ("public.jpeg", "jpg"),
    ("public.tiff", "tif"),
    ("public.heic", "heic"),
    ("public.heif", "heic"),
    ("public.avif", "avif"),
    ("org.webmproject.webp", "webp"),
    ("com.compuserve.gif", "gif"),
    ("com.microsoft.bmp", "bmp"),
    ("com.microsoft.ico", "ico"),
    ("com.ilm.openexr-image", "exr"),
    ("public.radiance", "hdr"),
];

/// Puts `content` on the clipboard under `mime_type`: words as a string, a
/// URI list as the file it names, which is what Finder pastes as the file
/// itself, and a PNG as a picture.
pub fn copy(content: &[u8], mime_type: &str) -> Result<()> {
    autoreleasepool(|_| {
        let board = NSPasteboard::generalPasteboard();
        board.clearContents();
        let written = match mime_type {
            TEXT => {
                let text =
                    std::str::from_utf8(content).context("copying words that are not text")?;
                board
                    .setString_forType(&NSString::from_str(text), unsafe { NSPasteboardTypeString })
            }
            URI_LIST => {
                let uri = std::str::from_utf8(content)
                    .context("copying a URI that is not text")?
                    .trim_end();
                let path = crate::uri::path_from_uri(uri)?;
                let url = NSURL::from_file_path(&path)
                    .with_context(|| format!("naming {}", crate::shown_path(&path)))?;
                let object = ProtocolObject::<dyn NSPasteboardWriting>::from_retained(url);
                board.writeObjects(&NSArray::from_retained_slice(&[object]))
            }
            PNG => board.setData_forType(Some(&NSData::with_bytes(content)), unsafe {
                NSPasteboardTypePNG
            }),
            other => bail!("the clipboard is not given {other} here"),
        };
        if !written {
            bail!("the clipboard would not take {mime_type}");
        }
        Ok(())
    })
}

/// What the clipboard is offering that this program could show, and `None`
/// when it is offering nothing of the sort.
///
/// A source puts its own type first and the conversions it can make after,
/// so the first offer this can read wins, as on Linux.
pub fn offered_image() -> Result<Option<Offer>> {
    Ok(autoreleasepool(|_| {
        let types = NSPasteboard::generalPasteboard().types()?;
        types.iter().find_map(|offered| {
            let offered = offered.to_string();
            let (_, extension) = IMAGE_TYPES
                .iter()
                .find(|(known, _)| known.eq_ignore_ascii_case(&offered))?;
            Some(Offer {
                mime: offered,
                extension,
            })
        })
    }))
}

/// Writes the clipboard's picture, as the type `kind` names, into the file
/// already made at `path`. Returns how many bytes it held.
///
/// The pasteboard hands the whole picture over at once, so unlike Linux it
/// is held in memory before it is written.
pub fn receive(kind: &str, path: &Path) -> Result<u64> {
    let bytes = autoreleasepool(|_| {
        NSPasteboard::generalPasteboard()
            .dataForType(&NSString::from_str(kind))
            .map(|data| data.to_vec())
    })
    .with_context(|| format!("the clipboard no longer holds {kind}"))?;
    let written = bytes.len() as u64;
    if written >= MAX_PASTE_BYTES {
        bail!(
            "the clipboard offered more than the {:.1} GB this build will hold",
            MAX_PASTE_BYTES as f64 / 1e9
        );
    }
    if written == 0 {
        bail!("the clipboard offered {kind} and then handed over nothing");
    }
    let mut file = File::create(path)
        .with_context(|| format!("opening {} to write", crate::shown_path(path)))?;
    file.write_all(&bytes)
        .and_then(|()| file.flush())
        .with_context(|| format!("writing {}", crate::shown_path(path)))?;
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A paste is written to a file and then opened again, so a type taken
    /// under an extension no decoder claims would be saved and never shown.
    #[test]
    fn every_type_a_paste_is_taken_under_can_be_read_back() {
        let readable = crate::image::decode::supported_extensions();
        for (kind, extension) in IMAGE_TYPES {
            assert!(
                readable.contains(extension),
                "{kind} is taken as .{extension}, which no decoder reads"
            );
        }
    }
}
