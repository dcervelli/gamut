//! What the interface layer actually puts on the device's pixels.
//!
//! The claim these check is the one the whole of `ui/icon.rs` is built on: a
//! stroke a whole number of device pixels wide, centred where those pixels
//! meet, comes out of the shader fully covered on the pixels it lands on and
//! not at all on the ones beside them. It cannot be checked on the CPU —
//! coverage is the fragment shader's arithmetic — so this runs a real
//! pipeline on a real adapter and reads the frame back, the way
//! [`filter_tests`](super::filter_tests) does. Where no adapter can be had,
//! [`alpha_of`] reports `None` and the tests that use it pass.

use super::{UI_FORMAT, UiFrame, UiRenderer, gpu};

/// The alpha of every device pixel [`pixels_of`] drew, row by row.
///
/// Alpha is the whole of what most of these tests want: the shader
/// premultiplies, so a pixel the shape covers entirely comes back at 255, one
/// it misses at 0, and one it feathers at something in between.
pub(crate) fn alpha_of(frame: &UiFrame, logical: [f32; 2], scale: f32) -> Option<Vec<u8>> {
    Some(
        pixels_of(frame, logical, scale)?
            .into_iter()
            .map(|texel| texel[3])
            .collect(),
    )
}

/// The interface's own fonts, for a test that has to know how wide a label
/// will come out: what a button sets aside for one is only right in the face
/// it will be set in. `None` where the machine has no adapter to build them
/// with, as [`pixels_of`] is.
pub(crate) fn test_fonts() -> Option<UiRenderer> {
    let gpu = gpu::test_context()?;
    Some(UiRenderer::new(&gpu.device, &gpu.queue, UI_FORMAT))
}

/// Draws `frame` into a `logical`-sized interface target at `scale` and hands
/// back every device pixel of it, row by row, as the target holds them:
/// sRGB-encoded and premultiplied. `None` where the machine has no adapter to
/// draw with.
pub(crate) fn pixels_of(frame: &UiFrame, logical: [f32; 2], scale: f32) -> Option<Vec<[u8; 4]>> {
    let gpu = gpu::test_context()?;
    let physical = [
        (logical[0] * scale).round() as u32,
        (logical[1] * scale).round() as u32,
    ];

    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("ui test target"),
        size: wgpu::Extent3d {
            width: physical[0],
            height: physical[1],
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: UI_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

    // Readback rows are padded to the copy alignment, and trimmed below.
    let row = (physical[0] as u64 * 4).div_ceil(256) * 256;
    let readback = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("ui test readback"),
        size: row * physical[1] as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    let mut ui = UiRenderer::new(&gpu.device, &gpu.queue, UI_FORMAT);
    ui.prepare(&gpu.device, &gpu.queue, frame, physical, scale)
        .expect("the frame prepares");

    let mut encoder = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("ui test"),
            color_attachments: &[Some(gpu::attachment(
                &view,
                wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
            ))],
            ..Default::default()
        });
        ui.render(&mut pass).expect("the frame draws");
    }
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(row as u32),
                rows_per_image: Some(physical[1]),
            },
        },
        wgpu::Extent3d {
            width: physical[0],
            height: physical[1],
            depth_or_array_layers: 1,
        },
    );
    gpu.queue.submit(Some(encoder.finish()));

    readback.map_async(wgpu::MapMode::Read, .., |result| {
        result.expect("the readback maps");
    });
    gpu.device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("the device finishes the frame");

    let mapped = readback.slice(..).get_mapped_range().expect("mapped");
    let mut pixels = Vec::with_capacity((physical[0] * physical[1]) as usize);
    for y in 0..physical[1] as usize {
        let start = y * row as usize;
        let bytes = &mapped[start..start + physical[0] as usize * 4];
        pixels.extend_from_slice(bytes.as_chunks::<4>().0);
    }
    Some(pixels)
}

