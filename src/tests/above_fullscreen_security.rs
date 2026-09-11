use std::cmp::min;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use niri_config::Config;
use smithay::backend::allocator::Fourcc;
use smithay::reexports::wayland_protocols::ext::session_lock::v1::client::ext_session_lock_manager_v1::ExtSessionLockManagerV1;
use smithay::reexports::wayland_protocols::ext::session_lock::v1::client::ext_session_lock_surface_v1::{self, ExtSessionLockSurfaceV1};
use smithay::reexports::wayland_protocols::ext::session_lock::v1::client::ext_session_lock_v1::{self, ExtSessionLockV1};
use smithay::utils::{Logical, Physical, Point, Scale, Size, Transform};
use smithay::reexports::wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_shell_v1;
use smithay::reexports::wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_surface_v1::Anchor;
use wayland_client::protocol::wl_registry::{self, WlRegistry};
use wayland_client::{Connection, Dispatch, Proxy as _, QueueHandle};

use super::client::State as ClientState;
use super::*;
use crate::render_helpers::{render_to_vec, RenderCtx, RenderTarget};
use crate::tests::client::LayerConfigureProps;

const OUT: (u16, u16) = (1920, 1080);

const CONFIG: &str = r##"
layout {
    gaps 0
    border { off; }
    focus-ring { off; }
}

hotkey-overlay {
    skip-at-startup
}

window-rule {
    match app-id="^over$"
    open-floating true
    above-fullscreen true
    default-floating-position x=40 y=40 relative-to="top-left"
    default-column-width { fixed 300; }
    default-window-height { fixed 150; }
}
"##;

#[derive(Clone, Default)]
struct LockGlobals(Arc<Mutex<Option<ExtSessionLockManagerV1>>>);

#[derive(Default)]
struct LockStatus {
    locked: AtomicBool,
    finished: AtomicBool,
}

impl Dispatch<WlRegistry, LockGlobals> for ClientState {
    fn event(
        _state: &mut Self,
        registry: &WlRegistry,
        event: <WlRegistry as wayland_client::Proxy>::Event,
        data: &LockGlobals,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = event
        {
            if interface == ExtSessionLockManagerV1::interface().name {
                let version = min(version, ExtSessionLockManagerV1::interface().version);
                *data.0.lock().unwrap() = Some(registry.bind(name, version, qh, ()));
            }
        }
    }
}

impl Dispatch<ExtSessionLockManagerV1, ()> for ClientState {
    fn event(
        _state: &mut Self,
        _proxy: &ExtSessionLockManagerV1,
        _event: <ExtSessionLockManagerV1 as wayland_client::Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        unreachable!()
    }
}

impl Dispatch<ExtSessionLockV1, Arc<LockStatus>> for ClientState {
    fn event(
        _state: &mut Self,
        _proxy: &ExtSessionLockV1,
        event: <ExtSessionLockV1 as wayland_client::Proxy>::Event,
        data: &Arc<LockStatus>,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        match event {
            ext_session_lock_v1::Event::Locked => data.locked.store(true, Ordering::Relaxed),
            ext_session_lock_v1::Event::Finished => data.finished.store(true, Ordering::Relaxed),
            _ => unreachable!(),
        }
    }
}

impl Dispatch<ExtSessionLockSurfaceV1, ()> for ClientState {
    fn event(
        _state: &mut Self,
        proxy: &ExtSessionLockSurfaceV1,
        event: <ExtSessionLockSurfaceV1 as wayland_client::Proxy>::Event,
        _data: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        if let ext_session_lock_surface_v1::Event::Configure { serial, .. } = event {
            proxy.ack_configure(serial);
        }
    }
}

fn add_win(f: &mut Fixture, id: client::ClientId, app_id: &str, rgb: (u32, u32, u32), fs: bool) {
    let w = f.client(id).create_window();
    let surface = w.surface.clone();
    w.set_app_id(app_id);
    if fs {
        w.set_fullscreen(None);
    }
    w.commit();
    f.roundtrip(id);

    let w = f.client(id).window(&surface);
    let size = w.recent_configures().last().unwrap().size;
    w.attach_new_buffer_with_color(rgb.0, rgb.1, rgb.2, u32::MAX);
    if fs {
        w.set_size(OUT.0, OUT.1);
    } else {
        w.set_size(size.0 as u16, size.1 as u16);
    }
    w.ack_last_and_commit();
    f.double_roundtrip(id);
}

