//! What the resampling filters actually produce, checked against arithmetic
//! done on the CPU.
//!
//! These run a real pipeline on a real adapter, which is the only way to cover
//! the parts that have no CPU equivalent: the bind group layouts, the coarse
//! chain's render passes, and the shader itself. Where no adapter can be had —
//! a machine with no GPU, or a CI container without one — they report success
//! rather than failing for a reason that has nothing to do with the code.

use half::f16;

use super::WORKING_FORMAT;
use super::gpu;
use super::image_layer::{Draw, ImageLayer};
use super::{Placement, Upscale};
use crate::image::display::Display;
use crate::image::{AlphaMode, Channels, ColorSpace, DecodedImage, Samples};

/// Draws `image` into a `target`-sized working-space texture and reads it
/// back, as RGBA rows of linear values.
fn draw(
    gpu: &gpu::TestContext,
    image: &DecodedImage,
    target: [u32; 2],
    placement: Placement,
) -> Vec<[f32; 4]> {
    draw_all(
        gpu,
        image,
        target,
        Draw {
            view: placement,
            thumbnail: None,
        },
    )
}

/// As [`draw`], for the frames that put down the minimap's thumbnail as well.
fn draw_all(
    gpu: &gpu::TestContext,
    image: &DecodedImage,
    target: [u32; 2],
    quads: Draw,
) -> Vec<[f32; 4]> {
    let mut layer = ImageLayer::new(&gpu.device, WORKING_FORMAT);
    let uploaded = layer
        .uploader(&gpu.device, &gpu.queue, gpu.capabilities)
        .run(image)
        .expect("the image uploads");
    layer.install(uploaded);

    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("filter test target"),
        size: wgpu::Extent3d {
            width: target[0],
            height: target[1],
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: WORKING_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

    // Readback rows are padded to the copy alignment, and trimmed below.
    let row = (target[0] as u64 * 8).div_ceil(256) * 256;
    let readback = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("filter test readback"),
        size: row * target[1] as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    let mut encoder = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    layer.prepare(
        &gpu.device,
        &gpu.queue,
        &mut encoder,
        quads,
        [target[0] as f32, target[1] as f32],
        &Display::default(),
    );
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("filter test"),
            color_attachments: &[Some(gpu::attachment(
                &view,
                wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
            ))],
            ..Default::default()
        });
        layer.render(&mut pass);
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
                rows_per_image: Some(target[1]),
            },
        },
        wgpu::Extent3d {
            width: target[0],
            height: target[1],
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
    let mut pixels = Vec::with_capacity((target[0] * target[1]) as usize);
    for y in 0..target[1] as usize {
        let start = y * row as usize;
        let bytes = &mapped[start..start + target[0] as usize * 8];
        for texel in bytes.as_chunks::<8>().0 {
            let mut components = [0.0f32; 4];
            for (slot, pair) in components.iter_mut().zip(texel.as_chunks::<2>().0) {
                *slot = f16::from_bits(u16::from_le_bytes(*pair)).to_f32();
            }
            pixels.push(components);
        }
    }
    pixels
}

fn gray_u8(width: u32, height: u32, data: Vec<u8>) -> DecodedImage {
    DecodedImage {
        width,
        height,
        samples: Samples::U8 {
            channels: Channels::Gray,
            data,
        },
        color: ColorSpace::LINEAR_BT709,
        alpha: AlphaMode::Opaque,
        value_range: None,
        nodata: None,
    }
}

/// The whole image, drawn to fill a target of `target` pixels.
fn whole(target: [u32; 2], image: &DecodedImage) -> Placement {
    Placement {
        x: 0.0,
        y: 0.0,
        width: target[0] as f32,
        height: target[1] as f32,
        zoom: target[0] as f32 / image.width as f32,
        upscale: Upscale::Nearest,
    }
}

/// The first component of the pixel at (`x`, `y`) in a `width`-wide readback.
fn at(pixels: &[[f32; 4]], width: usize, x: usize, y: usize) -> f32 {
    pixels[y * width + x][0]
}

fn close(a: f32, b: f32, tolerance: f32) -> bool {
    (a - b).abs() <= tolerance
}

