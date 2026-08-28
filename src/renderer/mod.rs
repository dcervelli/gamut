//! wgpu setup and the per-frame draw: background, image quad, status bar.

use std::sync::Arc;

use anyhow::{Context, Result, anyhow};
use bytemuck::{Pod, Zeroable};
use winit::window::Window;

use crate::formats::DecodedImage;
use crate::view::Placement;

mod overlay;

use overlay::Overlay;

/// Matches `struct Quad` in shader.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct QuadUniform {
    offset: [f32; 2],
    scale: [f32; 2],
    color: [f32; 4],
}

impl QuadUniform {
    /// Converts a top-left-origin window-pixel rectangle into clip space.
    fn from_rect(x: f32, y: f32, w: f32, h: f32, window: [f32; 2], color: [f32; 4]) -> Self {
        Self {
            offset: [x / window[0] * 2.0 - 1.0, 1.0 - y / window[1] * 2.0],
            scale: [w / window[0] * 2.0, h / window[1] * 2.0],
            color,
        }
    }
}

/// The uploaded texture for one image, with a bind group per sampling mode.
struct GpuImage {
    nearest: wgpu::BindGroup,
    linear: wgpu::BindGroup,
}

pub struct Renderer {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    image_pipeline: wgpu::RenderPipeline,
    bar_pipeline: wgpu::RenderPipeline,
    texture_layout: wgpu::BindGroupLayout,
    nearest_sampler: wgpu::Sampler,
    linear_sampler: wgpu::Sampler,
    image_uniform: wgpu::Buffer,
    image_uniform_group: wgpu::BindGroup,
    bar_uniform: wgpu::Buffer,
    bar_uniform_group: wgpu::BindGroup,
    image: Option<GpuImage>,
    overlay: Overlay,
}