fn render(f: &mut Fixture, target: RenderTarget) -> (Size<i32, Physical>, Vec<u8>) {
    let output = f.niri_output(1);
    let size = output.current_mode().unwrap().size;
    let scale = Scale::from(output.current_scale().fractional_scale());

    let state = f.niri_state();
    state.niri.update_render_elements(Some(&output));

    let niri = &state.niri;
    state
        .backend
        .with_primary_renderer(|renderer| {
            let ctx = RenderCtx {
                renderer,
                target,
                xray: None,
            };
            let elements = niri.render_to_vec(ctx, &output, false);
            let elements = elements.iter().rev();
            let pixels = render_to_vec(
                renderer,
                size,
                scale,
                Transform::Normal,
                Fourcc::Abgr8888,
                elements,
            )
            .unwrap();
            (size, pixels)
        })
        .unwrap()
}

fn count_color(pixels: &[u8], want: (u8, u8, u8)) -> usize {
    pixels
        .chunks_exact(4)
        .filter(|p| (p[0], p[1], p[2]) == want)
        .count()
}

#[test]
fn egl_above_fullscreen_does_not_render_over_the_lock_screen() {
    let config = Config::parse_mem(CONFIG).unwrap();
    let mut f = Fixture::with_config(config);
    f.niri_state().backend.headless().add_renderer().unwrap();
    f.add_output(1, OUT);
    let id = f.add_client();

    add_win(&mut f, id, "game", (u32::MAX, 0, 0), true);
    add_win(&mut f, id, "over", (0, 0, u32::MAX), false);
    f.niri().layout.focus_tiling();
    f.niri_complete_animations();
    f.double_roundtrip(id);

    // Sanity check: unlocked, the overlay is on screen above the fullscreen window.
    let (_size, pixels) = render(&mut f, RenderTarget::Output);
    assert!(
        count_color(&pixels, (0, 0, 255)) > 1000,
        "overlay must be visible before locking"
    );

    // Now lock the session.
    let globals = LockGlobals::default();
    let status = Arc::new(LockStatus::default());
    {
        let client = f.client(id);
        let qh = client.qh.clone();
        let _registry = client.display.get_registry(&qh, globals.clone());
        client.connection.flush().unwrap();
    }
    f.roundtrip(id);

    let manager = globals
        .0
        .lock()
        .unwrap()
        .clone()
        .expect("ext-session-lock-manager-v1 should be advertised to an unrestricted client");

    let (lock, lock_surface, lock_wl_surface, lock_viewport) = {
        let client = f.client(id);
        let qh = client.qh.clone();
        let lock = manager.lock(&qh, status.clone());
        let output = client.output("headless-1");
        let compositor = client.state.compositor.clone().unwrap();
        let viewporter = client.state.viewporter.clone().unwrap();
        let surface = compositor.create_surface(&qh, ());
        let viewport = viewporter.get_viewport(&surface, &qh, ());
        let lock_surface = lock.get_lock_surface(&surface, &output, &qh, ());
        client.connection.flush().unwrap();
        (lock, lock_surface, surface, viewport)
    };
    f.double_roundtrip(id);

    {
        let client = f.client(id);
        let qh = client.qh.clone();
        let spbm = client.state.spbm.clone().unwrap();
        // Paint the lock screen green.
        let buffer = spbm.create_u32_rgba_buffer(0, u32::MAX, 0, u32::MAX, &qh, ());
        lock_viewport.set_destination(i32::from(OUT.0), i32::from(OUT.1));
        lock_wl_surface.attach(Some(&buffer), 0, 0);
        lock_wl_surface.commit();
        client.connection.flush().unwrap();
    }
    f.double_roundtrip(id);
    f.niri_complete_animations();
    f.double_roundtrip(id);

    assert!(f.niri().is_locked(), "the session must be locked now");

    let (_size, pixels) = render(&mut f, RenderTarget::Output);
    let blue = count_color(&pixels, (0, 0, 255));
    let red = count_color(&pixels, (255, 0, 0));
    let green = count_color(&pixels, (0, 255, 0));

    assert_eq!(
        blue, 0,
        "the above-fullscreen overlay must not be rendered while locked"
    );
    assert_eq!(
        red, 0,
        "the fullscreen window must not be rendered while locked"
    );
    assert!(green > 1000, "the lock surface must be rendered");

    // The same must hold for screencast/screen capture targets while locked.
    for target in [RenderTarget::Screencast, RenderTarget::ScreenCapture] {
        let (_size, pixels) = render(&mut f, target);
        assert_eq!(
            count_color(&pixels, (0, 0, 255)),
            0,
            "overlay leaked into {target:?} while locked"
        );
    }

    drop(lock_surface);
    drop(lock);
}

