//! Every integer the shaders switch on, in one place.
//!
//! Each function here is one half of a contract whose other half is a
//! `switch` in a WGSL file. Keep the two in step: a value added on one side
//! and not the other fails silently, as the wrong branch rather than an error.

use crate::image::display::{Colormap, Headroom, ToneMap};
use crate::image::orient::Turn;
use crate::image::{AlphaMode, Channels};

use super::output::Encoding;
use super::placement::Upscale;

/// Which way the texture runs across the quad: quarter turns clockwise.
/// Matches the `switch` on `turn` in `vs_main` of `shaders/image.wgsl`,
/// whose corners are `orient::stored`'s reading of the same turn.
pub fn turn(turn: Turn) -> u32 {
    turn.quarters()
}

/// How the shader should expand the sampled components to RGBA.
/// Matches `swizzle` in `shaders/image.wgsl` and `shaders/reduce.wgsl`.
pub fn swizzle(channels: Channels) -> u32 {
    match channels {
        Channels::Gray => 0,
        Channels::GrayAlpha => 1,
        Channels::Rgb => 2,
        Channels::Rgba => 3,
    }
}

/// How the shader should treat the alpha channel it samples from the image
/// as uploaded. Matches `alpha_mode` in `shaders/image.wgsl` and
/// `shaders/reduce.wgsl`.
pub fn alpha(alpha: AlphaMode) -> u32 {
    match alpha {
        AlphaMode::Opaque => 0,
        AlphaMode::Straight => 1,
        AlphaMode::Premultiplied => 2,
    }
}

/// What a coarse level holds. Straight alpha has been multiplied through on
/// the way in; an image whose alpha channel is meaningless keeps it that way,
/// since dividing the color back out by it would be nonsense. Same switch as
/// [`alpha`].
pub fn level_alpha(alpha: AlphaMode) -> u32 {
    match alpha {
        AlphaMode::Opaque => 0,
        AlphaMode::Straight | AlphaMode::Premultiplied => 2,
    }
}

/// Which row of the ramps texture `image_layer` writes holds `map`, and
/// what `colormap` in `shaders/image.wgsl` reads it by. Gray is 0, which the
/// shader takes as no false color at all.
pub fn colormap(map: Colormap) -> u32 {
    match map {
        Colormap::Gray => 0,
        Colormap::Viridis => 1,
        Colormap::Magma => 2,
        Colormap::Turbo => 3,
    }
}

/// Matches `tone_map` in `shaders/composite.wgsl`, and `ToneMap::apply` in
/// `image/display/tone_map.rs`, which is the same match on the CPU: no curve is a
/// clip at white on an SDR surface and a pass-through on one with room above
/// it, and the curve is itself whatever the surface.
pub fn tone_map(map: ToneMap, headroom: Headroom) -> u32 {
    match (map, headroom) {
        (ToneMap::None, Headroom::None) => 0,
        (ToneMap::Neutral, _) => 1,
        (ToneMap::None, Headroom::Above) => 2,
    }
}

/// Which filter a draw at `zoom` runs. Matches `resampler` in
/// `shaders/image.wgsl`, where 0 is the area filter minification uses.
///
/// Minification is an area average; magnification is whichever of the two
/// the user asked for. At exactly 1:1 both come to the same thing, so the
/// boundary is not a visible one.
pub fn resampler(zoom: f32, upscale: Upscale) -> u32 {
    if zoom < 1.0 {
        return 0;
    }
    match upscale {
        Upscale::Nearest => 1,
        Upscale::Bicubic => 2,
    }
}

/// Which ends of the window the image shader paints its warning colors
/// over, as the bits of `marks` in `shaders/image.wgsl`: the pixels at or
/// below black, and the pixels at or above white. Nothing while the key for
/// them is up, which is the usual state; and white only where the surface
/// is actually clipping it — no curve on, and no room above white — since a
/// highlight rolled off by a curve or shown by an HDR surface is not lost.
pub fn marks(black: bool, white: bool) -> u32 {
    u32::from(black) | (u32::from(white) << 1)
}

/// The two warning colors, in linear light, as `MARK_WHITE` and `MARK_BLACK`
/// in `shaders/image.wgsl` have them: what a clipped highlight and a clipped
/// shadow are painted. Here so that a test can hold the shader to them.
#[cfg(test)]
pub const MARKS: [[f32; 3]; 2] = [[1.0, 0.02, 0.02], [0.02, 0.1, 1.0]];

/// Matches `encoding` in `shaders/composite.wgsl`.
pub fn encoding(encoding: Encoding) -> u32 {
    match encoding {
        Encoding::Srgb => 0,
        Encoding::ScRgbLinear => 1,
        Encoding::Pq => 2,
    }
}

