//! Decoding of image files into the one pixel format the renderer speaks.
//!
//! Every supported family of formats is a [`Decoder`]. To teach the viewer a
//! new format, write a `Decoder` and add it to [`DECODERS`] — nothing else in
//! the program needs to change.

use std::path::Path;

use anyhow::{Context, Result, anyhow};

mod image_rs;

/// A decoded image: 8-bit non-premultiplied RGBA, top row first, tightly packed.
pub struct DecodedImage {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

pub trait Decoder: Sync {
    /// Human-readable name, used in error messages.
    fn name(&self) -> &'static str;

    /// Lowercase file extensions, without the leading dot.
    fn extensions(&self) -> &'static [&'static str];

    /// Does this look like one of our formats, judging only by the leading
    /// bytes? Used to recover from a missing or misleading extension.
    fn sniff(&self, header: &[u8]) -> bool;

    fn decode(&self, bytes: &[u8]) -> Result<DecodedImage>;
}

/// The registry. Order matters only when two decoders claim the same
/// extension, in which case the first one wins.
static DECODERS: &[&dyn Decoder] = &[&image_rs::ImageRs];

/// Every extension the viewer knows how to open, for `--help` and errors.
pub fn supported_extensions() -> Vec<&'static str> {
    let mut exts: Vec<&'static str> = DECODERS
        .iter()
        .flat_map(|d| d.extensions())
        .copied()
        .collect();
    exts.sort_unstable();
    exts.dedup();
    exts
}

/// Read and decode `path`, picking a decoder by extension and falling back to
/// content sniffing.
pub fn load(path: &Path) -> Result<DecodedImage> {
    let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;

    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();

    let decoder = DECODERS
        .iter()
        .find(|d| d.extensions().contains(&ext.as_str()))
        .or_else(|| DECODERS.iter().find(|d| d.sniff(&bytes)))
        .ok_or_else(|| {
            anyhow!(
                "unsupported image format for {} (known extensions: {})",
                path.display(),
                supported_extensions().join(", ")
            )
        })?;

    let image = decoder
        .decode(&bytes)
        .with_context(|| format!("decoding {} as {}", path.display(), decoder.name()))?;

    if image.width == 0 || image.height == 0 {
        return Err(anyhow!("{} has zero size", path.display()));
    }
    Ok(image)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 3x2 image encoded with whichever format `image` writes for `format`.
    fn encode(format: image::ImageFormat) -> Vec<u8> {
        let mut source = image::RgbaImage::new(3, 2);
        source.put_pixel(0, 0, image::Rgba([255, 0, 0, 255]));
        source.put_pixel(2, 1, image::Rgba([0, 0, 255, 255]));

        let mut bytes = Vec::new();
        image::DynamicImage::ImageRgba8(source)
            .write_to(&mut std::io::Cursor::new(&mut bytes), format)
            .expect("encoding the fixture");
        bytes
    }

    fn decode_via_registry(bytes: &[u8], name: &str) -> DecodedImage {
        let dir = std::env::temp_dir().join(format!("image-view-test-{name}"));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, bytes).unwrap();
        let decoded = load(&path).expect("decoding through the registry");
        std::fs::remove_dir_all(&dir).ok();
        decoded
    }

    #[test]
    fn decodes_every_advertised_format() {
        for (name, format) in [
            ("fixture.png", image::ImageFormat::Png),
            ("fixture.jpg", image::ImageFormat::Jpeg),
            ("fixture.tif", image::ImageFormat::Tiff),
        ] {
            let decoded = decode_via_registry(&encode(format), name);
            assert_eq!((decoded.width, decoded.height), (3, 2), "{name}");
            assert_eq!(decoded.rgba.len(), 3 * 2 * 4, "{name}");
        }
    }

    #[test]
    fn a_wrong_extension_falls_back_to_sniffing() {
        // Named `.tif`, actually a PNG.
        let decoded = decode_via_registry(&encode(image::ImageFormat::Png), "liar.tif");
        assert_eq!((decoded.width, decoded.height), (3, 2));
    }

    #[test]
    fn an_unknown_format_is_reported_not_guessed() {
        let error = decode_via_registry_err(b"not an image at all", "mystery.xyz");
        assert!(error.contains("unsupported image format"), "{error}");
    }

    fn decode_via_registry_err(bytes: &[u8], name: &str) -> String {
        let dir = std::env::temp_dir().join(format!("image-view-test-{name}"));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, bytes).unwrap();
        let error = match load(&path) {
            Ok(_) => panic!("`{name}` should not have decoded"),
            Err(error) => error,
        };
        std::fs::remove_dir_all(&dir).ok();
        format!("{error:#}")
    }

    #[test]
    fn every_extension_is_claimed_by_exactly_one_decoder() {
        let mut seen = std::collections::HashSet::new();
        for decoder in DECODERS {
            for extension in decoder.extensions() {
                assert!(seen.insert(*extension), "`{extension}` is claimed twice");
                assert_eq!(
                    *extension,
                    extension.to_ascii_lowercase(),
                    "extensions must be lowercase"
                );
            }
        }
    }
}
