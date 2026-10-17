//! onsen-matrix: a screensaver that owns each output through a layer-shell
//! overlay surface and draws it with wgpu.
//!
//! This is the host. It does the fixed work (surfaces, GPU, input, pacing)
//! and draws whatever `shaders/matrix.wgsl` says, reloading that file
//! whenever it changes, so the picture can be iterated on without a
//! restart. Any key or pointer movement ends the run.

mod atlas;

use std::collections::HashMap;
use std::path::PathBuf;
use std::ptr::NonNull;
use std::time::{Instant, SystemTime};

use raw_window_handle::{RawDisplayHandle, RawWindowHandle, WaylandDisplayHandle, WaylandWindowHandle};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState, FrameCallbackData},
    delegate_registry,
    globals::GlobalData,
    reexports::protocols::wp::viewporter::client::{wp_viewport::WpViewport, wp_viewporter::WpViewporter},
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        keyboard::{KeyEvent, KeyboardHandler, Keysym, Modifiers, RawModifiers},
        pointer::{PointerEvent, PointerEventKind, PointerHandler},
        Capability, SeatHandler, SeatState,
    },
    shell::{
        wlr_layer::{Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface, LayerSurfaceConfigure},
        WaylandSurface,
    },
};
use wayland_client::{
    globals::registry_queue_init,
    protocol::{wl_keyboard, wl_output, wl_pointer, wl_seat, wl_surface},
    Connection, Dispatch, Proxy, QueueHandle,
};

#[repr(C)]
#[derive(Clone, Copy)]
struct Uniforms {
    resolution: [f32; 2],
    time: f32,
    _pad: f32,
}

/// The GPU, and the pipeline built from the current shader.
struct Gpu {
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
    layout: wgpu::BindGroupLayout,
    atlas_view: wgpu::TextureView,
    sampler: wgpu::Sampler,
    pipeline: Option<wgpu::RenderPipeline>,
    format: Option<wgpu::TextureFormat>,
}

/// The shader file and when it was last loaded.
struct Shader {
    path: PathBuf,
    loaded: Option<SystemTime>,
    checked: Instant,
}

/// One output's surface and what draws it.
struct Screen {
    name: String,
    layer: LayerSurface,
    /// The buffer is the output's physical size; the viewport maps it onto
    /// the surface's logical size, so every output pixel is drawn once.
    viewport: WpViewport,
    physical: (u32, u32),
    surface: wgpu::Surface<'static>,
    config: Option<wgpu::SurfaceConfiguration>,
    uniforms: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
}

struct App {
    registry_state: RegistryState,
    output_state: OutputState,
    seat_state: SeatState,
    compositor: CompositorState,
    layer_shell: LayerShell,
    viewporter: WpViewporter,
    instance: wgpu::Instance,
    gpu: Option<Gpu>,
    shader: Shader,
    only: Option<String>,
    /// --hold: never end on input, and take no keyboard, so the other
    /// screens stay usable while this one holds the picture for iteration.
    hold: bool,
    /// --height N: the number of lines drawn per output; the output scales
    /// the picture to fill itself. 0 means the output's own height.
    height: u32,
    started: Instant,
    screens: HashMap<u32, Screen>, // keyed by the wl_surface's protocol id
    keyboard: Option<wl_keyboard::WlKeyboard>,
    pointer: Option<wl_pointer::WlPointer>,
    pointer_origin: Option<(f64, f64)>,
    exit: bool,
}

