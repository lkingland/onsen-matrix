//! Idle inhibition: while the screensaver runs, the compositor's idle
//! timers should not fire, or Omarchy's own screensaver and lock would
//! take over the display after their timeouts. A video player says the
//! same thing with the same tool, Wayland's idle-inhibit protocol. The
//! time limit still ends the run, and then idle resumes.
//!
//! The inhibitor sits on a surface of our own that is never shown, on a
//! connection of our own, not on a window. Hyprland counts an inhibitor on
//! a window's surface only while the window is visible, and a window is
//! not visible while it fades in; it re-evaluates only when a window maps,
//! unmaps, changes focus or fullscreen. An inhibitor last evaluated during
//! the fade therefore stays off for the whole run, which is what happened
//! across three displays. A surface with no role belongs to no window, and
//! Hyprland assumes an inhibitor on one is visible: it holds from creation
//! until it is destroyed, whatever the windows, fades and focus do.

use bevy::prelude::*;
use wayland_client::globals::{registry_queue_init, GlobalListContents};
use wayland_client::protocol::{wl_compositor::WlCompositor, wl_registry, wl_surface::WlSurface};
use wayland_client::{Connection, Dispatch, EventQueue, QueueHandle};
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
wayland_client::delegate_noop!(State: ignore WlCompositor);
wayland_client::delegate_noop!(State: ignore WlSurface);
wayland_client::delegate_noop!(State: ignore ZwpIdleInhibitManagerV1);
wayland_client::delegate_noop!(State: ignore ZwpIdleInhibitorV1);

/// The inhibitor, with the surface and connection that keep it. Dropped,
/// the inhibitor is destroyed and idle resumes.
struct Inhibitor {
    queue: EventQueue<State>,
    state: State,
    _inhibitor: ZwpIdleInhibitorV1,
    _surface: WlSurface,
    _conn: Connection,
}

/// Inhibits idle once, on the first frame; after that keeps the
/// connection's queue drained, so the registry's news never backs up.
fn inhibit(mut done: Local<bool>, mut holder: Local<Option<Inhibitor>>) {
    if let Some(h) = holder.as_mut() {
        let _ = h.queue.dispatch_pending(&mut h.state);
        return;
    }
    if std::mem::replace(&mut *done, true) {
        return;
    }
    match create() {
        Ok(i) => {
            *holder = Some(i);
            info!("idle inhibited while the screensaver runs");
        }
        Err(e) => warn!("idle inhibit: {e}; the compositor's idle timers still apply"),
    }
}

fn create() -> Result<Inhibitor, String> {
    let conn = Connection::connect_to_env().map_err(|e| format!("no Wayland display: {e}"))?;
    let (globals, queue) = registry_queue_init::<State>(&conn).map_err(|e| format!("registry: {e}"))?;
    let qh = queue.handle();
    let compositor: WlCompositor = globals.bind(&qh, 1..=6, ()).map_err(|e| format!("no compositor: {e}"))?;
    let manager: ZwpIdleInhibitManagerV1 = globals.bind(&qh, 1..=1, ()).map_err(|e| format!("no idle-inhibit protocol: {e}"))?;
    let surface = compositor.create_surface(&qh, ());
    let inhibitor = manager.create_inhibitor(&surface, &qh, ());
    conn.flush().map_err(|e| format!("flush: {e}"))?;
    Ok(Inhibitor { queue, state: State, _inhibitor: inhibitor, _surface: surface, _conn: conn })
}
