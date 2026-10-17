//! The Wayland side: a layer-shell overlay surface on each wanted output,
//! the seat for ending the run on input, and frame pacing by the
//! compositor's frame callbacks.
//!
//! On Wayland this is how a screensaver owns a display: an overlay-layer
//! surface anchored to all four edges with exclusive keyboard focus sits
//! above every window and takes every key. A viewport lets the buffer be a
//! different size from the surface, so the picture is drawn at `--height`
//! lines and scaled by the compositor.

use std::collections::HashMap;
use std::ptr::NonNull;
use std::time::Instant;

use raw_window_handle::{RawDisplayHandle, RawWindowHandle, WaylandDisplayHandle, WaylandWindowHandle};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState, FrameCallbackData},
    delegate_registry,
    globals::GlobalData,
    output::{OutputHandler, OutputState},
    reexports::protocols::wp::viewporter::client::{wp_viewport::WpViewport, wp_viewporter::WpViewporter},
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

use crate::gpu::{Gpu, Shader, Target, Uniforms};
use crate::Args;

/// How far the pointer may drift before it counts as movement.
const POINTER_SLACK: f64 = 12.0;

/// One output's surface and what draws it.
struct Screen {
    name: String,
    layer: LayerSurface,
    viewport: WpViewport,
    /// The output's mode size, for drawing at its full resolution.
    physical: (u32, u32),
    surface: wgpu::Surface<'static>,
    configured: bool,
    target: Target,
}

struct App {
    args: Args,
    registry_state: RegistryState,
    output_state: OutputState,
    seat_state: SeatState,
    compositor: CompositorState,
    layer_shell: LayerShell,
    viewporter: WpViewporter,
    instance: wgpu::Instance,
    gpu: Option<Gpu>,
    started: Instant,
    screens: HashMap<u32, Screen>, // keyed by the wl_surface's protocol id
    keyboard: Option<wl_keyboard::WlKeyboard>,
    pointer: Option<wl_pointer::WlPointer>,
    pointer_origin: Option<(f64, f64)>,
    exit: bool,
}

