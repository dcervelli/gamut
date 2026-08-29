//! Draws the image into the linear working-space target.
//!
//! Everything colour-related that varies per frame lives in one uniform, so
//! changing exposure, the window or the colormap costs a buffer write rather
//! than a re-decode. Resampling is chosen the same way: which filter to run
//! and which level of the coarse chain to read are two more fields in it.

use anyhow::Result;
use bytemuck::{Pod, Zeroable};

use super::reduce::{self, Level, Reducer};
use super::upload::{self, Capabilities};
use crate::image::{AlphaMode, DecodedImage, display::Display};
use crate::view::Placement;

/// Layout must match `struct Params` in shaders/image.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Params {
    offset: [f32; 2],
    scale: [f32; 2],
    window: [f32; 2],
    texels_per_pixel: [f32; 2],
    extent: [f32; 2],
    _pad: [f32; 2],
    /// Column-major, each column padded to 16 bytes, as WGSL wants a mat3x3.
    primaries: [[f32; 4]; 3],
    swizzle: u32,
    alpha_mode: u32,
    colormap: u32,
    resampler: u32,
}

/// One uploaded image and the constants that describe it.
pub struct GpuImage {
    size: [u32; 2],
    view: wgpu::TextureView,
    /// Bind groups indexed by level: 0 is the image as uploaded, the rest are
    /// the coarse chain. Empty past the first until a view zooms out far
    /// enough to want it, since most never do.
    bindings: Vec<wgpu::BindGroup>,
    levels: Vec<Level>,
    /// Set once the chain has been built, which is not the same as its being
    /// non-empty: an image only a few texels across has no levels to make.
    chain_built: bool,
    level_format: wgpu::TextureFormat,
    swizzle: u32,
    alpha: AlphaMode,
    primaries: [[f32; 4]; 3],
    pub format: wgpu::TextureFormat,
    pub precision_note: Option<&'static str>,
}

pub struct ImageLayer {
    pipeline: wgpu::RenderPipeline,
    texture_layout: wgpu::BindGroupLayout,
    params: wgpu::Buffer,
    params_group: wgpu::BindGroup,
    reducer: Reducer,
    image: Option<GpuImage>,
    /// Which of the current image's bind groups the next draw reads, decided
    /// in `prepare` from the zoom.
    level: usize,
}

impl ImageLayer {
    pub fn new(device: &wgpu::Device, target_format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("image layer"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/image.wgsl").into()),
        });

