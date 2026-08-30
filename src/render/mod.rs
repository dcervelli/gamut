//! Rendering, in three separable layers.
//!
//! 1. [`image_layer`] draws the image into a linear working-space target.
//! 2. [`ui`] draws the interface into its own sRGB target.
//! 3. [`composite`] tone maps the first, lays the second over it, and encodes
//!    the result for whatever the surface turned out to be.
//!
//! Keeping the interface off the image's target is what lets UI code stay in
//! plain sRGB and logical pixels while the image is in extended-range linear —
//! and it is what makes a third-party text renderer usable at all, since
//! glyphon has no idea what an HDR surface is.

mod composite;
mod image_layer;
mod output;
mod reduce;
pub mod ui;

#[cfg(test)]
mod filter_tests;
pub(crate) mod upload;

use std::sync::Arc;

use anyhow::{Context, Result, anyhow};
use winit::window::Window;

use crate::image::{DecodedImage, display::Display};
use crate::view::Placement;

pub use composite::Backdrop;
pub use output::{HdrPreference, Output};
pub use ui::{Blend, Color, Rect, UiFrame};

use composite::Composite;
use image_layer::{Draw, ImageLayer};
use ui::UiRenderer;
use upload::Capabilities;

/// The working space every layer meets in: linear, BT.709 primaries, with
/// enough range above 1.0 for HDR content to survive until tone mapping.
const WORKING_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

/// The UI's own target. sRGB so that blending happens in linear and so that
/// glyphon's colour handling is correct without it knowing anything.
const UI_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

struct Targets {
    image: wgpu::TextureView,
    ui: wgpu::TextureView,
}

pub struct Renderer {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    output: Output,
    capabilities: Capabilities,
    adapter_name: String,

    targets: Targets,
    image_layer: ImageLayer,
    ui: UiRenderer,
    composite: Composite,
}

