# OST Player — TUI + Portable + Global Hotkeys

A lightweight, portable terminal-based music player for **Windows 11** (and Linux) with global hotkey support. Play `.mp3` and `.ogg` files from organized folders, manage playlists, and control playback without leaving the terminal or even focusing on the window.

## Features

- **Portable Storage**: All data lives in `./data/` relative to the executable—no registry, `%APPDATA%`, or system permissions required.
- **TUI Menus**: Manage folders, playlists, settings, and view now-playing info entirely from the terminal.
- **Global Hotkeys** (Windows/Linux): Control playback worldwide—play/pause, next/previous, seek, shuffle, and repeat toggles work even when the app is minimized.
- **Shuffle & Repeat Modes**: Shuffle tracks, loop all, or loop the current track.
- **Playlist Management**: Save and load playlists as named collections of folders.
- **Size Filtering**: Skip small files (podcast ads, silence, etc.) with configurable `min_size_kb` (persisted). Effective bytes are derived as `min_size_kb * 1024`.
- **Background Playback**: Minimize the terminal; audio continues playing.

## Installation & Setup

### Prerequisites

- **Rust 1.70+** (or later)
  - Download from [rustup.rs](https://rustup.rs/)
  - On Windows, installer handles setup automatically
  - Verify: `rustc --version` and `cargo --version`

### Build

```bash
cd app
cargo build --release
```

The portable executable will be at `app/target/release/ost_player.exe` (Windows) or `ost_player` (Linux).

> **Linux note (icons)**: when/if you ship a Linux desktop package, the app icon is handled by **packaging** (e.g. `.desktop` + `hicolor` PNGs). A plain `cargo build --release` does not embed a desktop icon into the Linux binary.

### Run

Move or copy the executable to your preferred location, then run it:

```bash
./ost_player.exe   # Windows
./ost_player       # Linux/macOS
```

On first launch, the app creates `data/` in the same directory with default `config.yaml` and `playlists.yaml`.

## Data Directory Layout

```
ost_player.exe
data/
  ├── config.yaml           # Settings: min_size_kb, shuffle, repeat, active folders, hotkeys, logging
  ├── playlists.yaml        # Named playlists and which is currently active
  ├── state.yaml            # Playback state (current track index, position, etc.)
  ├── logs/                 # Application logs (10-day buckets; retention cleanup on startup)
  ├── cache/                # Internal caches (if any)
  # Playlists live in playlists.yaml (no extra subdirectories)
```

## Configuration

All configuration is human-editable YAML. The app validates on startup and reports errors clearly.

### `config.yaml` Schema

```yaml
settings:
  min_size_kb: 1024                # Skip files smaller than this (default: 1024 KB = 1 MB)
  shuffle: false                   # Shuffle queue (default: off)
  repeat: off                       # off | all | one (default: off)
  supported_extensions:
    - mp3
    - ogg

folders:
  - "/path/to/music/folder1"
  - "/path/to/music/folder2"
  # Add more folders; app recursively scans each

hotkeys:
  timings:
    hold_threshold_ms: 300         # How long to hold before triggering hold action
    repeat_interval_ms: 250        # Interval for repeated hold actions (e.g., seeking)
    seek_step_seconds: 5           # How many seconds to seek per hold repeat

  bindings:
    play_pause:
      modifiers: [ctrl, rshift]
      key: Up

    repeat_toggle:
      modifiers: [ctrl, rshift]
      key: Down

    next:
      chord:
        modifiers: [ctrl, rshift]
        key: Right
      hold: { direction: 1 }        # Hold to fast-forward (seek +5s repeatedly)

    prev:
      chord:
        modifiers: [ctrl, rshift]
        key: Left
      hold: { direction: -1 }       # Hold to rewind (seek -5s repeatedly)

    shuffle_toggle:
      modifiers: [ctrl, rshift]
      key: S

    volume_up:                       # (optional) Global hotkey to increase volume
      modifiers: [lctrl, rshift]
      key: PageUp

    volume_down:                     # (optional) Global hotkey to decrease volume
      modifiers: [lctrl, rshift]
      key: PageDown

audio:
  default_volume_percent: 75        # Initial volume on app startup (0-100, default: 75)
  volume_step_percent: 5            # Volume change per hotkey press (default: 5)

logging:
  default_level: default            # default | debug | trace
  retention_days: 31                # delete logs older than this many days on startup
```

> **Note**: Hotkeys use modifier aliases:
> - `ctrl` = Ctrl
> - `lctrl` = Left Ctrl
> - `lshift`, `rshift` = Left/Right Shift
> - `lalt`, `ralt` = Left/Right Alt
> - `lwin`, `rwin` = Left/Right Windows key
> 
> Keys: `Up`, `Down`, `Left`, `Right`, `A`–`Z`, `0`–`9`, etc.

### `playlists.yaml` Schema

```yaml
current: "Favorite Soundtracks"    # Which playlist is active

playlists:
  - name: "Favorite Soundtracks"
    folders:
      - "/path/to/movie/soundtracks"
      - "/path/to/game/music"

  - name: "Ambient"
    folders:
      - "/path/to/ambient/music"
```

## Hotkeys (Default Bindings)

All hotkeys default to **Ctrl + Right Shift + [Key]** and work globally:

| Action | Default Binding | Notes |
|--------|-----------------|-------|
| **Play/Pause** | Ctrl+RightShift+↑ | Toggle between play and pause |
| **Next** | Ctrl+RightShift+→ | Tap: skip to next track; Hold: seek forward (+5s repeatedly) |
| **Previous** | Ctrl+RightShift+← | Tap: go to previous track; Hold: seek backward (-5s repeatedly) |
| **Repeat Toggle** | Ctrl+RightShift+↓ | Cycle: Off → All → One |
| **Shuffle Toggle** | Ctrl+RightShift+S | On/off |
| **Volume Up** | LeftCtrl+RightShift+PageUp | Increase volume by 5% (tap-only, 0–100% range) |
| **Volume Down** | LeftCtrl+RightShift+PageDown | Decrease volume by 5% (tap-only, 0–100% range) |

**Customization**: Edit `config.yaml`'s `hotkeys.bindings` section. Use any key and modifier combination; conflicts are reported at startup and that binding is skipped.

**Volume Hotkeys**: 
- Volume is **global to the entire app** (not per-track).
- Volume **does not persist** across app restarts; it resets to `audio.default_volume_percent` (default: 75%).
- Disable any volume hotkey by setting it to `null` (e.g., `volume_up: null`).
- **Windows note**: `RegisterHotKey` API doesn't strictly distinguish Left vs. Right modifiers; both `LeftCtrl` and `RightCtrl` may trigger, and both `LeftShift` and `RightShift` may trigger—this is a platform limitation.

## TUI Menus

### Main Menu
- **View**: Active folders, status (track count, settings).
- **Actions**:
  - `A` – Add folder
  - `R` – Remove selected folder(s)
  - `P` – Play (start playback from first track)
  - `S` – Enter Settings
  - `L` – Manage Playlists
  - `Q` / `Esc` – Quit

### Settings Menu
- **View**: Min file size, shuffle, repeat mode.
- **Actions**:
  - `↑` / `↓` – Navigate options
  - `Enter` – Toggle or edit setting
  - `Backspace` / `Esc` – Return to Main Menu

### Playlists Menu
- **View**: List of saved playlists, currently active.
- **Actions**:
  - `↑` / `↓` – Navigate playlists
  - `C` – Create new playlist from current folders
  - `L` – Load (swap to this playlist)
  - `O` – Overwrite (save current folders into selected playlist)
  - `R` – Rename
  - `D` – Delete
  - `Esc` – Return to Main Menu

### Now Playing Screen
- **View**: Current track, elapsed / total time, queue position, playback status.
- **Actions**:
  - `Space` – Play/Pause
  - `N` / `→` – Next track
  - `P` / `←` – Previous track
  - `Esc` – Return to Main Menu
  - Global hotkeys work seamlessly while playing

## Verification & Testing

### Local Verification (No Cargo Required)

If Rust is not installed, you can verify by running the pre-built executable:

1. **Portable Data Check**: Run the app once; confirm `data/` folder is created alongside the `.exe`.
2. **Config Validation**: Edit `data/config.yaml`, add a valid music folder, save, and restart the app. Confirm no errors and tracks are indexed.
3. **Playback**: Add a folder with `.mp3` or `.ogg` files (≥1 MB each), start playback via `P`, verify audio plays.
4. **Hotkeys**: While playing, use the default hotkeys (e.g., Ctrl+RightShift+→ to skip); confirm app responds globally.
5. **Playlists**: Create a playlist, save current folders into it, swap to it; confirm folders update in Main Menu.

### With Cargo (Full Test Suite)

```bash
cd app

# Format check
cargo fmt --all -- --check

# Linter (clippy)
cargo clippy --all-targets --all-features -- -D warnings

# Run tests
cargo test --all

# Build release
cargo build --release
```

For CI/CD, see `.github/workflows/ci.yml`; it runs on Windows and Linux on every push/PR.

## Troubleshooting

### App Won't Start: "Data directory not writable"
- **Cause**: The folder containing `ost_player.exe` is not writable (e.g., under `Program Files`).
- **Fix**: Move `ost_player.exe` to a writable location (e.g., `Downloads`, `Desktop`, or a user folder) and run it again.

### No Tracks Found
- **Cause**: Folders are empty, files are too small, or wrong extensions.
- **Check**:
  - Edit `config.yaml` and verify folder paths are correct.
  - Confirm files are `.mp3` or `.ogg` and ≥ your configured `settings.min_size_kb`.
  - Check logs in `data/logs/` for scan errors (bucketed files like `YYYY-MM-01_10.log`).

### Hotkeys Not Working
- **Cause**: Conflicting system hotkeys or clipboard manager.
- **Fix**: Change the bindings in `config.yaml` or disable the conflicting app.
- **Check**: Logs in `data/logs/` (bucketed files like `YYYY-MM-01_10.log`) show which hotkeys failed to register.

### Audio Glitches or Stuttering
- **Cause**: Weak PC, high system load, or unsupported audio format.
- **Workaround**: Try re-encoding to a standard MP3 (128–320 kbps) or OGG.

## Project Structure

```
app/
├── Cargo.toml              # Rust dependencies
├── README.md               # This file
├── src/
│   ├── main.rs             # Entry point
│   ├── lib.rs              # Library root
│   ├── config/             # Config schema + I/O
│   ├── playlists/          # Playlist management
│   ├── paths.rs            # Portable path resolution
│   ├── indexer/            # Track scanning + filtering
│   ├── player/             # Playback engine (queue, shuffle, repeat)
│   ├── audio/              # Audio backend (Symphonia + Rodio)
│   ├── hotkeys/            # Windows hotkey registration
│   ├── tui/                # Terminal UI screens and input handling
│   ├── command_bus.rs      # Unified command routing
│   ├── state.rs            # App state management
│   ├── error.rs            # Error types
│   ├── logging.rs          # Structured logging setup
│   └── persist.rs          # State persistence
├── tests/                  # Integration tests
└── scripts/
    ├── verify.ps1          # Windows CI verification
    └── verify.sh           # Linux CI verification
```

## Architecture Highlights

- **Portable Design**: All file I/O relative to executable; no dependencies on system paths.
- **Command Bus**: All player and TUI actions route through a unified command model for consistency.
- **Modular Audio**: Abstracted audio backend (Symphonia decoder + Rodio playback) for future extensibility.
- **Windows Hotkeys**: Platform-specific code isolated in `hotkeys/windows.rs`; tap vs. hold distinguished by timing.
- **YAML Configuration**: Human-readable, editable, validated on every load.

## License

(Add your license here if applicable)

## Contributing

Contributions welcome! Please ensure:
- `cargo fmt --all` passes (formatting)
- `cargo clippy --all-targets` passes (linting)
- `cargo test --all` passes (tests)

## Changelog

See `CHANGELOG.md` or git history for version details.
