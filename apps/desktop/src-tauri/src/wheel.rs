//! The radial menu window: when it opens, where, and for which files.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition};

use crate::catalog::{self, Catalog};
use crate::placement::{self, Placement};
use crate::settings::{self, Modifier};
use crate::watch::{Key, Signal};

/// Width and height of the wheel window, in logical pixels (see tauri.conf.json).
const WHEEL_SIZE: f64 = 560.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WheelKind {
    Convert,
    Tools,
    Actions,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// Opened mid-drag: the user will drop the file onto a slice.
    Drag,
    /// Opened from the file manager or tray: arrows, Enter and clicks pick.
    Keyboard,
}

struct State {
    open: bool,
    mode: Option<Mode>,
    dragging: bool,
    drag_paths: Option<Vec<String>>,
    generation: u64,
    /// Where the file was being dragged to when the wheel opened.
    placement: Option<Placement>,
}

static STATE: Mutex<State> = Mutex::new(State {
    open: false,
    mode: None,
    dragging: false,
    drag_paths: None,
    generation: 0,
    placement: None,
});

/// Native id of the wheel window (X11 id or HWND), once known.
static WHEEL_ID: AtomicU64 = AtomicU64::new(0);

/// Where results of the wheel's current choice should go, if it was opened mid-drag.
pub fn placement() -> Option<Placement> {
    STATE.lock().unwrap().placement.clone()
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct OpenPayload {
    kind: WheelKind,
    mode: Mode,
    paths: Vec<String>,
    catalog: Option<Catalog>,
    keys: KeyHints,
    /// What's behind the wheel, for the frosted-glass backdrop.
    backdrop: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyHints {
    convert: Modifier,
    tools: Modifier,
    actions: Modifier,
}

pub fn key_hints() -> KeyHints {
    let s = settings::get();
    KeyHints { convert: s.convert_key, tools: s.tools_key, actions: s.actions_key }
}

fn kind_for(modifier: Modifier) -> Option<WheelKind> {
    let s = settings::get();
    if modifier == s.convert_key {
        Some(WheelKind::Convert)
    } else if modifier == s.tools_key {
        Some(WheelKind::Tools)
    } else if modifier == s.actions_key {
        Some(WheelKind::Actions)
    } else {
        None
    }
}

pub fn handle_signal(app: &AppHandle, signal: Signal) {
    match signal {
        Signal::DragStarted { paths } => {
            let mut st = STATE.lock().unwrap();
            st.dragging = true;
            st.drag_paths = paths;
        }
        Signal::ModifierDown { modifier, x, y } => {
            if settings::get().paused {
                return;
            }
            let Some(kind) = kind_for(modifier) else { return };
            let (open, mode, paths) = {
                let st = STATE.lock().unwrap();
                (st.open, st.mode, st.drag_paths.clone().unwrap_or_default())
            };
            if open && mode == Some(Mode::Drag) {
                let _ = app.emit_to("wheel", "wheel://kind", kind);
            } else if !open {
                // Where the file was being dragged to is where results should land.
                // Worked out off the main path so the wheel appears instantly.
                STATE.lock().unwrap().placement = None;
                open_wheel(app, kind, Mode::Drag, paths, Some((x, y)));
                let generation = STATE.lock().unwrap().generation;
                thread::spawn(move || {
                    let spot = crate::watch::window_at(x, y).and_then(|w| placement::resolve(&w, x, y));
                    log::debug!("wheel opened over {spot:?}");
                    let mut st = STATE.lock().unwrap();
                    if st.generation == generation {
                        st.placement = spot;
                    }
                });
            }
        }
        Signal::Key(key) => {
            let (open, mode) = {
                let st = STATE.lock().unwrap();
                (st.open, st.mode)
            };
            if !open || mode != Some(Mode::Drag) {
                return;
            }
            if key == Key::Escape {
                close(app);
            } else {
                let name = match key {
                    Key::Left => "left",
                    Key::Right => "right",
                    Key::Up => "up",
                    Key::Down => "down",
                    Key::Enter => "enter",
                    Key::Escape | Key::Space => unreachable!(),
                };
                let _ = app.emit_to("wheel", "wheel://key", name);
            }
        }
        Signal::DragEnded => {
            let generation = {
                let mut st = STATE.lock().unwrap();
                st.dragging = false;
                st.drag_paths = None;
                if !(st.open && st.mode == Some(Mode::Drag)) {
                    return;
                }
                st.generation
            };
            // Give the drop a moment to land on the wheel before tidying up.
            let app = app.clone();
            thread::spawn(move || {
                thread::sleep(Duration::from_millis(700));
                let still_open = {
                    let st = STATE.lock().unwrap();
                    st.open && st.generation == generation
                };
                if still_open {
                    close(&app);
                }
            });
        }
    }
}

pub fn open_wheel(
    app: &AppHandle,
    kind: WheelKind,
    mode: Mode,
    paths: Vec<String>,
    at: Option<(i32, i32)>,
) {
    let Some(win) = app.get_webview_window("wheel") else { return };
    {
        let mut st = STATE.lock().unwrap();
        st.open = true;
        st.mode = Some(mode);
        st.generation += 1;
        if mode == Mode::Keyboard {
            st.placement = None;
        }
    }

    let (x, y) = at
        .or_else(crate::watch::pointer_position)
        .unwrap_or((800, 500));
    // A window that has never been shown reports 0×0; the wheel is always WHEEL_SIZE logical px.
    let side = (WHEEL_SIZE * win.scale_factor().unwrap_or(1.0)).round() as i32;
    let (w, h) = (side, side);
    let (mut left, mut top) = (x - w / 2, y - h / 2);
    // Keep the whole wheel on the monitor under the pointer (or at least the screen).
    let bounds = match app.monitor_from_point(x as f64, y as f64) {
        Ok(Some(m)) => Some((m.position().x, m.position().y, m.size().width as i32, m.size().height as i32)),
        _ => crate::watch::screen_size().map(|(sw, sh)| (0, 0, sw, sh)),
    };
    if let Some((mx, my, mw, mh)) = bounds {
        left = left.clamp(mx, (mx + mw - w).max(mx));
        top = top.clamp(my, (my + mh - h).max(my));
    }

    // Snapshot what the wheel is about to cover, before it's on screen.
    let backdrop = if win.is_visible().unwrap_or(false) {
        None
    } else {
        crate::watch::snapshot(left, top, w as u32, h as u32, 4)
    };
    log::debug!("wheel at {left},{top}, backdrop: {}", backdrop.as_ref().map(|b| b.len()).unwrap_or(0));
    let catalog = (!paths.is_empty()).then(|| catalog::for_paths(&paths));
    let payload = OpenPayload { kind, mode, paths, catalog, keys: key_hints(), backdrop };

    let focus = mode == Mode::Keyboard;
    let w2 = win.clone();
    let _ = app.run_on_main_thread(move || {
        native::set_accept_focus(&w2, focus);
        let _ = w2.set_position(PhysicalPosition::new(left, top));
        let _ = w2.show();
        let _ = w2.set_position(PhysicalPosition::new(left, top));
        let _ = w2.set_always_on_top(true);
        if focus {
            let _ = w2.set_focus();
            activate_window(&w2);
        }
    });
    if focus {
        close_when_unfocused(app.clone());
    }
    let _ = app.emit_to("wheel", "wheel://open", payload);
}

pub fn close(app: &AppHandle) {
    {
        let mut st = STATE.lock().unwrap();
        st.open = false;
        st.mode = None;
        st.generation += 1;
    }
    let _ = app.emit_to("wheel", "wheel://close", ());
    if let Some(win) = app.get_webview_window("wheel") {
        let _ = win.hide();
    }
}

/// Repaint the whole window after the page changes shape (see `native::repaint`).
pub fn repaint(app: &AppHandle) {
    let Some(win) = app.get_webview_window("wheel") else { return };
    let _ = app.run_on_main_thread(move || native::repaint(&win));
}

/// A keyboard-mode wheel closes when it loses focus, but if it never gets focus
/// (the user clicked elsewhere first) the page never hears about it. Watch the
/// active window from here too, and close the wheel once something else has it.
fn close_when_unfocused(app: AppHandle) {
    let generation = STATE.lock().unwrap().generation;
    thread::spawn(move || {
        // Give the window manager a moment to hand focus over.
        thread::sleep(Duration::from_millis(800));
        loop {
            {
                let st = STATE.lock().unwrap();
                if !st.open || st.generation != generation {
                    return;
                }
            }
            let wheel = WHEEL_ID.load(Ordering::Relaxed);
            let active = crate::watch::active_window();
            if wheel != 0 && active.is_some() && active != Some(wheel) {
                close(&app);
                return;
            }
            thread::sleep(Duration::from_millis(250));
        }
    });
}

/// Focus a window even when the desktop would rather not. Call on the main thread.
pub fn activate_window(win: &tauri::WebviewWindow) {
    native::activate(win);
}

/// Window setup that has to happen before the wheel is first shown.
pub fn prepare(app: &AppHandle) {
    if let Some(win) = app.get_webview_window("wheel") {
        let _ = app.run_on_main_thread(move || native::prepare(&win));
    }
}

/// The GTK side of the wheel window.
#[cfg(target_os = "linux")]
mod native {
    use std::sync::atomic::Ordering;
    use std::time::Duration;

    use gtk::prelude::*;

    use super::WHEEL_ID;

    pub fn set_accept_focus(win: &tauri::WebviewWindow, focus: bool) {
        if let Ok(gtk_win) = win.gtk_window() {
            gtk_win.set_accept_focus(focus);
        }
    }

    /// Transparent GTK windows don't clear pixels that the page stops drawing on,
    /// so a smaller ring would leave the bigger one showing behind it.
    pub fn repaint(win: &tauri::WebviewWindow) {
        let Ok(gtk_win) = win.gtk_window() else { return };
        // A size change gets the window a fresh, cleared buffer.
        let (w, h) = gtk_win.size();
        gtk_win.resize(w + 1, h);
        gtk::glib::timeout_add_local_once(Duration::from_millis(30), move || {
            gtk_win.resize(w, h);
        });
    }

    /// GNOME's focus-stealing prevention lets a pager's activation request through.
    pub fn activate(win: &tauri::WebviewWindow) {
        use gtk::glib::Cast;
        let Ok(gtk_win) = win.gtk_window() else { return };
        // Wait for the window to be mapped, or the window manager ignores the request.
        gtk::glib::timeout_add_local_once(Duration::from_millis(80), move || {
            let Some(gdk_win) = gtk_win.window() else { return };
            if let Ok(x11) = gdk_win.downcast::<gdkx11::X11Window>() {
                if gtk_win.title().map(|t| t.contains("Wheel")).unwrap_or(false) {
                    WHEEL_ID.store(x11.xid(), Ordering::Relaxed);
                }
                let _ = crate::x11watch::activate(x11.xid() as u32);
            }
        });
    }

    pub fn prepare(win: &tauri::WebviewWindow) {
        if let Ok(gtk_win) = win.gtk_window() {
            gtk_win.set_type_hint(gtk::gdk::WindowTypeHint::Utility);
            gtk_win.set_skip_pager_hint(true);
            gtk_win.set_skip_taskbar_hint(true);
            gtk_win.set_accept_focus(false);
            gtk_win.set_keep_above(true);
        }
    }
}

/// The Win32 side of the wheel window.
#[cfg(windows)]
mod native {
    use std::sync::atomic::Ordering;

    use super::WHEEL_ID;

    /// A window that can't be activated (WS_EX_NOACTIVATE) can sit under a
    /// drag without taking focus from the app the drag came from.
    pub fn set_accept_focus(win: &tauri::WebviewWindow, focus: bool) {
        let _ = win.set_focusable(focus);
    }

    /// WebView2 repaints transparent windows fully; nothing to do.
    pub fn repaint(_win: &tauri::WebviewWindow) {}

    pub fn activate(win: &tauri::WebviewWindow) {
        let Some(hwnd) = crate::win::hwnd_of(win) else { return };
        if win.label() == "wheel" {
            WHEEL_ID.store(crate::win::hwnd_id(hwnd), Ordering::Relaxed);
        }
        crate::win::activate(hwnd);
    }

    pub fn prepare(win: &tauri::WebviewWindow) {
        let _ = win.set_focusable(false);
        let _ = win.set_skip_taskbar(true);
        let _ = win.set_always_on_top(true);
        if let Some(hwnd) = crate::win::hwnd_of(win) {
            WHEEL_ID.store(crate::win::hwnd_id(hwnd), Ordering::Relaxed);
        }
    }
}