impl Renderer {
    pub fn new(window: Arc<Window>, hdr: HdrPreference) -> Result<Self> {
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

        let capabilities = Capabilities::from_adapter(&adapter);
        let adapter_name = adapter.get_info().name;

        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("image-view device"),
            required_features: capabilities.required_features(),
            required_limits: adapter.limits(),
            ..Default::default()
        }))
        .context("requesting a GPU device")?;

        let surface_capabilities = surface.get_capabilities(&adapter);
        let output = Output::choose(&surface_capabilities, hdr)
            .ok_or_else(|| anyhow!("the GPU adapter cannot present to this window"))?;

        let mut config = surface
            .get_default_config(&adapter, width, height)
            .ok_or_else(|| anyhow!("the GPU adapter cannot present to this window"))?;
        config.format = output.format;
        config.color_space = output.color_space;
        surface.configure(&device, &config);

        let targets = Targets::new(&device, width, height);
        let image_layer = ImageLayer::new(&device, WORKING_FORMAT);
        let ui = UiRenderer::new(&device, &queue, UI_FORMAT);
        let mut composite = Composite::new(&device, output.format);
        composite.bind_targets(&device, &targets.image, &targets.ui);

        Ok(Self {
            surface,
            device,
            queue,
            config,
            output,
            capabilities,
            adapter_name,
            targets,
            image_layer,
            ui,
            composite,
        })
    }

    /// Physical pixels.
    pub fn size(&self) -> [f32; 2] {
        [self.config.width as f32, self.config.height as f32]
    }

    pub fn output(&self) -> &Output {
        &self.output
    }

    pub fn adapter_name(&self) -> &str {
        &self.adapter_name
    }

    /// Text measurement, for UI code that needs to lay something out next to
    /// a label.
    pub fn measure_text(&mut self, text: &str, size: f32) -> [f32; 2] {
        self.ui.measure(text, size)
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
        self.targets = Targets::new(&self.device, width, height);
        self.composite
            .bind_targets(&self.device, &self.targets.image, &self.targets.ui);
    }

    /// Uploads `image`, replacing whatever was shown before. Returns a note
    /// when the device forced a lossy storage format.
    pub fn set_image(&mut self, image: &DecodedImage) -> Result<Option<&'static str>> {
        let limit = self.device.limits().max_texture_dimension_2d;
        if image.width > limit || image.height > limit {
            return Err(anyhow!(
                "{}x{} exceeds this GPU's {limit}x{limit} texture limit",
                image.width,
                image.height
            ));
        }
        self.image_layer
            .set_image(&self.device, &self.queue, image, self.capabilities)?;
        Ok(self
            .image_layer
            .current()
            .and_then(|image| image.precision_note))
    }

    /// The texture format the current image ended up in, for the UI to report.
    pub fn image_format(&self) -> Option<wgpu::TextureFormat> {
        self.image_layer.current().map(|image| image.format)
    }

    /// `thumbnail`, when the minimap is on screen, is where the whole image
    /// is to be drawn a second time. It goes into the image layer rather than
    /// the interface's, since it is the image: the same window, tone map and
    /// colormap apply to it without any of that having to be reimplemented in
    /// sRGB.
    pub fn render(
        &mut self,
        placement: Placement,
        thumbnail: Option<Placement>,
        display: &Display,
        frame: &UiFrame,
        scale: f32,
        backdrop: Backdrop,
    ) -> Result<()> {
        use wgpu::CurrentSurfaceTexture as Acquired;

        let surface_texture = match self.surface.get_current_texture() {
            Acquired::Success(texture) => texture,
            Acquired::Suboptimal(texture) => {
                self.surface.configure(&self.device, &self.config);
                texture
            }
            Acquired::Timeout | Acquired::Occluded => return Ok(()),
            Acquired::Outdated | Acquired::Lost => {
                self.surface.configure(&self.device, &self.config);
                match self.surface.get_current_texture() {
                    Acquired::Success(texture) | Acquired::Suboptimal(texture) => texture,
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

        let target = surface_texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let size = self.size();

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("frame"),
            });

        // Before the passes below, since a view that has just zoomed out past
        // what the coarse chain covers builds the rest of it here.
        self.image_layer.prepare(
            &self.device,
            &self.queue,
            &mut encoder,
            Draw {
                view: placement,
                thumbnail,
            },
            size,
            display,
        );
        self.ui.prepare(
            &self.device,
            &self.queue,
            frame,
            [self.config.width, self.config.height],
            scale,
        )?;
        // Where an image quad lands, and so where transparency has to read as
        // a checkerboard rather than as the plain backdrop. Asked of the image
        // layer rather than assumed from `placement`, since a frame drawn
        // before the first file has decoded has a placement but no image.
        let checkered = if self.image_layer.current().is_some() {
            [Some(placement), thumbnail]
        } else {
            [None, None]
        };
        self.composite.prepare(
            &self.queue,
            display,
            &self.output,
            backdrop,
            scale,
            checkered,
        );

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("image layer"),
                // Transparent, not a colour: what is behind the image is the
                // compositor's business, since it belongs to the interface
                // and must not go through the tone curve with the image.
                color_attachments: &[Some(attachment(
                    &self.targets.image,
                    wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                ))],
                ..Default::default()
            });
            self.image_layer.render(&mut pass);
        }
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("ui layer"),
                color_attachments: &[Some(attachment(
                    &self.targets.ui,
                    wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                ))],
                ..Default::default()
            });
            self.ui.render(&mut pass)?;
        }
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("composite"),
                color_attachments: &[Some(attachment(
                    &target,
                    wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                ))],
                ..Default::default()
            });
            self.composite.render(&mut pass);
        }

        self.queue.submit(Some(encoder.finish()));
        self.queue.present(surface_texture);
        self.ui.trim();
        Ok(())
    }
}

impl Targets {
    fn new(device: &wgpu::Device, width: u32, height: u32) -> Self {
        let make = |label: &str, format: wgpu::TextureFormat| {
            device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size: wgpu::Extent3d {
                        width,
                        height,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                        | wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                })
                .create_view(&wgpu::TextureViewDescriptor::default())
        };

        Self {
            image: make("image target", WORKING_FORMAT),
            ui: make("ui target", UI_FORMAT),
        }
    }
}

fn attachment(
    view: &wgpu::TextureView,
    load: wgpu::LoadOp<wgpu::Color>,
) -> wgpu::RenderPassColorAttachment<'_> {
    wgpu::RenderPassColorAttachment {
        view,
        depth_slice: None,
        resolve_target: None,
        ops: wgpu::Operations {
            load,
            store: wgpu::StoreOp::Store,
        },
    }
}
