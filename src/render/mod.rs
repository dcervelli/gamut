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
mod gpu;
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
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Context, Result, anyhow};
use winit::window::Window;

use crate::image::{
    DecodedImage,
    display::{Display, Headroom},
};
use crate::timing;

pub use composite::Backdrop;
pub use output::{HdrPreference, Output};
pub use placement::{Placement, Upscale};
pub use ui_layer::{Blend, Color, Popup, PopupGrid, PopupSection, Rect, UiFrame};

use composite::Composite;
use gpu::attachment;
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
#[derive(Clone, Copy)]
pub struct Scene<'a> {
    pub placement: Placement,
    pub thumbnail: Option<Placement>,
    pub display: &'a Display,
    pub frame: &'a UiFrame,
    /// Physical pixels to the logical one the interface is laid out in.
    pub scale: f32,
    pub backdrop: Backdrop,
    /// Whether the picture is going out with room above white, which decides
    /// what the compositor does with no curve on the highlights: clip, or
    /// let them through. Not the surface's alone to say: an HDR surface on a
    /// monitor in SDR mode has none, and the switch can turn it off.
    pub headroom: Headroom,
}

/// Text measurement, for interface code that has to lay something out next
/// to a label. A trait rather than a method on [`Renderer`] so that the
/// interface can be built against something that is not a GPU.
pub trait TextMeasure {
    /// Width and height of `text` at `size`, in logical pixels.
    fn measure_text(&mut self, text: &str, size: f32) -> [f32; 2];

    /// As [`TextMeasure::measure_text`], for a run drawn with
    /// [`UiFrame::text_clipped_mono`].
    fn measure_mono(&mut self, text: &str, size: f32) -> [f32; 2];

    /// Width and height of `text` at `size` once it is broken across lines at
    /// `width`, in logical pixels: how much room a paragraph will take, for
    /// anything stacking one under another.
    fn measure_wrapped(&mut self, text: &str, size: f32, width: f32) -> [f32; 2];
}

pub struct Renderer {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    output: Output,
    /// What the surface can be configured as, kept so that the output can be
    /// chosen again when it is switched.
    surface_capabilities: wgpu::SurfaceCapabilities,
    capabilities: Capabilities,
    adapter_name: String,

    targets: Targets,
    image_layer: ImageLayer,
    ui: UiRenderer,
    composite: Composite,
}

/// A GPU driver error would otherwise be a process-fatal panic (wgpu's default
/// uncaptured-error handler), and on the loader thread that silently strands
/// the app. Reported once instead: a hostile file that trips a device limit,
/// or a driver reset, leaves a line on stderr rather than taking the window
/// down.
static GPU_ERROR_REPORTED: AtomicBool = AtomicBool::new(false);

fn report_gpu_error(what: &str, detail: impl std::fmt::Display) {
    if !GPU_ERROR_REPORTED.swap(true, Ordering::Relaxed) {
        eprintln!("gamut: {what}: {detail}");
    }
}

/// Installs the handlers that keep a GPU error from aborting the process, and
/// clamps a size to what the device can actually hold.
fn install_error_handlers(device: &wgpu::Device) {
    device.on_uncaptured_error(Arc::new(|error| {
        report_gpu_error("the GPU driver reported an error", error);
    }));
    device.set_device_lost_callback(|_reason, message| {
        report_gpu_error("the GPU device was lost", message);
    });
}

/// Clamps a surface size to the device's maximum texture dimension. A window
/// dragged wider than the GPU can hold would otherwise make the offscreen
/// targets fail to allocate, and with them the whole frame. Clamping scales
/// the output down instead of crashing — a rare, graceful degradation.
fn clamp_to_device(device: &wgpu::Device, width: u32, height: u32) -> (u32, u32) {
    let limit = device.limits().max_texture_dimension_2d;
    (width.min(limit).max(1), height.min(limit).max(1))
}

