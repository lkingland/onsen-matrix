//! Idle inhibition: while the screensaver runs, the compositor's idle
//! timers should not fire, or Omarchy's own screensaver and lock would
//! take over the display after their timeouts. A video player says the
//! same thing with the same tool: an idle inhibitor on its surface, from
//! Wayland's idle-inhibit protocol. The time limit still ends the run, and
//! then idle resumes.
//!
//! winit does not expose this, so it is done on winit's own display and
//! surface, handed over through their raw handles: a connection wrapped
//! around the same libwayland display, and the inhibitor made on the
//! window's surface. One inhibitor is enough.

use bevy::ecs::system::NonSendMarker;
use bevy::prelude::*;
use bevy::window::RawHandleWrapper;
use raw_window_handle::{RawDisplayHandle, RawWindowHandle};
use wayland_client::backend::{Backend, ObjectId};
use wayland_client::globals::{registry_queue_init, GlobalListContents};
use wayland_client::protocol::{wl_registry, wl_surface::WlSurface};
use wayland_client::{Connection, Dispatch, EventQueue, Proxy, QueueHandle};
use wayland_protocols::wp::idle_inhibit::zv1::client::{zwp_idle_inhibit_manager_v1::ZwpIdleInhibitManagerV1, zwp_idle_inhibitor_v1::ZwpIdleInhibitorV1};

pub struct InhibitPlugin;

impl Plugin for InhibitPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, inhibit);
    }
}

struct State;

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for State {
    fn event(_: &mut Self, _: &wl_registry::WlRegistry, _: wl_registry::Event, _: &GlobalListContents, _: &Connection, _: &QueueHandle<Self>) {}
}
wayland_client::delegate_noop!(State: ignore ZwpIdleInhibitManagerV1);
wayland_client::delegate_noop!(State: ignore ZwpIdleInhibitorV1);

/// The inhibitor, and the connection and queue that keep it. Dropped, the
/// inhibitor is destroyed and idle resumes.
struct Inhibitor {
    queue: EventQueue<State>,
    state: State,
    _inhibitor: ZwpIdleInhibitorV1,
    _conn: Connection,
}

/// Once a window has its handles, inhibits idle on its surface; after
/// that, keeps the connection's queue drained. Main thread only, like
/// everything that touches winit's display.
fn inhibit(_: NonSendMarker, mut done: Local<bool>, mut holder: Local<Option<Inhibitor>>, windows: Query<&RawHandleWrapper, With<Window>>) {
    if let Some(h) = holder.as_mut() {
        let _ = h.queue.dispatch_pending(&mut h.state);
        return;
    }
    if *done {
        return;
    }
    let Some(handles) = windows.iter().next() else { return };
    *done = true;
    let (RawDisplayHandle::Wayland(display), RawWindowHandle::Wayland(window)) = (handles.get_display_handle(), handles.get_window_handle()) else {
        info!("idle inhibit: not on Wayland; nothing to do");
        return;
    };
    // Safety: the handles are winit's live display and surface, used on the
    // main thread for the window's lifetime, which the holder never outlives.
    match unsafe { create(display.display.as_ptr(), window.surface.as_ptr()) } {
        Ok(i) => {
            *holder = Some(i);
            info!("idle inhibited while the screensaver runs");
        }
        Err(e) => warn!("idle inhibit: {e}; the compositor's idle timers still apply"),
    }
}

unsafe fn create(display: *mut std::ffi::c_void, surface: *mut std::ffi::c_void) -> Result<Inhibitor, String> {
    let backend = unsafe { Backend::from_foreign_display(display as *mut _) };
    let conn = Connection::from_backend(backend);
    let (globals, queue) = registry_queue_init::<State>(&conn).map_err(|e| format!("registry: {e}"))?;
    let qh = queue.handle();
    let manager: ZwpIdleInhibitManagerV1 = globals.bind(&qh, 1..=1, ()).map_err(|e| format!("no idle-inhibit protocol: {e}"))?;
    let id = unsafe { ObjectId::from_ptr(WlSurface::interface(), surface as *mut _) }.map_err(|e| format!("surface: {e}"))?;
    let wl_surface = WlSurface::from_id(&conn, id).map_err(|e| format!("surface: {e}"))?;
    let inhibitor = manager.create_inhibitor(&wl_surface, &qh, ());
    conn.flush().map_err(|e| format!("flush: {e}"))?;
    Ok(Inhibitor { queue, state: State, _inhibitor: inhibitor, _conn: conn })
}