/// The claim minification rests on: an output pixel is the mean of exactly the
/// texels it covers, not a bilinear tap at its centre.
#[test]
fn minification_averages_every_texel_it_covers() {
    let Some(gpu) = gpu::test_context() else {
        return;
    };
    let data: Vec<u8> = (0..16).map(|value| value * 16).collect();
    let image = gray_u8(4, 4, data.clone());

    let pixels = draw(gpu, &image, [1, 1], whole([1, 1], &image));
    let expected = data.iter().map(|v| *v as f32 / 255.0).sum::<f32>() / 16.0;
    assert!(
        close(pixels[0][0], expected, 1e-3),
        "got {}, expected the mean {expected}",
        pixels[0][0]
    );
}

/// Two levels of the coarse chain, then the draw's own area filter, have to
/// come to the same thing as averaging the source directly. Sixteen to one is
/// the factor that forces the chain to be used at all.
#[test]
fn the_coarse_chain_agrees_with_a_direct_average() {
    let Some(gpu) = gpu::test_context() else {
        return;
    };
    const SIZE: u32 = 64;
    let data: Vec<u8> = (0..SIZE * SIZE).map(|index| (index % 251) as u8).collect();
    let image = gray_u8(SIZE, SIZE, data.clone());

    let pixels = draw(gpu, &image, [4, 4], whole([4, 4], &image));
    for block_y in 0..4usize {
        for block_x in 0..4usize {
            let mut total = 0.0f32;
            for y in 0..16usize {
                for x in 0..16usize {
                    let index = (block_y * 16 + y) * SIZE as usize + block_x * 16 + x;
                    total += data[index] as f32 / 255.0;
                }
            }
            let expected = total / 256.0;
            let got = at(&pixels, 4, block_x, block_y);
            assert!(
                close(got, expected, 2e-3),
                "block ({block_x}, {block_y}): got {got}, expected {expected}"
            );
        }
    }
}

/// Antialiased nearest has to stay nearest where it matters: at a whole-number
/// zoom every output pixel is a texel, with nothing blended in between.
#[test]
fn antialiased_nearest_is_exact_at_whole_zooms() {
    let Some(gpu) = gpu::test_context() else {
        return;
    };
    let image = gray_u8(2, 2, vec![0u8, 255, 255, 0]);

    let mut placement = whole([10, 10], &image);
    placement.upscale = Upscale::Nearest;
    let pixels = draw(gpu, &image, [10, 10], placement);

    for (index, pixel) in pixels.iter().enumerate() {
        let value = pixel[0];
        assert!(
            close(value, 0.0, 1e-3) || close(value, 1.0, 1e-3),
            "pixel ({}, {}) came out at {value}, between two texels at 5:1",
            index % 10,
            index / 10
        );
    }
    // And it is the right texel of the two, not merely a crisp one.
    assert!(close(at(&pixels, 10, 1, 1), 0.0, 1e-3));
    assert!(close(at(&pixels, 10, 8, 1), 1.0, 1e-3));
    assert!(close(at(&pixels, 10, 1, 8), 1.0, 1e-3));
    assert!(close(at(&pixels, 10, 8, 8), 0.0, 1e-3));
}

/// The case plain nearest gets wrong: at 4.5:1 the texel edge lands mid-pixel,
/// and that one pixel resolves it rather than having to pick a side.
#[test]
fn antialiased_nearest_resolves_an_edge_that_lands_mid_pixel() {
    let Some(gpu) = gpu::test_context() else {
        return;
    };
    let image = gray_u8(2, 2, vec![0u8, 255, 255, 0]);

    let mut placement = whole([9, 9], &image);
    placement.upscale = Upscale::Nearest;
    let pixels = draw(gpu, &image, [9, 9], placement);

    // Well inside a texel, still that texel.
    assert!(close(at(&pixels, 9, 1, 1), 0.0, 1e-3));
    assert!(close(at(&pixels, 9, 7, 1), 1.0, 1e-3));
    assert!(close(at(&pixels, 9, 1, 7), 1.0, 1e-3));
    // The middle pixel sits on the crossing of both edges, so it is the mean
    // of all four texels rather than whichever one won a rounding.
    assert!(
        close(at(&pixels, 9, 4, 4), 0.5, 5e-3),
        "got {}",
        at(&pixels, 9, 4, 4)
    );
}

/// Catmull-Rom is interpolating: a magnified texel centre reproduces the texel
/// exactly, however its neighbours ring around it.
#[test]
fn bicubic_passes_texel_centres_through() {
    let Some(gpu) = gpu::test_context() else {
        return;
    };
    let image = gray_u8(2, 2, vec![0u8, 255, 255, 0]);

    let mut placement = whole([10, 10], &image);
    placement.upscale = Upscale::Bicubic;
    let pixels = draw(gpu, &image, [10, 10], placement);

    // At five-to-one, the centre of texel 0 falls on the centre of pixel 2 and
    // the centre of texel 1 on that of pixel 7.
    assert!(close(at(&pixels, 10, 2, 2), 0.0, 2e-3));
    assert!(close(at(&pixels, 10, 7, 2), 1.0, 2e-3));
    assert!(close(at(&pixels, 10, 2, 7), 1.0, 2e-3));
}

