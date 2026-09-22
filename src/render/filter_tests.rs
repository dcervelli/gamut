//! What the resampling filters actually produce, checked against arithmetic
//! done on the CPU.
//!
//! These run a real pipeline on a real adapter, which is the only way to cover
//! the parts that have no CPU equivalent: the bind group layouts, the coarse
//! chain's render passes, and the shader itself. Where no adapter can be had —
//! a machine with no GPU, or a CI container without one — they report success
//! rather than failing for a reason that has nothing to do with the code.

use half::f16;

use super::composite::{Backdrop, Composite};
use super::gpu;
use super::image_layer::{Draw, ImageLayer};
use super::output::{Encoding, Output};
use super::{Color, Placement, Scene, UI_FORMAT, UiPaint, Upscale, WORKING_FORMAT};
use crate::image::color::Transfer;
use crate::image::display::{Colormap, Display, Headroom, ToneMap};
use crate::image::{AlphaMode, Channels, ColorSpace, DecodedImage, Referred, Samples};

/// Draws `image` into a `target`-sized working-space texture and reads it
/// back, as RGBA rows of linear values.
fn draw(
    gpu: &gpu::TestContext,
    image: &DecodedImage,
    target: [u32; 2],
    placement: Placement,
) -> Vec<[f32; 4]> {
    draw_all(gpu, image, target, Draw::plain(placement, None))
}

/// As [`draw`], for the frames that put down the minimap's thumbnail as well.
fn draw_all(
    gpu: &gpu::TestContext,
    image: &DecodedImage,
    target: [u32; 2],
    quads: Draw,
) -> Vec<[f32; 4]> {
    let mut layer = ImageLayer::new(&gpu.device, &gpu.queue, WORKING_FORMAT);
    let uploaded = layer
        .uploader(&gpu.device, &gpu.queue, gpu.capabilities)
        .run(image)
        .expect("the image uploads");
    layer.install(uploaded);
    draw_layer(gpu, &mut layer, target, quads)
}

/// Draws whatever `layer` holds into a `target`-sized texture and reads it
/// back.
fn draw_layer(
    gpu: &gpu::TestContext,
    layer: &mut ImageLayer,
    target: [u32; 2],
    quads: Draw,
) -> Vec<[f32; 4]> {
    draw_layer_as(gpu, layer, target, quads, &Display::default())
}

/// As [`draw_layer`], under `display` rather than the identity.
fn draw_layer_as(
    gpu: &gpu::TestContext,
    layer: &mut ImageLayer,
    target: [u32; 2],
    quads: Draw,
    display: &Display,
) -> Vec<[f32; 4]> {
    render_to(gpu, target, |encoder, view| {
        layer.prepare(
            &gpu.device,
            &gpu.queue,
            encoder,
            quads,
            [target[0] as f32, target[1] as f32],
            display,
        );
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("filter test"),
            color_attachments: &[Some(gpu::attachment(
                view,
                wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
            ))],
            ..Default::default()
        });
        layer.render(&mut pass);
    })
}

/// Runs `frame` — whatever it records into the encoder, and the pass it
/// opens on the view it is handed — into a `target`-sized working-space
/// texture, and reads the result back.
fn render_to(
    gpu: &gpu::TestContext,
    target: [u32; 2],
    frame: impl FnOnce(&mut wgpu::CommandEncoder, &wgpu::TextureView),
) -> Vec<[f32; 4]> {
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
    frame(&mut encoder, &view);
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
    gray(
        width,
        height,
        Samples::U8 {
            channels: Channels::Gray,
            data,
        },
    )
}

fn gray_f32(width: u32, height: u32, data: Vec<f32>) -> DecodedImage {
    gray(
        width,
        height,
        Samples::F32 {
            channels: Channels::Gray,
            data,
        },
    )
}

