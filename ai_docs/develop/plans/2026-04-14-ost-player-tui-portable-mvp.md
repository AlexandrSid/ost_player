# Plan: OST Player (TUI + Portable + Global Hotkeys) — MVP

**Created:** 2026-04-14  
**Orchestration:** orch-2026-04-14-15-02-ost-player  
**Status:** 🟢 Ready  
**Repo constraint:** All new implementation lives under top-level `app/` (keep existing files outside `app/` unchanged unless absolutely required).

## Goal
Build a **portable**, **terminal (TUI)** OST/music player for **Windows 11** that plays `.mp3` / `.ogg` from user-selected folders with **global hotkeys**, supports **playlists (as sets of folders)**, filters small files by size threshold, and supports shuffle/repeat + tap/hold seeking.

## Stack / Project Type (detected)
- Repository currently contains specs only (no existing codebase / build system detected).
- Implement as **Rust** (recommended by both TZs) with a single portable executable.

## Consolidated Acceptance Criteria (from `tz/TZ_OST_Player_TUI_Portable.md` + `tz/TZ.md`)
- **Portable storage**: All app data stored inside the app folder (no `%APPDATA%`, registry, ProgramData, etc.).
- **Write-access guard**: If app folder is not writable, show a clear error instructing user to move the folder to a writable location and exit.
- **TUI menus** (pre-playback):
  - Show active folders (for active playlist) and menu items:
    - add folder, remove folder(s), play, settings, playlists, exit
  - Provide clear status (folders count, tracks found, current settings).
- **Settings**:
  - `min_size_bytes`
  - `shuffle` on/off
  - `repeat` off / (one optional) / all (MVP supports off + all + one if feasible)
- **Playlists**:
  - A playlist is **name + list of folders** (not list of tracks).
  - Operations: list, create (from current folders), overwrite/save current into existing (with confirmation), load/swap (replace current folders), rename, delete (with confirmation).
  - For MVP, swap during playback may follow variant **A**: stop playback → load playlist → return to menu.
- **Indexing**:
  - Recursive scan of all active playlist folders.
  - Supported extensions default `.mp3`, `.ogg` (extensible via config).
  - Filter by filesystem size: include only files with `size >= min_size_bytes`.
  - Deterministic order when shuffle is off (e.g., sort by path).
  - Robust to missing/inaccessible folders/files: skip with warnings/counters; do not crash.
- **Playback**:
  - Plays `.mp3` and `.ogg`.
  - Continues playing when terminal is minimized (background playback).
  - Decoding errors: skip track and continue, report succinctly in UI/log.
  - Shuffle: when on, queue is shuffled.
  - Repeat: off stops at end; all loops; one loops current track (optional but targeted).
- **Global hotkeys (Windows 11 обязательны)**:
  - Work globally without terminal focus.
  - Configurable via config file; provide defaults.
  - Default semantics:
    - Play/Pause
    - Next / Previous (tap)
    - Fast-forward / Rewind (hold) as repeated seek steps (default **±5s** every **200–300ms**)
    - Repeat toggle (cycle; exact order documented)
    - Shuffle toggle must exist; Up+Down combo may be unreliable; allow alternative default (e.g., `Ctrl+RightShift+S`) while keeping configurability.
  - Tap vs hold for Left/Right with configurable `hold_threshold_ms` (default ~250–350ms).
  - Hotkey registration conflicts: show clear message; MVP prefers **continue without that hotkey**.
- **Config persistence**:
  - Human-editable config, validated with clear errors on parse/validation failure.
  - Autopersist: any change via TUI is saved immediately.
  - On first start, create default config files if missing.
  - **Decision (to satisfy stricter TZ):** use **YAML** and split into:
    - `./data/config.yaml` (settings + active folders + hotkeys; no playlists)
    - `./data/playlists.yaml` (named playlists + current/active)

## Execution Notes / Architectural Constraints
- Keep **platform-specific code isolated** (Windows hotkeys module; later Linux possible).
- Unify TUI input + hotkeys into a single command model (e.g., `PlayerCommand`).
- Prefer a background player thread/task; TUI remains responsive.

## Tasks (≤10)

### ✅ OST-001: Project bootstrap under `app/`
- **Objective**: Create a Rust binary project and baseline module structure fully under `app/`, with `main` starting the TUI loop and loading config from portable `data/`.
- **Key files/dirs (under `app/`)**:
  - `app/Cargo.toml`
  - `app/src/main.rs`
  - `app/src/lib.rs`
  - `app/src/error.rs`
  - `app/src/paths.rs` (portable base dir + `data/` resolution)
- **Verification**:
  - `cargo build` succeeds from `app/`
  - Running produces a TUI skeleton and exits cleanly.

### ✅ OST-002: Portable data layout + config/playlists YAML persistence
- **Objective**: Implement portable storage rules and auto-create/load/save:
  - `data/config.yaml`
  - `data/playlists.yaml`
  - Write-access guard on startup (fail fast with actionable message).
- **Key files/dirs**:
  - `app/src/config/mod.rs` (schema + validation)
  - `app/src/config/io.rs` (load/save defaults, error messages)
  - `app/src/config/defaults.rs`
  - `app/src/playlists/mod.rs` (schema + ops)
  - `app/src/playlists/io.rs`
