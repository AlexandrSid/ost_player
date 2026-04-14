mod defaults;
pub mod io;

use serde::{Deserialize, Serialize};
use serde_yaml::Value;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RepeatMode {
    Off,
    All,
    One,
}

impl Default for RepeatMode {
    fn default() -> Self {
        Self::Off
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettingsConfig {
    #[serde(default = "defaults::default_min_size_bytes")]
    pub min_size_bytes: u64,

    #[serde(default)]
    pub shuffle: bool,

    #[serde(default)]
    pub repeat: RepeatMode,

    #[serde(default = "defaults::default_supported_extensions")]
    pub supported_extensions: Vec<String>,

    /// Preserve unknown `settings.*` fields for forward compatibility.
    #[serde(flatten, default)]
    pub extra: BTreeMap<String, Value>,
}

impl Default for SettingsConfig {
    fn default() -> Self {
        defaults::default_settings()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HotkeysTimings {
    #[serde(default = "HotkeysTimings::default_hold_threshold_ms")]
    pub hold_threshold_ms: u64,

    #[serde(default = "HotkeysTimings::default_repeat_interval_ms")]
    pub repeat_interval_ms: u64,

    #[serde(default = "HotkeysTimings::default_seek_step_seconds")]
    pub seek_step_seconds: u64,
}

impl HotkeysTimings {
    fn default_hold_threshold_ms() -> u64 {
        300
    }
    fn default_repeat_interval_ms() -> u64 {
        250
    }
    fn default_seek_step_seconds() -> u64 {
        5
    }
}

impl Default for HotkeysTimings {
    fn default() -> Self {
        Self {
            hold_threshold_ms: Self::default_hold_threshold_ms(),
            repeat_interval_ms: Self::default_repeat_interval_ms(),
            seek_step_seconds: Self::default_seek_step_seconds(),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum HotkeyModifier {
    Ctrl,
    Alt,
    Shift,
    Win,
    LeftShift,
    RightShift,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum HotkeyKey {
    Up,
    Down,
    Left,
    Right,
    Space,
    S,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HotkeyChord {
    #[serde(default)]
    pub modifiers: Vec<HotkeyModifier>,
    pub key: HotkeyKey,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TapHoldBinding {
    pub chord: HotkeyChord,

    /// Optional. If present, holding the key repeats this action until release.
    #[serde(default)]
    pub hold: Option<HotkeyHoldAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum HotkeyHoldAction {
    /// Seek while held, repeating every `hotkeys.timings.repeat_interval_ms`.
    ///
    /// The seek amount per tick is `direction * hotkeys.timings.seek_step_seconds`.
    SeekStep { direction: i64 },
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HotkeysBindings {
    /// Toggle play/pause.
    #[serde(default)]
    pub play_pause: Option<HotkeyChord>,

    /// Next track (tap) / fast-forward (hold).
    #[serde(default)]
    pub next: Option<TapHoldBinding>,

    /// Previous track (tap) / rewind (hold).
    #[serde(default)]
    pub prev: Option<TapHoldBinding>,

    /// Cycle repeat mode (off -> all -> one -> off).
    #[serde(default)]
    pub repeat_toggle: Option<HotkeyChord>,

    /// Toggle shuffle (separate hotkey; "Up+Down" is not supported by RegisterHotKey).
    #[serde(default)]
    pub shuffle_toggle: Option<HotkeyChord>,

    /// Preserve unknown `hotkeys.bindings.*` fields for forward compatibility.
    #[serde(flatten, default)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HotkeysConfig {
    #[serde(default)]
    pub timings: HotkeysTimings,

    #[serde(default)]
    pub bindings: HotkeysBindings,
}

impl Default for HotkeysConfig {
    fn default() -> Self {
        defaults::default_hotkeys()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    /// For forward compatibility. Bump only for breaking schema changes.
    #[serde(default = "AppConfig::default_schema_version")]
    pub schema_version: u32,

    #[serde(default)]
    pub settings: SettingsConfig,

    /// Active folders used for indexing/playback (portable: absolute paths are expected).
    #[serde(default)]
    pub folders: Vec<String>,

    #[serde(default)]
    pub hotkeys: HotkeysConfig,

    /// Preserve unknown top-level fields when reading/writing.
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl AppConfig {
    fn default_schema_version() -> u32 {
        1
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.settings.supported_extensions.is_empty() {
            return Err("settings.supported_extensions must not be empty".to_string());
        }

        // Hotkeys timings: non-zero and within sane bounds to avoid hangs/spins.
        let t = &self.hotkeys.timings;
        if t.hold_threshold_ms == 0 {
            return Err("hotkeys.timings.hold_threshold_ms must be > 0".to_string());
        }
        if t.repeat_interval_ms == 0 {
            return Err("hotkeys.timings.repeat_interval_ms must be > 0".to_string());
        }
        if t.seek_step_seconds == 0 {
            return Err("hotkeys.timings.seek_step_seconds must be > 0".to_string());
        }

        // Reasonable UX ranges (portable app; avoid accidental values like 1ms or hours).
        if !(50..=5_000).contains(&t.hold_threshold_ms) {
            return Err("hotkeys.timings.hold_threshold_ms must be within 50..=5000".to_string());
        }
        if !(10..=5_000).contains(&t.repeat_interval_ms) {
            return Err("hotkeys.timings.repeat_interval_ms must be within 10..=5000".to_string());
        }
        if !(1..=3_600).contains(&t.seek_step_seconds) {
            return Err("hotkeys.timings.seek_step_seconds must be within 1..=3600".to_string());
        }

        Ok(())
    }

    pub fn normalized(mut self) -> Self {
        self.folders = dedup_keep_order(self.folders);
        self.settings.supported_extensions =
            dedup_keep_order(self.settings.supported_extensions.clone());
        self
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            schema_version: Self::default_schema_version(),
            settings: SettingsConfig::default(),
            folders: Vec::new(),
            hotkeys: HotkeysConfig::default(),
            extra: BTreeMap::new(),
        }
    }
}

fn dedup_keep_order(mut items: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::BTreeSet::new();
    items.retain(|v| seen.insert(v.clone()));
    items
}

