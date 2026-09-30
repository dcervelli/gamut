//! Google's words for a depth map: the `GDepth` block of a JPEG's XMP, as
//! the camera app's portraits and Lens Blur write it.
//!
//! The block names two planes, `Near` and `Far`, in `Units`, and a `Format`
//! saying how the codes run between them: `RangeLinear`, evenly over the
//! distance, or `RangeInverse`, evenly over its inverse — the first code at
//! the near plane either way. Where the map itself is kept is the JPEG's
//! business; see `decode/jpeg/depth.rs`.

use super::{Accuracy, Quantity, Scale, Unit, Vendor};
use crate::image::xmp::Xmp;

/// Google's namespace for the depth map's properties.
pub const NAMESPACE: &str = "http://ns.google.com/photos/1.0/depthmap/";

/// The scale the block states, or `None` where it states no format this
/// knows or no pair of planes. Nothing in the block says the distances are
/// estimates, so they are taken as stated.
pub fn scale(xmp: &Xmp) -> Option<Scale> {
    let word = |name| xmp.property(NAMESPACE, name)?.first().cloned();
    let number = |name| {
        word(name)?
            .trim()
            .parse::<f32>()
            .ok()
            .filter(|value| value.is_finite())
    };
    let (near, far) = (number("Near")?, number("Far")?);
    let (quantity, values) = match word("Format")?.as_str() {
        "RangeLinear" => (Quantity::Distance, [near, far]),
        "RangeInverse" if near > 0.0 && far > 0.0 => (Quantity::Inverse, [1.0 / near, 1.0 / far]),
        _ => return None,
    };
    Some(Scale {
        codes: None,
        values,
        quantity,
        unit: word("Units").map_or(Unit::Unknown, |word| Unit::parse(&word)),
        accuracy: Accuracy::Absolute,
        vendor: Vendor::Google,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(attributes: &str) -> Xmp {
        Xmp::parse(
            format!(
                r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
                <rdf:Description rdf:about="" xmlns:GDepth="{NAMESPACE}" {attributes}/>
                </rdf:RDF></x:xmpmeta>"#
            )
            .as_bytes(),
        )
        .expect("a packet")
    }

    /// The two formats are the same two planes, spread over the distance or
    /// over its inverse; the first code is the near plane either way.
    #[test]
    fn both_formats_run_from_the_near_plane_to_the_far_one() {
        let linear = scale(&block(
            r#"GDepth:Format="RangeLinear" GDepth:Near="1" GDepth:Far="4" GDepth:Units="m""#,
        ))
        .expect("a scale");
        assert_eq!(linear.quantity, Quantity::Distance);
        assert_eq!(linear.values, [1.0, 4.0]);
        assert_eq!(linear.unit, Unit::Meters);

        let inverse = scale(&block(
            r#"GDepth:Format="RangeInverse" GDepth:Near="0.5" GDepth:Far="4" GDepth:Units="mm""#,
        ))
        .expect("a scale");
        assert_eq!(inverse.quantity, Quantity::Inverse);
        assert_eq!(inverse.values, [2.0, 0.25]);
        assert_eq!(inverse.unit, Unit::Millimeters);
    }

    #[test]
    fn a_block_missing_a_plane_or_a_known_format_states_nothing() {
        assert_eq!(
            scale(&block(r#"GDepth:Format="RangeLinear" GDepth:Near="1""#)),
            None
        );
        assert_eq!(
            scale(&block(
                r#"GDepth:Format="RangeCubic" GDepth:Near="1" GDepth:Far="4""#
            )),
            None
        );
        assert_eq!(
            scale(&block(
                r#"GDepth:Format="RangeInverse" GDepth:Near="0" GDepth:Far="4""#
            )),
            None
        );
    }
}
