//! The shape a region is held to: free, the picture's own, or a ratio of
//! width to height chosen from the presets or typed.
//!
//! The picture's own is kept as itself rather than as the ratio it came to
//! when it was chosen, so that it follows the picture: stepping to a file of
//! another shape, or turning this one, holds the region to that shape.
//! Turned, it is the picture's ratio the other way up.

/// What a region is held to.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub enum Aspect {
    /// Nothing: a region is whatever shape it is drawn.
    #[default]
    Free,
    /// The picture's own width to height, or — `turned` — its height to
    /// its width.
    Original { turned: bool },
    /// So many across to so many down, each positive and finite.
    Ratio(f64, f64),
}

/// The ratios the menu offers, landscape: each is offered the other way
/// up too while the aspect in force is portrait.
pub const PRESETS: [(f64, f64); 5] = [(1.0, 1.0), (5.0, 4.0), (4.0, 3.0), (3.0, 2.0), (16.0, 9.0)];

/// The longest ratio either way a typed one may be. Past it the shorter
/// side of any region worth drawing is under a pixel.
const MOST: f64 = 100.0;

impl Aspect {
    /// The width over the height a region is held to on a picture `image`
    /// pixels across and down, as shown, whose pixels are `shape` as wide
    /// as they are tall, or `None` for a free one.
    ///
    /// In what the picture is measured in rather than in its pixels: a 3:2
    /// typed while a print or a map is measured is 3:2 on the paper or the
    /// ground, which on pixels that are not square is another shape in
    /// them. The picture's own shape is its shape in the same units, and so
    /// comes out its pixels' either way.
    pub fn ratio(self, image: [u32; 2], shape: f64) -> Option<f64> {
        match self {
            Aspect::Free => None,
            Aspect::Original { turned } => {
                let [width, height] = image.map(|side| f64::from(side.max(1)));
                let ratio = width * shape / height;
                Some(if turned { 1.0 / ratio } else { ratio })
            }
            Aspect::Ratio(across, down) => Some(across / down),
        }
    }

    /// The same ratio in the picture's pixels, which is what a region is
    /// held to.
    pub fn in_pixels(self, image: [u32; 2], shape: f64) -> Option<f64> {
        self.ratio(image, shape).map(|ratio| ratio / shape)
    }

    /// The same shape the other way up. Free is free either way up.
    pub fn swapped(self) -> Self {
        match self {
            Aspect::Free => Aspect::Free,
            Aspect::Original { turned } => Aspect::Original { turned: !turned },
            Aspect::Ratio(across, down) => Aspect::Ratio(down, across),
        }
    }

    /// What it becomes once the picture is turned a quarter: a ratio is
    /// of the region, which turns with the picture, and so turns too; the
    /// picture's own is of the picture, which has turned with it already.
    pub fn quarter_turned(self) -> Self {
        match self {
            Aspect::Ratio(..) => self.swapped(),
            Aspect::Free | Aspect::Original { .. } => self,
        }
    }

    /// Whether it is taller than it is wide on a picture `image` pixels
    /// across and down whose pixels are `shape`: which way up the menu
    /// offers its ratios.
    pub fn portrait(self, image: [u32; 2], shape: f64) -> bool {
        self.ratio(image, shape).is_some_and(|ratio| ratio < 1.0)
    }

    /// The ratios the menu offers, the way up `portrait` asks for.
    pub fn presets(portrait: bool) -> impl Iterator<Item = Aspect> {
        PRESETS
            .into_iter()
            .map(move |(across, down)| match portrait {
                true => Aspect::Ratio(down, across),
                false => Aspect::Ratio(across, down),
            })
    }

    /// What it is called: `Free`, `Original`, or the ratio as `3:2`.
    pub fn label(self) -> String {
        match self {
            Aspect::Free => "Free".to_string(),
            Aspect::Original { .. } => "Original".to_string(),
            Aspect::Ratio(across, down) => format!("{}:{}", number(across), number(down)),
        }
    }

    /// The aspect typed into the panel's field: `3:2`, `3/2`, `3x2` or
    /// `3×2`, a single number as that to one, or the words `free` and
    /// `original`. `None` for anything else, a side that is not a positive
    /// number, and a ratio longer than a hundred to one either way.
    pub fn parse(typed: &str) -> Option<Self> {
        let typed = typed.trim();
        if typed.eq_ignore_ascii_case("free") {
            return Some(Aspect::Free);
        }
        if typed.eq_ignore_ascii_case("original") {
            return Some(Aspect::Original { turned: false });
        }
        let side = |word: &str| {
            let value: f64 = word.trim().parse().ok()?;
            (value.is_finite() && value > 0.0).then_some(value)
        };
        let (across, down) = match typed.split_once([':', '/', 'x', 'X', '\u{00d7}']) {
            Some((across, down)) => (side(across)?, side(down)?),
            None => (side(typed)?, 1.0),
        };
        let ratio = across / down;
        (ratio.is_finite() && (1.0 / MOST..=MOST).contains(&ratio))
            .then_some(Aspect::Ratio(across, down))
    }
}

