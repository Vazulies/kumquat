//! Watches Windows for file drags.
//!
//! Windows has no public "a drag started" event. Shell drags (Explorer, the
//! desktop, browsers and most apps that drag files) show their drag image in a
//! layered `SysDragImage` window, so while a mouse button is held we look for
//! one. Keys are read with GetAsyncKeyState, which works whichever app has
//! focus. The dragged files themselves arrive when the drag enters the wheel,
//! which is an OLE drop target.

use std::thread;
use std::time::{Duration, Instant};

use windows::core::w;
use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
use windows::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, ReleaseDC, SelectObject, BITMAPINFO,
    BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, SRCCOPY,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, VIRTUAL_KEY, VK_CONTROL, VK_DOWN, VK_ESCAPE, VK_LBUTTON, VK_LEFT, VK_LWIN, VK_MENU, VK_RBUTTON,
    VK_RETURN, VK_RIGHT, VK_RWIN, VK_SHIFT, VK_SPACE, VK_UP,
};
use windows::Win32::UI::WindowsAndMessaging::{
    FindWindowExW, GetClassNameW, GetCursorPos, GetForegroundWindow, GetSystemMetrics, GetTopWindow, GetWindow,
    GetWindowLongPtrW, GetWindowRect, GetWindowThreadProcessId, IsWindowVisible, GWL_EXSTYLE,
    GW_HWNDNEXT, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, WS_EX_TRANSPARENT,
};

use super::from_wide;
use crate::settings::Modifier;
use crate::watch::{Key, Signal};

const MODIFIERS: [(Modifier, &[VIRTUAL_KEY]); 4] = [
    (Modifier::Shift, &[VK_SHIFT]),
    (Modifier::Control, &[VK_CONTROL]),
    (Modifier::Alt, &[VK_MENU]),
    (Modifier::Super, &[VK_LWIN, VK_RWIN]),
];

const KEYS: [(VIRTUAL_KEY, Key); 7] = [
    (VK_LEFT, Key::Left),
    (VK_UP, Key::Up),
    (VK_RIGHT, Key::Right),
    (VK_DOWN, Key::Down),
    (VK_RETURN, Key::Enter),
    (VK_ESCAPE, Key::Escape),
    (VK_SPACE, Key::Space),
];

fn down(vk: VIRTUAL_KEY) -> bool {
    unsafe { GetAsyncKeyState(vk.0 as i32) as u16 & 0x8000 != 0 }
}

/// Physical buttons, so swapped mouse buttons don't matter; right-drags count too.
fn button_down() -> bool {
    down(VK_LBUTTON) || down(VK_RBUTTON)
}

/// Current pointer position, in physical pixels.
pub fn pointer_position() -> Option<(i32, i32)> {
    let mut p = POINT::default();
    unsafe { GetCursorPos(&mut p) }.ok()?;
    Some((p.x, p.y))
}

/// The virtual screen spanning all monitors: (left, top, width, height).
fn virtual_screen() -> (i32, i32, i32, i32) {
    unsafe {
        (
            GetSystemMetrics(SM_XVIRTUALSCREEN),
            GetSystemMetrics(SM_YVIRTUALSCREEN),
            GetSystemMetrics(SM_CXVIRTUALSCREEN),
            GetSystemMetrics(SM_CYVIRTUALSCREEN),
        )
    }
}

/// Size of the whole desktop, in pixels.
pub fn screen_size() -> Option<(i32, i32)> {
    let (_, _, w, h) = virtual_screen();
    (w > 0 && h > 0).then_some((w, h))
}

/// The window that has keyboard focus.
pub fn active_window() -> Option<u64> {
    let hwnd = unsafe { GetForegroundWindow() };
    (!hwnd.is_invalid()).then(|| super::hwnd_id(hwnd))
}

/// A blurred-ready snapshot of a screen area (what the wheel will sit on),
/// scaled down by `shrink` and returned as a PNG data URL.
pub fn snapshot(x: i32, y: i32, w: u32, h: u32, shrink: u32) -> Option<String> {
    let (vx, vy, vw, vh) = virtual_screen();
    // Stay inside the desktop.
    let x0 = x.clamp(vx, vx + vw - 1);
    let y0 = y.clamp(vy, vy + vh - 1);
    let w = (w as i32).min(vx + vw - x0).max(1);
    let h = (h as i32).min(vy + vh - y0).max(1);
    unsafe {
        let screen = GetDC(None);
        if screen.is_invalid() {
            return None;
        }
        let mem = CreateCompatibleDC(Some(screen));
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                // Negative: rows top to bottom.
                biHeight: -h,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits = std::ptr::null_mut();
        let shot = match CreateDIBSection(Some(screen), &info, DIB_RGB_COLORS, &mut bits, None, 0) {
            Ok(bitmap) => {
                let old = SelectObject(mem, bitmap.into());
                let copied = BitBlt(mem, 0, 0, w, h, Some(screen), x0, y0, SRCCOPY).is_ok();
                // 32 bits per pixel, BGRX, rows packed.
                let len = w as usize * h as usize * 4;
                let shot = (copied && !bits.is_null())
                    .then(|| std::slice::from_raw_parts(bits as *const u8, len))
                    .and_then(|data| crate::watch::frost(data, w as usize, h as usize, w as usize * 4, shrink));
                SelectObject(mem, old);
                let _ = DeleteObject(bitmap.into());
                shot
            }
            Err(_) => None,
        };
        let _ = DeleteDC(mem);
        ReleaseDC(None, screen);
        shot
    }
}