/// Same as above, but the lock surface is fully transparent, so anything the compositor draws
/// underneath it would be plainly visible. This is the negative control: if the layout were
/// rendered while locked, the overlay would show up here.
#[test]
fn egl_above_fullscreen_hidden_under_transparent_lock_surface() {
    let config = Config::parse_mem(CONFIG).unwrap();
    let mut f = Fixture::with_config(config);
    f.niri_state().backend.headless().add_renderer().unwrap();
    f.add_output(1, OUT);
    let id = f.add_client();

    add_win(&mut f, id, "game", (u32::MAX, 0, 0), true);
    add_win(&mut f, id, "over", (0, 0, u32::MAX), false);
    f.niri().layout.focus_tiling();
    f.niri_complete_animations();
    f.double_roundtrip(id);

    let (_size, pixels) = render(&mut f, RenderTarget::Output);
    assert!(
        count_color(&pixels, (0, 0, 255)) > 1000,
        "overlay must be visible before locking"
    );

    let globals = LockGlobals::default();
    let status = Arc::new(LockStatus::default());
    {
        let client = f.client(id);
        let qh = client.qh.clone();
        let _registry = client.display.get_registry(&qh, globals.clone());
        client.connection.flush().unwrap();
    }
    f.roundtrip(id);

    let manager = globals.0.lock().unwrap().clone().unwrap();

    let (lock, lock_surface, lock_wl_surface, lock_viewport) = {
        let client = f.client(id);
        let qh = client.qh.clone();
        let lock = manager.lock(&qh, status.clone());
        let output = client.output("headless-1");
        let compositor = client.state.compositor.clone().unwrap();
        let viewporter = client.state.viewporter.clone().unwrap();
        let surface = compositor.create_surface(&qh, ());
        let viewport = viewporter.get_viewport(&surface, &qh, ());
        let lock_surface = lock.get_lock_surface(&surface, &output, &qh, ());
        client.connection.flush().unwrap();
        (lock, lock_surface, surface, viewport)
    };
    f.double_roundtrip(id);

    {
        let client = f.client(id);
        let qh = client.qh.clone();
        let spbm = client.state.spbm.clone().unwrap();
        let buffer = spbm.create_u32_rgba_buffer(0, 0, 0, 0, &qh, ());
        // Fully transparent lock surface: anything drawn beneath it would be visible.
        lock_viewport.set_destination(i32::from(OUT.0), i32::from(OUT.1));
        lock_wl_surface.attach(Some(&buffer), 0, 0);
        lock_wl_surface.commit();
        client.connection.flush().unwrap();
    }
    f.double_roundtrip(id);
    f.niri_complete_animations();
    f.double_roundtrip(id);

    assert!(f.niri().is_locked(), "the session must be locked now");

    let (_size, pixels) = render(&mut f, RenderTarget::Output);
    assert_eq!(
        count_color(&pixels, (0, 0, 255)),
        0,
        "the above-fullscreen overlay leaked under a transparent lock surface"
    );
    assert_eq!(
        count_color(&pixels, (255, 0, 0)),
        0,
        "the fullscreen window leaked under a transparent lock surface"
    );

    drop(lock_surface);
    drop(lock);
}

