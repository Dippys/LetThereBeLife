//! GPU-facing data: camera uniforms and bindings, the instance vertex layout, and immutable/dynamic instance buffers.

use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

use super::MAX_INSTANCES_PER_BUFFER;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub(super) struct CameraUniform {
    center: [f32; 2],
    viewport: [f32; 2],
    scale: f32,
    _padding: [f32; 3],
}

impl CameraUniform {
    pub(super) const fn new(center: [f32; 2], viewport: [f32; 2], scale: f32) -> Self {
        Self {
            center,
            viewport,
            scale,
            _padding: [0.0; 3],
        }
    }
}

const _: () = assert!(size_of::<CameraUniform>() == 32);
const _: () = assert!(size_of::<Instance>() == 20);

pub(super) struct CameraBinding {
    buffer: wgpu::Buffer,
    pub(super) bind_group: wgpu::BindGroup,
}

impl CameraBinding {
    pub(super) fn new(device: &wgpu::Device, layout: &wgpu::BindGroupLayout, label: &str) -> Self {
        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(label),
            contents: bytemuck::bytes_of(&CameraUniform::zeroed()),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(label),
            layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: buffer.as_entire_binding(),
            }],
        });
        Self { buffer, bind_group }
    }

    pub(super) fn write(&self, queue: &wgpu::Queue, uniform: CameraUniform) {
        queue.write_buffer(&self.buffer, 0, bytemuck::bytes_of(&uniform));
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub(super) struct Instance {
    pub(super) position: [f32; 2],
    pub(super) size: [f32; 2],
    pub(super) color: u32,
}

impl Instance {
    pub(super) const fn new(x: f32, y: f32, width: f32, height: f32, color: u32) -> Self {
        Self {
            position: [x, y],
            size: [width, height],
            color,
        }
    }

    pub(super) fn layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: size_of::<Self>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &[
                wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Float32x2,
                    offset: 0,
                    shader_location: 0,
                },
                wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Float32x2,
                    offset: 8,
                    shader_location: 1,
                },
                wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Uint32,
                    offset: 16,
                    shader_location: 2,
                },
            ],
        }
    }
}

pub(super) struct InstanceBuffer {
    buffer: wgpu::Buffer,
    count: u32,
}

pub(super) struct StaticInstanceBuffers {
    buffers: Vec<InstanceBuffer>,
}

impl StaticInstanceBuffers {
    pub(super) fn new(device: &wgpu::Device, label: &str, instances: &[Instance]) -> Self {
        let chunks = static_instance_chunks(instances);
        let mut buffers = Vec::with_capacity(chunks.len());
        buffers.extend(chunks.enumerate().map(|(index, chunk)| {
            InstanceBuffer::immutable(device, &format!("{label} {index}"), chunk)
        }));
        Self { buffers }
    }

    pub(super) fn draw<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>) {
        for buffer in &self.buffers {
            buffer.draw(pass);
        }
    }
}

impl InstanceBuffer {
    fn immutable(device: &wgpu::Device, label: &str, instances: &[Instance]) -> Self {
        let empty = Instance::zeroed();
        let contents = if instances.is_empty() {
            bytemuck::bytes_of(&empty)
        } else {
            bytemuck::cast_slice(instances)
        };
        let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(label),
            contents,
            usage: wgpu::BufferUsages::VERTEX,
        });
        Self {
            buffer,
            count: instances.len() as u32,
        }
    }

    pub(super) fn dynamic(device: &wgpu::Device, label: &str, capacity: usize) -> Self {
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size: (capacity * size_of::<Instance>()) as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self { buffer, count: 0 }
    }

    pub(super) fn write(&mut self, queue: &wgpu::Queue, instances: &[Instance]) {
        self.count = instances.len() as u32;
        if !instances.is_empty() {
            queue.write_buffer(&self.buffer, 0, bytemuck::cast_slice(instances));
        }
    }

    pub(super) fn draw<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>) {
        if self.count > 0 {
            pass.set_vertex_buffer(0, self.buffer.slice(..));
            pass.draw(0..6, 0..self.count);
        }
    }
}

pub(super) fn static_instance_chunks(instances: &[Instance]) -> std::slice::Chunks<'_, Instance> {
    instances.chunks(MAX_INSTANCES_PER_BUFFER)
}
