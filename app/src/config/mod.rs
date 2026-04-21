mod defaults;
pub mod io;

use serde::de;
use serde::{Deserialize, Serialize};
use serde_yaml::Value;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum MainMenuCommand {
    AddFolder,
    RemoveSelectedFolder,
    CycleSelectedFolderScanDepth,
    SetSelectedFolderCustomMinSizeKb,
    Play,
    Settings,
    Playlists,
    RescanLibrary,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MainMenuNumericBinding {
    pub key: u8,
    pub command: MainMenuCommand,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TuiConfig {
    /// Optional numeric mapping for Main Menu digits (1..9).
    ///
    /// If present, digits are handled by this mapping and UI renders mapped commands in 1..9
    /// order (gaps allowed). If absent, the hardcoded default mapping is used.
    #[serde(default)]
    pub main_menu_numeric_mapping: Option<Vec<MainMenuNumericBinding>>,

    /// Preserve unknown `tui.*` fields for forward compatibility.
    #[serde(flatten, default)]
    pub extra: BTreeMap<String, Value>,
}

impl TuiConfig {
    pub fn resolved_main_menu_numeric_mapping(&self) -> Option<[Option<MainMenuCommand>; 9]> {
        let list = self.main_menu_numeric_mapping.as_ref()?;
        let mut out: [Option<MainMenuCommand>; 9] = [None; 9];
        for b in list {
            if !(1..=9).contains(&b.key) {
                continue;
            }
            out[(b.key - 1) as usize] = Some(b.command);
        }
        Some(out)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ScanDepth {
    /// Scan only the folder root (depth = 0).
    #[default]
    RootOnly,
    /// Scan folder root + one nested level (depth = 1).
    OneLevel,
    /// Scan recursively without a depth limit (depth = ∞).
    Recursive,
}

impl ScanDepth {
    pub fn cycle_next(self) -> Self {
        match self {
            Self::RootOnly => Self::OneLevel,
            Self::OneLevel => Self::Recursive,
            Self::Recursive => Self::RootOnly,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum LoggingLevel {
    /// Opinionated defaults: include domain config-changing events; suppress noisy deps; exclude
    /// playback chatter.
    #[default]
    Default,
    Debug,
    Trace,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingConfig {
    /// Logging detail preset.
    #[serde(default)]
    pub default_level: LoggingLevel,

    /// Delete log files older than this many days on startup.
    #[serde(default = "defaults::default_logging_retention_days")]
    pub retention_days: u64,

    /// Preserve unknown `logging.*` fields for forward compatibility.
    #[serde(flatten, default)]
    pub extra: BTreeMap<String, Value>,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            default_level: LoggingLevel::Default,
            retention_days: defaults::default_logging_retention_days(),
            extra: Default::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum RepeatMode {
    #[default]
    Off,
    All,
    One,
}

#[derive(Debug, Clone)]
pub struct SettingsConfig {
    /// Minimum size threshold, in kilobytes (1 kb = 1024 bytes).
    ///
    /// Persisted as `settings.min_size_kb`.
    pub min_size_kb: u64,

    /// Derived value for existing call sites; not persisted.
    pub min_size_bytes: u64,

    /// Per-folder custom min_size_kb lower bound (inclusive). Values outside the range are ignored.
    ///
    /// Persisted as `settings.min_size_custom_kb_min`.
    pub min_size_custom_kb_min: u32,

    /// Per-folder custom min_size_kb upper bound (inclusive). Values outside the range are ignored.
    ///
    /// Persisted as `settings.min_size_custom_kb_max`.
    pub min_size_custom_kb_max: u32,

    pub shuffle: bool,

    pub repeat: RepeatMode,

    pub supported_extensions: Vec<String>,

    /// Preserve unknown `settings.*` fields for forward compatibility.
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Deserialize)]
struct SettingsConfigDe {
    #[serde(default)]
    min_size_kb: Option<u64>,

    #[serde(default)]
    shuffle: bool,

    #[serde(default)]
    min_size_custom_kb_min: Option<u32>,

    #[serde(default)]
    min_size_custom_kb_max: Option<u32>,

    #[serde(default)]
    repeat: RepeatMode,

    #[serde(default = "defaults::default_supported_extensions")]
    supported_extensions: Vec<String>,

    /// Preserve unknown `settings.*` fields for forward compatibility.
    #[serde(flatten, default)]
    extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize)]
struct SettingsConfigSer<'a> {
    min_size_kb: u64,
    min_size_custom_kb_min: u32,
    min_size_custom_kb_max: u32,
    shuffle: bool,
    repeat: RepeatMode,
    supported_extensions: &'a Vec<String>,

    /// Preserve unknown `settings.*` fields for forward compatibility.
    #[serde(flatten)]
    extra: &'a BTreeMap<String, Value>,
}

impl<'de> Deserialize<'de> for SettingsConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        let v = Option::<SettingsConfigDe>::deserialize(deserializer)?;
        let v = match v {
            None => return Ok(Self::default()),
            Some(v) => v,
        };

        let min_size_kb = v.min_size_kb.unwrap_or_else(defaults::default_min_size_kb);

        let min_size_bytes = min_size_kb
            .checked_mul(1024)
            .ok_or_else(|| <D::Error as de::Error>::custom("settings.min_size_kb is too large"))?;

        let min_size_custom_kb_min = v
            .min_size_custom_kb_min
            .unwrap_or_else(defaults::default_min_size_custom_kb_min);
        let min_size_custom_kb_max = v
            .min_size_custom_kb_max
            .unwrap_or_else(defaults::default_min_size_custom_kb_max);

        Ok(Self {
            min_size_kb,
            min_size_bytes,
            min_size_custom_kb_min,
            min_size_custom_kb_max,
            shuffle: v.shuffle,
            repeat: v.repeat,
            supported_extensions: v.supported_extensions,
            extra: v.extra,
        })
    }
}

impl Serialize for SettingsConfig {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        // Migration-on-save: write `min_size_kb` only (legacy `min_size_bytes` omitted).
        SettingsConfigSer {
            min_size_kb: self.min_size_kb,
            min_size_custom_kb_min: self.min_size_custom_kb_min,
            min_size_custom_kb_max: self.min_size_custom_kb_max,
            shuffle: self.shuffle,
            repeat: self.repeat,
            supported_extensions: &self.supported_extensions,
            extra: &self.extra,
        }
        .serialize(serializer)
    }
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
    LeftCtrl,
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
    PageUp,
    PageDown,
    S,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HotkeyChord {
    #[serde(default)]
    pub modifiers: Vec<HotkeyModifier>,
    pub key: HotkeyKey,
}

#[derive(Debug, Clone)]
pub struct AudioConfig {
    /// Initial volume for the current app session, in percent (0..=100).
    ///
    /// Persisted as `audio.volume_default_percent`.
    pub volume_default_percent: u8,

    /// Discrete volume ladder in percent (0..=100), sorted and unique.
    ///
    /// Persisted as `audio.volume_available_percent`.
    pub volume_available_percent: Vec<u8>,

    /// Preserve unknown `audio.*` fields for forward compatibility.
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Deserialize)]
struct AudioConfigDe {
    /// New field.
    #[serde(default)]
    volume_default_percent: Option<u8>,

    /// New field.
    #[serde(default)]
    volume_available_percent: Option<Vec<u8>>,

    /// Preserve unknown `audio.*` fields for forward compatibility.
    #[serde(flatten, default)]
    extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize)]
struct AudioConfigSer<'a> {
    volume_default_percent: u8,
    volume_available_percent: &'a Vec<u8>,

    /// Preserve unknown `audio.*` fields for forward compatibility.
    #[serde(flatten)]
    extra: &'a BTreeMap<String, Value>,
}

impl<'de> Deserialize<'de> for AudioConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        let v = Option::<AudioConfigDe>::deserialize(deserializer)?;
        let v = match v {
            None => return Ok(Self::default()),
            Some(v) => v,
        };

        let volume_default_percent = v
            .volume_default_percent
            .unwrap_or_else(defaults::default_volume_default_percent);

        let volume_available_percent = v
            .volume_available_percent
            .unwrap_or_else(defaults::default_volume_available_percent);

        Ok(Self {
            volume_default_percent,
            volume_available_percent,
            extra: v.extra,
        })
    }
}

