use niri_config::{Action, Config};

#[test]
fn press_and_release_coexist_and_have_phase_specific_defaults() {
    let config = Config::parse_mem(
        r#"binds {
            Mod+Space { spawn "start"; }
            Mod+Space on-release=true { spawn "stop"; }
            Mod+Return on-release=false { spawn "press"; }
        }"#,
    )
    .unwrap();
    let binds = &config.binds.0;
    assert_eq!(binds.len(), 3);
    assert!(!binds[0].on_release && binds[0].repeat);
    assert!(binds[1].on_release && !binds[1].repeat);
    assert!(!binds[2].on_release && binds[2].repeat);
}

#[test]
fn reject_duplicates_within_each_phase() {
    for phase in [false, true] {
        assert!(Config::parse_mem(&format!(
            "binds {{ Mod+Space on-release={phase} {{ spawn \"one\"; }}; \
             Mod+Space on-release={phase} {{ spawn \"two\"; }}; }}"
        ))
        .is_err());
    }
}

#[test]
fn release_bindings_cannot_repeat_or_use_non_keyboard_triggers() {
    assert!(Config::parse_mem(
        r#"binds { Mod+Space on-release=true repeat=true { spawn "stop"; }; }"#
    )
    .is_err());
    assert!(Config::parse_mem(
        r#"binds { Mod+Space repeat=false on-release=true { spawn "stop"; }; }"#
    )
    .is_ok());
    for trigger in [
        "MouseLeft",
        "MouseRight",
        "WheelScrollDown",
        "TouchpadScrollUp",
        "TabletStylusButton1",
    ] {
        assert!(Config::parse_mem(&format!(
            "binds {{ Mod+{trigger} on-release=true {{ spawn \"stop\"; }}; }}"
        ))
        .is_err());
    }
}

#[test]
fn includes_override_only_the_same_phase() {
    let path = std::env::temp_dir().join(format!("niri-release-binds-{}.kdl", std::process::id()));
    std::fs::write(
        &path,
        r#"binds {
            Mod+Space { spawn "old-start"; }
            Mod+Space on-release=true { spawn "old-stop"; }
        }"#,
    )
    .unwrap();
    for phase in [false, true] {
        let text = format!(
            "include {:?}\nbinds {{ Mod+Space on-release={phase} {{ spawn \"new\"; }}; }}",
            path.to_str().unwrap(),
        );
        let config = Config::parse_mem(&text).unwrap();
        assert_eq!(config.binds.0.len(), 2);
        for bind in &config.binds.0 {
            let expected = if bind.on_release == phase {
                "new"
            } else if bind.on_release {
                "old-stop"
            } else {
                "old-start"
            };
            assert_eq!(bind.action, Action::Spawn(vec![expected.into()]));
        }
    }
    std::fs::remove_file(path).unwrap();
}
