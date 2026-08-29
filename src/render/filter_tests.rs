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
use super::image_layer::ImageLayer;
use super::upload::Capabilities;
use crate::image::display::Display;
use crate::image::{AlphaMode, Channels, ColorSpace, DecodedImage, Samples};
use crate::view::{Placement, Upscale};

struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    capabilities: Capabilities,
}

fn gpu() -> Option<Gpu> {
    let instance =
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        ..Default::default()
    }))
    .ok()?;
    let capabilities = Capabilities::from_adapter(&adapter);
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("filter tests"),
        required_features: capabilities.required_features(),
        required_limits: adapter.limits(),
        ..Default::default()
    }))
    .ok()?;
    Some(Gpu {
        device,
        queue,
        capabilities,
    })
}

/// Draws `image` into a `target`-sized working-space texture and reads it
/// back, as RGBA rows of linear values.
fn draw(gpu: &Gpu, image: &DecodedImage, target: [u32; 2], placement: Placement) -> Vec<[f32; 4]> {
    let mut layer = ImageLayer::new(&gpu.device, WORKING_FORMAT);
    layer
        .set_image(&gpu.device, &gpu.queue, image, gpu.capabilities)
        .expect("the image uploads");

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
        placement,
        [target[0] as f32, target[1] as f32],
        &Display::default(),
    );
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("filter test"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
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
    let Some(gpu) = gpu() else {
        return;
    };
    let data: Vec<u8> = (0..16).map(|value| value * 16).collect();
    let image = gray_u8(4, 4, data.clone());

    let pixels = draw(&gpu, &image, [1, 1], whole([1, 1], &image));
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
    let Some(gpu) = gpu() else {
        return;
    };
    const SIZE: u32 = 64;
    let data: Vec<u8> = (0..SIZE * SIZE).map(|index| (index % 251) as u8).collect();
    let image = gray_u8(SIZE, SIZE, data.clone());

    let pixels = draw(&gpu, &image, [4, 4], whole([4, 4], &image));
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
    let Some(gpu) = gpu() else {
        return;
    };
    let image = gray_u8(2, 2, vec![0u8, 255, 255, 0]);

    let mut placement = whole([10, 10], &image);
    placement.upscale = Upscale::Nearest;
    let pixels = draw(&gpu, &image, [10, 10], placement);

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
    let Some(gpu) = gpu() else {
        return;
    };
    let image = gray_u8(2, 2, vec![0u8, 255, 255, 0]);

    let mut placement = whole([9, 9], &image);
    placement.upscale = Upscale::Nearest;
    let pixels = draw(&gpu, &image, [9, 9], placement);

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
    let Some(gpu) = gpu() else {
        return;
    };
    let image = gray_u8(2, 2, vec![0u8, 255, 255, 0]);

    let mut placement = whole([10, 10], &image);
    placement.upscale = Upscale::Bicubic;
    let pixels = draw(&gpu, &image, [10, 10], placement);

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
    let Some(gpu) = gpu() else {
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
    let pixels = draw(&gpu, &image, [1, 1], placement);

    // The target holds premultiplied colour, so half coverage of opaque red
    // reads as half red, half alpha, and no green whatsoever.
    assert!(close(pixels[0][0], 0.5, 5e-3), "red: {:?}", pixels[0]);
    assert!(close(pixels[0][1], 0.0, 5e-3), "green: {:?}", pixels[0]);
    assert!(close(pixels[0][3], 0.5, 5e-3), "alpha: {:?}", pixels[0]);
}
