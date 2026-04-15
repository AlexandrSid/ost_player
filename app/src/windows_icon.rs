use tracing::debug;

#[cfg(windows)]
use windows::{
    core::PCWSTR,
    Win32::{
        Foundation::{LPARAM, WPARAM},
        System::{Console::GetConsoleWindow, LibraryLoader::GetModuleHandleW},
        UI::WindowsAndMessaging::{
            LoadImageW, SendMessageW, HICON, ICON_BIG, ICON_SMALL, IMAGE_ICON, LR_DEFAULTSIZE,
            LR_SHARED, WM_SETICON,
        },
    },
};

/// Best-effort: sets the console window icon (ConHost).
///
/// In Windows Terminal this may be ignored; failures are logged and ignored.
pub fn best_effort_set_console_window_icon_from_resource_id(resource_id: u16) {
    #[cfg(not(windows))]
    {
        let _ = resource_id;
        return;
    }

    #[cfg(windows)]
    unsafe {
        let hwnd = GetConsoleWindow();
        if hwnd.0.is_null() {
            debug!("GetConsoleWindow returned null; skipping console icon");
            return;
        }

        let hinstance = match GetModuleHandleW(None) {
            Ok(h) => h,
            Err(e) => {
                debug!(error = ?e, "GetModuleHandleW failed; skipping console icon");
                return;
            }
        };

        // MAKEINTRESOURCEW: integer resource IDs are passed as pointer values.
        let res_name = PCWSTR::from_raw(resource_id as usize as *const u16);

        let h = LoadImageW(
            Some(hinstance.into()),
            res_name,
            IMAGE_ICON,
            0,
            0,
            // Prefer shared handle to avoid leaking an owned HICON.
            // If loading a shared icon fails, we skip (best-effort) rather than risk
            // leaking or using a handle with uncertain lifetime semantics.
            LR_DEFAULTSIZE | LR_SHARED,
        );
        let h = match h {
            Ok(h) => h,
            Err(e) => {
                debug!(
                    error = ?e,
                    resource_id,
                    "LoadImageW failed (LR_SHARED); skipping console icon"
                );
                return;
            }
        };
        if h.0.is_null() {
            debug!(
                resource_id,
                "LoadImageW returned null (LR_SHARED); skipping console icon"
            );
            return;
        }

        let hicon = HICON(h.0 as *mut _);
        let lparam = LPARAM(hicon.0 as isize);
        let _ = SendMessageW(
            hwnd,
            WM_SETICON,
            Some(WPARAM(ICON_SMALL as usize)),
            Some(lparam),
        );
        let _ = SendMessageW(
            hwnd,
            WM_SETICON,
            Some(WPARAM(ICON_BIG as usize)),
            Some(lparam),
        );
    }
}