        let params_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("image params"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        // No sampler: the shader loads texels and weights them itself, which
        // is what lets one pipeline serve an area filter, an antialiased
        // nearest and a bicubic. Every format `upload::plan` can produce is
        // filterable all the same, and so is every format `reduce` writes.
        let texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("image texture"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            }],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("image layer"),
            bind_group_layouts: &[Some(&params_layout), Some(&texture_layout)],
            immediate_size: 0,
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("image layer"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleStrip,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: target_format,
                    // The shader emits premultiplied colour.
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });

        let params = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("image params"),
            size: size_of::<Params>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let params_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("image params"),
            layout: &params_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: params.as_entire_binding(),
            }],
        });

        Self {
            pipeline,
            texture_layout,
            params,
            params_group,
            reducer: Reducer::new(device),
            image: None,
            level: 0,
        }
    }

    pub fn current(&self) -> Option<&GpuImage> {
        self.image.as_ref()
    }

    pub fn set_image(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        image: &DecodedImage,
        capabilities: Capabilities,
    ) -> Result<()> {
        let plan = upload::plan(image, capabilities);

        let size = wgpu::Extent3d {
            width: image.width,
            height: image.height,
            depth_or_array_layers: 1,
        };
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("image"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: plan.format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            plan.pixels.as_bytes(),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(plan.bytes_per_row),
                rows_per_image: Some(image.height),
            },
            size,
        );

        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bindings = vec![binding(device, &self.texture_layout, &view)];

        // Dropping the previous image here is what keeps the coarse chain
        // bounded: only one is ever alive, and it goes with the image it
        // describes rather than accumulating as files are stepped through.
        self.image = Some(GpuImage {
            size: [image.width, image.height],
            view,
            bindings,
            levels: Vec::new(),
            chain_built: false,
            level_format: reduce::level_format(plan.format),
            swizzle: upload::swizzle_code(image.channels()),
            alpha: image.alpha,
            primaries: to_columns(image.color.primaries.to_bt709()),
            format: plan.format,
            precision_note: plan.precision_note,
        });
        self.level = 0;
        Ok(())
    }

    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        placement: Placement,
        target: [f32; 2],
        display: &Display,
    ) {
        let Some(image) = &mut self.image else {
            return;
        };
        let (low, gain) = display.transform();

        // How much the draw has to shrink the image by, in source texels per
        // output pixel. Below one the view is magnifying and reads the image
        // itself; above it, the coarse chain does everything past a factor of
        // four so that the filter's tap count stays small.
        let factor = if placement.zoom > 0.0 {
            1.0 / placement.zoom
        } else {
            1.0
        };
        if factor > reduce::STEP as f32 && !image.chain_built {
            image.levels = self.reducer.build(
                device,
                encoder,
                reduce::Source {
                    view: &image.view,
                    size: image.size,
                    format: image.level_format,
                    swizzle: image.swizzle,
                    alpha: image.alpha,
                },
            );
            for level in &image.levels {
                image
                    .bindings
                    .push(binding(device, &self.texture_layout, &level.view));
            }
            image.chain_built = true;
        }

        let level = reduce::level_for(factor, image.levels.len());
        self.level = level;

        let divisor = (reduce::STEP as f32).powi(level as i32);
        let extent = [
            image.size[0] as f32 / divisor,
            image.size[1] as f32 / divisor,
        ];
        // Read off the quad rather than from the zoom, so that the filters and
        // the geometry cannot drift apart.
        let texels_per_pixel = [
            extent[0] / placement.width.max(1e-6),
            extent[1] / placement.height.max(1e-6),
        ];

        queue.write_buffer(
            &self.params,
            0,
            bytemuck::bytes_of(&Params {
                offset: [
                    placement.x / target[0] * 2.0 - 1.0,
                    1.0 - placement.y / target[1] * 2.0,
                ],
                scale: [
                    placement.width / target[0] * 2.0,
                    placement.height / target[1] * 2.0,
                ],
                window: [low, gain],
                texels_per_pixel,
                extent,
                _pad: [0.0; 2],
                primaries: image.primaries,
                swizzle: image.swizzle,
                alpha_mode: if level == 0 {
                    upload::alpha_code(image.alpha)
                } else {
                    reduce::level_alpha_code(image.alpha)
                },
                colormap: display.colormap.index(),
                // Minification is an area average; magnification is whichever
                // of the two the user asked for. At exactly 1:1 both come to
                // the same thing, so the boundary is not a visible one.
                resampler: if placement.zoom < 1.0 {
                    0
                } else {
                    placement.upscale.index()
                },
            }),
        );
    }

    pub fn render(&self, pass: &mut wgpu::RenderPass<'_>) {
        let Some(image) = &self.image else {
            return;
        };
        let Some(binding) = image.bindings.get(self.level) else {
            return;
        };
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.params_group, &[]);
        pass.set_bind_group(1, binding, &[]);
        pass.draw(0..4, 0..1);
    }
}

fn binding(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    view: &wgpu::TextureView,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("image texture"),
        layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: wgpu::BindingResource::TextureView(view),
        }],
    })
}

/// WGSL matrices are column-major with 16-byte column stride, while
/// `Primaries::to_bt709` is written out in rows for readability.
fn to_columns(rows: [[f32; 3]; 3]) -> [[f32; 4]; 3] {
    let mut columns = [[0.0f32; 4]; 3];
    for (column_index, column) in columns.iter_mut().enumerate() {
        for (row_index, row) in rows.iter().enumerate() {
            column[row_index] = row[column_index];
        }
    }
    columns
}
