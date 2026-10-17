//! The GPU side: one device shared by every screen, the glyph atlas bound to
//! it, and the render pipeline built from the shader file, rebuilt whenever
//! that file changes.

use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime};

use crate::atlas;

/// What the shader is told each frame. Layout matches `Uniforms` in the shader.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Uniforms {
    pub resolution: [f32; 2],
    pub time: f32,
    pub _pad: f32,
}

/// The shader file, watched for changes.
pub struct Shader {
    path: PathBuf,
    loaded: Option<SystemTime>,
    checked: Instant,
}

impl Shader {
    pub fn new(path: PathBuf) -> Self {
        Shader { path, loaded: None, checked: Instant::now() }
    }
}

pub struct Gpu {
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    layout: wgpu::BindGroupLayout,
    atlas_view: wgpu::TextureView,
    sampler: wgpu::Sampler,
    pipeline: Option<wgpu::RenderPipeline>,
    /// The surface format, known once the first screen is configured; the
    /// pipeline targets it.
    pub format: Option<wgpu::TextureFormat>,
    shader: Shader,
}

/// A screen's GPU-side state: its uniform buffer and the bind group tying
/// it and the atlas to the pipeline.
pub struct Target {
    pub uniforms: wgpu::Buffer,
    pub bind_group: wgpu::BindGroup,
}

impl Gpu {
    /// Opens the device able to present to `surface`, and builds the atlas.
    pub fn new(instance: &wgpu::Instance, surface: &wgpu::Surface<'_>, shader: Shader) -> Self {
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(surface),
            ..Default::default()
        }))
        .expect("a GPU adapter");
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor { label: Some("onsen-matrix"), ..Default::default() }))
            .expect("a GPU device");
        log::info!("gpu: {}", adapter.get_info().name);
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("frame"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let (atlas_view, rows) = atlas::upload(&device, &queue);
        log::info!("atlas grid: {}x{rows}", atlas::COLS);
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("atlas"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        Gpu { adapter, device, queue, layout, atlas_view, sampler, pipeline: None, format: None, shader }
    }

    pub fn target(&self, label: &str) -> Target {
        let uniforms = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(label),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: uniforms.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&self.atlas_view) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&self.sampler) },
            ],
        });
        Target { uniforms, bind_group }
    }

    /// Rebuilds the pipeline if the shader file changed since it was last
    /// loaded, keeping the old pipeline when the new shader fails. Checks the
    /// file at most ten times a second.
    pub fn reload_shader(&mut self) {
        if self.shader.checked.elapsed().as_millis() < 100 && self.shader.loaded.is_some() {
            return;
        }
        self.shader.checked = Instant::now();
        let Some(format) = self.format else { return };
        let mtime = std::fs::metadata(&self.shader.path).and_then(|m| m.modified()).ok();
        if mtime.is_some() && mtime == self.shader.loaded {
            return;
        }
        self.shader.loaded = mtime;
        let source = match std::fs::read_to_string(&self.shader.path) {
            Ok(s) => s,
            Err(e) => {
                log::error!("{}: {e}", self.shader.path.display());
                return;
            }
        };
        match self.build_pipeline(&source, format) {
            Ok(p) => {
                self.pipeline = Some(p);
                log::info!("shader loaded: {}", self.shader.path.display());
            }
            Err(e) => log::error!("shader not loaded:\n{e}"),
        }
    }

    fn build_pipeline(&self, source: &str, format: wgpu::TextureFormat) -> Result<wgpu::RenderPipeline, wgpu::Error> {
        let scope = self.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let module = self.device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("matrix"), source: wgpu::ShaderSource::Wgsl(source.into()) });
        let layout = self.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("matrix"), bind_group_layouts: &[Some(&self.layout)], immediate_size: 0 });
        let pipeline = self.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("matrix"),
            layout: Some(&layout),
            vertex: wgpu::VertexState { module: &module, entry_point: Some("vs_main"), buffers: &[], compilation_options: Default::default() },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::ALL })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        match pollster::block_on(scope.pop()) {
            Some(e) => Err(e),
            None => Ok(pipeline),
        }
    }

    /// Draws one frame into `frame` for `target`, and if `snapshot` names a
    /// file, also writes the frame there as a PPM. Does not present.
    pub fn draw(&self, frame: &wgpu::SurfaceTexture, target: &Target, uniforms: Uniforms, snapshot: Option<&Path>) {
        let view = frame.texture.create_view(&Default::default());
        let bytes = unsafe { std::slice::from_raw_parts(&uniforms as *const Uniforms as *const u8, std::mem::size_of::<Uniforms>()) };
        self.queue.write_buffer(&target.uniforms, 0, bytes);
        let mut encoder = self.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("matrix"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            if let Some(pipeline) = self.pipeline.as_ref() {
                pass.set_pipeline(pipeline);
                pass.set_bind_group(0, &target.bind_group, &[]);
                pass.draw(0..6, 0..1);
            }
        }
        let (w, h) = (frame.texture.width(), frame.texture.height());
        let readback = snapshot.map(|_| {
            let bpr = (w * 4).div_ceil(256) * 256;
            let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("snapshot"),
                size: (bpr * h) as u64,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            encoder.copy_texture_to_buffer(
                wgpu::TexelCopyTextureInfo { texture: &frame.texture, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
                wgpu::TexelCopyBufferInfo { buffer: &buffer, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(bpr), rows_per_image: Some(h) } },
                wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            );
            (buffer, bpr)
        });
        self.queue.submit([encoder.finish()]);
        if let (Some(path), Some((buffer, bpr))) = (snapshot, readback) {
            self.write_snapshot(path, &buffer, bpr, w, h);
        }
    }

    /// Writes a frame read back into `buffer` (BGRA rows padded to `bpr`) as a PPM.
    fn write_snapshot(&self, path: &Path, buffer: &wgpu::Buffer, bpr: u32, w: u32, h: u32) {
        let slice = buffer.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
        let data = slice.get_mapped_range().expect("the mapped snapshot");
        let mut out = format!("P6\n{w} {h}\n255\n").into_bytes();
        for y in 0..h {
            let row = &data[(y * bpr) as usize..(y * bpr + w * 4) as usize];
            for px in row.chunks(4) {
                out.extend_from_slice(&[px[2], px[1], px[0]]);
            }
        }
        drop(data);
        match std::fs::write(path, out) {
            Ok(()) => log::info!("snapshot: {}", path.display()),
            Err(e) => log::error!("snapshot {}: {e}", path.display()),
        }
    }
}