fn arg(name: &str) -> Option<String> {
    std::env::args().skip_while(|a| a != name).nth(1)
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("onsen_matrix=info")).init();
    let only = arg("--output");
    let hold = std::env::args().any(|a| a == "--hold");
    let height: u32 = arg("--height").and_then(|h| h.parse().ok()).unwrap_or(1440);
    let shader_path = arg("--shader")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("shaders/matrix.wgsl"));

    let conn = Connection::connect_to_env().expect("a Wayland display");
    let (globals, mut queue) = registry_queue_init(&conn).expect("the registry");
    let qh = queue.handle();

    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::VULKAN | wgpu::Backends::GL,
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });

    let mut app = App {
        registry_state: RegistryState::new(&globals),
        output_state: OutputState::new(&globals, &qh),
        seat_state: SeatState::new(&globals, &qh),
        compositor: CompositorState::bind(&globals, &qh).expect("wl_compositor"),
        layer_shell: LayerShell::bind(&globals, &qh).expect("the layer shell; Hyprland provides it"),
        viewporter: globals.bind(&qh, 1..=1, GlobalData).expect("wp_viewporter"),
        instance,
        gpu: None,
        shader: Shader { path: shader_path, loaded: None, checked: Instant::now() },
        only,
        hold,
        height,
        started: Instant::now(),
        screens: HashMap::new(),
        keyboard: None,
        pointer: None,
        pointer_origin: None,
        exit: false,
    };

    // Let the outputs arrive, then make a surface for each wanted one.
    queue.roundtrip(&mut app).unwrap();
    queue.roundtrip(&mut app).unwrap();
    let outputs: Vec<wl_output::WlOutput> = app.output_state.outputs().collect();
    for output in outputs {
        app.add_screen(&conn, &qh, &output);
    }
    if app.screens.is_empty() {
        eprintln!("no output matched");
        return;
    }

    while !app.exit {
        queue.blocking_dispatch(&mut app).unwrap();
    }
}