- **Verification**:
  - First run creates `data/` and both YAML files beside the executable (during dev: beside `app/target/...`).
  - Invalid YAML yields a clear error and safe exit.
  - Any TUI change persists immediately.

### ✅ OST-003: TUI main menu + settings + playlists submenus
- **Objective**: Implement the TUI screens required by the TZs:
  - main menu (folders list + actions)
  - settings screen (min_size_bytes, shuffle, repeat)
  - playlists screen (create/rename/delete/load/swap, overwrite with confirmation)
- **Key files/dirs**:
  - `app/src/tui/mod.rs`
  - `app/src/tui/screens/{main_menu,settings,playlists,now_playing}.rs`
  - `app/src/tui/state.rs`
- **Verification**:
  - All menu actions reachable via keyboard (numbers/keys).
  - Folder add/remove updates config on disk.
  - Playlist ops update playlists file on disk.

### ⏳ OST-004: Indexer (scan folders → track list) with filtering + error reporting
- **Objective**: Build a track indexer that recursively scans folders, filters by extension and size threshold, deduplicates, sorts deterministically (shuffle off), and reports warnings/errors without crashing.
- **Key files/dirs**:
  - `app/src/indexer/mod.rs`
  - `app/src/indexer/scan.rs`
  - `app/src/indexer/model.rs`
- **Verification**:
  - Given a mixed folder tree, only `.mp3/.ogg` ≥ threshold appear.
  - Missing/inaccessible folders increment counters and show warnings in UI.

### ✅ OST-005: Player core (queue + shuffle/repeat) and audio output
- **Objective**: Implement playback engine:
  - decode `.mp3/.ogg` and output audio
  - queue navigation (next/prev)
  - shuffle/repeat behaviors
  - skip on decode failure
- **Key files/dirs**:
  - `app/src/player/mod.rs`
  - `app/src/player/queue.rs`
  - `app/src/player/engine.rs`
  - `app/src/audio/mod.rs` (backend abstraction)
  - `app/src/audio/symphonia.rs`
  - `app/src/audio/cpal.rs`
- **Verification**:
  - Plays mp3/ogg from indexed list.
  - Minimizing terminal does not stop playback.
  - Repeat off/all (and one if implemented) works as specified.

### ⏳ OST-006: Windows global hotkeys provider + tap/hold + conflict handling
- **Objective**: Implement Windows hotkeys with configurable bindings and robust behavior:
  - RegisterHotKey-based provider
  - play/pause, next/prev tap
  - hold → repeated seek ±5s every repeat interval
  - shuffle/repeat toggles
  - conflict handling message and continue without failed hotkey
- **Key files/dirs**:
  - `app/src/hotkeys/mod.rs` (trait + command mapping)
  - `app/src/hotkeys/windows.rs`
  - `app/src/hotkeys/tap_hold.rs`
- **Verification**:
  - Hotkeys work globally on Windows 11.
  - Tap vs hold behaves per configured thresholds.
  - Conflict results in clear warning and app continues (minus that binding).

### ⏳ OST-007: Command bus wiring (TUI + hotkeys → player) + Now Playing screen
- **Objective**: Unify events into `PlayerCommand`, wire TUI and hotkeys to control the player, and implement “Now Playing” screen with current track, status, queue position, and hotkey hints.
- **Key files/dirs**:
  - `app/src/commands.rs`
  - `app/src/app_state.rs`
  - `app/src/tui/screens/now_playing.rs`
- **Verification**:
  - Now Playing screen updates on track changes.
  - Hotkeys affect playback while TUI is open/minimized.

### ⏳ OST-008: Swap behavior during playback + UX hardening
- **Objective**: Define and implement playlist swap behavior during playback (MVP: **stop playback, swap, return to menu**), plus user-facing messages for major failure modes (permissions, invalid paths, decode errors).
- **Key files/dirs**:
  - `app/src/playlists/swap.rs` (or integrate into playlists module)
  - `app/src/tui/notifications.rs`
- **Verification**:
  - Swapping active playlist while playing stops audio and returns to menu with new folders active.
  - Major errors are understandable and non-crashing.

### ⏳ OST-009: Tests + verification scripts (worker/test-writer/test-runner ready)
- **Objective**: Add test coverage for non-audio parts (config, playlists, indexer, queue logic) and ensure a repeatable local verification flow.
- **Key files/dirs**:
  - `app/src/**` unit tests
  - `app/tests/**` integration tests (where feasible)
- **Verification**:
  - `cargo test` passes.
  - Tests cover:
    - YAML load/save roundtrip + validation failures
    - playlist CRUD/swap semantics
    - indexer filtering/dedup/sort
    - queue shuffle/repeat logic (pure logic)

## Dependencies Graph (critical path)
OST-001 → OST-002 → OST-003 → OST-004 → OST-005 → OST-006 → OST-007 → OST-008 → OST-009

## Notes for implementers
- Keep all new source and build artifacts within `app/` (no root-level Cargo workspace unless absolutely required).
- Prefer explicit error messages in terminal for permission/config/hotkey failures.