impl Renderer {
    pub fn new(window: Arc<Window>, hdr: HdrPreference) -> Result<Self> {
        let size = window.inner_size();
        let (width, height) = (size.width.max(1), size.height.max(1));

        // `..._from_env` reads `WGPU_BACKEND`, `WGPU_ADAPTER_NAME`,
        // `WGPU_POWER_PREF`, `WGPU_DEBUG`, `WGPU_VALIDATION` and
        // `WGPU_GPU_BASED_VALIDATION` from the environment. They select the
        // backend and toggle the driver's validation layers — the user's to
        // set, but named here so the configuration is not invisible.
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
            label: Some("gamut device"),
            required_features: capabilities.required_features(),
            required_limits: adapter.limits(),
            ..Default::default()
        }))
        .context("requesting a GPU device")?;
        install_error_handlers(&device);
        let (width, height) = clamp_to_device(&device, width, height);

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
            surface_capabilities,
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

    /// Whether the driver offers an HDR colour space for this window, and so
    /// whether there is anything for [`Renderer::set_hdr`] to switch to.
    pub fn hdr_available(&self) -> bool {
        Output::hdr_available(&self.surface_capabilities)
    }

    /// Puts the surface onto an HDR colour space, or back onto sRGB. Returns
    /// whether the output changed.
    ///
    /// The surface is configured afresh with the new format and colour
    /// space, and the compositor — the one pass that writes to the surface,
    /// and so the one pipeline keyed on its format — is built again for it.
    /// The offscreen targets are untouched: the image and the interface are
    /// drawn the same way whatever they are going out to.
    pub fn set_hdr(&mut self, on: bool) -> bool {
        let preference = if on {
            HdrPreference::On
        } else {
            HdrPreference::Off
        };
        let Some(output) = Output::choose(&self.surface_capabilities, preference) else {
            return false;
        };
        if output.format == self.output.format && output.color_space == self.output.color_space {
            return false;
        }
        self.config.format = output.format;
        self.config.color_space = output.color_space;
        self.surface.configure(&self.device, &self.config);
        self.composite = Composite::new(&self.device, output.format);
        self.composite
            .bind_targets(&self.device, &self.targets.image, &self.targets.ui);
        self.output = output;
        true
    }

    pub fn adapter_name(&self) -> &str {
        &self.adapter_name
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        let (width, height) = clamp_to_device(&self.device, width, height);
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

        // The backdrop is the compositor's, and it reads the scene itself.
        let Scene {
            placement,
            thumbnail,
            display,
            frame,
            scale,
            ..
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

        // The interface is staged first, before the image layer records
        // anything into the encoder. `ui.prepare` is the one step here that
        // can fail — a full glyph atlas — and the image layer's `prepare`
        // marks its coarse chain built as a side effect of recording it. Were
        // that to run first, a text-atlas failure would drop the encoder
        // unsubmitted while the chain still counted as built, and the image
        // would go blank when zoomed out until the file was reloaded. Ordered
        // this way, a failure here returns before the image layer touches its
        // state. The two are otherwise independent.
        self.ui.prepare(
            &self.device,
            &self.queue,
            frame,
            [self.config.width, self.config.height],
            scale,
        )?;
        // A view that has just zoomed out past what the coarse chain covers
        // builds the rest of it here.
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
        // Where an image quad lands, and so where transparency has to read as
        // a checkerboard rather than as the plain backdrop. Asked of the image
        // layer rather than assumed from `placement`, since a frame drawn
        // before the first file has decoded has a placement but no image.
        let (checkered, gray) = match self.image_layer.current() {
            Some(image) => ([Some(placement), thumbnail], image.is_gray()),
            None => ([None, None], false),
        };
        self.composite
            .prepare(&self.queue, &scene, gray, &self.output, checkered);

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

    fn measure_mono(&mut self, text: &str, size: f32) -> [f32; 2] {
        self.ui.measure_mono(text, size)
    }

    fn measure_wrapped(&mut self, text: &str, size: f32, width: f32) -> [f32; 2] {
        self.ui.measure_wrapped(text, size, width)
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