impl App {
    fn add_screen(&mut self, conn: &Connection, qh: &QueueHandle<Self>, output: &wl_output::WlOutput) {
        let info = self.output_state.info(output);
        let name = info.as_ref().and_then(|i| i.name.clone()).unwrap_or_default();
        let physical = info
            .as_ref()
            .and_then(|i| i.modes.iter().find(|m| m.current).map(|m| (m.dimensions.0 as u32, m.dimensions.1 as u32)))
            .unwrap_or((0, 0));
        if let Some(only) = &self.only {
            if &name != only {
                return;
            }
        }
        let wl_surface = self.compositor.create_surface(qh);
        let layer = self.layer_shell.create_layer_surface(qh, wl_surface, Layer::Overlay, Some("onsen-matrix"), Some(output));
        layer.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
        layer.set_exclusive_zone(-1);
        layer.set_keyboard_interactivity(if self.hold { KeyboardInteractivity::None } else { KeyboardInteractivity::Exclusive });
        layer.set_size(0, 0);
        let viewport = self.viewporter.get_viewport(layer.wl_surface(), qh, ());
        layer.commit();

        // A wgpu surface over the wl_surface, from the raw display and surface pointers.
        let raw_display = RawDisplayHandle::Wayland(WaylandDisplayHandle::new(
            NonNull::new(conn.backend().display_ptr() as *mut _).expect("display pointer"),
        ));
        let raw_window = RawWindowHandle::Wayland(WaylandWindowHandle::new(
            NonNull::new(layer.wl_surface().id().as_ptr() as *mut _).expect("surface pointer"),
        ));
        let surface = unsafe {
            self.instance
                .create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle { raw_display_handle: Some(raw_display), raw_window_handle: raw_window })
                .expect("a wgpu surface")
        };
        if self.gpu.is_none() {
            let adapter = pollster::block_on(self.instance.request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                ..Default::default()
            }))
            .expect("a GPU adapter");
            let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                label: Some("onsen-matrix"),
                ..Default::default()
            }))
            .expect("a GPU device");
            log::info!("gpu: {}", adapter.get_info().name);
            let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("uniforms"),
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
            let atlas_view = atlas::upload(&device, &queue);
            let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("atlas"),
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                mipmap_filter: wgpu::MipmapFilterMode::Linear,
                ..Default::default()
            });
            self.gpu = Some(Gpu { adapter, device, queue, layout, atlas_view, sampler, pipeline: None, format: None });
        }
        let gpu = self.gpu.as_ref().unwrap();
        let uniforms = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(&name),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(&name),
            layout: &gpu.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: uniforms.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&gpu.atlas_view) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&gpu.sampler) },
            ],
        });
        let id = layer.wl_surface().id().protocol_id();
        self.screens.insert(id, Screen { name, layer, viewport, physical, surface, config: None, uniforms, bind_group });
    }

    /// Reloads the shader if its file changed, keeping the old pipeline on error.
    fn reload_shader(&mut self) {
        if self.shader.checked.elapsed().as_millis() < 100 && self.shader.loaded.is_some() {
            return;
        }
        self.shader.checked = Instant::now();
        let Some(gpu) = self.gpu.as_mut() else { return };
        let Some(format) = gpu.format else { return };
        let mtime = std::fs::metadata(&self.shader.path).and_then(|m| m.modified()).ok();
        if mtime.is_some() && mtime == self.shader.loaded {
            return;
        }
        let source = match std::fs::read_to_string(&self.shader.path) {
            Ok(s) => s,
            Err(e) => {
                log::error!("{}: {e}", self.shader.path.display());
                self.shader.loaded = mtime;
                return;
            }
        };
        let scope = gpu.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let module = gpu.device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("matrix"),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });
        let pipeline_layout = gpu.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("matrix"),
            bind_group_layouts: &[Some(&gpu.layout)],
            immediate_size: 0,
        });
        let pipeline = gpu.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("matrix"),
            layout: Some(&pipeline_layout),
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
            Some(e) => log::error!("shader not loaded:\n{e}"),
            None => {
                gpu.pipeline = Some(pipeline);
                log::info!("shader loaded: {}", self.shader.path.display());
            }
        }
        self.shader.loaded = mtime;
    }

    fn draw(&mut self, qh: &QueueHandle<Self>, id: u32) {
        self.reload_shader();
        let Some(gpu) = self.gpu.as_ref() else { return };
        let Some(screen) = self.screens.get_mut(&id) else { return };
        let Some(config) = screen.config.as_ref() else { return };
        let frame = match screen.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f) | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
            other => {
                log::warn!("{}: no frame: {other:?}", screen.name);
                return;
            }
        };
        let view = frame.texture.create_view(&Default::default());
        let u = Uniforms { resolution: [config.width as f32, config.height as f32], time: self.started.elapsed().as_secs_f32(), _pad: 0.0 };
        let bytes = unsafe { std::slice::from_raw_parts(&u as *const Uniforms as *const u8, std::mem::size_of::<Uniforms>()) };
        gpu.queue.write_buffer(&screen.uniforms, 0, bytes);
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
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
            if let Some(pipeline) = gpu.pipeline.as_ref() {
                pass.set_pipeline(pipeline);
                pass.set_bind_group(0, &screen.bind_group, &[]);
                pass.draw(0..6, 0..1);
            }
        }
        // A snapshot request: `<request>` names the file to write; the frame is
        // read back and written there as a PPM, then the request is removed.
        let snap = std::env::var("ONSEN_MATRIX_SNAP").ok().map(PathBuf::from).filter(|p| p.exists());
        let readback = snap.as_ref().map(|_| {
            let bpr = (config.width * 4 + 255) / 256 * 256;
            let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("snapshot"),
                size: (bpr * config.height) as u64,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            encoder.copy_texture_to_buffer(
                wgpu::TexelCopyTextureInfo { texture: &frame.texture, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
                wgpu::TexelCopyBufferInfo { buffer: &buffer, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(bpr), rows_per_image: Some(config.height) } },
                wgpu::Extent3d { width: config.width, height: config.height, depth_or_array_layers: 1 },
            );
            (buffer, bpr)
        });
        gpu.queue.submit([encoder.finish()]);
        if let (Some(request), Some((buffer, bpr))) = (snap, readback) {
            let target = std::fs::read_to_string(&request).unwrap_or_default().trim().to_string();
            let (w, h) = (config.width, config.height);
            let slice = buffer.slice(..);
            slice.map_async(wgpu::MapMode::Read, |_| {});
            let _ = gpu.device.poll(wgpu::PollType::wait_indefinitely());
            let data = slice.get_mapped_range().expect("the mapped snapshot");
            let mut out = format!("P6\n{w} {h}\n255\n").into_bytes();
            for y in 0..h {
                let row = &data[(y * bpr) as usize..(y * bpr + w * 4) as usize];
                for px in row.chunks(4) {
                    out.extend_from_slice(&[px[2], px[1], px[0]]); // BGRA to RGB
                }
            }
            drop(data);
            match std::fs::write(&target, out) {
                Ok(()) => log::info!("snapshot: {target}"),
                Err(e) => log::error!("snapshot {target}: {e}"),
            }
            let _ = std::fs::remove_file(&request);
        }
        // Ask for the next frame before presenting, so the compositor paces us.
        screen.layer.wl_surface().frame(qh, FrameCallbackData(screen.layer.wl_surface().clone()));
        gpu.queue.present(frame);
    }
}

