# Plan: Windows app icon (exe + console)

**Created:** 2026-04-15  
**Orchestration:** orch-2026-04-15-12-00-windows-app-icon  
**Status:** 🔵 Planning  
**Goal:** Implement Windows icon embedding for `ost_player.exe` and best-effort console window icon at runtime, per `tz/TZ_Windows_AppIcon_Exe_Console_LinuxNote.md`, using `app/resources/app_icon.ico`.

## Requirements Summary (from spec)
- **Windows .exe icon (A)**: Embed `app/resources/app_icon.ico` into `app/target/release/ost_player.exe` so Explorer/shortcuts/pinning show the correct icon.
- **Windows console window icon (B, best-effort)**: On startup, try to set the console window icon in classic ConHost (cmd/PowerShell). Windows Terminal may ignore this and is **not a blocker**.
- **Stable resource ID**: Ensure a stable icon resource identifier (e.g. `1`) so runtime code can load the same embedded icon.
- **ICO quality**: `app_icon.ico` should contain at least 16/32/48/256 (preferably also 24/64). Missing sizes causing blurry icons is considered an asset defect (regenerate the ICO).
- **Linux note**: Desktop icons are packaging-level (`.desktop` + hicolor PNGs), not `cargo build`; ensure docs mention this (currently present in `app/README.md`).

## Project Type / Build Pipeline
- **Stack**: Rust 2021; TUI (`ratatui`, `crossterm`); audio via `rodio`; Windows API via `windows` crate (already configured in `Cargo.toml`).
- **Build**: `cd app && cargo build --release` produces `app/target/release/ost_player.exe`.
- **CI**: GitHub Actions runs `cargo fmt`, `clippy`, `test` on Windows + Ubuntu; any `build.rs` must be cross-platform safe (no Windows-only tooling invoked on Linux).

## Tasks (≤10), ordered

- [x] **WINICON-001: Confirm icon embedding strategy + stable ID** (High, Simple) — ✅ Completed  
  **Approach**: Prefer explicit `.rc` resource script for a guaranteed stable ID, compiled by `build.rs` (via `embed-resource` or `winres` if it can guarantee ID).  
  **Acceptance**: Chosen approach is documented in the plan (and in-code implementation notes during execution) and provides a stable icon resource ID usable by runtime WinAPI loading.

- [ ] **WINICON-002: Validate `app_icon.ico` contains required sizes** (High, Simple) — ⏳ Pending  
  **Acceptance**: One of:
  - Verified the ICO contains at least 16/32/48/256 (and ideally 24/64), or
  - ICO is regenerated/replaced so that Windows displays crisp icons at multiple Explorer sizes.

- [ ] **WINICON-003: Embed icon into `ost_player.exe` during `cargo build --release`** (Critical, Moderate) — ⏳ Pending  
  **Implementation targets**: `app/build.rs`, `app/Cargo.toml`, `app/resources/*.rc` (or equivalent).  
  **Acceptance**: After building release, `app/target/release/ost_player.exe` shows the correct icon in Explorer (small + large icon views), shortcut icon is correct, and “Pin to taskbar/Start” retains the icon.

- [ ] **WINICON-004: Add best-effort runtime console-window icon setter (ConHost)** (High, Moderate) — ⏳ Pending  
  **Implementation targets**: likely `app/src/main.rs` (startup hook) + a small `app/src/windows_icon.rs`-style helper module (Windows-only).  
  **Behavior**: On `cfg(windows)`, call `GetConsoleWindow()`, load `HICON` from the embedded resource ID, then send `WM_SETICON` for both `ICON_SMALL` and `ICON_BIG`.  
  **Acceptance**: In classic ConHost (cmd/PowerShell), launching the app updates the window icon. Any failures do not crash the app and produce a debug/warn log entry.

- [ ] **WINICON-005: Ensure cross-platform builds + CI remain green** (Critical, Simple) — ⏳ Pending  
  **Acceptance**: `cargo fmt --all -- --check`, `cargo clippy --all-targets --all-features -- -D warnings`, and `cargo test --all` pass on both Windows and Linux CI (or locally equivalent). `build.rs` is a no-op on non-Windows.

- [ ] **WINICON-006: Documentation updates (Windows + Linux notes)** (Medium, Simple) — ⏳ Pending  
  **Targets**: `app/README.md` and/or `tz/TZ_Windows_AppIcon_Exe_Console_LinuxNote.md` if needed.  
  **Acceptance**: Docs explicitly note:
  - `.exe` icon is embedded at build-time (Windows resources),
  - console icon is best-effort (Windows Terminal may ignore),
  - Linux icon is packaging-level (already present; confirm it stays).

- [ ] **WINICON-007: Manual verification checklist on Windows** (High, Simple) — ⏳ Pending  
  **Acceptance**: A repeatable checklist exists and is executed:
  - Explorer: list/details + large icons show correct `.exe` icon
  - Shortcut icon correct
  - Pinned icon correct
  - ConHost shows runtime icon change
  - Windows Terminal behavior recorded as best-effort (non-blocking)

## Dependencies
WINICON-001 → WINICON-003 → WINICON-004 → WINICON-005 → WINICON-007  
WINICON-002 should be done before or alongside WINICON-003.

