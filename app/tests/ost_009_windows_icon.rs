#![cfg(windows)]

use ost_player::windows_icon::best_effort_set_console_window_icon_from_resource_id;

// These tests are intentionally "smoke tests":
// - We can't reliably assert the icon was applied (depends on host: ConHost vs Windows Terminal).
// - We *can* ensure the Windows-only code links, calls Win32 APIs, and never panics.

#[test]
fn set_console_window_icon_from_resource_id_does_not_panic_for_common_ids() {
    // Expected embedded icon resource ID per `app.rc`.
    best_effort_set_console_window_icon_from_resource_id(1);

    // A high ID that is very unlikely to exist. Function should fail gracefully and return.
    best_effort_set_console_window_icon_from_resource_id(u16::MAX);
}
