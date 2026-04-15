# Plan: UI fixes — scan symbols + menu numbering + TextInput

**Created:** 2026-04-15  
**Orchestration:** orch-2026-04-15-10-30-ui-fixes-menu-scan-textinput  
**Status:** ✅ Completed  
**Source TZ:** `tz/TZ_UI_Fixes_Menu_ScanSymbols_TextInput.md`

## Goal
Implement all UX fixes from the TZ:
- Unambiguous scan-mode indicators (`>>>` recursive, `>|⋮` root-only) with stable alignment.
- Main menu is strictly numbered (every item starts with a digit) and `toggle root_only...` gets a numeric hotkey in addition to `t` (if no conflicts).
- `Add folder (absolute path)` TextInput shows typed/edited text, is 1 line tall, and supports a scrolling viewport that keeps the cursor visible.

## Constraints / Notes
- **Minimize operator involvement**: tasks should be executable end-to-end by the orchestrator.
- **Testing**: if automated tests can't run in this environment, defer to manual `cargo test` and the manual TUI checks listed in the TZ.
- **Do not break** existing navigation (Up/Down, Enter, Esc) and keep behavior stable on Windows terminals.

## Tasks (<= 10)

- [x] **UI-FIX-001 (✅ Completed): Scan mode indicator spec + formatting helper**
  - **Priority:** High
  - **Dependencies:** None
  - **Scope:**
    - Define the new symbols:
      - Recursive: `>>>`
      - Root-only: `>|⋮`
    - Ensure **semantics** match: `>>>` = recursive, `>|⋮` = root-only.
    - Introduce/adjust a single formatting path so the indicator occupies a **fixed width** (padding/alignment) to prevent column jitter.
  - **Expected files (likely):**
    - `app/src/tui/ui.rs` (or module where per-folder UI rows are composed)
  - **Acceptance criteria:**
    - Wherever scan mode is displayed, it uses exactly the new symbols.
    - Indicator width is stable (rows remain aligned when toggling mode).

- [x] **UI-FIX-002 (✅ Completed): Apply indicator change everywhere it's shown**
  - **Priority:** High
  - **Dependencies:** UI-FIX-001
  - **Scope:**
    - Update all UI surfaces where scan mode appears (folders list, settings/status lines, etc.) to use the helper/consistent mapping.
    - Audit that there is no swapped meaning anywhere.
  - **Expected files (likely):**
    - `app/src/tui/ui.rs` (and any adjacent UI modules)
  - **Acceptance criteria:**
    - Toggling scan mode immediately updates the displayed indicator.
    - `>>>` always corresponds to recursive; `>|⋮` always corresponds to root-only.
    - No visual misalignment regressions vs. current layout.

- [x] **UI-FIX-003 (✅ Completed): Main menu render — strict numeric prefix for every item**
  - **Priority:** High
  - **Dependencies:** None
  - **Scope:**
    - Render every main-menu item in a single consistent format where the **line begins with a digit**.
    - Recommended format (or equivalent): `N / <key(s)>  <action>`
    - Reorder/renumber is allowed if needed to avoid conflicts.
  - **Expected files (likely):**
    - `app/src/tui/screens/main_menu.rs`
  - **Acceptance criteria:**
    - There are **zero** main-menu items without a leading numeric prefix.
    - Menu looks consistent across all entries (spacing/formatting uniform).

- [x] **UI-FIX-004 (✅ Completed): Main menu bindings — numeric hotkey for toggle root_only (+ keep `t`)**
  - **Priority:** High
  - **Dependencies:** UI-FIX-003
  - **Scope:**
    - Ensure `toggle root_only for selected folder` is callable via:
      - a **digit key** (target: `3`, unless conflicts force a different digit),
      - `t` preserved as a fast hotkey **if non-conflicting**.
    - Ensure all other menu entries also have digit hotkeys and remain functional (Up/Down/Enter/Esc unaffected).
    - Prefer the proposed layout from TZ if feasible:
      - `1 / a   add folder`
      - `2 / d   remove selected folder`
      - `3 / t   toggle root_only for selected folder`
      - `4 / Enter / Space  play`
      - `5 / s   settings`
      - `6 / p   playlists`
      - `7 / r   rescan library`
      - `0 / q   exit`
  - **Expected files (likely):**
    - `app/src/tui/screens/main_menu.rs`
  - **Acceptance criteria:**
    - `toggle root_only...` works via its digit hotkey and via `t` (if kept).
    - No regressions: Up/Down navigation still works; Enter/Esc behave as before.

