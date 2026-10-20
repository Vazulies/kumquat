//! The Windows backend: drag watching, Explorer and the shell.

pub mod integration;
pub mod shell;
pub mod watch;

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;

use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};

/// A NUL-terminated UTF-16 copy of `s`, for Win32 calls.
pub fn wide(s: impl AsRef<OsStr>) -> Vec<u16> {
    s.as_ref().encode_wide().chain(std::iter::once(0)).collect()
}

/// UTF-16 buffer up to its first NUL, as a String.
pub fn from_wide(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

/// COM for the current thread, for as long as this lives.
pub struct Com(bool);

impl Com {
    pub fn init() -> Self {
        // S_FALSE (already initialised) still needs a matching uninitialise.
        Self(unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.is_ok())
    }
}

impl Drop for Com {
    fn drop(&mut self) {
        if self.0 {
            unsafe { CoUninitialize() };
        }
    }
}

pub fn hwnd_id(hwnd: HWND) -> u64 {
    hwnd.0 as usize as u64
}

/// The native handle of one of Kumquat's windows.
pub fn hwnd_of(win: &tauri::WebviewWindow) -> Option<HWND> {
    win.hwnd().ok().map(|h| HWND(h.0 as _))
}

/// Bring a window to the front and give it the keyboard. Windows only lets the
/// foreground app do that, so borrow the foreground thread's input state.
pub fn activate(hwnd: HWND) {
    use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
    use windows::Win32::UI::WindowsAndMessaging::{
        BringWindowToTop, GetForegroundWindow, GetWindowThreadProcessId, SetForegroundWindow, ShowWindow, SW_SHOW,
    };
    unsafe {
        let fg = GetForegroundWindow();
        let fg_thread = GetWindowThreadProcessId(fg, None);
        let me = GetCurrentThreadId();
        let attached = fg_thread != 0 && fg_thread != me && AttachThreadInput(me, fg_thread, true).as_bool();
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = BringWindowToTop(hwnd);
        let _ = SetForegroundWindow(hwnd);
        if attached {
            let _ = AttachThreadInput(me, fg_thread, false);
        }
    }
}

/// Start a program without the console window Windows would give it.
pub fn no_console(cmd: &mut std::process::Command) -> &mut std::process::Command {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    cmd.creation_flags(CREATE_NO_WINDOW)
}
