//! Puts the image and UI targets onto the surface.
//!
//! The only stage that knows what the display can accept.

use bytemuck::{Pod, Zeroable};

use super::Scene;
use super::gpu::{self, Fullscreen};
use super::output::Output;
use super::placement::Placement;
use super::shader_codes;
use super::ui_layer::Color;
use crate::image::display::{Colormap, Headroom, ToneMap};

/// The most regions the checkerboard can be cut into: the image, and the
/// minimap's thumbnail.
const REGIONS: usize = 2;

/// Layout must match `struct Params` in shaders/composite.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Params {
    tone_map: u32,
    encoding: u32,
    white_scale: f32,
    checker: f32,
    base: [f32; 4],
    alternate: [f32; 4],
    regions: [[f32; 4]; REGIONS],
}

/// What is behind the image: the interface's own colour everywhere, turning
/// into a checkerboard of it and `alternate` wherever an image is drawn, so
/// that transparency reads as transparency rather than as dark pixels.
///
/// The colours come from the interface rather than being chosen here, so that
/// the backdrop and the panels around it cannot drift apart.
#[derive(Clone, Copy)]
pub struct Backdrop {
    pub base: Color,
    pub alternate: Color,
    /// Side of one square, in logical pixels.
    pub square: f32,
}

pub struct Composite {
    pipeline: wgpu::RenderPipeline,
    params: wgpu::Buffer,
    params_group: wgpu::BindGroup,
    targets_layout: wgpu::BindGroupLayout,
    targets_group: Option<wgpu::BindGroup>,
}

impl Composite {
    pub fn new(device: &wgpu::Device, surface_format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("composite"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/composite.wgsl").into()),
        });

        let params_layout =
            gpu::uniform_layout(device, "composite params", wgpu::ShaderStages::FRAGMENT);
        // Both targets are read with `textureLoad`, so no filterability
        // requirement.
        let targets_layout = gpu::texture_layout(device, "composite targets", 2, false);
        let pipeline_layout =
            gpu::pipeline_layout(device, "composite", &[&params_layout, &targets_layout]);
        let pipeline = gpu::fullscreen_pipeline(
            device,
            Fullscreen {
                label: "composite",
                shader: &shader,
                layout: &pipeline_layout,
                topology: wgpu::PrimitiveTopology::TriangleList,
                format: surface_format,
                blend: None,
            },
        );

        let params = gpu::uniform_buffer::<Params>(device, "composite params");
        let params_group = gpu::buffer_group(device, "composite params", &params_layout, &params);

        Self {
            pipeline,
            params,
            params_group,
            targets_layout,
            targets_group: None,
        }
    }

    /// Called whenever the offscreen targets are recreated.
    pub fn bind_targets(
        &mut self,
        device: &wgpu::Device,
        image_target: &wgpu::TextureView,
        ui_target: &wgpu::TextureView,
    ) {
        self.targets_group = Some(gpu::texture_group(
            device,
            "composite targets",
            &self.targets_layout,
            &[image_target, ui_target],
        ));
    }

    /// `regions` is where the checkerboard shows: the image quads this frame
    /// draws, and nothing at all on a frame with no image on screen. `gray`
    /// is whether the image on screen has one channel, which is what decides
    /// whether the false colour is on it. Of the scene, this reads the
    /// display state, the backdrop and the headroom; of the output, only how
    /// it is encoded.
    pub fn prepare(
        &self,
        queue: &wgpu::Queue,
        scene: &Scene<'_>,
        gray: bool,
        output: &Output,
        regions: [Option<Placement>; REGIONS],
    ) {
        let Scene {
            display,
            backdrop,
            scale,
            headroom,
            ..
        } = *scene;
        // False colour is already display-referred: a tone curve on top of a
        // colormap would distort the mapping the viewer is reading values
        // off, and headroom above the top of the ramp is a colour the ramp
        // does not have — so a plain clip, whatever the surface. The same
        // choice `Display::curve` makes for the readouts, and on the same
        // test: the display ignores a colormap on a colour image, so the
        // compositor has to as well.
        let tone_map = if gray && display.colormap != Colormap::Gray {
            shader_codes::tone_map(ToneMap::None, Headroom::None)
        } else {
            shader_codes::tone_map(display.tone_map, headroom)
        };

        // A region that is not drawn stays the empty rectangle it starts as,
        // which the shader's half-open test never matches.
        let mut bounds = [[0.0f32; 4]; REGIONS];
        for (slot, region) in bounds.iter_mut().zip(regions.into_iter().flatten()) {
            *slot = [
                region.x,
                region.y,
                region.x + region.width,
                region.y + region.height,
            ];
        }

        queue.write_buffer(
            &self.params,
            0,
            bytemuck::bytes_of(&Params {
                tone_map,
                encoding: shader_codes::encoding(output.encoding),
                white_scale: 1.0,
                checker: (backdrop.square * scale).max(1.0),
                base: backdrop.base.to_linear(),
                alternate: backdrop.alternate.to_linear(),
                regions: bounds,
            }),
        );
    }

    pub fn render(&self, pass: &mut wgpu::RenderPass<'_>) {
        let Some(targets) = &self.targets_group else {
            return;
        };
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.params_group, &[]);
        pass.set_bind_group(1, targets, &[]);
        pass.draw(0..3, 0..1);
    }
}
