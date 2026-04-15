use ost_player::config::{AppConfig, HotkeyHoldAction, HotkeyKey, HotkeyModifier};
use ost_player::hotkeys::logic::{HotkeysEngine, KeyDirection, KeyEvent};
use ost_player::tui::action::Action;
use serde_yaml::Value;
use std::collections::{HashMap, HashSet};

fn parse_yaml<T: serde::de::DeserializeOwned>(s: &str) -> T {
    serde_yaml::from_str(s).expect("yaml should parse")
}

fn yaml_value(s: &str) -> Value {
    parse_yaml(s)
}

fn hs_mods(mods: &[HotkeyModifier]) -> HashSet<HotkeyModifier> {
    mods.iter().copied().collect()
}

#[test]
fn hotkeys_config_roundtrip_preserves_unknown_hotkeys_bindings_fields() {
    let raw = r#"
schema_version: 1
settings:
  supported_extensions: [mp3, ogg]
folders: []
hotkeys:
  timings:
    hold_threshold_ms: 333
    repeat_interval_ms: 222
    seek_step_seconds: 7
  bindings:
    play_pause: { modifiers: [ctrl, right_shift], key: up }
    next:
      chord: { modifiers: [ctrl, right_shift], key: right }
      hold: { action: seek_step, direction: 1 }
    future_binding: 123
"#;

    let cfg: AppConfig = parse_yaml(raw);
    assert_eq!(cfg.hotkeys.timings.hold_threshold_ms, 333);
    assert_eq!(cfg.hotkeys.timings.repeat_interval_ms, 222);
    assert_eq!(cfg.hotkeys.timings.seek_step_seconds, 7);
    assert!(cfg.hotkeys.bindings.extra.contains_key("future_binding"));

    let serialized = serde_yaml::to_string(&cfg).expect("should serialize");
    let v = yaml_value(&serialized);
    assert!(
        v.get("hotkeys")
            .and_then(|h| h.get("bindings"))
            .and_then(|b| b.get("future_binding"))
            .is_some(),
        "future_binding should survive roundtrip"
    );
}

#[test]
fn chord_matching_requires_all_required_modifiers_but_allows_extra_modifiers() {
    let cfg = AppConfig::default();
    let chord = cfg
        .hotkeys
        .bindings
        .play_pause
        .clone()
        .expect("default play_pause chord");

    let down = hs_mods(&[HotkeyModifier::Ctrl, HotkeyModifier::RightShift]);
    assert!(HotkeysEngine::chord_matches(&chord, HotkeyKey::Up, &down));

    let down_extra = hs_mods(&[
        HotkeyModifier::Ctrl,
        HotkeyModifier::RightShift,
        HotkeyModifier::Alt,
    ]);
    assert!(HotkeysEngine::chord_matches(
        &chord,
        HotkeyKey::Up,
        &down_extra
    ));

    let down_missing = hs_mods(&[HotkeyModifier::Ctrl]);
    assert!(!HotkeysEngine::chord_matches(
        &chord,
        HotkeyKey::Up,
        &down_missing
    ));
}

#[test]
fn tap_emits_tap_action_when_released_before_hold_threshold() {
    let cfg = AppConfig::default();
    let mut engine = HotkeysEngine::from_config(&cfg.hotkeys);
    assert!(engine.bindings_len() > 0);

    let mods = hs_mods(&[HotkeyModifier::Ctrl, HotkeyModifier::RightShift]);

    // Tap "next" chord key (Right arrow) quickly.
    let actions = engine.handle_event(KeyEvent {
        now_ms: 0,
        key: HotkeyKey::Right,
        direction: KeyDirection::Down,
        modifiers_down: mods.clone(),
    });
    assert!(
        actions.is_empty(),
        "no actions on keydown for tap/hold binding"
    );

    let actions = engine.handle_event(KeyEvent {
        now_ms: 100, // < default 300ms threshold
        key: HotkeyKey::Right,
        direction: KeyDirection::Up,
        modifiers_down: mods,
    });
    assert_eq!(actions, vec![Action::PlayerNext]);
}