impl Serialize for AudioConfig {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        AudioConfigSer {
            volume_default_percent: self.volume_default_percent,
            volume_available_percent: &self.volume_available_percent,
            extra: &self.extra,
        }
        .serialize(serializer)
    }
}

impl Default for AudioConfig {
    fn default() -> Self {
        defaults::default_audio()
    }
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

    /// Volume up (tap-only).
    #[serde(default = "defaults::default_hotkey_volume_up")]
    pub volume_up: Option<HotkeyChord>,

    /// Volume down (tap-only).
    #[serde(default = "defaults::default_hotkey_volume_down")]
    pub volume_down: Option<HotkeyChord>,

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
    pub folders: Vec<FolderEntry>,

    #[serde(default)]
    pub hotkeys: HotkeysConfig,

    #[serde(default)]
    pub audio: AudioConfig,

    #[serde(default)]
    pub logging: LoggingConfig,

    #[serde(default)]
    pub tui: TuiConfig,

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

        if self.settings.min_size_kb.checked_mul(1024).is_none() {
            return Err("settings.min_size_kb is too large".to_string());
        }

        if self.settings.min_size_custom_kb_min > self.settings.min_size_custom_kb_max {
            return Err(
                "settings.min_size_custom_kb_min must be <= settings.min_size_custom_kb_max"
                    .to_string(),
            );
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

        // Audio: basic ranges.
        if self.audio.volume_default_percent > 100 {
            return Err("audio.volume_default_percent must be within 0..=100".to_string());
        }

        if self.audio.volume_available_percent.is_empty() {
            return Err("audio.volume_available_percent must not be empty".to_string());
        }

        // Must be sorted, unique, include 0 and 100, and each element within 0..=100.
        let mut prev: Option<u8> = None;
        let mut has_0 = false;
        let mut has_100 = false;
        for &p in &self.audio.volume_available_percent {
            if p > 100 {
                return Err(
                    "audio.volume_available_percent values must be within 0..=100".to_string(),
                );
            }
            if p == 0 {
                has_0 = true;
            }
            if p == 100 {
                has_100 = true;
            }
            if let Some(prev) = prev {
                if p <= prev {
                    return Err(
                        "audio.volume_available_percent must be unique and sorted ascending"
                            .to_string(),
                    );
                }
            }
            prev = Some(p);
        }
        if !has_0 || !has_100 {
            return Err("audio.volume_available_percent must include 0 and 100".to_string());
        }

        if let Some(list) = self.tui.main_menu_numeric_mapping.as_ref() {
            for b in list {
                if !(1..=9).contains(&b.key) {
                    return Err(format!(
                        "tui.main_menu_numeric_mapping.key must be within 1..=9 (got {})",
                        b.key
                    ));
                }
            }
        }

        Ok(())
    }