/// What's under a point on screen.
#[derive(Debug, Clone, Default)]
pub struct WindowInfo {
    pub desktop: bool,
    pub class: String,
    pub hwnd: u64,
}

fn class_name(hwnd: HWND) -> String {
    let mut buf = [0u16; 256];
    let n = unsafe { GetClassNameW(hwnd, &mut buf) };
    from_wide(&buf[..n.max(0) as usize])
}

fn is_cloaked(hwnd: HWND) -> bool {
    let mut cloaked = 0u32;
    unsafe {
        DwmGetWindowAttribute(hwnd, DWMWA_CLOAKED, &mut cloaked as *mut u32 as *mut _, std::mem::size_of::<u32>() as u32)
    }
    .is_ok()
        && cloaked != 0
}

/// The topmost visible top-level window containing (x, y), skipping Kumquat's
/// own windows and the drag image that follows the pointer.
pub fn window_at(x: i32, y: i32) -> Option<WindowInfo> {
    let me = std::process::id();
    let mut next = unsafe { GetTopWindow(None) }.ok();
    while let Some(hwnd) = next {
        next = unsafe { GetWindow(hwnd, GW_HWNDNEXT) }.ok();
        if !unsafe { IsWindowVisible(hwnd) }.as_bool() || is_cloaked(hwnd) {
            continue;
        }
        let mut rect = RECT::default();
        if unsafe { GetWindowRect(hwnd, &mut rect) }.is_err()
            || x < rect.left
            || y < rect.top
            || x >= rect.right
            || y >= rect.bottom
        {
            continue;
        }
        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
        let ex_style = unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) } as u32;
        let class = class_name(hwnd);
        if pid == me || ex_style & WS_EX_TRANSPARENT.0 != 0 || class == "SysDragImage" {
            continue;
        }
        return Some(WindowInfo {
            desktop: class == "Progman" || class == "WorkerW",
            class,
            hwnd: super::hwnd_id(hwnd),
        });
    }
    Some(WindowInfo { desktop: true, ..Default::default() })
}

/// Is a shell drag image on screen right now?
fn drag_image_visible() -> bool {
    let mut after: Option<HWND> = None;
    loop {
        let Ok(hwnd) = (unsafe { FindWindowExW(None, after, w!("SysDragImage"), None) }) else {
            return false;
        };
        if hwnd.is_invalid() {
            return false;
        }
        if unsafe { IsWindowVisible(hwnd) }.as_bool() {
            return true;
        }
        after = Some(hwnd);
    }
}

pub fn spawn(on_signal: impl Fn(Signal) + Send + 'static) {
    thread::Builder::new()
        .name("win-drag-watch".into())
        .spawn(move || {
            log::info!("watching Windows drags");
            loop {
                // Idle: cheap checks until a button is held and a drag image shows up.
                if button_down() && drag_image_visible() {
                    track_drag(&on_signal);
                }
                thread::sleep(Duration::from_millis(if button_down() { 25 } else { 60 }));
            }
        })
        .expect("spawn drag watcher");
}

fn modifier_states() -> [bool; 4] {
    MODIFIERS.map(|(_, keys)| keys.iter().any(|&k| down(k)))
}

fn key_states() -> [bool; 7] {
    KEYS.map(|(vk, _)| down(vk))
}

fn track_drag(on_signal: &dyn Fn(Signal)) {
    // Which files are moving only shows once the drag reaches the wheel.
    on_signal(Signal::DragStarted { paths: None });

    let start = Instant::now();
    let mut prev_mods = modifier_states();
    let mut prev_keys = key_states();
    let mut released_polls = 0;

    loop {
        if !button_down() {
            released_polls += 1;
            if released_polls >= 2 && start.elapsed() > Duration::from_millis(80) {
                on_signal(Signal::DragEnded);
                return;
            }
        } else {
            released_polls = 0;
        }

        let (x, y) = pointer_position().unwrap_or_default();
        let mods = modifier_states();
        for (i, (modifier, _)) in MODIFIERS.iter().enumerate() {
            if mods[i] && !prev_mods[i] {
                on_signal(Signal::ModifierDown { modifier: *modifier, x, y });
            }
        }
        prev_mods = mods;

        let keys = key_states();
        for (i, (_, key)) in KEYS.iter().enumerate() {
            if keys[i] && !prev_keys[i] {
                if *key == Key::Space {
                    // Space can open a wheel, like the modifiers.
                    on_signal(Signal::ModifierDown { modifier: Modifier::Space, x, y });
                } else {
                    on_signal(Signal::Key(*key));
                }
            }
        }
        prev_keys = keys;

        if start.elapsed() > Duration::from_secs(600) {
            on_signal(Signal::DragEnded);
            return;
        }
        thread::sleep(Duration::from_millis(12));
    }
}