fn locked_element_count(f: &mut Fixture, target: RenderTarget) -> usize {
    let output = f.niri_output(1);
    let state = f.niri_state();
    state.niri.update_render_elements(Some(&output));
    let niri = &state.niri;
    state
        .backend
        .with_primary_renderer(|renderer| {
            let ctx = RenderCtx {
                renderer,
                target,
                xray: None,
            };
            niri.render_to_vec(ctx, &output, false).len()
        })
        .unwrap()
}

/// Sensitivity control for the two tests above: while locked, the compositor must submit only the
/// lock surface plus the solid lock background, and nothing from the layout. Pixel checks alone
/// cannot see this, because the opaque lock background is stacked above the layout.
#[test]
fn egl_locked_frame_contains_no_layout_elements() {
    let config = Config::parse_mem(CONFIG).unwrap();
    let mut f = Fixture::with_config(config);
    f.niri_state().backend.headless().add_renderer().unwrap();
    f.add_output(1, OUT);
    let id = f.add_client();

    add_win(&mut f, id, "game", (u32::MAX, 0, 0), true);
    add_win(&mut f, id, "over", (0, 0, u32::MAX), false);
    f.niri().layout.focus_tiling();
    f.niri_complete_animations();
    f.double_roundtrip(id);

    let unlocked = locked_element_count(&mut f, RenderTarget::Output);
    assert!(
        unlocked >= 3,
        "unlocked frame should contain the game, the overlay and a background: {unlocked}"
    );

    let globals = LockGlobals::default();
    let status = Arc::new(LockStatus::default());
    {
        let client = f.client(id);
        let qh = client.qh.clone();
        let _registry = client.display.get_registry(&qh, globals.clone());
        client.connection.flush().unwrap();
    }
    f.roundtrip(id);

    let manager = globals.0.lock().unwrap().clone().unwrap();

    let (lock, lock_surface, lock_wl_surface, lock_viewport) = {
        let client = f.client(id);
        let qh = client.qh.clone();
        let lock = manager.lock(&qh, status.clone());
        let output = client.output("headless-1");
        let compositor = client.state.compositor.clone().unwrap();
        let viewporter = client.state.viewporter.clone().unwrap();
        let surface = compositor.create_surface(&qh, ());
        let viewport = viewporter.get_viewport(&surface, &qh, ());
        let lock_surface = lock.get_lock_surface(&surface, &output, &qh, ());
        client.connection.flush().unwrap();
        (lock, lock_surface, surface, viewport)
    };
    f.double_roundtrip(id);

    {
        let client = f.client(id);
        let qh = client.qh.clone();
        let spbm = client.state.spbm.clone().unwrap();
        let buffer = spbm.create_u32_rgba_buffer(0, u32::MAX, 0, u32::MAX, &qh, ());
        lock_viewport.set_destination(i32::from(OUT.0), i32::from(OUT.1));
        lock_wl_surface.attach(Some(&buffer), 0, 0);
        lock_wl_surface.commit();
        client.connection.flush().unwrap();
    }
    f.double_roundtrip(id);
    f.niri_complete_animations();
    f.double_roundtrip(id);

    assert!(f.niri().is_locked(), "the session must be locked now");

    for target in [
        RenderTarget::Output,
        RenderTarget::Screencast,
        RenderTarget::ScreenCapture,
    ] {
        let n = locked_element_count(&mut f, target);
        assert_eq!(
            n, 2,
            "locked {target:?} frame must contain only the lock surface and the lock background, \
             got {n} elements"
        );
    }

    drop(lock_surface);
    drop(lock);
}

const CONFIG_BLOCKED: &str = r##"
layout {
    gaps 0
    border { off; }
    focus-ring { off; }
}

hotkey-overlay {
    skip-at-startup
}

window-rule {
    match app-id="^over$"
    open-floating true
    above-fullscreen true
    block-out-from "screencast"
    default-floating-position x=40 y=40 relative-to="top-left"
    default-column-width { fixed 300; }
    default-window-height { fixed 150; }
}
"##;