    pub fn normalized(mut self) -> Self {
        self.folders = dedup_keep_order_folders(self.folders);
        self.folders = normalize_custom_min_size_keep_invalid_as_none(
            self.folders,
            self.settings.min_size_custom_kb_min,
            self.settings.min_size_custom_kb_max,
        );
        self.settings.supported_extensions =
            dedup_keep_order(self.settings.supported_extensions.clone());
        // Keep the audio ladder stable even if user provided duplicates/out-of-order;
        // validation is strict, but normalization is cheap and helps call sites.
        self.audio.volume_available_percent.sort_unstable();
        self.audio.volume_available_percent.dedup();
        self
    }

    pub fn folder_paths(&self) -> Vec<String> {
        self.folders.iter().map(|f| f.path.clone()).collect()
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            schema_version: Self::default_schema_version(),
            settings: SettingsConfig::default(),
            folders: Vec::new(),
            hotkeys: HotkeysConfig::default(),
            audio: AudioConfig::default(),
            logging: LoggingConfig::default(),
            tui: TuiConfig::default(),
            extra: BTreeMap::new(),
        }
    }
}

fn default_scan_depth() -> ScanDepth {
    ScanDepth::RootOnly
}

fn is_default_scan_depth(v: &ScanDepth) -> bool {
    *v == default_scan_depth()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderEntry {
    pub path: String,
    pub scan_depth: ScanDepth,
    pub custom_min_size_kb: Option<u32>,
}

impl FolderEntry {
    pub fn new(path: String) -> Self {
        Self {
            path,
            scan_depth: default_scan_depth(),
            custom_min_size_kb: None,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
struct FolderEntryDe {
    path: String,

    #[serde(default)]
    scan_depth: ScanDepth,

    #[serde(default)]
    custom_min_size_kb: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
struct FolderEntrySer<'a> {
    path: &'a str,

    #[serde(skip_serializing_if = "is_default_scan_depth")]
    scan_depth: ScanDepth,

    #[serde(skip_serializing_if = "Option::is_none")]
    custom_min_size_kb: Option<u32>,
}

impl<'de> Deserialize<'de> for FolderEntry {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: de::Deserializer<'de>,
    {
        let v = FolderEntryDe::deserialize(deserializer)?;
        Ok(Self {
            path: v.path,
            scan_depth: v.scan_depth,
            custom_min_size_kb: v.custom_min_size_kb,
        })
    }
}

impl Serialize for FolderEntry {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        FolderEntrySer {
            path: self.path.as_str(),
            scan_depth: self.scan_depth,
            custom_min_size_kb: self.custom_min_size_kb,
        }
        .serialize(serializer)
    }
}

fn normalize_custom_min_size_keep_invalid_as_none(
    mut folders: Vec<FolderEntry>,
    min_kb: u32,
    max_kb: u32,
) -> Vec<FolderEntry> {
    for f in &mut folders {
        if let Some(v) = f.custom_min_size_kb {
            if !(min_kb..=max_kb).contains(&v) {
                f.custom_min_size_kb = None;
            }
        }
    }
    folders
}

pub fn effective_min_size_kb_for_folder(folder: &FolderEntry, settings: &SettingsConfig) -> u64 {
    let min_kb = settings.min_size_custom_kb_min;
    let max_kb = settings.min_size_custom_kb_max;
    if let Some(v) = folder.custom_min_size_kb {
        if (min_kb..=max_kb).contains(&v) {
            return v as u64;
        }
    }
    settings.min_size_kb
}

fn dedup_keep_order(mut items: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::BTreeSet::new();
    items.retain(|v| seen.insert(v.clone()));
    items
}

fn dedup_keep_order_folders(mut items: Vec<FolderEntry>) -> Vec<FolderEntry> {
    let mut seen = std::collections::BTreeSet::<String>::new();
    items.retain(|v| seen.insert(v.path.clone()));
    items
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_depth_cycle_next_cycles_root_only_one_level_recursive_root_only() {
        let a = ScanDepth::RootOnly;
        let b = a.cycle_next();
        let c = b.cycle_next();
        let d = c.cycle_next();
        assert_eq!(b, ScanDepth::OneLevel);
        assert_eq!(c, ScanDepth::Recursive);
        assert_eq!(d, ScanDepth::RootOnly);
    }

    #[test]
    fn new_yaml_folders_object_form_deserializes_and_scan_depth_defaults_root_only_when_omitted() {
        let raw = r#"
schema_version: 1
folders:
  - path: "C:\\Music"
    scan_depth: recursive
  - path: "D:\\OST"
settings:
  supported_extensions: [mp3]
"#;

        let cfg: AppConfig = serde_yaml::from_str(raw).unwrap();
        assert_eq!(
            cfg.folders,
            vec![
                FolderEntry {
                    path: "C:\\Music".to_string(),
                    scan_depth: ScanDepth::Recursive,
                    custom_min_size_kb: None,
                },
                FolderEntry {
                    path: "D:\\OST".to_string(),
                    scan_depth: ScanDepth::RootOnly,
                    custom_min_size_kb: None,
                }
            ]
        );
    }

    #[test]
    fn normalized_dedup_keeps_first_occurrence_and_order_stable_for_folders() {
        let cfg = AppConfig {
            folders: vec![
                FolderEntry {
                    path: "C:\\Music".to_string(),
                    scan_depth: ScanDepth::Recursive,
                    custom_min_size_kb: None,
                },
                FolderEntry {
                    path: "C:\\Music".to_string(),
                    scan_depth: ScanDepth::RootOnly,
                    custom_min_size_kb: None,
                },
                FolderEntry {
                    path: "D:\\OST".to_string(),
                    scan_depth: ScanDepth::RootOnly,
                    custom_min_size_kb: None,
                },
                FolderEntry {
                    path: "C:\\Music".to_string(),
                    scan_depth: ScanDepth::Recursive,
                    custom_min_size_kb: None,
                },
            ],
            ..Default::default()
        };

        let normalized = cfg.normalized();
        assert_eq!(
            normalized.folders,
            vec![
                FolderEntry {
                    path: "C:\\Music".to_string(),
                    scan_depth: ScanDepth::Recursive,
                    custom_min_size_kb: None,
                },
                FolderEntry {
                    path: "D:\\OST".to_string(),
                    scan_depth: ScanDepth::RootOnly,
                    custom_min_size_kb: None,
                },
            ]
        );
    }

    #[test]
    fn folder_entry_serialization_skips_default_scan_depth() {
        let entry = FolderEntry {
            path: "C:\\Music".to_string(),
            scan_depth: ScanDepth::RootOnly,
            custom_min_size_kb: None,
        };
        let y = serde_yaml::to_string(&entry).unwrap();
        assert!(!y.contains("scan_depth"));
    }

    #[test]
    fn folder_entry_serialization_includes_non_default_scan_depth() {
        let entry = FolderEntry {
            path: "C:\\Music".to_string(),
            scan_depth: ScanDepth::OneLevel,
            custom_min_size_kb: None,
        };
        let y = serde_yaml::to_string(&entry).unwrap();
        assert!(y.contains("scan_depth"));
        assert!(y.contains("one_level"));
    }

    #[test]
    fn audio_volume_available_percent_validation_requires_sorted_unique_and_bounds() {
        let cfg = AppConfig {
            audio: AudioConfig {
                volume_default_percent: 50,
                volume_available_percent: vec![0, 10, 10, 100],
                extra: Default::default(),
            },
            ..Default::default()
        };
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn normalized_clears_out_of_range_folder_custom_min_size_kb() {
        let cfg = AppConfig {
            settings: SettingsConfig {
                min_size_kb: 100,
                min_size_bytes: 100 * 1024,
                min_size_custom_kb_min: 10,
                min_size_custom_kb_max: 10_000,
                ..Default::default()
            },
            folders: vec![
                FolderEntry {
                    path: "C:\\TooSmall".to_string(),
                    scan_depth: ScanDepth::RootOnly,
                    custom_min_size_kb: Some(9),
                },
                FolderEntry {
                    path: "C:\\Ok".to_string(),
                    scan_depth: ScanDepth::RootOnly,
                    custom_min_size_kb: Some(222),
                },
                FolderEntry {
                    path: "C:\\TooBig".to_string(),
                    scan_depth: ScanDepth::RootOnly,
                    custom_min_size_kb: Some(10_001),
                },
            ],
            ..Default::default()
        };

        let normalized = cfg.normalized();
        assert_eq!(normalized.folders[0].custom_min_size_kb, None);
        assert_eq!(normalized.folders[1].custom_min_size_kb, Some(222));
        assert_eq!(normalized.folders[2].custom_min_size_kb, None);
    }

    #[test]
    fn tui_main_menu_numeric_mapping_rejects_keys_outside_1_to_9() {
        let cfg = AppConfig {
            tui: TuiConfig {
                main_menu_numeric_mapping: Some(vec![MainMenuNumericBinding {
                    key: 0,
                    command: MainMenuCommand::Play,
                }]),
                extra: Default::default(),
            },
            ..Default::default()
        };
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn tui_main_menu_numeric_mapping_resolves_last_wins_for_duplicate_keys() {
        let tui = TuiConfig {
            main_menu_numeric_mapping: Some(vec![
                MainMenuNumericBinding {
                    key: 1,
                    command: MainMenuCommand::AddFolder,
                },
                MainMenuNumericBinding {
                    key: 1,
                    command: MainMenuCommand::Playlists,
                },
            ]),
            extra: Default::default(),
        };
        let resolved = tui.resolved_main_menu_numeric_mapping().unwrap();
        assert_eq!(resolved[0], Some(MainMenuCommand::Playlists));
    }
}