/// The shaders as naga reads them, for the tests that hold one side of a
/// contract to the other: where `struct Params` puts each member, which
/// values a `switch` on one of them has arms for, what a member is compared
/// against, and what a constant holds. Read from the parsed module rather
/// than from the text, so that reformatting a shader cannot fail a test
/// that is about its meaning.
#[cfg(test)]
pub(super) mod wgsl {
    use std::collections::BTreeSet;

    use wgpu::naga::{
        BinaryOperator, Block, Expression, Function, Handle, Literal, Module, Statement,
        SwitchValue, TypeInner, front::wgsl::parse_str,
    };

    /// The names and offsets of a Rust `Params`, in declaration order, as
    /// [`assert_params_match`] takes them.
    macro_rules! fields {
        ($params:ty: $($field:ident),* $(,)?) => {
            &[$((stringify!($field), std::mem::offset_of!($params, $field))),*]
        };
    }
    pub(crate) use fields;

    /// `source` parsed, or the parser's own account of why it could not be.
    pub fn parse(source: &str) -> Module {
        parse_str(source).unwrap_or_else(|error| panic!("{}", error.emit_to_string(source)))
    }

    /// Holds a Rust `Params` to the shader's `struct Params`: the same
    /// members, in the same order, at the same offsets, and the two the
    /// same size. `fields` is the Rust side, from [`fields!`]; `size` is
    /// `size_of` it.
    pub fn assert_params_match(source: &str, fields: &[(&str, usize)], size: usize) {
        let module = parse(source);
        let (_, params) = module
            .types
            .iter()
            .find(|(_, ty)| ty.name.as_deref() == Some("Params"))
            .expect("the shader declares a struct Params");
        let TypeInner::Struct { members, span } = &params.inner else {
            panic!("Params is not a struct");
        };
        let shader: Vec<(&str, usize)> = members
            .iter()
            .map(|member| {
                (
                    member.name.as_deref().expect("a named member"),
                    member.offset as usize,
                )
            })
            .collect();
        assert_eq!(shader, fields, "the members and their offsets");
        assert_eq!(*span as usize, size, "the struct's size");
    }

