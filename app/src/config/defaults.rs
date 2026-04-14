use super::{HotkeysConfig, HotkeysTimings, RepeatMode, SettingsConfig};
use super::{HotkeyChord, HotkeyKey, HotkeyModifier, HotkeysBindings, HotkeyHoldAction, TapHoldBinding};

pub fn default_settings() -> SettingsConfig {
    SettingsConfig {
        min_size_bytes: default_min_size_bytes(),
        shuffle: false,
        repeat: RepeatMode::Off,
        supported_extensions: default_supported_extensions(),
        extra: Default::default(),
    }
}

pub fn default_min_size_bytes() -> u64 {
    1_000_000
}

pub fn default_supported_extensions() -> Vec<String> {
    vec!["mp3".to_string(), "ogg".to_string()]
}

pub fn default_hotkeys() -> HotkeysConfig {
    HotkeysConfig {
        timings: HotkeysTimings::default(),
        bindings: HotkeysBindings {
            play_pause: Some(HotkeyChord {
                modifiers: vec![HotkeyModifier::Ctrl, HotkeyModifier::RightShift],
                key: HotkeyKey::Up,
            }),
            repeat_toggle: Some(HotkeyChord {
                modifiers: vec![HotkeyModifier::Ctrl, HotkeyModifier::RightShift],
                key: HotkeyKey::Down,
            }),
            next: Some(TapHoldBinding {
                chord: HotkeyChord {
                    modifiers: vec![HotkeyModifier::Ctrl, HotkeyModifier::RightShift],
                    key: HotkeyKey::Right,
                },
                hold: Some(HotkeyHoldAction::SeekStep { direction: 1 }),
            }),
            prev: Some(TapHoldBinding {
                chord: HotkeyChord {
                    modifiers: vec![HotkeyModifier::Ctrl, HotkeyModifier::RightShift],
                    key: HotkeyKey::Left,
                },
                hold: Some(HotkeyHoldAction::SeekStep { direction: -1 }),
            }),
            shuffle_toggle: Some(HotkeyChord {
                modifiers: vec![HotkeyModifier::Ctrl, HotkeyModifier::RightShift],
                key: HotkeyKey::S,
            }),
            extra: Default::default(),
        },
    }
}