impl Renderer {
    pub fn new(window: Arc<Window>) -> Result<Self> {
        let size = window.inner_size();
        let (width, height) = (size.width.max(1), size.height.max(1));

        let instance =
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
        let surface = instance
            .create_surface(window.clone())
            .context("creating a drawing surface for the window")?;

        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            compatible_surface: Some(&surface),
            ..Default::default()
        }))
        .context("no suitable GPU adapter found")?;

        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("image-view device"),
            required_limits: adapter.limits(),
            ..Default::default()
        }))
        .context("requesting a GPU device")?;

        let capabilities = surface.get_capabilities(&adapter);
        let mut config = surface
            .get_default_config(&adapter, width, height)
            .ok_or_else(|| anyhow!("the GPU adapter cannot present to this window"))?;
        // Prefer an sRGB target so the sRGB image texture round-trips exactly.
        if let Some(srgb) = capabilities.formats.iter().copied().find(|f| f.is_srgb()) {
            config.format = srgb;
        }
        surface.configure(&device, &config);

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("quad shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });

        let uniform_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("quad uniform layout"),
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

        let texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("image texture layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let blend = Some(wgpu::BlendState::ALPHA_BLENDING);
        let target = wgpu::ColorTargetState {
            format: config.format,
            blend,
            write_mask: wgpu::ColorWrites::ALL,
        };

        let make_pipeline = |label: &str,
                             layouts: &[Option<&wgpu::BindGroupLayout>],
                             fragment_entry: &str| {
            let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some(label),
                bind_group_layouts: layouts,
                immediate_size: 0,
            });
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_quad"),
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
                    entry_point: Some(fragment_entry),
                    compilation_options: Default::default(),
                    targets: &[Some(target.clone())],
                }),
                multiview_mask: None,
                cache: None,
            })
        };

        let image_pipeline = make_pipeline(
            "image quad",
            &[Some(&uniform_layout), Some(&texture_layout)],
            "fs_image",
        );
        let bar_pipeline = make_pipeline("status bar", &[Some(&uniform_layout)], "fs_solid");

        let make_sampler = |label: &str, filter: wgpu::FilterMode| {
            device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some(label),
                address_mode_u: wgpu::AddressMode::ClampToEdge,
                address_mode_v: wgpu::AddressMode::ClampToEdge,
                address_mode_w: wgpu::AddressMode::ClampToEdge,
                mag_filter: filter,
                min_filter: filter,
                ..Default::default()
            })
        };
        let nearest_sampler = make_sampler("nearest", wgpu::FilterMode::Nearest);
        let linear_sampler = make_sampler("linear", wgpu::FilterMode::Linear);

        let make_uniform = |label: &str| {
            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: size_of::<QuadUniform>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(label),
                layout: &uniform_layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: buffer.as_entire_binding(),
                }],
            });
            (buffer, group)
        };
        let (image_uniform, image_uniform_group) = make_uniform("image quad uniform");
        let (bar_uniform, bar_uniform_group) = make_uniform("status bar uniform");

        let overlay = Overlay::new(&device, &queue, config.format, window.scale_factor() as f32);

        Ok(Self {
            surface,
            device,
            queue,
            config,
            image_pipeline,
            bar_pipeline,
            texture_layout,
            nearest_sampler,
            linear_sampler,
            image_uniform,
            image_uniform_group,
            bar_uniform,
            bar_uniform_group,
            image: None,
            overlay,
        })
    }

    pub fn size(&self) -> [f32; 2] {
        [self.config.width as f32, self.config.height as f32]
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        if width == self.config.width && height == self.config.height {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
    }

    /// Uploads `image`, replacing whatever was shown before.
    pub fn set_image(&mut self, image: &DecodedImage) {
        let max = self.device.limits().max_texture_dimension_2d;
        if image.width > max || image.height > max {
            eprintln!(
                "image-view: {}x{} exceeds this GPU's {max}x{max} texture limit",
                image.width, image.height
            );
        }

        let size = wgpu::Extent3d {
            width: image.width,
            height: image.height,
            depth_or_array_layers: 1,
        };
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("image"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &image.rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(image.width * 4),
                rows_per_image: Some(image.height),
            },
            size,
        );

        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let make_group = |label: &str, sampler: &wgpu::Sampler| {
            self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(label),
                layout: &self.texture_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(sampler),
                    },
                ],
            })
        };

        self.image = Some(GpuImage {
            nearest: make_group("image nearest", &self.nearest_sampler),
            linear: make_group("image linear", &self.linear_sampler),
        });
    }

    pub fn render(&mut self, placement: Placement, scale: f32, status: &str) -> Result<()> {
        let window = self.size();

        use wgpu::CurrentSurfaceTexture as Acquired;
        let frame = match self.surface.get_current_texture() {
            Acquired::Success(frame) => frame,
            // Suboptimal still draws correctly; reconfiguring next frame is enough.
            Acquired::Suboptimal(frame) => {
                self.surface.configure(&self.device, &self.config);
                frame
            }
            // Nothing to draw into right now: skip the frame rather than fail.
            Acquired::Timeout | Acquired::Occluded => return Ok(()),
            Acquired::Outdated | Acquired::Lost => {
                self.surface.configure(&self.device, &self.config);
                match self.surface.get_current_texture() {
                    Acquired::Success(frame) | Acquired::Suboptimal(frame) => frame,
                    other => {
                        return Err(anyhow!(
                            "could not acquire a frame after reconfiguring the surface: {other:?}"
                        ));
                    }
                }
            }
            Acquired::Validation => {
                return Err(anyhow!("the GPU driver rejected the surface configuration"));
            }
        };
        let target = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        self.queue.write_buffer(
            &self.image_uniform,
            0,
            bytemuck::bytes_of(&QuadUniform::from_rect(
                placement.x,
                placement.y,
                placement.width,
                placement.height,
                window,
                [0.0; 4],
            )),
        );

        let bar_height = self.overlay.bar_height().min(window[1]);
        self.queue.write_buffer(
            &self.bar_uniform,
            0,
            bytemuck::bytes_of(&QuadUniform::from_rect(
                0.0,
                window[1] - bar_height,
                window[0],
                bar_height,
                window,
                [0.0, 0.0, 0.0, 0.62],
            )),
        );

        self.overlay.prepare(
            &self.device,
            &self.queue,
            self.config.width,
            self.config.height,
            scale,
            status,
        )?;

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("frame"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("frame"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.06,
                            g: 0.06,
                            b: 0.07,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });

            if let Some(image) = &self.image {
                // Magnification shows the real pixel grid; minification is
                // smoothed so downscaled photos do not shimmer.
                let group = if placement.zoom >= 1.0 {
                    &image.nearest
                } else {
                    &image.linear
                };
                pass.set_pipeline(&self.image_pipeline);
                pass.set_bind_group(0, &self.image_uniform_group, &[]);
                pass.set_bind_group(1, group, &[]);
                pass.draw(0..4, 0..1);
            }

            pass.set_pipeline(&self.bar_pipeline);
            pass.set_bind_group(0, &self.bar_uniform_group, &[]);
            pass.draw(0..4, 0..1);

            self.overlay.render(&mut pass)?;
        }

        self.queue.submit(Some(encoder.finish()));
        self.queue.present(frame);
        self.overlay.trim();
        Ok(())
    }
}