/// Runs until input ends it (or forever, with `--hold`).
pub fn run(args: Args) {
    let conn = Connection::connect_to_env().expect("a Wayland display");
    let (globals, mut queue) = registry_queue_init(&conn).expect("the registry");
    let qh = queue.handle();
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor { backends: wgpu::Backends::VULKAN | wgpu::Backends::GL, ..wgpu::InstanceDescriptor::new_without_display_handle() });
    let mut app = App {
        args,
        registry_state: RegistryState::new(&globals),
        output_state: OutputState::new(&globals, &qh),
        seat_state: SeatState::new(&globals, &qh),
        compositor: CompositorState::bind(&globals, &qh).expect("wl_compositor"),
        layer_shell: LayerShell::bind(&globals, &qh).expect("the layer shell; Hyprland provides it"),
        viewporter: globals.bind(&qh, 1..=1, GlobalData).expect("wp_viewporter"),
        instance,
        gpu: None,
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
        if self.args.output.as_ref().is_some_and(|only| only != &name) {
            return;
        }
        let physical = info
            .as_ref()
            .and_then(|i| i.modes.iter().find(|m| m.current).map(|m| (m.dimensions.0 as u32, m.dimensions.1 as u32)))
            .unwrap_or((0, 0));
        let wl_surface = self.compositor.create_surface(qh);
        let layer = self.layer_shell.create_layer_surface(qh, wl_surface, Layer::Overlay, Some("onsen-matrix"), Some(output));
        layer.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
        layer.set_exclusive_zone(-1);
        layer.set_keyboard_interactivity(if self.args.hold { KeyboardInteractivity::None } else { KeyboardInteractivity::Exclusive });
        layer.set_size(0, 0);
        let viewport = self.viewporter.get_viewport(layer.wl_surface(), qh, ());
        layer.commit();

        // A wgpu surface over the wl_surface, from the raw display and surface pointers.
        let raw_display = RawDisplayHandle::Wayland(WaylandDisplayHandle::new(NonNull::new(conn.backend().display_ptr() as *mut _).expect("display pointer")));
        let raw_window = RawWindowHandle::Wayland(WaylandWindowHandle::new(NonNull::new(layer.wl_surface().id().as_ptr() as *mut _).expect("surface pointer")));
        let surface = unsafe {
            self.instance
                .create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle { raw_display_handle: Some(raw_display), raw_window_handle: raw_window })
                .expect("a wgpu surface")
        };
        let gpu = self.gpu.get_or_insert_with(|| Gpu::new(&self.instance, &surface, Shader::new(self.args.shader.clone())));
        let target = gpu.target(&name);
        let id = layer.wl_surface().id().protocol_id();
        self.screens.insert(id, Screen { name, layer, viewport, physical, surface, configured: false, target });
    }

    /// Sizes a screen's buffer for the surface size the compositor gave it.
    fn configure_screen(&mut self, id: u32, (w, h): (u32, u32)) {
        let Some(gpu) = self.gpu.as_mut() else { return };
        let Some(screen) = self.screens.get_mut(&id) else { return };
        let (pw, ph) = if screen.physical.0 > 0 { screen.physical } else { (w, h) };
        let wanted = self.args.height;
        let (bw, bh) = if wanted > 0 && wanted < ph { ((pw * wanted + ph / 2) / ph, wanted) } else { (pw, ph) };
        screen.viewport.set_destination(w as i32, h as i32);
        let mut config = screen.surface.get_default_config(&gpu.adapter, bw, bh).expect("a surface configuration");
        config.present_mode = wgpu::PresentMode::Fifo;
        config.usage |= wgpu::TextureUsages::COPY_SRC; // for snapshots
        screen.surface.configure(&gpu.device, &config);
        gpu.format.get_or_insert(config.format);
        screen.configured = true;
        log::info!("{}: {bw}x{bh} shown at {w}x{h} {:?}", screen.name, config.format);
    }

    fn draw(&mut self, qh: &QueueHandle<Self>, id: u32) {
        let Some(gpu) = self.gpu.as_mut() else { return };
        gpu.reload_shader();
        let Some(screen) = self.screens.get_mut(&id) else { return };
        if !screen.configured {
            return;
        }
        let frame = match screen.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f) | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
            other => {
                log::warn!("{}: no frame: {other:?}", screen.name);
                return;
            }
        };
        // A snapshot request names the file to write the frame to.
        let request = self.args.snap.as_ref().filter(|p| p.exists());
        let snapshot = request.and_then(|p| std::fs::read_to_string(p).ok()).map(|s| std::path::PathBuf::from(s.trim()));
        let uniforms = Uniforms {
            resolution: [frame.texture.width() as f32, frame.texture.height() as f32],
            time: self.started.elapsed().as_secs_f32(),
            loaded: gpu.loaded_at.duration_since(self.started).as_secs_f32(),
        };
        gpu.draw(&frame, &screen.target, uniforms, snapshot.as_deref());
        if let Some(p) = request {
            let _ = std::fs::remove_file(p);
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
        self.configure_screen(id, (w.max(1), h.max(1)));
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
            Capability::Keyboard if self.keyboard.is_none() => self.keyboard = self.seat_state.get_keyboard(qh, &seat, None).ok(),
            Capability::Pointer if self.pointer.is_none() => self.pointer = self.seat_state.get_pointer(qh, &seat).ok(),
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
        self.exit = !self.args.hold;
    }
    fn repeat_key(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, _: KeyEvent) {}
    fn release_key(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, _: KeyEvent) {}
    fn update_modifiers(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, _: Modifiers, _: RawModifiers, _: u32) {}
}

impl PointerHandler for App {
    fn pointer_frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_pointer::WlPointer, events: &[PointerEvent]) {
        if self.args.hold {
            return;
        }
        for event in events {
            match event.kind {
                PointerEventKind::Motion { .. } => {
                    let (x, y) = event.position;
                    match self.pointer_origin {
                        None => self.pointer_origin = Some((x, y)),
                        Some((ox, oy)) if (x - ox).abs() > POINTER_SLACK || (y - oy).abs() > POINTER_SLACK => self.exit = true,
                        _ => {}
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
