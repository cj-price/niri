use super::*;
use crate::animation::Clock;

struct Fixture {
    binds: Binds,
    suppressed: HashMap<&'static str, HashSet<Keycode>>,
    pending: HashMap<&'static str, HashMap<Keycode, Bind>>,
    screenshot: ScreenshotUi,
    inhibited: bool,
    locked: bool,
}

impl Fixture {
    fn new(binds: &str) -> Self {
        Self {
            binds: Config::parse_mem(&format!("binds {{\n{binds}\n}}"))
                .unwrap()
                .binds,
            suppressed: HashMap::new(),
            pending: HashMap::new(),
            screenshot: ScreenshotUi::new(Clock::default(), Default::default()),
            inhibited: false,
            locked: false,
        }
    }

    fn event(
        &mut self,
        device: &'static str,
        code: u32,
        sym: Keysym,
        pressed: bool,
        logo: bool,
    ) -> FilterResult<Option<Bind>> {
        should_intercept_key(
            self.suppressed.entry(device).or_default(),
            self.pending.entry(device).or_default(),
            self.binds.0.iter(),
            ModKey::Super,
            Keycode::from(code),
            sym,
            Some(sym),
            pressed,
            ModifiersState {
                logo,
                ..Default::default()
            },
            &self.screenshot,
            false,
            self.inhibited,
            self.locked,
        )
    }

    fn space(&mut self, pressed: bool, logo: bool) -> FilterResult<Option<Bind>> {
        self.event("keyboard", 65, Keysym::space, pressed, logo)
    }
}

#[track_caller]
fn assert_spawn(result: FilterResult<Option<Bind>>, command: &str, on_release: bool) {
    let FilterResult::Intercept(Some(bind)) = result else {
        panic!("expected spawn {command}");
    };
    assert_eq!(bind.action, Action::Spawn(vec![command.into()]));
    assert_eq!(bind.on_release, on_release);
    if on_release {
        assert!(!bind.repeat);
    }
}

#[test]
fn paired_bindings_fire_once_in_either_release_order() {
    for release_modifier_first in [false, true] {
        let mut f = Fixture::new(
            r#"Mod+Space repeat=false { spawn "start"; }
               Mod+Space on-release=true { spawn "stop"; }"#,
        );
        assert_spawn(f.space(true, true), "start", false);
        if release_modifier_first {
            assert!(matches!(
                f.event("keyboard", 133, Keysym::Super_L, false, false),
                FilterResult::Forward
            ));
        }
        assert_spawn(f.space(false, !release_modifier_first), "stop", true);
        assert!(matches!(f.space(false, false), FilterResult::Forward));
        assert!(f.pending["keyboard"].is_empty());
        assert!(f.suppressed["keyboard"].is_empty());
    }
}