/// How many pixels of `alpha` are neither covered nor missed: the feather,
/// which is what a stroke off the device's grid leaves behind.
pub(crate) fn feathered(alpha: &[u8]) -> usize {
    alpha
        .iter()
        .filter(|value| **value != 0 && **value != 255)
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::{Color, Rect};

    const INK: Color = Color::rgb(255, 255, 255);
    /// The scales a display actually asks for, whole and fractional. 1.6 is
    /// what this was written on.
    const SCALES: [f32; 6] = [1.0, 1.25, 1.5, 1.6, 1.75, 2.0];

    const AREA: [f32; 2] = [20.0, 12.0];

    /// The alpha down one column of the frame.
    fn column(alpha: &[u8], at: f32, scale: f32) -> Vec<u8> {
        let width = (AREA[0] * scale).round() as usize;
        let x = (at * scale) as usize;
        alpha.chunks(width).map(|row| row[x]).collect()
    }

    /// A stroke put where [`UiFrame::stroke_centre_in_device`] says has both long
    /// edges on a device pixel boundary: a slice across it is that many whole
    /// pixels of ink and nothing else. That is the entire claim `ui/icon.rs`
    /// rests on, and it holds at every scale.
    ///
    /// Across rather than over the whole frame because the ends of a stroke
    /// are round caps, as Lucide draws them, and the four pixels a cap curves
    /// through are meant to be feathered — that is the cap, not a blur.
    #[test]
    fn a_snapped_stroke_covers_whole_pixels_and_no_others() {
        for scale in SCALES {
            let mut frame = UiFrame::new(scale);
            let width = frame.line_width(1.0);
            let y = frame.stroke_centre_in_device(5.3 * scale, width);
            let (from, to) = (
                [frame.stroke_centre_in_device(2.7 * scale, width), y],
                [frame.stroke_centre_in_device(17.2 * scale, width), y],
            );
            frame.stroke(from, to, width, INK);
            let Some(alpha) = alpha_of(&frame, AREA, scale) else {
                return;
            };
            let across = column(&alpha, 10.0, scale);
            assert_eq!(
                feathered(&across),
                0,
                "scale {scale}: a feathered stroke, {across:?}"
            );
            assert_eq!(
                across.iter().filter(|value| **value == 255).count(),
                (width * scale).round() as usize,
                "scale {scale}: the stroke is not the width it was asked for"
            );
        }
    }

    /// And the same stroke rounded to a whole *logical* pixel instead — what
    /// the interface did before — is feathered along its whole length
    /// wherever the scale is not a whole number. Here so that the test above
    /// is known to be able to tell the difference.
    #[test]
    fn a_stroke_rounded_in_logical_pixels_is_not() {
        let scale = 1.5;
        let mut frame = UiFrame::new(scale);
        let width = frame.line_width(1.0);
        frame.stroke([3.0, 5.0], [17.0, 5.0], width, INK);
        let Some(alpha) = alpha_of(&frame, AREA, scale) else {
            return;
        };
        assert!(
            feathered(&column(&alpha, 10.0, scale)) > 0,
            "a stroke off the device grid came out sharp anyway"
        );
    }

    /// A label placed by [`UiRenderer::cap_centre`] sits level in its box.
    ///
    /// The reported fault: a run is laid out in a box that keeps room under
    /// the baseline for descenders, so centring *that* leaves a label with
    /// none — "17%", "500 px" — hanging below the mark beside it.
    ///
    /// Measured on the top and bottom of the ink, which for a label of digits
    /// is the box its capitals stand in. Not on the ink's weight: a seven is
    /// top-heavy, so the weight of "17%" sits above its own middle and would
    /// call the label misplaced when it is not. A device pixel is as close as
    /// snapping the run to whole pixels can land it.
    #[test]
    fn a_label_sits_level_in_its_box() {
        use crate::render::TextMeasure;

        let Some(context) = gpu::test_context() else {
            return;
        };
        for scale in SCALES {
            let mut fonts = UiRenderer::new(&context.device, &context.queue, UI_FORMAT);
            let mut frame = UiFrame::new(scale);
            let button = Rect::new(0.0, 0.0, 40.0, 22.0);
            let top = frame.snap(button.y + button.height / 2.0 - fonts.cap_centre(13.0));
            frame.text([4.0, top], 13.0, INK, "17%");

            let Some(alpha) = alpha_of(&frame, [button.width, button.height], scale) else {
                return;
            };
            let width = (button.width * scale).round() as usize;
            // Faint enough to catch the feathered edge of a glyph, dark
            // enough to ignore the fringe beyond it.
            let inked: Vec<usize> = alpha
                .chunks(width)
                .enumerate()
                .filter(|(_, row)| row.iter().any(|value| *value > 16))
                .map(|(row, _)| row)
                .collect();
            assert!(!inked.is_empty(), "scale {scale}: nothing drawn");
            let caps = (inked[0] + inked[inked.len() - 1] + 1) as f32 / 2.0;
            let middle = (button.height * scale).round() / 2.0;
            assert!(
                (caps - middle).abs() <= 1.0,
                "scale {scale}: the capitals centre on {caps}, the box on {middle}"
            );
        }
    }

    /// A filled rectangle snapped with [`UiFrame::snap_rect`] has no soft
    /// edges either, anywhere: the same promise for the shapes that are not
    /// strokes, and with no caps to make an exception for.
    #[test]
    fn a_snapped_rectangle_has_hard_edges() {
        for scale in SCALES {
            let mut frame = UiFrame::new(scale);
            let rect = frame.snap_rect(Rect::new(2.3, 3.7, 9.4, 4.1));
            frame.rect(rect, INK);
            let Some(alpha) = alpha_of(&frame, AREA, scale) else {
                return;
            };
            assert!(alpha.contains(&255), "scale {scale}: nothing drawn");
            assert_eq!(feathered(&alpha), 0, "scale {scale}: a feathered rectangle");
        }
    }
}
