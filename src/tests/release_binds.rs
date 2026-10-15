use std::collections::{HashMap, HashSet};
use std::time::Duration;

use calloop::timer::{TimeoutAction, Timer};
use niri_config::Config;
use smithay::backend::input::{
    Device, DeviceCapability, Event, InputBackend, InputEvent, InputTime, KeyState,
    KeyboardKeyEvent, Keycode, UnusedEvent,
};
use smithay::output::Output;

use super::Fixture;
use crate::input::backend_ext::NiriInputDevice;
use crate::niri::State;

#[derive(Clone, PartialEq, Eq, Hash)]
struct Keyboard(&'static str);

impl Device for Keyboard {
    fn id(&self) -> String {
        self.0.into()
    }
    fn name(&self) -> String {
        self.id()
    }
    fn has_capability(&self, capability: DeviceCapability) -> bool {
        capability == DeviceCapability::Keyboard
    }
    fn usb_id(&self) -> Option<(u32, u32)> {
        None
    }
    fn syspath(&self) -> Option<std::path::PathBuf> {
        None
    }
}

impl NiriInputDevice for Keyboard {
    fn output(&self, _: &State) -> Option<Output> {
        None
    }
}

struct KeyboardBackend;
struct KeyEvent {
    device: Keyboard,
    code: u32,
    state: KeyState,
}

impl Event<KeyboardBackend> for KeyEvent {
    fn time(&self) -> InputTime {
        InputTime::from_millis(1)
    }
    fn device(&self) -> Keyboard {
        self.device.clone()
    }
}

impl KeyboardKeyEvent<KeyboardBackend> for KeyEvent {
    fn key_code(&self) -> Keycode {
        Keycode::from(self.code)
    }
    fn state(&self) -> KeyState {
        self.state
    }
    fn count(&self) -> u32 {
        u32::from(self.state == KeyState::Pressed)
    }
}

impl InputBackend for KeyboardBackend {
    type Device = Keyboard;
    type KeyboardKeyEvent = KeyEvent;
    type PointerAxisEvent = UnusedEvent;
    type PointerButtonEvent = UnusedEvent;
    type PointerMotionEvent = UnusedEvent;
    type PointerMotionAbsoluteEvent = UnusedEvent;
    type GestureSwipeBeginEvent = UnusedEvent;
    type GestureSwipeUpdateEvent = UnusedEvent;
    type GestureSwipeEndEvent = UnusedEvent;
    type GesturePinchBeginEvent = UnusedEvent;
    type GesturePinchUpdateEvent = UnusedEvent;
    type GesturePinchEndEvent = UnusedEvent;
    type GestureHoldBeginEvent = UnusedEvent;
    type GestureHoldEndEvent = UnusedEvent;
    type TouchDownEvent = UnusedEvent;
    type TouchUpEvent = UnusedEvent;
    type TouchMotionEvent = UnusedEvent;
    type TouchCancelEvent = UnusedEvent;
    type TouchFrameEvent = UnusedEvent;
    type TabletToolAxisEvent = UnusedEvent;
    type TabletToolProximityEvent = UnusedEvent;
    type TabletToolTipEvent = UnusedEvent;
    type TabletToolButtonEvent = UnusedEvent;
    type SwitchToggleEvent = UnusedEvent;
    type SpecialEvent = UnusedEvent;
}

fn key(f: &mut Fixture, code: u32, state: KeyState) {
    f.niri_state()
        .process_input_event::<KeyboardBackend>(InputEvent::Keyboard {
            event: KeyEvent {
                device: Keyboard("test-keyboard"),
                code,
                state,
            },
        });
}

#[test]
fn keyboard_dispatch_executes_both_phases_with_modifier_released_first() {
    let config = Config::parse_mem(
        r#"binds {
        Mod+Space repeat=false { debug-toggle-opaque-regions; }
        Mod+Space on-release=true { debug-toggle-opaque-regions; }
    }"#,
    )
    .unwrap();
    let mut f = Fixture::with_config(config);
    key(&mut f, 133, KeyState::Pressed); // Super_L
    key(&mut f, 65, KeyState::Pressed); // Space
    assert!(f.niri().debug_draw_opaque_regions);
    assert!(f.niri().bind_repeat_timer.is_none());
    key(&mut f, 133, KeyState::Released);
    assert!(f.niri().debug_draw_opaque_regions);
    key(&mut f, 65, KeyState::Released);
    assert!(!f.niri().debug_draw_opaque_regions);
    assert!(f.niri().bind_repeat_timer.is_none());
    assert!(f.niri().release_binds["test-keyboard"].is_empty());
    assert!(f.niri().suppressed_keys["test-keyboard"].is_empty());
}