#[test]
fn release_only_reserves_press_and_does_not_fire_without_matching_press() {
    let mut f = Fixture::new(r#"Mod+Space on-release=true { spawn "stop"; }"#);
    assert!(matches!(f.space(false, true), FilterResult::Forward));
    // Press without Mod, then add Mod before release: must not activate.
    assert!(matches!(f.space(true, false), FilterResult::Forward));
    assert!(matches!(f.space(false, true), FilterResult::Forward));
    assert!(matches!(f.space(true, true), FilterResult::Intercept(None)));
    assert_spawn(f.space(false, false), "stop", true);
}

#[test]
fn unrelated_keys_layout_changes_and_duplicate_presses_do_not_disarm() {
    let mut f = Fixture::new(
        r#"Mod+Space { spawn "start"; }
           Mod+Space on-release=true { spawn "stop"; }"#,
    );
    assert_spawn(f.space(true, true), "start", false);
    assert!(matches!(f.space(true, true), FilterResult::Intercept(None)));
    assert!(matches!(
        f.event("keyboard", 38, Keysym::a, true, false),
        FilterResult::Forward
    ));
    assert!(matches!(
        f.event("keyboard", 38, Keysym::a, false, false),
        FilterResult::Forward
    ));
    // The physical key, not the release-time keysym, identifies the armed action.
    assert_spawn(
        f.event("keyboard", 65, Keysym::b, false, false),
        "stop",
        true,
    );
}

#[test]
fn independent_devices_and_simultaneous_holds() {
    let mut f = Fixture::new(
        r#"Mod+Space on-release=true { spawn "space"; }
           Mod+a on-release=true { spawn "a"; }"#,
    );
    assert!(matches!(f.space(true, true), FilterResult::Intercept(None)));
    // Another device's release must neither execute nor consume this binding.
    assert!(matches!(
        f.event("other", 65, Keysym::space, false, false),
        FilterResult::Forward
    ));
    assert!(matches!(
        f.event("other", 65, Keysym::space, true, true),
        FilterResult::Intercept(None)
    ));
    assert!(matches!(
        f.event("keyboard", 38, Keysym::a, true, true),
        FilterResult::Intercept(None)
    ));
    assert_spawn(f.space(false, false), "space", true);
    assert_spawn(f.event("keyboard", 38, Keysym::a, false, false), "a", true);
    assert_spawn(
        f.event("other", 65, Keysym::space, false, false),
        "space",
        true,
    );
}

#[test]
fn inhibition_is_decided_on_press() {
    let mut f = Fixture::new(r#"Mod+Space on-release=true { spawn "stop"; }"#);
    f.inhibited = true;
    assert!(matches!(f.space(true, true), FilterResult::Forward));
    f.inhibited = false;
    assert!(matches!(f.space(false, true), FilterResult::Forward));
    assert!(matches!(f.space(true, true), FilterResult::Intercept(None)));
    f.inhibited = true;
    assert_spawn(f.space(false, true), "stop", true);

    let mut f =
        Fixture::new(r#"Mod+Space on-release=true allow-inhibiting=false { spawn "stop"; }"#);
    f.inhibited = true;
    assert!(matches!(f.space(true, true), FilterResult::Intercept(None)));
    assert_spawn(f.space(false, true), "stop", true);
}

#[test]
fn locked_actions_are_not_armed_and_execution_rechecks_lock_policy() {
    let mut f = Fixture::new(r#"Mod+Space on-release=true { spawn "stop"; }"#);
    f.locked = true;
    assert!(matches!(f.space(true, true), FilterResult::Forward));
    f.locked = false;
    assert!(matches!(f.space(false, true), FilterResult::Forward));
    assert!(matches!(f.space(true, true), FilterResult::Intercept(None)));
    f.locked = true;
    assert!(matches!(
        f.space(false, true),
        FilterResult::Intercept(None)
    ));
    assert!(f.pending["keyboard"].is_empty());

    let mut f =
        Fixture::new(r#"Mod+Space on-release=true allow-when-locked=true { spawn "stop"; }"#);
    f.locked = true;
    assert!(matches!(f.space(true, true), FilterResult::Intercept(None)));
    assert_spawn(f.space(false, true), "stop", true);
}

#[test]
fn config_reload_preserves_the_original_release_snapshot() {
    let mut f = Fixture::new(r#"Mod+Space on-release=true { spawn "old"; }"#);
    assert!(matches!(f.space(true, true), FilterResult::Intercept(None)));
    f.binds = Config::parse_mem(r#"binds { Mod+Space on-release=true { spawn "new"; }; }"#)
        .unwrap()
        .binds;
    assert_spawn(f.space(false, true), "old", true);
    assert!(matches!(f.space(true, true), FilterResult::Intercept(None)));
    f.binds.0.clear();
    assert_spawn(f.space(false, true), "new", true);
}

#[test]
fn cancellation_keeps_suppression_but_never_replays_actions() {
    let mut f = Fixture::new(r#"Mod+Space on-release=true { spawn "stop"; }"#);
    assert!(matches!(f.space(true, true), FilterResult::Intercept(None)));
    // Locking cancels pending actions but retains suppression until release.
    f.pending.clear();
    assert!(matches!(
        f.space(false, true),
        FilterResult::Intercept(None)
    ));
    assert!(matches!(f.space(false, true), FilterResult::Forward));
}

#[test]
fn inhibited_configured_suspend_does_not_override_uninhibitable_release() {
    let mut f = Fixture::new(
        r#"Mod+Space { suspend; }
           Mod+Space on-release=true allow-inhibiting=false { spawn "stop"; }"#,
    );
    f.inhibited = true;
    assert!(matches!(f.space(true, true), FilterResult::Intercept(None)));
    assert_spawn(f.space(false, true), "stop", true);
}

#[test]
fn hardcoded_power_and_vt_shortcuts_do_not_arm_release_bindings() {
    let mut f = Fixture::new(
        r#"XF86PowerOff on-release=true { spawn "power"; }
           XF86Switch_VT_1 on-release=true { spawn "vt"; }"#,
    );
    for (code, sym) in [(124, Keysym::XF86_PowerOff), (67, Keysym::XF86_Switch_VT_1)] {
        assert!(matches!(
            f.event("keyboard", code, sym, true, false),
            FilterResult::Intercept(Some(_))
        ));
        assert!(f.pending["keyboard"].is_empty());
        assert!(matches!(
            f.event("keyboard", code, sym, false, false),
            FilterResult::Intercept(None)
        ));
    }
}