impl CompositorHandler for App {
    fn scale_factor_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: i32) {}
    fn transform_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: wl_output::Transform) {}
    fn frame(&mut self, _: &Connection, qh: &QueueHandle<Self>, surface: &wl_surface::WlSurface, _: u32) {
        self.draw(qh, surface.id().protocol_id());
    }
    fn surface_enter(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
    fn surface_leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
}

impl OutputHandler for App {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl LayerShellHandler for App {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &LayerSurface) {
        self.exit = true;
    }
    fn configure(&mut self, _: &Connection, qh: &QueueHandle<Self>, layer: &LayerSurface, configure: LayerSurfaceConfigure, _: u32) {
        let id = layer.wl_surface().id().protocol_id();
        let (w, h) = configure.new_size;
        let (w, h) = (w.max(1), h.max(1));
        let Some(gpu) = self.gpu.as_mut() else { return };
        if let Some(screen) = self.screens.get_mut(&id) {
            // Draw at the chosen size, shown at the surface's logical size.
            let (pw, ph) = if screen.physical.0 > 0 { screen.physical } else { (w, h) };
            let (bw, bh) = if self.height > 0 && self.height < ph { ((pw * self.height + ph / 2) / ph, self.height) } else { (pw, ph) };
            screen.viewport.set_destination(w as i32, h as i32);
            let mut config = screen.surface.get_default_config(&gpu.adapter, bw, bh).expect("a surface configuration");
            config.present_mode = wgpu::PresentMode::Fifo;
            config.usage |= wgpu::TextureUsages::COPY_SRC; // for snapshots
            screen.surface.configure(&gpu.device, &config);
            if gpu.format.is_none() {
                gpu.format = Some(config.format);
            }
            log::info!("{}: {bw}x{bh} shown at {w}x{h} {:?}", screen.name, config.format);
            screen.config = Some(config);
        }
        self.draw(qh, id);
    }
}

impl SeatHandler for App {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat_state
    }
    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
    fn new_capability(&mut self, _: &Connection, qh: &QueueHandle<Self>, seat: wl_seat::WlSeat, capability: Capability) {
        match capability {
            Capability::Keyboard if self.keyboard.is_none() => {
                self.keyboard = self.seat_state.get_keyboard(qh, &seat, None).ok();
            }
            Capability::Pointer if self.pointer.is_none() => {
                self.pointer = self.seat_state.get_pointer(qh, &seat).ok();
            }
            _ => {}
        }
    }
    fn remove_capability(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat, _: Capability) {}
    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl KeyboardHandler for App {
    fn enter(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: &wl_surface::WlSurface, _: u32, _: &[u32], _: &[Keysym]) {}
    fn leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: &wl_surface::WlSurface, _: u32) {}
    fn press_key(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, _: KeyEvent) {
        self.exit = !self.hold;
    }
    fn repeat_key(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, _: KeyEvent) {}
    fn release_key(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, _: KeyEvent) {}
    fn update_modifiers(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, _: Modifiers, _: RawModifiers, _: u32) {}
}

impl PointerHandler for App {
    fn pointer_frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_pointer::WlPointer, events: &[PointerEvent]) {
        if self.hold {
            return;
        }
        for event in events {
            match event.kind {
                PointerEventKind::Motion { .. } => {
                    let (x, y) = event.position;
                    match self.pointer_origin {
                        None => self.pointer_origin = Some((x, y)),
                        Some((ox, oy)) => {
                            if (x - ox).abs() > 12.0 || (y - oy).abs() > 12.0 {
                                self.exit = true;
                            }
                        }
                    }
                }
                PointerEventKind::Press { .. } | PointerEventKind::Axis { .. } => self.exit = true,
                _ => {}
            }
        }
    }
}

impl ProvidesRegistryState for App {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState, SeatState];
}

impl Dispatch<WpViewporter, GlobalData> for App {
    fn event(_: &mut Self, _: &WpViewporter, _: <WpViewporter as Proxy>::Event, _: &GlobalData, _: &Connection, _: &QueueHandle<Self>) {}
}
impl Dispatch<WpViewport, ()> for App {
    fn event(_: &mut Self, _: &WpViewport, _: <WpViewport as Proxy>::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

delegate_registry!(App);
smithay_client_toolkit::delegate_dispatch2!(App);