/// Filtering straight alpha without multiplying it through first is what puts
/// a halo of a transparent texel's colour along a hard edge. Here the
/// transparent half is green, and none of it may reach the result.
#[test]
fn a_transparent_texel_does_not_bleed_its_colour() {
    let Some(gpu) = gpu::test_context() else {
        return;
    };
    let image = DecodedImage {
        width: 2,
        height: 1,
        samples: Samples::U8 {
            channels: Channels::Rgba,
            data: vec![255, 0, 0, 255, 0, 255, 0, 0],
        },
        color: ColorSpace::LINEAR_BT709,
        alpha: AlphaMode::Straight,
        value_range: None,
        nodata: None,
    };

    let placement = Placement {
        x: 0.0,
        y: 0.0,
        width: 1.0,
        height: 1.0,
        zoom: 0.5,
        upscale: Upscale::Nearest,
    };
    let pixels = draw(gpu, &image, [1, 1], placement);

    // The target holds premultiplied colour, so half coverage of opaque red
    // reads as half red, half alpha, and no green whatsoever.
    assert!(close(pixels[0][0], 0.5, 5e-3), "red: {:?}", pixels[0]);
    assert!(close(pixels[0][1], 0.0, 5e-3), "green: {:?}", pixels[0]);
    assert!(close(pixels[0][3], 0.5, 5e-3), "alpha: {:?}", pixels[0]);
}

/// The minimap's thumbnail is a second quad in the same pass, from the same
/// texture, and it is what decides whether the coarse chain is built: here
/// the view is at 1:1 and wants nothing of it, while the thumbnail is shrunk
/// sixteen to one and cannot be drawn without it.
#[test]
fn the_thumbnail_is_drawn_beside_the_view_and_builds_the_chain_it_needs() {
    let Some(gpu) = gpu::test_context() else {
        return;
    };
    const SIZE: u32 = 64;
    const THUMBNAIL: u32 = 4;
    let data: Vec<u8> = (0..SIZE * SIZE).map(|index| (index % 251) as u8).collect();
    let image = gray_u8(SIZE, SIZE, data.clone());

    let target = [SIZE + THUMBNAIL, SIZE + THUMBNAIL];
    let pixels = draw_all(
        gpu,
        &image,
        target,
        Draw {
            view: Placement {
                x: 0.0,
                y: 0.0,
                width: SIZE as f32,
                height: SIZE as f32,
                zoom: 1.0,
                upscale: Upscale::Nearest,
            },
            thumbnail: Some(Placement {
                x: SIZE as f32,
                y: SIZE as f32,
                width: THUMBNAIL as f32,
                height: THUMBNAIL as f32,
                zoom: THUMBNAIL as f32 / SIZE as f32,
                upscale: Upscale::Nearest,
            }),
        },
    );

    let width = target[0] as usize;
    // The view is untouched by the second draw: 1:1, texel for texel.
    for (x, y) in [(0usize, 0usize), (17, 5), (63, 63)] {
        let expected = data[y * SIZE as usize + x] as f32 / 255.0;
        let got = at(&pixels, width, x, y);
        assert!(close(got, expected, 2e-3), "view ({x}, {y}): {got}");
    }

    // And the thumbnail is the whole image averaged down, block by block.
    for block_y in 0..THUMBNAIL as usize {
        for block_x in 0..THUMBNAIL as usize {
            let mut total = 0.0f32;
            for y in 0..16usize {
                for x in 0..16usize {
                    let index = (block_y * 16 + y) * SIZE as usize + block_x * 16 + x;
                    total += data[index] as f32 / 255.0;
                }
            }
            let expected = total / 256.0;
            let got = at(
                &pixels,
                width,
                SIZE as usize + block_x,
                SIZE as usize + block_y,
            );
            assert!(
                close(got, expected, 2e-3),
                "thumbnail ({block_x}, {block_y}): got {got}, expected {expected}"
            );
        }
    }

    // Nothing was drawn in the corner neither of them covers.
    assert_eq!(at(&pixels, width, SIZE as usize + 1, 3), 0.0);
}
