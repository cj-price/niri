use std::path::PathBuf;

use client::ClientId;
use niri_config::Config;
use smithay::backend::allocator::Fourcc;
use smithay::utils::{Physical, Scale, Size, Transform};

use super::*;
use crate::render_helpers::{render_to_vec, RenderCtx, RenderTarget};

const OUTPUT_SIZE: (u16, u16) = (1920, 1080);
const OVERLAY_SIZE: (u16, u16) = (400, 200);

const CONFIG_BASE: &str = r##"
layout {
    gaps 0
    border { off; }
    focus-ring { off; }
}

hotkey-overlay {
    skip-at-startup
}
"##;

const RULE_ON: &str = r##"
window-rule {
    match app-id="^overlay$"

    open-floating true
    default-floating-position x=40 y=40 relative-to="top-right"
    above-fullscreen true
}
"##;

const RULE_OFF: &str = r##"
window-rule {
    match app-id="^overlay$"

    open-floating true
    default-floating-position x=40 y=40 relative-to="top-right"
}
"##;

fn set_up(
    rule: &str,
) -> (
    Fixture,
    ClientId,
    wayland_client::protocol::wl_surface::WlSurface,
) {
    let config = Config::parse_mem(&format!("{CONFIG_BASE}{rule}")).unwrap();

    let mut f = Fixture::with_config(config);
    f.niri_state().backend.headless().add_renderer().unwrap();
    f.add_output(1, OUTPUT_SIZE);

    let id = f.add_client();

    let game = f.client(id).create_window();
    let game_surface = game.surface.clone();
    game.set_app_id("game");
    game.set_fullscreen(None);
    game.commit();
    f.roundtrip(id);

    let game = f.client(id).window(&game_surface);
    game.attach_new_buffer_with_color(u32::MAX, 0, 0, u32::MAX);
    game.set_size(OUTPUT_SIZE.0, OUTPUT_SIZE.1);
    game.ack_last_and_commit();
    f.double_roundtrip(id);

    let overlay = f.client(id).create_window();
    let overlay_surface = overlay.surface.clone();
    overlay.set_app_id("overlay");
    overlay.commit();
    f.roundtrip(id);

    let overlay = f.client(id).window(&overlay_surface);
    overlay.attach_new_buffer_with_color(0, 0, u32::MAX, u32::MAX);
    overlay.set_size(OVERLAY_SIZE.0, OVERLAY_SIZE.1);
    overlay.ack_last_and_commit();
    f.double_roundtrip(id);

    f.niri().layout.focus_tiling();
    f.niri_complete_animations();
    f.double_roundtrip(id);

    (f, id, overlay_surface)
}

fn overlay_rect(f: &mut Fixture) -> (f64, f64, f64, f64) {
    let ws = f.niri().layout.active_workspace().unwrap();
    let (tile, pos, _) = ws
        .tiles_with_render_positions()
        .find(|(tile, _, _)| tile.tile_size().w == f64::from(OVERLAY_SIZE.0))
        .unwrap();
    let size = tile.tile_size();
    (pos.x, pos.y, size.w, size.h)
}

fn render_output(f: &mut Fixture) -> (Size<i32, Physical>, Vec<u8>) {
    let output = f.niri_output(1);
    let size = output.current_mode().unwrap().size;
    let scale = Scale::from(output.current_scale().fractional_scale());

    let state = f.niri_state();
    state.niri.update_render_elements(Some(&output));

    let niri = &state.niri;
    let pixels = state
        .backend
        .with_primary_renderer(|renderer| {
            let ctx = RenderCtx {
                renderer,
                target: RenderTarget::Output,
                xray: None,
            };
            let elements = niri.render_to_vec(ctx, &output, false);
            let elements = elements.iter().rev();
            render_to_vec(
                renderer,
                size,
                scale,
                Transform::Normal,
                Fourcc::Abgr8888,
                elements,
            )
            .unwrap()
        })
        .unwrap();

    (size, pixels)
}

