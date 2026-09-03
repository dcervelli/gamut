//! The wgpu incantations every pass repeats, written out once.
//!
//! Nothing here decides anything: each function is a descriptor that four
//! modules used to spell out by hand, with the two or three fields that ever
//! varied left as parameters.

use bytemuck::Pod;

/// A bind group layout holding one uniform buffer at binding 0.
pub fn uniform_layout(
    device: &wgpu::Device,
    label: &str,
    visibility: wgpu::ShaderStages,
) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some(label),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }],
    })
}

/// A bind group layout holding `count` 2D float textures at bindings
/// `0..count`, read in the fragment stage. No sampler: every shader here
/// loads texels and weights them itself.
pub fn texture_layout(
    device: &wgpu::Device,
    label: &str,
    count: u32,
    filterable: bool,
) -> wgpu::BindGroupLayout {
    let entries: Vec<wgpu::BindGroupLayoutEntry> = (0..count)
        .map(|binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        })
        .collect();
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some(label),
        entries: &entries,
    })
}

pub fn pipeline_layout(
    device: &wgpu::Device,
    label: &str,
    layouts: &[&wgpu::BindGroupLayout],
) -> wgpu::PipelineLayout {
    let layouts: Vec<Option<&wgpu::BindGroupLayout>> = layouts.iter().map(|l| Some(*l)).collect();
    device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some(label),
        bind_group_layouts: &layouts,
        immediate_size: 0,
    })
}

/// A uniform buffer sized for one `T`, to be written with `Queue::write_buffer`.
pub fn uniform_buffer<T>(device: &wgpu::Device, label: &str) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: size_of::<T>() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

/// A bind group with one buffer at binding 0.
pub fn buffer_group(
    device: &wgpu::Device,
    label: &str,
    layout: &wgpu::BindGroupLayout,
    buffer: &wgpu::Buffer,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some(label),
        layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: buffer.as_entire_binding(),
        }],
    })
}

/// A bind group with one texture view per binding, in order.
pub fn texture_group(
    device: &wgpu::Device,
    label: &str,
    layout: &wgpu::BindGroupLayout,
    views: &[&wgpu::TextureView],
) -> wgpu::BindGroup {
    let entries: Vec<wgpu::BindGroupEntry> = views
        .iter()
        .enumerate()
        .map(|(binding, view)| wgpu::BindGroupEntry {
            binding: binding as u32,
            resource: wgpu::BindingResource::TextureView(view),
        })
        .collect();
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some(label),
        layout,
        entries: &entries,
    })
}

/// A pipeline with no vertex buffers: the vertex shader makes its geometry
/// from the vertex index, and `fs_main` writes one target.
pub struct Fullscreen<'a> {
    pub label: &'a str,
    pub shader: &'a wgpu::ShaderModule,
    pub layout: &'a wgpu::PipelineLayout,
    pub topology: wgpu::PrimitiveTopology,
    pub format: wgpu::TextureFormat,
    pub blend: Option<wgpu::BlendState>,
}

pub fn fullscreen_pipeline(device: &wgpu::Device, spec: Fullscreen<'_>) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(spec.label),
        layout: Some(spec.layout),
        vertex: wgpu::VertexState {
            module: spec.shader,
            entry_point: Some("vs_main"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        primitive: wgpu::PrimitiveState {
            topology: spec.topology,
            cull_mode: None,
            ..Default::default()
        },
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: spec.shader,
            entry_point: Some("fs_main"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: spec.format,
                blend: spec.blend,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview_mask: None,
        cache: None,
    })
}

/// One colour attachment, stored after the pass.
pub fn attachment(
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

/// A vertex buffer that grows to fit whatever a frame writes into it, and
/// never shrinks: a frame that once needed the room will need it again.
pub struct GrowableBuffer {
    buffer: wgpu::Buffer,
    capacity: usize,
    stride: usize,
    label: &'static str,
}

impl GrowableBuffer {
    /// Room for `capacity` values of `T` to begin with.
    pub fn new<T>(device: &wgpu::Device, label: &'static str, capacity: usize) -> Self {
        let stride = size_of::<T>();
        Self {
            buffer: Self::allocate(device, label, capacity * stride),
            capacity,
            stride,
            label,
        }
    }

    fn allocate(device: &wgpu::Device, label: &str, bytes: usize) -> wgpu::Buffer {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size: bytes as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    }

    /// Replaces the contents with `data`, reallocating first if it does not
    /// fit. Writes nothing for an empty slice.
    pub fn write<T: Pod>(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, data: &[T]) {
        debug_assert_eq!(
            size_of::<T>(),
            self.stride,
            "{}: wrong element type",
            self.label
        );
        if data.len() > self.capacity {
            self.capacity = data.len().next_power_of_two();
            self.buffer = Self::allocate(device, self.label, self.capacity * self.stride);
        }
        if !data.is_empty() {
            queue.write_buffer(&self.buffer, 0, bytemuck::cast_slice(data));
        }
    }

    pub fn slice(&self) -> wgpu::BufferSlice<'_> {
        self.buffer.slice(..)
    }
}

/// One device for the whole test run, and the capabilities of the adapter it
/// came from. `None` where the machine has no adapter to draw with, which is
/// how the tests that use it report success rather than failing for a reason
/// that has nothing to do with the code.
///
/// Shared rather than opened per test, and per *frame* within a test, because
/// `wgpu::Instance::new` opens a Vulkan instance and the loader's own locking
/// does not survive a dozen threads doing that at once: the process dies in
/// `vkCreateInstance` with no error the test harness can report. One instance
/// is also very much faster.
#[cfg(test)]
pub(crate) fn test_context() -> Option<&'static TestContext> {
    use std::sync::OnceLock;

    static CONTEXT: OnceLock<Option<TestContext>> = OnceLock::new();
    CONTEXT.get_or_init(open_test_context).as_ref()
}

#[cfg(test)]
pub(crate) struct TestContext {
    pub(crate) device: wgpu::Device,
    pub(crate) queue: wgpu::Queue,
    pub(crate) capabilities: crate::render::upload::Capabilities,
}

#[cfg(test)]
fn open_test_context() -> Option<TestContext> {
    let instance =
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        ..Default::default()
    }))
    .ok()?;
    let capabilities = crate::render::upload::Capabilities::from_adapter(&adapter);
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("tests"),
        required_features: capabilities.required_features(),
        required_limits: adapter.limits(),
        ..Default::default()
    }))
    .ok()?;
    Some(TestContext {
        device,
        queue,
        capabilities,
    })
}