#[test]
fn confirmation_dialog_press_does_not_arm_release() {
    let config = Config::parse_mem(
        r#"binds {
        Space on-release=true { debug-toggle-opaque-regions; }
    }"#,
    )
    .unwrap();
    let mut f = Fixture::with_config(config);
    f.niri().exit_confirm_dialog.show();
    key(&mut f, 65, KeyState::Pressed);
    key(&mut f, 65, KeyState::Released);
    assert!(!f.niri().debug_draw_opaque_regions);
    assert!(f.niri().release_binds.values().all(HashMap::is_empty));
    assert!(f.niri().suppressed_keys.values().all(HashSet::is_empty));
}

#[test]
fn device_removal_cancels_armed_release_and_repeat() {
    let config = Config::parse_mem(
        r#"binds {
        Space { debug-toggle-opaque-regions; }
        Space on-release=true { debug-toggle-opaque-regions; }
    }"#,
    )
    .unwrap();
    let mut f = Fixture::with_config(config);
    key(&mut f, 65, KeyState::Pressed);
    assert!(f.niri().debug_draw_opaque_regions);
    assert!(f.niri().bind_repeat_timer.is_some());
    f.niri_state()
        .process_input_event::<KeyboardBackend>(InputEvent::DeviceRemoved {
            device: Keyboard("test-keyboard"),
        });
    assert!(f.niri().release_binds.is_empty());
    assert!(f.niri().suppressed_keys.is_empty());
    assert!(f.niri().bind_repeat_timer.is_none());
    key(&mut f, 65, KeyState::Released);
    assert!(f.niri().debug_draw_opaque_regions);
}

fn config() -> Config {
    Config::parse_mem(
        r#"binds {
            Mod+Space cooldown-ms=10000 { close-window; }
            Mod+Space on-release=true cooldown-ms=10000 { close-window; }
        }"#,
    )
    .unwrap()
}

#[test]
fn press_cooldown_does_not_suppress_release_cooldown() {
    let config = config();
    let press = config.binds.0[0].clone();
    let release = config.binds.0[1].clone();
    let mut f = Fixture::with_config(config);
    f.niri_state().handle_bind(press.clone());
    f.niri_state().handle_bind(release.clone());
    assert_eq!(f.niri().bind_cooldown_timers.len(), 2);
    assert!(f
        .niri()
        .bind_cooldown_timers
        .contains_key(&(press.key, false)));
    assert!(f
        .niri()
        .bind_cooldown_timers
        .contains_key(&(release.key, true)));
    // Repeated handling during cooldown must not add more timers.
    f.niri_state().handle_bind(press);
    f.niri_state().handle_bind(release);
    assert_eq!(f.niri().bind_cooldown_timers.len(), 2);
}

#[test]
fn input_loss_cancels_pending_actions_suppression_and_repeat() {
    let config = config();
    let release = config.binds.0[1].clone();
    let mut f = Fixture::with_config(config);
    let key = Keycode::from(65u32);
    let niri = f.niri();
    niri.release_binds
        .insert("keyboard".into(), HashMap::from([(key, release)]));
    niri.suppressed_keys
        .insert("keyboard".into(), HashSet::from([key]));
    niri.bind_repeat_timer = Some(
        niri.event_loop
            .insert_source(Timer::from_duration(Duration::from_secs(60)), |_, _, _| {
                TimeoutAction::Drop
            })
            .unwrap(),
    );
    niri.cancel_keyboard_binds();
    assert!(niri.release_binds.is_empty());
    assert!(niri.suppressed_keys.is_empty());
    assert!(niri.bind_repeat_timer.is_none());
    assert!(niri.bind_cooldown_timers.is_empty());
    // Cancellation is idempotent and does not invoke the cooldown-bearing action.
    niri.cancel_keyboard_binds();
    assert!(niri.bind_cooldown_timers.is_empty());
}