fn gray(width: u32, height: u32, samples: Samples) -> DecodedImage {
    DecodedImage {
        width,
        height,
        samples,
        color: ColorSpace::LINEAR_BT709,
        alpha: AlphaMode::Opaque,
        referred: Referred::Scene,
        exposure: None,
        nodata: None,
        gain_map: None,
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

/// The next frame of an animation is written into the texture the last one
/// has, and is what the next draw shows — through the coarse chain as well,
/// which was reduced from the old pixels and has to be built again.
#[test]
fn a_refilled_texture_draws_the_new_frame() {
    let Some(gpu) = gpu::test_context() else {
        return;
    };
    const SIZE: u32 = 64;
    let dark = gray_u8(SIZE, SIZE, vec![0; (SIZE * SIZE) as usize]);
    let bright = gray_u8(SIZE, SIZE, vec![255; (SIZE * SIZE) as usize]);

    let mut layer = ImageLayer::new(&gpu.device, &gpu.queue, WORKING_FORMAT);
    let upload = layer.uploader(&gpu.device, &gpu.queue, gpu.capabilities);
    layer.install(upload.run(&dark).expect("the image uploads"));
    // Sixteen to one builds the chain from the dark frame.
    let pixels = draw_layer(
        gpu,
        &mut layer,
        [4, 4],
        Draw::plain(whole([4, 4], &dark), None),
    );
    assert!(close(at(&pixels, 4, 1, 1), 0.0, 1e-3));

    assert!(
        layer
            .refill(&upload, &bright)
            .expect("the refill is accepted")
    );
    let pixels = draw_layer(
        gpu,
        &mut layer,
        [4, 4],
        Draw::plain(whole([4, 4], &bright), None),
    );
    assert!(
        close(at(&pixels, 4, 1, 1), 1.0, 1e-3),
        "got {}, expected the new frame through a rebuilt chain",
        at(&pixels, 4, 1, 1)
    );

    // A frame of another shape is not written into it.
    let other = gray_u8(SIZE / 2, SIZE, vec![0; (SIZE * SIZE / 2) as usize]);
    assert!(
        !layer
            .refill(&upload, &other)
            .expect("a mismatch is not an error")
    );
}

/// The claim minification rests on: an output pixel is the mean of exactly the
/// texels it covers, not a bilinear tap at its center.
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

/// Catmull-Rom is interpolating: a magnified texel center reproduces the texel
/// exactly, however its neighbors ring around it.
#[test]
fn bicubic_passes_texel_centers_through() {
    let Some(gpu) = gpu::test_context() else {
        return;
    };
    let image = gray_u8(2, 2, vec![0u8, 255, 255, 0]);

    let mut placement = whole([10, 10], &image);
    placement.upscale = Upscale::Bicubic;
    let pixels = draw(gpu, &image, [10, 10], placement);

    // At five-to-one, the center of texel 0 falls on the center of pixel 2 and
    // the center of texel 1 on that of pixel 7.
    assert!(close(at(&pixels, 10, 2, 2), 0.0, 2e-3));
    assert!(close(at(&pixels, 10, 7, 2), 1.0, 2e-3));
    assert!(close(at(&pixels, 10, 2, 7), 1.0, 2e-3));
}

/// Filtering straight alpha without multiplying it through first is what puts
/// a halo of a transparent texel's color along a hard edge. Here the
/// transparent half is green, and none of it may reach the result.
#[test]
fn a_transparent_texel_does_not_bleed_its_color() {
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
        referred: Referred::Scene,
        exposure: None,
        nodata: None,
        gain_map: None,
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

    // The target holds premultiplied color, so half coverage of opaque red
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
            mark_clipped: false,
            headroom: Headroom::None,
            lift: 0.0,
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

/// A picture with a gain map: 32 wide, 8 high, a ramp across, and a map
/// half its size that asks for nothing on the left and two stops on the
/// right, with an offset either side of the product so that the offsets
/// are seen to be applied too.
fn gain_mapped() -> DecodedImage {
    use crate::image::gain_map::{GainMap, Lift};
    use std::sync::Arc;

    const WIDTH: u32 = 32;
    const HEIGHT: u32 = 8;
    let data: Vec<u8> = (0..WIDTH * HEIGHT)
        .flat_map(|index| {
            let x = index % WIDTH;
            [(x * 8) as u8, 128, (255 - x * 8) as u8]
        })
        .collect();
    let mut image = DecodedImage::new(
        WIDTH,
        HEIGHT,
        Samples::U8 {
            channels: Channels::Rgb,
            data,
        },
        ColorSpace::SRGB,
        AlphaMode::Opaque,
    );
    // `GainMapMetadata` is non-exhaustive, so it is built by amending the
    // defaults rather than by naming every field.
    let mut metadata = ultrahdr_rs::GainMapMetadata::default();
    metadata.gain_map_max = [2.0; 3];
    metadata.gain_map_min = [0.0; 3];
    metadata.gamma = [1.0; 3];
    metadata.base_offset = [0.015625; 3];
    metadata.alternate_offset = [0.01; 3];
    metadata.alternate_hdr_headroom = 2.0;
    image.gain_map = Some(Arc::new(GainMap {
        width: WIDTH / 2,
        height: HEIGHT / 2,
        channels: 1,
        data: (0..WIDTH / 2 * HEIGHT / 2)
            .map(|index| {
                if index % (WIDTH / 2) < WIDTH / 4 {
                    0
                } else {
                    255
                }
            })
            .collect(),
        lift: Lift::Iso(metadata),
    }));
    image
}

/// The shader lifts the picture through the same map and the same table
/// the readout reads it through: at a weight, every texel drawn 1:1 is
/// what `DecodedImage::sample` says it is; at no weight it is the base;
/// and the weight is changed on the picture already on the device.
#[test]
fn the_lift_on_the_device_agrees_with_the_readout() {
    let Some(gpu) = gpu::test_context() else {
        return;
    };
    let image = gain_mapped();
    let map = image.gain_map.as_ref().unwrap();
    let target = [image.width, image.height];

    let mut layer = ImageLayer::new(&gpu.device, &gpu.queue, WORKING_FORMAT);
    let uploaded = layer
        .uploader(&gpu.device, &gpu.queue, gpu.capabilities)
        .run(&image)
        .expect("the image uploads");
    layer.install(uploaded);

    for weight in [1.0f32, 0.0, 0.5] {
        let table = map.table(weight);
        let mut quads = Draw::plain(whole(target, &image), None);
        quads.lift = weight;
        let pixels = draw_layer(gpu, &mut layer, target, quads);
        for y in 0..image.height {
            for x in 0..image.width {
                let expected = image.sample(x, y, Some(&table)).unwrap();
                let got = pixels[(y * image.width + x) as usize];
                // Relative: the device's hardware sRGB decode of the base
                // is a few parts in a thousand from the CPU's, and the lift
                // multiplies the difference along with the value.
                for (channel, (got, want)) in got.iter().zip(expected.color()).enumerate() {
                    assert!(
                        close(*got, *want, 5e-3 * want.abs().max(1.0)),
                        "weight {weight} at ({x}, {y}) channel {channel}: got {got}, expected {want}",
                    );
                }
            }
        }
    }
}

/// The coarse chain is reduced from lifted light, so a minified draw of a
/// lifted picture is the average of the lifted texels — and it is built
/// again when the weight changes, or the old lift would stay on screen at
/// every zoom past the chain's first level.
#[test]
fn the_coarse_chain_is_reduced_from_lifted_light() {
    let Some(gpu) = gpu::test_context() else {
        return;
    };
    let image = gain_mapped();
    let map = image.gain_map.as_ref().unwrap();
    // Sixteen to one across: 32 wide to 2, 8 high to 1 — wide enough to
    // force the chain.
    let target = [2, 1];
    let placement = Placement {
        x: 0.0,
        y: 0.0,
        width: 2.0,
        height: 1.0,
        zoom: 1.0 / 16.0,
        upscale: Upscale::Nearest,
    };

    let mut layer = ImageLayer::new(&gpu.device, &gpu.queue, WORKING_FORMAT);
    let uploaded = layer
        .uploader(&gpu.device, &gpu.queue, gpu.capabilities)
        .run(&image)
        .expect("the image uploads");
    layer.install(uploaded);

    for weight in [0.0f32, 1.0] {
        let table = map.table(weight);
        let mut quads = Draw::plain(placement, None);
        quads.lift = weight;
        let pixels = draw_layer(gpu, &mut layer, target, quads);
        for block in 0..2u32 {
            let mut total = [0.0f32; 3];
            for y in 0..image.height {
                for x in block * 16..block * 16 + 16 {
                    let sample = image.sample(x, y, Some(&table)).unwrap();
                    for (sum, value) in total.iter_mut().zip(sample.color()) {
                        *sum += value;
                    }
                }
            }
            let got = pixels[block as usize];
            for (channel, (got, sum)) in got.iter().zip(total).enumerate() {
                let expected = sum / (16.0 * image.height as f32);
                assert!(
                    close(*got, expected, 4e-3),
                    "weight {weight} block {block} channel {channel}: got {got}, expected {expected}"
                );
            }
        }
    }
}

/// A false color on the device is the readout's: every ramp, drawn over a
/// gray sweep through and past the window, is `Colormap::color` of the
/// windowed value — including the ends, which out-of-window values take.
/// Gray is no false color at all: the windowed value itself, unclamped,
/// left for the compositor's curve to clip.
#[test]
fn the_false_color_on_the_device_is_the_readouts() {
    let Some(gpu) = gpu::test_context() else {
        return;
    };
    const WIDTH: u32 = 300;
    let values: Vec<f32> = (0..WIDTH)
        .map(|index| index as f32 / (WIDTH - 1) as f32 * 1.2 - 0.1)
        .collect();
    let image = gray_f32(WIDTH, 1, values.clone());
    let target = [WIDTH, 1];

    let mut layer = ImageLayer::new(&gpu.device, &gpu.queue, WORKING_FORMAT);
    let uploaded = layer
        .uploader(&gpu.device, &gpu.queue, gpu.capabilities)
        .run(&image)
        .expect("the image uploads");
    layer.install(uploaded);

    for map in Colormap::ALL {
        let mut display = Display::default();
        display.set_colormap(map, true);
        let pixels = draw_layer_as(
            gpu,
            &mut layer,
            target,
            Draw::plain(whole(target, &image), None),
            &display,
        );
        for (x, value) in values.iter().enumerate() {
            let expected = if map == Colormap::Gray {
                [*value; 3]
            } else {
                map.color(*value)
            };
            let got = pixels[x];
            for (channel, (got, want)) in got.iter().zip(expected).enumerate() {
                assert!(
                    close(*got, want, 2e-3),
                    "{} at {value}: channel {channel} got {got}, expected {want}",
                    map.label()
                );
            }
        }
    }
}

/// A one-row texture in `format`, holding `texels` — written as the
/// working format's halves, or as the interface target's bytes.
fn row_texture(
    gpu: &gpu::TestContext,
    format: wgpu::TextureFormat,
    bytes: &[u8],
    width: u32,
) -> wgpu::TextureView {
    let size = wgpu::Extent3d {
        width,
        height: 1,
        depth_or_array_layers: 1,
    };
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("composite test source"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    gpu.queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        bytes,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(bytes.len() as u32),
            rows_per_image: Some(1),
        },
        size,
    );
    texture.create_view(&wgpu::TextureViewDescriptor::default())
}

/// Composites a row of opaque linear `colors`, as the image layer would
/// have left them, with nothing drawn by the interface and a black
/// backdrop, under `display` on a surface with `headroom`, out to
/// `encoding`; and reads the row back.
fn composited(
    gpu: &gpu::TestContext,
    colors: &[[f32; 3]],
    display: &Display,
    gray: bool,
    headroom: Headroom,
    encoding: Encoding,
) -> Vec<[f32; 4]> {
    let width = colors.len() as u32;
    let halves: Vec<u8> = colors
        .iter()
        .flat_map(|[r, g, b]| [*r, *g, *b, 1.0])
        .flat_map(|value| f16::from_f32(value).to_le_bytes())
        .collect();
    let image = row_texture(gpu, WORKING_FORMAT, &halves, width);
    let ui = row_texture(gpu, UI_FORMAT, &vec![0u8; width as usize * 4], width);

    let mut composite = Composite::new(&gpu.device, WORKING_FORMAT);
    composite.bind_targets(&gpu.device, &image, &ui);
    let output = Output {
        format: WORKING_FORMAT,
        color_space: wgpu::SurfaceColorSpace::Srgb,
        encoding,
        label: "test",
        is_hdr: false,
    };
    let paint = UiPaint {
        primitives: Vec::new(),
        pixels_per_point: 1.0,
    };
    let scene = Scene {
        placement: Placement {
            x: 0.0,
            y: 0.0,
            width: width as f32,
            height: 1.0,
            zoom: 1.0,
            upscale: Upscale::Nearest,
        },
        thumbnail: None,
        display,
        ui: &paint,
        scale: 1.0,
        backdrop: Backdrop {
            base: Color::rgb(0, 0, 0),
            alternate: Color::rgb(0, 0, 0),
            square: 8.0,
        },
        headroom,
        mark_clipped: false,
        lift: 0.0,
    };
    composite.prepare(&gpu.queue, &scene, gray, &output, [None, None]);
    render_to(gpu, [width, 1], |encoder, view| {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("composite test"),
            color_attachments: &[Some(gpu::attachment(
                view,
                wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
            ))],
            ..Default::default()
        });
        composite.render(&mut pass);
    })
}