- [x] **UI-FIX-005 (✅ Completed): TextInput — render 1-line field with visible text**
  - **Priority:** High
  - **Dependencies:** None
  - **Scope:**
    - Fix `crate::tui::widgets::TextInput` so the input line:
      - is visually **exactly 1 row tall**,
      - renders the current buffer text (not just a caret),
      - supports insertion, backspace/delete, and cursor movement (Left/Right; Home/End if already supported).
    - Ensure cursor is drawn at the correct visual column within the displayed fragment.
  - **Expected files (likely):**
    - `crate::tui::widgets::TextInput` implementation file (exact path depends on crate layout)
  - **Acceptance criteria:**
    - Typing shows characters immediately.
    - Backspace/Delete visibly updates the buffer on screen.
    - Cursor matches the edit position.
    - Field height is visually 1 row (no "pseudo-field" squeezed height).

- [x] **UI-FIX-006 (✅ Completed): TextInput — viewport/scroll for long paths (cursor always visible)**
  - **Priority:** High
  - **Dependencies:** UI-FIX-005
  - **Scope:**
    - Implement a viewport window over the buffer for cases where text width exceeds the widget width.
    - Keep the cursor visible at all times by shifting the viewport as the cursor moves.
    - Ensure paste-from-clipboard results are visible (render path after paste).
  - **Expected files (likely):**
    - `crate::tui::widgets::TextInput`
  - **Acceptance criteria:**
    - For long paths, the rendered substring scrolls so the cursor never disappears.
    - Paste produces visible text; further edits remain visible and stable.

- [x] **UI-FIX-007 (✅ Completed): Wire TextInput behavior into "Add folder (absolute path)" dialog**
  - **Priority:** High
  - **Dependencies:** UI-FIX-006
  - **Scope:**
    - Ensure the `Add folder` flow in the main menu uses the fixed `TextInput` rendering and cursor placement.
    - Confirm Enter = save, Esc = cancel remain intact.
  - **Expected files (likely):**
    - `app/src/tui/screens/main_menu.rs`
  - **Acceptance criteria:**
    - In `Add folder (absolute path)` dialog: visible text, correct cursor, 1-line height.
    - Enter saves, Esc cancels; no input regressions.

- [x] **UI-FIX-008 (✅ Completed): Verification pass (manual mini-test plan + `cargo test`)**
  - **Priority:** High
  - **Dependencies:** UI-FIX-001..UI-FIX-007
  - **Scope:**
    - Run automated checks if available; otherwise record that tests should be run manually via `cargo test`.
    - Perform TZ mini-test plan manually:
      - Scan symbols toggle semantics + alignment
      - Main menu numbering and hotkeys (incl. digit for toggle root_only and `t` if kept)
      - Add folder TextInput: short path, long path viewport, paste, backspace/delete, cursor movement, Enter/Esc
  - **Acceptance criteria:**
    - All TZ checks pass (no behavior regressions on Windows terminals).
    - Any environment limitation is explicitly noted (e.g., "tests deferred to manual `cargo test`").
  - **Note:** Full implementation report available at `ai_docs/develop/reports/2026-04-15-ui-fixes-menu-scan-textinput-implementation.md`. Run `cargo test --manifest-path app/Cargo.toml --all` locally to verify all changes.

## Dependency graph

```text
UI-FIX-001 → UI-FIX-002
UI-FIX-003 → UI-FIX-004
UI-FIX-005 → UI-FIX-006 → UI-FIX-007
UI-FIX-001..007 → UI-FIX-008
```