/// `block-out-from` must still redact a window that is drawn above a fullscreen window.
#[test]
fn egl_block_out_from_screencast_still_redacts_above_fullscreen_window() {
    let config = Config::parse_mem(CONFIG_BLOCKED).unwrap();
    let mut f = Fixture::with_config(config);
    f.niri_state().backend.headless().add_renderer().unwrap();
    f.add_output(1, OUT);
    let id = f.add_client();

    add_win(&mut f, id, "game", (u32::MAX, 0, 0), true);
    add_win(&mut f, id, "over", (0, 0, u32::MAX), false);
    f.niri().layout.focus_tiling();
    f.niri_complete_animations();
    f.double_roundtrip(id);

    let (_size, pixels) = render(&mut f, RenderTarget::Output);
    let blue = count_color(&pixels, (0, 0, 255));
    assert!(
        blue > 1000,
        "overlay is visible on the real output above the fullscreen window"
    );

    let (_size, pixels) = render(&mut f, RenderTarget::Screencast);
    assert_eq!(
        count_color(&pixels, (0, 0, 255)),
        0,
        "the overlay must be blocked out of a screencast"
    );
}

/// While locked, pointer input must land on the lock surface, never on the above-fullscreen
/// window, even though window_under() would now hit it.
#[test]
fn egl_locked_pointer_input_does_not_reach_above_fullscreen_window() {
    let config = Config::parse_mem(CONFIG).unwrap();
    let mut f = Fixture::with_config(config);
    f.niri_state().backend.headless().add_renderer().unwrap();
    f.add_output(1, OUT);
    let id = f.add_client();

    add_win(&mut f, id, "game", (u32::MAX, 0, 0), true);
    add_win(&mut f, id, "over", (0, 0, u32::MAX), false);
    f.niri().layout.focus_tiling();
    f.niri_complete_animations();
    f.double_roundtrip(id);

    // A point inside the overlay.
    let point = Point::from((100., 100.));

    let overlay_surface = {
        let ws = f.niri().layout.active_workspace().unwrap();
        let (win, _) = ws.window_under(point).unwrap();
        win.toplevel().wl_surface().clone()
    };

    let contents = f.niri().contents_under(point);
    assert_eq!(
        contents.surface.as_ref().map(|(s, _)| s.clone()),
        Some(overlay_surface.clone()),
        "unlocked, the overlay takes the click"
    );

    let globals = LockGlobals::default();
    let status = Arc::new(LockStatus::default());
    {
        let client = f.client(id);
        let qh = client.qh.clone();
        let _registry = client.display.get_registry(&qh, globals.clone());
        client.connection.flush().unwrap();
    }
    f.roundtrip(id);

    let manager = globals.0.lock().unwrap().clone().unwrap();

    let (lock, lock_surface, lock_wl_surface, lock_viewport) = {
        let client = f.client(id);
        let qh = client.qh.clone();
        let lock = manager.lock(&qh, status.clone());
        let output = client.output("headless-1");
        let compositor = client.state.compositor.clone().unwrap();
        let viewporter = client.state.viewporter.clone().unwrap();
        let surface = compositor.create_surface(&qh, ());
        let viewport = viewporter.get_viewport(&surface, &qh, ());
        let lock_surface = lock.get_lock_surface(&surface, &output, &qh, ());
        client.connection.flush().unwrap();
        (lock, lock_surface, surface, viewport)
    };
    f.double_roundtrip(id);

    {
        let client = f.client(id);
        let qh = client.qh.clone();
        let spbm = client.state.spbm.clone().unwrap();
        let buffer = spbm.create_u32_rgba_buffer(0, u32::MAX, 0, u32::MAX, &qh, ());
        lock_viewport.set_destination(i32::from(OUT.0), i32::from(OUT.1));
        lock_wl_surface.attach(Some(&buffer), 0, 0);
        lock_wl_surface.commit();
        client.connection.flush().unwrap();
    }
    f.double_roundtrip(id);
    f.niri_complete_animations();
    f.double_roundtrip(id);

    assert!(f.niri().is_locked());

    let contents = f.niri().contents_under(point);
    let hit = contents.surface.as_ref().map(|(s, _)| s.clone());
    assert_ne!(
        hit,
        Some(overlay_surface),
        "locked, the click must not reach the above-fullscreen window"
    );

    drop(lock_surface);
    drop(lock);
}

