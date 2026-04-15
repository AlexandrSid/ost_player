# Refactor Report: TextInput Display Width Off-by-One Fix

**Date:** 2026-04-15  
**Scope:** `src/tui/widgets.rs`  
**Status:** ✅ Fixed (pending verification)

## Issue Summary

The `TextInput::display_for_width` method had an off-by-one error in calculating the scroll start position. This caused text rendering and cursor positioning to be incorrect when the input was scrolled, leading to test failures.

## Root Cause

The scroll start calculation did not properly account for the display width constraint. When the cursor position exceeded the visible width, the scroll offset was computed incorrectly, resulting in misaligned text display and cursor placement.

## Change Applied

**File:** `src/tui/widgets.rs`  
**Method:** `TextInput::display_for_width`

**Fix:** Updated scroll start calculation to use `cursor_cells - (width - 1)` via `max_cursor_x`.

This ensures that:
- The cursor stays visible within the display width
- The scroll offset correctly positions the viewport
- The off-by-one error is eliminated by accounting for both the cursor position and the available display width

## Verification

To verify the fix resolves the failing tests, run:

```bash
cargo test --lib tui::widgets::textinput
```

Or to run all tests:

```bash
cargo test
```

Expected result: All TextInput tests should pass.

## Files Modified

- `src/tui/widgets.rs` - TextInput display logic

## Technical Notes

This was a precision issue in viewport calculations common in terminal UI rendering where:
- Cursor position in cells must stay within `[0, width-1]`
- Scroll offset must ensure the cursor position maps to a visible column
- The relationship is: `display_start = cursor_cells - (width - 1)`
