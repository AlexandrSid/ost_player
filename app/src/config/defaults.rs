use super::{AudioConfig, HotkeysConfig, HotkeysTimings, RepeatMode, SettingsConfig};
use super::{
    HotkeyChord, HotkeyHoldAction, HotkeyKey, HotkeyModifier, HotkeysBindings, TapHoldBinding,
};

pub fn default_settings() -> SettingsConfig {
    let min_size_kb = default_min_size_kb();
    let min_size_bytes = min_size_kb
        .checked_mul(1024)
        .expect("default_min_size_kb must not overflow when converting to bytes");
    SettingsConfig {
        min_size_kb,
        min_size_bytes,
        min_size_custom_kb_min: default_min_size_custom_kb_min(),
        min_size_custom_kb_max: default_min_size_custom_kb_max(),
        shuffle: false,
        repeat: RepeatMode::Off,
        supported_extensions: default_supported_extensions(),
        extra: Default::default(),
    }
}

pub fn default_min_size_kb() -> u64 {
    1024
}

pub fn default_min_size_custom_kb_min() -> u32 {
    10
}

pub fn default_min_size_custom_kb_max() -> u32 {
    10_000
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
            volume_up: default_hotkey_volume_up(),
            volume_down: default_hotkey_volume_down(),
            extra: Default::default(),
        },
    }
}

pub fn default_hotkey_volume_up() -> Option<HotkeyChord> {
    Some(HotkeyChord {
        modifiers: vec![HotkeyModifier::LeftCtrl, HotkeyModifier::RightShift],
        key: HotkeyKey::PageUp,
    })
}

pub fn default_hotkey_volume_down() -> Option<HotkeyChord> {
    Some(HotkeyChord {
        modifiers: vec![HotkeyModifier::LeftCtrl, HotkeyModifier::RightShift],
        key: HotkeyKey::PageDown,
    })
}

pub fn default_audio() -> AudioConfig {
    AudioConfig {
        volume_default_percent: default_volume_default_percent(),
        volume_available_percent: default_volume_available_percent(),
        extra: Default::default(),
    }
}

pub fn default_volume_default_percent() -> u8 {
    75
}

pub fn default_volume_available_percent() -> Vec<u8> {
    let mut v = vec![0, 1, 2, 3, 5, 7, 10, 13, 16, 20];
    v.extend((25..=100).step_by(5));
    v
}

pub fn default_logging_retention_days() -> u64 {
    31
}