    /// The function called `name`, whether it is an entry point or not.
    fn function<'a>(module: &'a Module, name: &str) -> &'a Function {
        module
            .functions
            .iter()
            .map(|(_, function)| function)
            .chain(module.entry_points.iter().map(|entry| &entry.function))
            .find(|function| function.name.as_deref() == Some(name))
            .unwrap_or_else(|| panic!("no function {name}"))
    }

    /// Which member of the uniform `params` the expression reads, where it
    /// is a read of one: `params.swizzle` lowers to a load through an index
    /// into the global.
    fn member_read(
        module: &Module,
        function: &Function,
        expression: Handle<Expression>,
    ) -> Option<String> {
        match function.expressions[expression] {
            Expression::Load { pointer } => member_read(module, function, pointer),
            Expression::AccessIndex { base, index } => {
                let Expression::GlobalVariable(global) = function.expressions[base] else {
                    return None;
                };
                let global = &module.global_variables[global];
                if global.name.as_deref() != Some("params") {
                    return None;
                }
                let TypeInner::Struct { members, .. } = &module.types[global.ty].inner else {
                    return None;
                };
                members[index as usize].name.clone()
            }
            _ => None,
        }
    }

    /// Every statement in `block` and the blocks nested in it, in order.
    fn walk<'a>(block: &'a Block, each: &mut impl FnMut(&'a Statement)) {
        for statement in block.iter() {
            each(statement);
            match statement {
                Statement::Block(inner) => walk(inner, each),
                Statement::If { accept, reject, .. } => {
                    walk(accept, each);
                    walk(reject, each);
                }
                Statement::Switch { cases, .. } => {
                    for case in cases {
                        walk(&case.body, each);
                    }
                }
                Statement::Loop {
                    body, continuing, ..
                } => {
                    walk(body, each);
                    walk(continuing, each);
                }
                _ => {}
            }
        }
    }

    /// The `case` values of every `switch` on `params.<member>` in the
    /// function called `name`, one set per switch in the order they
    /// appear. Each switch has a `default` arm besides, so what a set
    /// leaves out is what the default takes.
    pub fn cases(module: &Module, name: &str, member: &str) -> Vec<BTreeSet<u32>> {
        let function = function(module, name);
        let mut switches = Vec::new();
        walk(&function.body, &mut |statement| {
            let Statement::Switch { selector, cases } = statement else {
                return;
            };
            if member_read(module, function, *selector).as_deref() != Some(member) {
                return;
            }
            switches.push(
                cases
                    .iter()
                    .filter_map(|case| match case.value {
                        SwitchValue::U32(value) => Some(value),
                        SwitchValue::I32(value) => Some(value as u32),
                        SwitchValue::Default => None,
                    })
                    .collect(),
            );
        });
        assert!(
            !switches.is_empty(),
            "no switch on params.{member} in {name}"
        );
        switches
    }

    /// Every comparison of `params.<member>` against a literal in the
    /// function called `name`, in the order they appear.
    pub fn comparisons(module: &Module, name: &str, member: &str) -> Vec<(BinaryOperator, u32)> {
        let function = function(module, name);
        function
            .expressions
            .iter()
            .filter_map(|(_, expression)| {
                let Expression::Binary { op, left, right } = *expression else {
                    return None;
                };
                if member_read(module, function, left).as_deref() != Some(member) {
                    return None;
                }
                match function.expressions[right] {
                    Expression::Literal(Literal::U32(value)) => Some((op, value)),
                    Expression::Literal(Literal::I32(value)) => Some((op, value as u32)),
                    _ => None,
                }
            })
            .collect()
    }

    /// The components of the module-scope `const` called `name`, which is
    /// a float or a vector of them.
    pub fn constant(module: &Module, name: &str) -> Vec<f32> {
        fn floats(module: &Module, expression: Handle<Expression>, out: &mut Vec<f32>) {
            match &module.global_expressions[expression] {
                Expression::Literal(Literal::F32(value)) => out.push(*value),
                Expression::Literal(Literal::AbstractFloat(value)) => out.push(*value as f32),
                Expression::Compose { components, .. } => {
                    for component in components {
                        floats(module, *component, out);
                    }
                }
                Expression::Splat { size, value } => {
                    for _ in 0..*size as usize {
                        floats(module, *value, out);
                    }
                }
                other => panic!("{other:?} is not a float constant"),
            }
        }
        let (_, constant) = module
            .constants
            .iter()
            .find(|(_, constant)| constant.name.as_deref() == Some(name))
            .unwrap_or_else(|| panic!("no const {name}"));
        let mut out = Vec::new();
        floats(module, constant.init, &mut out);
        out
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use wgpu::naga::BinaryOperator;

    use super::wgsl;
    use super::*;
    use crate::render::{IMAGE_SHADER, REDUCE_SHADER};

    const IMAGE: &str = include_str!("shaders/image.wgsl");
    const REDUCE: &str = include_str!("shaders/reduce.wgsl");
    const COMPOSITE: &str = include_str!("shaders/composite.wgsl");

    fn set(codes: impl IntoIterator<Item = u32>) -> BTreeSet<u32> {
        codes.into_iter().collect()
    }

    /// The layout is switched on twice over: to premultiply, in the texel
    /// reading both image shaders share, where gray and RGB have no alpha
    /// and are left to the default; and to expand to RGBA in the image
    /// layer, where RGBA is the default.
    #[test]
    fn the_layouts_are_the_shaders_swizzle_arms() {
        let with_alpha = set([swizzle(Channels::GrayAlpha), swizzle(Channels::Rgba)]);
        let expanded = set([
            swizzle(Channels::Gray),
            swizzle(Channels::GrayAlpha),
            swizzle(Channels::Rgb),
        ]);
        for source in [IMAGE_SHADER, REDUCE_SHADER] {
            let module = wgsl::parse(source);
            assert_eq!(
                wgsl::cases(&module, "premultiplied", "swizzle"),
                std::slice::from_ref(&with_alpha)
            );
        }
        let image = wgsl::parse(IMAGE_SHADER);
        assert_eq!(wgsl::cases(&image, "expanded", "swizzle"), [expanded]);
    }

    /// The texel reading is shared by being prepended, not copied: neither
    /// shader carries a reading of its own.
    #[test]
    fn the_texel_reading_is_in_neither_shader() {
        for source in [IMAGE, REDUCE] {
            for name in ["fn premultiplied", "fn gains", "fn gain", "fn load"] {
                assert!(!source.contains(name), "{name}");
            }
            assert!(source.contains("load("));
        }
    }

    /// Alpha is compared rather than switched on: straight alpha is the
    /// one mode that is multiplied through, and opaque the one the image
    /// layer does not divide back out.
    #[test]
    fn the_alpha_modes_are_the_shaders_comparisons() {
        let straight = (BinaryOperator::NotEqual, alpha(AlphaMode::Straight));
        for source in [IMAGE_SHADER, REDUCE_SHADER] {
            let module = wgsl::parse(source);
            assert_eq!(
                wgsl::comparisons(&module, "premultiplied", "alpha_mode"),
                [straight]
            );
        }
        let image = wgsl::parse(IMAGE_SHADER);
        let opaque = (BinaryOperator::Equal, alpha(AlphaMode::Opaque));
        assert_eq!(
            wgsl::comparisons(&image, "expanded", "alpha_mode"),
            [opaque]
        );
        assert_eq!(
            level_alpha(AlphaMode::Straight),
            alpha(AlphaMode::Premultiplied)
        );
    }

    /// The ramps are rows of one texture, so the codes are its row indices:
    /// every map has a row of its own, and gray's is the first, which the
    /// shader takes as no false color.
    #[test]
    fn the_colormaps_are_the_ramps_rows() {
        let rows = set(Colormap::ALL.iter().map(|map| colormap(*map)));
        assert_eq!(rows, set(0..Colormap::ALL.len() as u32));
        assert_eq!(colormap(Colormap::Gray), 0);
        let image = wgsl::parse(IMAGE_SHADER);
        assert_eq!(
            wgsl::comparisons(&image, "shade", "colormap"),
            [(BinaryOperator::NotEqual, colormap(Colormap::Gray))]
        );
    }

    /// The curve and the pass-through are arms; the clip is the default.
    #[test]
    fn the_tone_maps_are_the_compositors_arms() {
        let arms = set([
            tone_map(ToneMap::Neutral, Headroom::None),
            tone_map(ToneMap::None, Headroom::Above),
        ]);
        let composite = wgsl::parse(COMPOSITE);
        assert_eq!(wgsl::cases(&composite, "tone_map", "tone_map"), [arms]);
        assert_eq!(
            tone_map(ToneMap::None, Headroom::None),
            0,
            "the clip is the default arm"
        );
        assert_eq!(
            tone_map(ToneMap::Neutral, Headroom::Above),
            tone_map(ToneMap::Neutral, Headroom::None),
            "the curve is itself whatever the surface"
        );
    }

    /// The two magnifiers are arms; the area filter is the default.
    #[test]
    fn the_magnifiers_are_the_shaders_arms() {
        let arms = set([
            resampler(2.0, Upscale::Nearest),
            resampler(2.0, Upscale::Bicubic),
        ]);
        let image = wgsl::parse(IMAGE_SHADER);
        assert_eq!(wgsl::cases(&image, "resample", "resampler"), [arms]);
        for upscale in [Upscale::Nearest, Upscale::Bicubic] {
            assert_eq!(resampler(0.5, upscale), 0, "minifying is the default arm");
        }
    }

    /// The three quarter turns are arms; no turn is the default.
    #[test]
    fn the_turns_are_the_shaders_arms() {
        let quarter = Turn::NONE.clockwise();
        let arms = set([
            turn(quarter),
            turn(quarter.clockwise()),
            turn(quarter.clockwise().clockwise()),
        ]);
        let image = wgsl::parse(IMAGE_SHADER);
        assert_eq!(wgsl::cases(&image, "turned", "turn"), [arms]);
        assert_eq!(turn(Turn::NONE), 0, "no turn is the default arm");
    }

    /// The two SDR-shaped encodings are arms; PQ is the default.
    #[test]
    fn the_encodings_are_the_compositors_arms() {
        let arms = set([encoding(Encoding::Srgb), encoding(Encoding::ScRgbLinear)]);
        let composite = wgsl::parse(COMPOSITE);
        assert_eq!(wgsl::cases(&composite, "fs_main", "encoding"), [arms]);
        assert_eq!(encoding(Encoding::Pq), 2, "PQ is the default arm");
    }

    /// The two bits are the two ends, and nothing is marked while the key
    /// is up.
    #[test]
    fn the_marks_are_one_bit_an_end() {
        assert_eq!(marks(false, false), 0);
        assert_eq!(marks(true, false), 1);
        assert_eq!(marks(false, true), 2);
        assert_eq!(marks(true, true), 3);
    }

    /// The shader paints the two colors [`MARKS`] says it does: each of its
    /// constants is held to one, so that a change to either side without
    /// the other fails here rather than on screen.
    #[test]
    fn the_shader_paints_the_marks_the_codes_name() {
        let image = wgsl::parse(IMAGE_SHADER);
        for (name, color) in [("MARK_WHITE", MARKS[0]), ("MARK_BLACK", MARKS[1])] {
            assert_eq!(wgsl::constant(&image, name), color, "{name}");
        }
    }
}