/// A side of a ratio written as short as it goes: `3`, `2.39`, never
/// `3.0` or a float's tail of digits.
fn number(value: f64) -> String {
    let rounded = format!("{value:.3}");
    rounded
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_typed_ratio_is_read_in_any_of_its_spellings() {
        for typed in ["3:2", " 3 : 2 ", "3/2", "3x2", "3X2", "3\u{00d7}2"] {
            assert_eq!(
                Aspect::parse(typed),
                Some(Aspect::Ratio(3.0, 2.0)),
                "{typed}"
            );
        }
        assert_eq!(Aspect::parse("2.39"), Some(Aspect::Ratio(2.39, 1.0)));
        assert_eq!(Aspect::parse("2.39:1"), Some(Aspect::Ratio(2.39, 1.0)));
        assert_eq!(Aspect::parse("FREE"), Some(Aspect::Free));
        assert_eq!(
            Aspect::parse("Original"),
            Some(Aspect::Original { turned: false })
        );
        for refused in [
            "", ":", "3:", ":2", "0:1", "-3:2", "a:b", "inf:1", "NaN", "101:1", "1:101", "3:2:1",
        ] {
            assert_eq!(Aspect::parse(refused), None, "{refused:?}");
        }
    }

    #[test]
    fn a_ratio_is_written_as_short_as_it_goes() {
        assert_eq!(Aspect::Ratio(3.0, 2.0).label(), "3:2");
        assert_eq!(Aspect::Ratio(2.39, 1.0).label(), "2.39:1");
        assert_eq!(Aspect::Ratio(1.0 / 3.0, 1.0).label(), "0.333:1");
        assert_eq!(Aspect::Free.label(), "Free");
        assert_eq!(Aspect::Original { turned: true }.label(), "Original");
        // What is written reads back as what it says.
        let written = Aspect::Ratio(16.0, 9.0).label();
        assert_eq!(Aspect::parse(&written), Some(Aspect::Ratio(16.0, 9.0)));
    }

    #[test]
    fn the_pictures_own_follows_the_picture_and_turns() {
        let original = Aspect::Original { turned: false };
        assert_eq!(original.ratio([600, 400], 1.0), Some(1.5));
        assert_eq!(original.ratio([400, 600], 1.0), Some(400.0 / 600.0));
        assert_eq!(
            original.swapped().ratio([600, 400], 1.0),
            Some(400.0 / 600.0)
        );
        assert!(original.swapped().portrait([600, 400], 1.0));
        // The picture turned has turned it already.
        assert_eq!(original.quarter_turned(), original);
        assert_eq!(Aspect::Free.ratio([600, 400], 1.0), None);
        assert_eq!(Aspect::Free.swapped(), Aspect::Free);
    }

    /// On pixels twice as wide as they are tall — a raster 20 m across a
    /// pixel and 10 m down — a square on the ground is twice as many pixels
    /// down as across; the picture's own shape is its pixels' either way.
    #[test]
    fn a_ratio_is_in_the_units_and_not_the_pixels() {
        let square = Aspect::Ratio(1.0, 1.0);
        assert_eq!(square.ratio([600, 400], 2.0), Some(1.0));
        assert_eq!(square.in_pixels([600, 400], 2.0), Some(0.5));
        let original = Aspect::Original { turned: false };
        assert_eq!(original.ratio([600, 400], 2.0), Some(3.0));
        assert_eq!(original.in_pixels([600, 400], 2.0), Some(1.5));
        // 600 by 400 pixels is 12000 by 4000 meters: wide, whichever way the
        // pixels alone would say.
        assert!(!original.portrait([300, 400], 2.0));
    }

    #[test]
    fn a_ratio_swaps_and_turns_with_the_region() {
        let ratio = Aspect::Ratio(3.0, 2.0);
        assert_eq!(ratio.swapped(), Aspect::Ratio(2.0, 3.0));
        assert_eq!(ratio.quarter_turned(), Aspect::Ratio(2.0, 3.0));
        assert_eq!(ratio.swapped().swapped(), ratio);
        assert!(!ratio.portrait([1, 1], 1.0));
        assert!(ratio.swapped().portrait([1, 1], 1.0));
        let portrait: Vec<_> = Aspect::presets(true).collect();
        assert!(portrait.contains(&Aspect::Ratio(2.0, 3.0)));
        assert!(portrait.contains(&Aspect::Ratio(1.0, 1.0)));
    }
}