fn pixel_at(size: Size<i32, Physical>, pixels: &[u8], x: f64, y: f64) -> (u8, u8, u8) {
    let idx = ((y as i32 * size.w + x as i32) * 4) as usize;
    (pixels[idx], pixels[idx + 1], pixels[idx + 2])
}

fn save_png(name: &str, size: Size<i32, Physical>, pixels: &[u8]) {
    let Some(dir) = std::env::var_os("NIRI_TEST_SCREENSHOT_DIR") else {
        return;
    };

    let path = PathBuf::from(dir).join(format!("{name}.png"));
    let file = std::fs::File::create(&path).unwrap();
    let mut encoder = png::Encoder::new(file, size.w as u32, size.h as u32);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder
        .write_header()
        .unwrap()
        .write_image_data(pixels)
        .unwrap();

    eprintln!("wrote {}", path.display());
}

#[test]
fn egl_overlay_is_drawn_over_the_fullscreen_window() {
    let (mut f, _id, _) = set_up(RULE_ON);

    let (x, y, w, h) = overlay_rect(&mut f);
    let (size, pixels) = render_output(&mut f);
    save_png("above-fullscreen-on", size, &pixels);

    let center = pixel_at(size, &pixels, x + w / 2., y + h / 2.);
    assert_eq!(center, (0, 0, 255), "overlay center should be blue");

    let outside = pixel_at(size, &pixels, x + w / 2., y + h + 10.);
    assert_eq!(outside, (255, 0, 0), "outside the overlay should be red");
}

#[test]
fn egl_overlay_is_hidden_without_the_rule() {
    let (mut f, _id, _) = set_up(RULE_OFF);

    let (x, y, w, h) = overlay_rect(&mut f);
    let (size, pixels) = render_output(&mut f);
    save_png("above-fullscreen-off", size, &pixels);

    let center = pixel_at(size, &pixels, x + w / 2., y + h / 2.);
    assert_eq!(
        center,
        (255, 0, 0),
        "overlay center should be covered by red"
    );
}

#[test]
fn egl_above_fullscreen_rule_updates_when_app_id_changes() {
    let (mut f, id, surface) = set_up(RULE_ON);
    let (x, y, w, h) = overlay_rect(&mut f);

    for (app_id, expected) in [("other", (255, 0, 0)), ("overlay", (0, 0, 255))] {
        let window = f.client(id).window(&surface);
        window.set_app_id(app_id);
        window.commit();
        f.double_roundtrip(id);
        f.niri_complete_animations();
        let (size, pixels) = render_output(&mut f);
        assert_eq!(pixel_at(size, &pixels, x + w / 2., y + h / 2.), expected);
    }
}

#[test]
fn egl_later_false_rule_overrides_above_fullscreen() {
    let rule =
        format!("{RULE_ON}\nwindow-rule {{ match app-id=\"^overlay$\"; above-fullscreen false; }}");
    let (mut f, _id, _) = set_up(&rule);
    let (x, y, w, h) = overlay_rect(&mut f);
    let (size, pixels) = render_output(&mut f);
    assert_eq!(pixel_at(size, &pixels, x + w / 2., y + h / 2.), (255, 0, 0));
}

#[test]
fn egl_above_fullscreen_close_animation_stays_visible() {
    for (rule, expected) in [(RULE_ON, (0, 0, 255)), (RULE_OFF, (255, 0, 0))] {
        let (mut f, id, surface) = set_up(rule);
        let (x, y, w, h) = overlay_rect(&mut f);
        // Freeze time so rendering speed cannot finish the closing animation.
        f.niri().clock.set_rate(0.0);
        // Populate the render state used to take the unmap snapshot.
        render_output(&mut f);
        let window = f.client(id).window(&surface);
        window.attach_null();
        window.commit();
        f.double_roundtrip(id);

        let (size, pixels) = render_output(&mut f);
        assert_eq!(pixel_at(size, &pixels, x + w / 2., y + h / 2.), expected);

        f.niri_complete_animations();
        let (size, pixels) = render_output(&mut f);
        assert_eq!(pixel_at(size, &pixels, x + w / 2., y + h / 2.), (255, 0, 0));
    }
}
