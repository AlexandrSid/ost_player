# Orchestration Completion Report: TZ Fixes (Now Playing, Hotkeys, Volumes, Playlists)

**Date:** 2026-04-21  
**Orchestration ID:** `orch-2026-04-21-15-00-tz-fixes`  
**Status:** ✅ Completed  
**Total Changes:** 856 insertions, 47 deletions across 8 files

---

## Summary

Successfully implemented two interconnected bug-fix and feature packages:
1. **TZ_Bugfix_NowPlaying_Navigation_And_HeaderDuplication** — Eliminated header duplication, fixed navigation to Now Playing screen, and introduced reliable playback source tracking
2. **TZ_Fixes_Logging_NowPlaying_ScanDepth_TUI_Volume_Playlists_MinSizePerFolder** — Comprehensive modernization of config system, UI/UX, and per-folder customization

## Completed Requirements

### From TZ_Bugfix_NowPlaying_Navigation_And_HeaderDuplication

- ✅ **Header Duplication Fixed** — Now Playing screen displays single title; removed duplicate header line from content
- ✅ **Immediate Navigation on Play** — User switches to Now Playing screen instantly (not blocked by scan)  
- ✅ **Smart Playback Source Tracking** — Added `playback_source` field to AppState tracking whether playback is from active playlist or folder hash
- ✅ **Guard for Already-Playing Playlist** — If selected playlist matches current playback source, navigate to Now Playing without interruption

### From TZ_Fixes_Logging_NowPlaying_ScanDepth_TUI_Volume_Playlists_MinSizePerFolder

- ✅ **Managed Logging** — Config-driven logging levels (Default/Debug/Trace) with dependency filtering, 3-file-per-month rotation, 31-day retention
- ✅ **Scan Depth Modes** — Three modes: recursive (`>>>`) | root-only (`>|⋮`) | one-level (`>⋮|`)
- ✅ **Discrete Volume Levels** — Fine-grained control at low volumes: `0 1 2 3 5 7 10 13 16 20`, then 5% steps to 100
- ✅ **TUI Refresh Policy** — Configurable timer: 1s (focused+playing), 5s (focus xor playing), off (minimized)
- ✅ **Per-Folder min_size Override** — Custom file-size threshold per folder with visual indicators (`◼` default, `🄲` custom)
- ✅ **Dynamic Hotkey Hints** — Menu actions (Keys block) pull from config instead of hardcoded strings
- ✅ **Removed Unused Directory** — Eliminated empty `data/playlists/` directory

---

## Key Files Modified

| File | Changes | Purpose |
|------|---------|---------|
| `app/src/config/mod.rs` | +74 lines | Config schema: logging levels, volume arrays, scan depth enum, per-folder min_size |
| `app/src/config/io.rs` | +8 lines | Config I/O: backward-compatible migrations for volume_step_percent and default_volume |
| `app/src/tui/app.rs` | +240 lines | App reducer: playback source tracking, immediate Now Playing navigation, guard logic |
| `app/src/tui/state.rs` | +37 lines | State: new `playback_source` enum and tracking field |
| `app/src/tui/ui.rs` | +349 lines | UI rendering: single header fix, status bar volume/min_size display, hotkey hints from config |
| `app/src/tui/screens/main_menu.rs` | +117 lines | Menu: numeric hint mapping, per-folder min_size modal, improved action layout |
| `app/src/hotkeys/mod.rs` | +1 line | Reference to new hints module |
| `app/src/hotkeys/hints.rs` | NEW | Dynamic hotkey hint generation from config |

---

## Verification Results

### Code Quality ✅

```bash
$ cargo fmt --check
# ✅ PASS — All code properly formatted

$ cargo clippy --all-targets
# ✅ PASS — Finished `dev` profile [unoptimized + debuginfo] target(s) in 4.73s
```

### Tests ✅

```bash
$ cargo test --lib
# ✅ PASS — 193 tests passed, 0 failed
#
# Key tests added:
#   • tz_002_load_playlist_active_but_player_stopped_loads_normally
#   • tz_004_guard_does_not_trigger_when_playback_source_differs_even_if_paused
#   • now_playing_does_not_duplicate_header_inside_content
#   • status_bar_renders_default_min_size_kb_and_volume_percent
#   • numeric_mapping_* (hotkey hints from config)
#   • folder_min_size_marker_and_kb_uses_custom_marker_only_when_override_in_range
```

### Acceptance Criteria ✅

| Criterion | Status | Evidence |
|-----------|--------|----------|
| Single "Now Playing" header in title, not duplicated in content | ✅ | Test: `now_playing_does_not_duplicate_header_inside_content` |
| Play action immediately shows Now Playing (even during scan) | ✅ | Immediate Navigate(NowPlaying) before queue loads |
| Already-playing playlist returns to Now Playing without restart | ✅ | Test: `tz_004_guard_does_not_trigger_when_playback_source_differs` |
| Scan depth mode switching cycles through 3 states | ✅ | Config enum + UI toggle logic |
| Volume changes via discrete levels from config | ✅ | Volume control reads from `audio.volume_available_percent` |
| Status bar shows volume % and effective min_size KB | ✅ | Test: `status_bar_renders_default_min_size_kb_and_volume_percent` |
| Per-folder min_size: indicators (`◼`/`🄲`) display correctly | ✅ | Test: `folder_min_size_marker_and_kb_uses_custom_marker_only_when_override_in_range` |
| Hotkey hints in UI pull from config, not hardcoded | ✅ | Test: `now_playing_keys_block_uses_hotkeys_bindings_from_config` |

---

## Technical Decisions

1. **PlaybackSource Enum** — Distinguishes between active playlist and folder hash sources to reliably detect "already playing" scenario without false positives
2. **Immediate Navigation** — Now Playing becomes active instantly; status bar shows "Scanning..." during queue prep, preventing user confusion
3. **Config-Driven Logging** — Dependency filtering at logger init time reduces noise and supports YAML customization without code changes
4. **Per-Folder Override Pattern** — Optional `min_size_kb` field with validation range (10..=10000 KB) allows flexible per-folder customization while maintaining safety
5. **Discrete Volume Array** — Logarithmic-like progression at low volumes improves UX for subtle audio adjustments

---

## Next Steps

- Merge to main branch
- Test in production environment with various playlist/folder combinations
- Gather user feedback on volume granularity and hotkey hint usability

---

## Related Documentation

- Plan: [ai_docs/develop/plans/2026-04-21-tz-fixes-nowplaying.md](../plans/2026-04-21-tz-fixes-nowplaying.md)
- Requirements: [tz/TZ_Bugfix_NowPlaying_Navigation_And_HeaderDuplication.md](../../tz/TZ_Bugfix_NowPlaying_Navigation_And_HeaderDuplication.md)
- Requirements: [tz/TZ_Fixes_Logging_NowPlaying_ScanDepth_TUI_Volume_Playlists_MinSizePerFolder.md](../../tz/TZ_Fixes_Logging_NowPlaying_ScanDepth_TUI_Volume_Playlists_MinSizePerFolder.md)
