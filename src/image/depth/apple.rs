//! Apple's words for a depth map: the XMP an iPhone writes beside the
//! auxiliary image a portrait's depth is kept in.
//!
//! Two namespaces between them say what the codes are. Apple's pixel data
//! info (`apdi`) says what the camera recorded, `NativeFormat` — a Core
//! Video pixel format, written as the number its four characters make:
//! half- or full-float disparity, or depth — and how it was squeezed into
//! the codes stored, `IntMinValue` to `IntMaxValue` spread evenly over
//! `FloatMinValue` to `FloatMaxValue`. Disparity is in one over meters, and
//! depth in meters. The depth data namespace says, among the camera's
//! calibration, whether those meters can be believed: `Accuracy` is
//! `absolute` for a map measured in them, and `relative` for one two
//! cameras estimated, right about what is nearer but not about how far.
//!
//! The same packet sits beside the portrait's other auxiliary images — the
//! mattes for the person, their skin, their hair — with a `NativeFormat`
//! that is plain gray, and those are no depth at all.

use super::{Accuracy, Quantity, Scale, Unit, Vendor};
use crate::image::xmp::Xmp;

/// Apple's pixel data info.
pub const PIXEL_DATA_INFO: &str = "http://ns.apple.com/pixeldatainfo/1.0/";

/// Apple's depth data: the calibration, and how accurate the map is.
pub const DEPTH_DATA: &str = "http://ns.apple.com/depthData/1.0/";

/// The Core Video formats a depth map is recorded in, and what each holds:
/// disparity or depth, in half or full floats.
const FORMATS: [(&[u8; 4], Quantity); 4] = [
    (b"hdis", Quantity::Inverse),
    (b"fdis", Quantity::Inverse),
    (b"hdep", Quantity::Distance),
    (b"fdep", Quantity::Distance),
];

/// The scale the packet states, or `None` where it is not a depth map's —
/// a matte's, or none of Apple's — or leaves out the range.
pub fn scale(xmp: &Xmp) -> Option<Scale> {
    let word = |namespace, name| xmp.property(namespace, name)?.first().cloned();
    let number = |name| {
        word(PIXEL_DATA_INFO, name)?
            .trim()
            .parse::<f32>()
            .ok()
            .filter(|value| value.is_finite())
    };
    let format = word(PIXEL_DATA_INFO, "NativeFormat")?;
    let quantity = quantity(&format)?;
    let codes = [number("IntMinValue")?, number("IntMaxValue")?];
    let values = [number("FloatMinValue")?, number("FloatMaxValue")?];
    let accuracy = match word(DEPTH_DATA, "Accuracy").as_deref() {
        Some("absolute") => Accuracy::Absolute,
        // Unsaid is taken as the weaker of the two: a map that does not
        // claim its meters is not given them.
        _ => Accuracy::Relative,
    };
    Some(Scale {
        codes: Some(codes),
        values,
        quantity,
        unit: Unit::Meters,
        accuracy,
        vendor: Vendor::Apple,
    })
}

/// What a `NativeFormat` records: the number its four characters make, as
/// Apple writes it, or the characters themselves.
fn quantity(format: &str) -> Option<Quantity> {
    let format = format.trim();
    let code = match format.parse::<u32>() {
        Ok(number) => number.to_be_bytes(),
        Err(_) => format.as_bytes().try_into().ok()?,
    };
    FORMATS
        .iter()
        .find(|(name, _)| **name == code)
        .map(|(_, quantity)| *quantity)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The fields an iPhone wrote beside a portrait's disparity map,
    /// the calibration between them left out.
    const PORTRAIT: &str = r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
        <rdf:Description rdf:about="" xmlns:apdi="http://ns.apple.com/pixeldatainfo/1.0/" xmlns:depthData="http://ns.apple.com/depthData/1.0/">
        <apdi:IntMaxValue>255</apdi:IntMaxValue>
        <apdi:StoredFormat>1278226488</apdi:StoredFormat>
        <apdi:NativeFormat>1751411059</apdi:NativeFormat>
        <apdi:IntMinValue>0</apdi:IntMinValue>
        <apdi:FloatMaxValue>1.800781</apdi:FloatMaxValue>
        <apdi:FloatMinValue>0.360596</apdi:FloatMinValue>
        <apdi:AuxiliaryImageType>disparity</apdi:AuxiliaryImageType>
        <depthData:Quality>high</depthData:Quality>
        <depthData:Accuracy>relative</depthData:Accuracy>
        </rdf:Description></rdf:RDF></x:xmpmeta>"#;

    /// The matte beside it, which is gray and no depth.
    const MATTE: &str = r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
        <rdf:Description rdf:about="" xmlns:apdi="http://ns.apple.com/pixeldatainfo/1.0/">
        <apdi:AuxiliaryImageSubType>portraiteffectsmatte</apdi:AuxiliaryImageSubType>
        <apdi:NativeFormat>1278226488</apdi:NativeFormat>
        <apdi:AuxiliaryImageType>depth</apdi:AuxiliaryImageType>
        <apdi:StoredFormat>1278226488</apdi:StoredFormat>
        </rdf:Description></rdf:RDF></x:xmpmeta>"#;

    fn parse(packet: &str) -> Xmp {
        Xmp::parse(packet.as_bytes()).expect("a packet")
    }

    /// Half-float disparity squeezed into eight bits, in one over meters:
    /// the lowest code is the farthest thing, at 1 / 0.360596 m, and the
    /// highest the nearest, at 1 / 1.800781 m — and the map says it is an
    /// estimate.
    #[test]
    fn a_portrait_s_disparity_map_reads_in_meters() {
        let scale = scale(&parse(PORTRAIT)).expect("a scale");
        assert_eq!(scale.quantity, Quantity::Inverse);
        assert_eq!(scale.codes, Some([0.0, 255.0]));
        assert_eq!(scale.values, [0.360596, 1.800781]);
        assert_eq!(scale.unit, Unit::Meters);
        assert_eq!(scale.accuracy, Accuracy::Relative);

        let samples = crate::image::Samples::U8 {
            channels: crate::image::Channels::Gray,
            data: Vec::new(),
        };
        let at = |code| scale.distance(code, &samples).expect("a distance").value;
        assert!((at(0.0) - 2.7732).abs() < 1e-3, "{}", at(0.0));
        assert!((at(255.0) - 0.5553).abs() < 1e-3, "{}", at(255.0));
    }

    #[test]
    fn a_matte_is_no_depth() {
        assert_eq!(scale(&parse(MATTE)), None);
    }

    /// The four characters are what the number is made of, and either is
    /// read; anything else is no depth.
    #[test]
    fn a_native_format_is_read_as_its_four_characters() {
        assert_eq!(quantity("1751411059"), Some(Quantity::Inverse));
        assert_eq!(quantity("hdis"), Some(Quantity::Inverse));
        assert_eq!(quantity("fdep"), Some(Quantity::Distance));
        assert_eq!(
            quantity(&u32::from_be_bytes(*b"fdep").to_string()),
            Some(Quantity::Distance)
        );
        assert_eq!(quantity("1278226488"), None, "eight-bit gray");
        assert_eq!(quantity("disparity"), None);
    }
}