/// A sweep of colors through the shadows, past white and below black, with
/// the channels apart so that the curve's desaturation has something to do.
fn sweep() -> Vec<[f32; 3]> {
    (0..64)
        .map(|index| {
            let t = index as f32 / 63.0;
            [t * 1.6 - 0.05, t * 1.2 - 0.1, t * 0.9]
        })
        .collect()
}

/// The tone curve on the device is the readout's: over a sweep past white,
/// every curve on every surface comes out as `ToneMap::apply` says — and a
/// false color holds the curve at a clip, whatever was asked for, as
/// `Display::curve_on` says.
#[test]
fn the_tone_curve_on_the_device_is_the_readouts() {
    let Some(gpu) = gpu::test_context() else {
        return;
    };
    let colors = sweep();
    let cases = [
        (ToneMap::None, Headroom::None, Colormap::Gray, false),
        (ToneMap::None, Headroom::Above, Colormap::Gray, false),
        (ToneMap::Neutral, Headroom::None, Colormap::Gray, false),
        (ToneMap::Neutral, Headroom::Above, Colormap::Gray, false),
        (ToneMap::Neutral, Headroom::Above, Colormap::Viridis, true),
        (ToneMap::Neutral, Headroom::Above, Colormap::Viridis, false),
    ];
    for (tone_map, headroom, colormap, gray) in cases {
        let mut display = Display::default();
        display.set_tone_map(tone_map, true);
        display.set_colormap(colormap, true);
        let pixels = composited(
            gpu,
            &colors,
            &display,
            gray,
            headroom,
            Encoding::ScRgbLinear,
        );
        let (curve, room) = display.curve_on(gray, headroom);
        for (color, got) in colors.iter().zip(&pixels) {
            let expected = curve.apply(*color, room);
            for (channel, (got, want)) in got.iter().zip(expected).enumerate() {
                assert!(
                    close(*got, want, 3e-3),
                    "{tone_map:?} on {headroom:?}, {} on gray {gray}, {color:?} channel {channel}: got {got}, expected {want}",
                    colormap.label()
                );
            }
        }
    }
}

/// An HDR10 surface takes the PQ curve the transfer functions define: over
/// a gray sweep, which the gamut conversion leaves alone, the device
/// encodes what `Transfer::Pq.to_encoded` does.
#[test]
fn the_pq_encoding_on_the_device_is_the_transfers() {
    let Some(gpu) = gpu::test_context() else {
        return;
    };
    let colors: Vec<[f32; 3]> = (0..64)
        .map(|index| [index as f32 / 63.0 * 2.0; 3])
        .collect();
    let display = Display::default();
    let pixels = composited(gpu, &colors, &display, false, Headroom::Above, Encoding::Pq);
    for (color, got) in colors.iter().zip(&pixels) {
        let expected = Transfer::Pq.to_encoded(color[0]);
        for (channel, got) in got[..3].iter().enumerate() {
            assert!(
                close(*got, expected, 2e-3),
                "{} channel {channel}: got {got}, expected {expected}",
                color[0]
            );
        }
    }
}
