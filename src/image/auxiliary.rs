//! The images a picture carries beside it, and which of a file's images is
//! the one on screen.
//!
//! A file read in one [`Rendering`](super::decode::Rendering) gives one
//! picture, and the picture may carry other images of the same scene: a depth
//! map and a gain map now, and the mattes a portrait keeps — the person, their hair, their
//! skin, the sky — as the kinds grow. Each of them can be shown in the
//! picture's place, and what is shown is what everything else reads: the size
//! the top bar gives, the pixel under the pointer, the histogram, a copy, an
//! export. So an auxiliary image is turned into a [`DecodedImage`] of its own
//! by [`DecodedImage::auxiliary`], and from there on is shown like any
//! picture — at its own size, through a display of its own — rather than
//! being a special case of each of those readers.
//!
//! That is the difference between *showing* an auxiliary image and *applying*
//! one. The gain map is applied: it changes how the picture itself is drawn,
//! and is carried on the picture for that. It can also be shown, as a picture
//! of its own made from it, which is not the map the picture is drawn
//! through: the picture shown is the map read out in stops, and the picture
//! held while it is up keeps its lift. A matte applied as the picture's alpha
//! would be the same kind of thing, and would be carried the same way;
//! [`Showing`] is only ever about which image is up.

use super::DecodedImage;

/// A kind of image a picture can carry beside it.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Auxiliary {
    /// How far away each pixel was — see [`super::depth`].
    Depth,
    /// How far above SDR white each pixel goes — see [`super::gain_map`].
    GainMap,
}

/// Which of a file's images is on screen: the picture, or one it carries.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum Showing {
    #[default]
    Picture,
    Auxiliary(Auxiliary),
}

impl DecodedImage {
    /// The auxiliary image of `kind` this picture carries, as a picture of
    /// its own to be shown in its place: `None` where it carries none.
    pub fn auxiliary(&self, kind: Auxiliary) -> Option<DecodedImage> {
        match kind {
            Auxiliary::Depth => self.depth.as_ref().map(|map| map.image()),
            Auxiliary::GainMap => self.gain_map.as_ref().map(|map| map.image()),
        }
    }

    /// Whether this picture carries an auxiliary image of `kind`, without
    /// making the image to find out.
    pub fn carries(&self, kind: Auxiliary) -> bool {
        match kind {
            Auxiliary::Depth => self.depth.is_some(),
            Auxiliary::GainMap => self.gain_map.is_some(),
        }
    }
}