fn layer_test(
    layer: zwlr_layer_shell_v1::Layer,
) -> (
    Fixture,
    client::ClientId,
    wayland_client::protocol::wl_surface::WlSurface,
    Point<f64, Logical>,
) {
    let config = Config::parse_mem(CONFIG).unwrap();
    let mut f = Fixture::with_config(config);
    f.niri_state().backend.headless().add_renderer().unwrap();
    f.add_output(1, OUT);
    let id = f.add_client();

    add_win(&mut f, id, "game", (u32::MAX, 0, 0), true);
    add_win(&mut f, id, "over", (0, 0, u32::MAX), false);

    let ls = f.client(id).create_layer(None, layer, "panel");
    let ls_surface = ls.surface.clone();
    ls.set_configure_props(LayerConfigureProps {
        anchor: Some(Anchor::Left | Anchor::Right | Anchor::Top | Anchor::Bottom),
        size: Some((0, 0)),
        ..Default::default()
    });
    ls.commit();
    f.roundtrip(id);

    let ls = f.client(id).layer(&ls_surface);
    ls.attach_new_buffer();
    ls.set_size(OUT.0, OUT.1);
    ls.ack_last_and_commit();
    f.double_roundtrip(id);

    f.niri().layout.focus_tiling();
    f.niri_complete_animations();
    f.double_roundtrip(id);

    (f, id, ls_surface, Point::from((100., 100.)))
}

/// An above-fullscreen window must stay below the `overlay` layer-shell layer for input, so that it
/// cannot cover a privileged panel there. It is expected (documented) to cover the `top` layer.
#[test]
fn egl_above_fullscreen_input_vs_layer_shell_layers() {
    let (mut f, _id, _ls_surface, point) = layer_test(zwlr_layer_shell_v1::Layer::Overlay);
    let hit = f.niri().contents_under(point).surface.map(|(s, _)| s);
    let hit = hit.expect("something must be hit");
    let hit_window = f
        .niri()
        .layout
        .active_workspace()
        .unwrap()
        .find_wl_surface(&hit)
        .map(|w| w.id().get());
    assert_eq!(
        hit_window, None,
        "the overlay layer must take input over an above-fullscreen window"
    );

    let (mut f, _id, _ls_surface, point) = layer_test(zwlr_layer_shell_v1::Layer::Top);
    let expected = f
        .niri()
        .layout
        .active_workspace()
        .unwrap()
        .window_under(point)
        .map(|(w, _)| w.id().get());
    let hit = f.niri().contents_under(point).surface.map(|(s, _)| s);
    let hit = hit.expect("something must be hit");
    let hit_window = f
        .niri()
        .layout
        .active_workspace()
        .unwrap()
        .find_wl_surface(&hit)
        .map(|w| w.id().get());
    assert!(expected.is_some());
    assert_eq!(
        hit_window, expected,
        "the above-fullscreen window is documented to cover the top layer"
    );
}

/// The rule is config-only, but it is keyed on the client-provided app-id, so any *other* client
/// that claims the same app-id inherits it.
#[test]
fn egl_second_client_can_claim_the_rule_by_app_id() {
    let config = Config::parse_mem(CONFIG).unwrap();
    let mut f = Fixture::with_config(config);
    f.niri_state().backend.headless().add_renderer().unwrap();
    f.add_output(1, OUT);

    let victim = f.add_client();
    add_win(&mut f, victim, "game", (u32::MAX, 0, 0), true);

    // A completely separate client, which merely says app-id = "over".
    let attacker = f.add_client();
    add_win(&mut f, attacker, "over", (0, 0, u32::MAX), false);

    f.niri().layout.focus_tiling();
    f.niri_complete_animations();
    f.double_roundtrip(victim);
    f.double_roundtrip(attacker);

    let (_size, pixels) = render(&mut f, RenderTarget::Output);
    assert!(
        count_color(&pixels, (0, 0, 255)) > 1000,
        "a different client claiming the same app-id is drawn over the fullscreen window"
    );

    let hit = f.niri().contents_under(Point::from((100., 100.))).surface;
    assert!(hit.is_some(), "and it takes the pointer input there");
}
