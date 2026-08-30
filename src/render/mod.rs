//! Rendering, in three separable layers.
//!
//! 1. [`image_layer`] draws the image into a linear working-space target.
//! 2. [`ui_layer`] draws the interface into its own sRGB target.
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
mod placement;
mod reduce;
mod shader_codes;
pub mod ui_layer;

#[cfg(test)]
mod filter_tests;
pub(crate) mod upload;

use std::sync::Arc;

use anyhow::{Context, Result, anyhow};
use winit::window::Window;

use crate::image::{DecodedImage, display::Display};
use crate::timing;

pub use composite::Backdrop;
pub use output::{HdrPreference, Output};
pub use placement::{Placement, Upscale};
pub use ui_layer::{Blend, Color, Rect, UiFrame};

use composite::Composite;
use image_layer::{Draw, ImageLayer};
pub use image_layer::{GpuImage, Upload};
use ui_layer::UiRenderer;
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

/// Everything one frame draws. `thumbnail`, when the minimap is on screen,
/// is where the whole image is drawn a second time. It goes into the image
/// layer rather than the interface's, since it is the image: the same window,
/// tone map and colormap apply to it without any of that having to be
/// reimplemented in sRGB.
pub struct Scene<'a> {
    pub placement: Placement,
    pub thumbnail: Option<Placement>,
    pub display: &'a Display,
    pub frame: &'a UiFrame,
    /// Physical pixels to the logical one the interface is laid out in.
    pub scale: f32,
    pub backdrop: Backdrop,
}

/// Text measurement, for interface code that has to lay something out next
/// to a label. A trait rather than a method on [`Renderer`] so that the
/// interface can be built against something that is not a GPU.
pub trait TextMeasure {
    /// Width and height of `text` at `size`, in logical pixels.
    fn measure_text(&mut self, text: &str, size: f32) -> [f32; 2];
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

    /// Uploads `image` and puts it on screen, replacing whatever was there.
    /// Returns a note when the device forced a lossy storage format.
    ///
    /// The synchronous path, for the file named on the command line: it is
    /// already decoded by the time the window exists, and there is nothing on
    /// screen yet for the wait to interrupt. Everything opened afterwards
    /// goes through [`Renderer::uploader`] instead.
    pub fn set_image(&mut self, image: &DecodedImage) -> Result<Option<&'static str>> {
        let uploaded = self.uploader().run(image)?;
        Ok(self.install_image(uploaded))
    }

    /// A handle for uploading images from another thread. Safe to keep: it
    /// holds the device, the queue and the bind group layout, all of which
    /// outlive any one image.
    pub fn uploader(&self) -> Upload {
        self.image_layer
            .uploader(&self.device, &self.queue, self.capabilities)
    }

    /// Puts an image uploaded elsewhere on screen, returning its precision
    /// note. Cheap: the pixels are already across, and this is the swap.
    pub fn install_image(&mut self, image: GpuImage) -> Option<&'static str> {
        let note = image.precision_note;
        self.image_layer.install(image);
        note
    }

    /// What the current image was stored as on the device, for the interface
    /// to report. A label rather than the format itself, so that nothing above
    /// the renderer has to name a GPU type.
    pub fn image_format_label(&self) -> Option<String> {
        self.image_layer
            .current()
            .map(|image| format!("{:?}", image.format))
    }

    pub fn render(&mut self, scene: Scene<'_>) -> Result<()> {
        use wgpu::CurrentSurfaceTexture as Acquired;

        let Scene {
            placement,
            thumbnail,
            display,
            frame,
            scale,
            backdrop,
        } = scene;

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
        // Here rather than at the call site because the paths above that give
        // up on acquiring a surface texture also return `Ok`, and a frame that
        // was never drawn is not the frame anyone is timing. Handing it to the
        // presentation engine is as close to "on screen" as this side gets.
        if self.image_layer.current().is_some() {
            timing::first_image_frame();
        }
        Ok(())
    }
}

impl TextMeasure for Renderer {
    fn measure_text(&mut self, text: &str, size: f32) -> [f32; 2] {
        self.ui.measure(text, size)
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