#[test]
fn hold_emits_seek_steps_and_never_taps_after_hold_fires() {
    let cfg = AppConfig::default();
    let mut engine = HotkeysEngine::from_config(&cfg.hotkeys);

    let mods = hs_mods(&[HotkeyModifier::Ctrl, HotkeyModifier::RightShift]);

    // Hold "next" (Right arrow): should seek after threshold and repeat.
    let _ = engine.handle_event(KeyEvent {
        now_ms: 0,
        key: HotkeyKey::Right,
        direction: KeyDirection::Down,
        modifiers_down: mods.clone(),
    });

    // Provide modifiers snapshot keyed by main key for tick validation.
    let mut mods_map: HashMap<HotkeyKey, HashSet<HotkeyModifier>> = HashMap::new();
    mods_map.insert(HotkeyKey::Right, mods.clone());

    // Before threshold: no hold actions.
    let actions = engine.tick(299, &mods_map);
    assert!(actions.is_empty());

    // At/after threshold: hold action fires (seek +5s once).
    let actions = engine.tick(300, &mods_map);
    assert_eq!(actions, vec![Action::PlayerSeekRelativeSeconds(5)]);

    // Sparse tick: catch up repeats at 300, 550, 800 (3 total <= 800, but 300 already fired above).
    let actions = engine.tick(800, &mods_map);
    assert_eq!(
        actions,
        vec![
            Action::PlayerSeekRelativeSeconds(5),
            Action::PlayerSeekRelativeSeconds(5)
        ]
    );

    // Release after hold has fired: should not tap next.
    let actions = engine.handle_event(KeyEvent {
        now_ms: 900,
        key: HotkeyKey::Right,
        direction: KeyDirection::Up,
        modifiers_down: mods,
    });
    assert!(actions.is_empty());
}

#[test]
fn hold_action_serializes_as_tagged_enum() {
    let a = HotkeyHoldAction::SeekStep { direction: -1 };
    let s = serde_yaml::to_string(&a).expect("serialize hold action");
    let v = yaml_value(&s);
    assert_eq!(v.get("action").and_then(Value::as_str), Some("seek_step"));
    assert_eq!(v.get("direction").and_then(Value::as_i64), Some(-1));
}

#[test]
fn tap_is_not_emitted_if_released_after_hold_threshold_even_if_tick_never_ran() {
    let cfg = AppConfig::default();
    let mut engine = HotkeysEngine::from_config(&cfg.hotkeys);
    let mods = hs_mods(&[HotkeyModifier::Ctrl, HotkeyModifier::RightShift]);

    let _ = engine.handle_event(KeyEvent {
        now_ms: 0,
        key: HotkeyKey::Right,
        direction: KeyDirection::Down,
        modifiers_down: mods.clone(),
    });

    // No tick() calls here. Release after threshold.
    let actions = engine.handle_event(KeyEvent {
        now_ms: cfg.hotkeys.timings.hold_threshold_ms,
        key: HotkeyKey::Right,
        direction: KeyDirection::Up,
        modifiers_down: mods,
    });
    assert!(
        actions.is_empty(),
        "tap must not be emitted when held >= threshold"
    );
}

#[test]
fn app_config_validate_rejects_bad_hotkey_timings() {
    let mut cfg = AppConfig::default();
    cfg.settings.supported_extensions = vec!["mp3".to_string()];

    cfg.hotkeys.timings.hold_threshold_ms = 0;
    assert!(cfg.validate().unwrap_err().contains("hold_threshold_ms"));

    cfg.hotkeys.timings.hold_threshold_ms = 300;
    cfg.hotkeys.timings.repeat_interval_ms = 0;
    assert!(cfg.validate().unwrap_err().contains("repeat_interval_ms"));

    cfg.hotkeys.timings.repeat_interval_ms = 250;
    cfg.hotkeys.timings.seek_step_seconds = 0;
    assert!(cfg.validate().unwrap_err().contains("seek_step_seconds"));

    cfg.hotkeys.timings.seek_step_seconds = 5;
    cfg.hotkeys.timings.hold_threshold_ms = 10; // too small per sane range
    assert!(cfg.validate().unwrap_err().contains("within 50..=5000"));
}
