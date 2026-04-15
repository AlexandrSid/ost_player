# Documentation: Volume Hotkeys Feature

**Date:** 2026-04-15  
**Orchestration:** orch-2026-04-15-13-01-tz-volume-hotkeys  
**Status:** ✅ Documented

## Summary

Updated project documentation to reflect the new volume control hotkeys feature and related audio configuration options.

## Documentation Updates

### 1. **README.md** - Configuration Section

**Location:** `app/README.md` > Configuration > `config.yaml` Schema

**Changes:**
- Added new `volume_up` binding under `hotkeys.bindings`
- Added new `volume_down` binding under `hotkeys.bindings`
- Added new `audio` section with:
  - `default_volume_percent: 75` (initial volume on startup)
  - `volume_step_percent: 5` (volume change per hotkey press)

**Example:**
```yaml
hotkeys:
  bindings:
    volume_up:
      modifiers: [lctrl, rshift]
      key: PageUp
    
    volume_down:
      modifiers: [lctrl, rshift]
      key: PageDown

audio:
  default_volume_percent: 75        # 0-100, default: 75%
  volume_step_percent: 5            # Volume change per press, default: 5%
```

### 2. **README.md** - Hotkeys Reference

**Location:** `app/README.md` > Hotkeys (Default Bindings)

**Changes:**
- Added two new rows to the hotkeys table:
  - **Volume Up** | LeftCtrl+RightShift+PageUp
  - **Volume Down** | LeftCtrl+RightShift+PageDown
- Added "Volume Hotkeys" subsection with key characteristics:
  - Volume is global to the entire app (not per-track)
  - Volume does not persist across app restarts (resets to `audio.default_volume_percent`)
  - Can be disabled by setting to `null` in config
  - Windows platform limitation note: `RegisterHotKey` doesn't strictly distinguish L/R modifiers

### 3. **README.md** - Modifier Aliases

**Location:** `app/README.md` > Configuration > Note

**Changes:**
- Added `lctrl` (Left Ctrl) to the list of modifier aliases
- Now includes: `ctrl`, `lctrl`, `lshift`, `rshift`, `lalt`, `ralt`, `lwin`, `rwin`

## Config Schema Changes Documented

### New Hotkey Bindings

| Key | Type | Default | Notes |
|-----|------|---------|-------|
| `hotkeys.bindings.volume_up` | Optional chord | LeftCtrl+RightShift+PageUp | Tap-only, increases volume by `volume_step_percent` |
| `hotkeys.bindings.volume_down` | Optional chord | LeftCtrl+RightShift+PageDown | Tap-only, decreases volume by `volume_step_percent` |

### New Audio Config

| Key | Type | Default | Range | Notes |
|-----|------|---------|-------|-------|
| `audio.default_volume_percent` | u8 | 75 | 0-100 | Initial volume on app startup |
| `audio.volume_step_percent` | u8 | 5 | 0-100 | Volume change per hotkey press |

## Implementation Details Documented

### Volume Behavior
- **Scope:** Global to entire app (not per-track)
- **Persistence:** In-session only; resets to default on restart
- **Range:** 0–100% with automatic clamping
- **Control:** Tap-only (no hold action like seek controls)

### Windows Platform Behavior
- `RegisterHotKey` API limitation: Cannot strictly distinguish Left vs. Right modifiers
- Both `LeftCtrl` and `RightCtrl` may trigger the hotkey
- Both `LeftShift` and `RightShift` may trigger the hotkey
- This is documented as a known limitation in the README

### Configuration Options
- Disable volume_up hotkey: Set `volume_up: null` in config
- Disable volume_down hotkey: Set `volume_down: null` in config
- Missing keys use defaults from `app/src/config/defaults.rs`
- Extra unknown fields are preserved (forward compatibility via `#[serde(flatten)]`)

## Files Updated

- ✅ `app/README.md` - Added config examples, hotkeys table, volume behavior notes, modifier aliases

## Alignment with Spec

The documentation reflects all requirements from `tz/TZ_VolumeHotkeys.md`:

- ✅ New config keys documented: `hotkeys.bindings.volume_up`, `hotkeys.bindings.volume_down`
- ✅ Disable via `null` capability documented
- ✅ Missing uses defaults documented
- ✅ New audio config keys documented: `audio.default_volume_percent` (75), `audio.volume_step_percent` (5)
- ✅ Windows behavior and RegisterHotKey limitations clearly explained
- ✅ Runtime validation for tap-only hotkeys noted
- ✅ Concise style aligned with existing README format

## Style Consistency

- Follows existing README conventions (markdown tables, code blocks, note callouts)
- Uses consistent formatting for configuration examples
- Platform-specific notes use **Windows note:** prefix like other platform-specific items
- Configuration section maintains existing structure and tone
