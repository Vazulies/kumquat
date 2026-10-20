//! Watches the X11 session for file drags.
//!
//! Any app that starts a drag-and-drop on X11 takes ownership of the
//! `XdndSelection` selection, and XFixes tells us when that happens. While the
//! drag is in progress we poll the pointer and keyboard (both work even though
//! the drag source has grabbed them) to notice when a modifier is tapped.

use std::thread;
use std::time::{Duration, Instant};

use x11rb::connection::Connection;
use x11rb::protocol::xfixes::{ConnectionExt as _, SelectionEvent, SelectionEventMask};
use x11rb::protocol::xproto::{
    Atom, AtomEnum, ConnectionExt as _, CreateWindowAux, KeyButMask, Window, WindowClass,
};
use x11rb::protocol::Event;
use x11rb::rust_connection::RustConnection;

use crate::settings::Modifier;
use crate::watch::{Key, Signal};

const MODIFIERS: [(Modifier, KeyButMask); 4] = [
    (Modifier::Shift, KeyButMask::SHIFT),
    (Modifier::Control, KeyButMask::CONTROL),
    (Modifier::Alt, KeyButMask::MOD1),
    (Modifier::Super, KeyButMask::MOD4),
];

const KEYSYMS: [(u32, Key); 8] = [
    (0xff51, Key::Left),
    (0xff52, Key::Up),
    (0xff53, Key::Right),
    (0xff54, Key::Down),
    (0xff0d, Key::Enter),
    (0xff8d, Key::Enter),
    (0xff1b, Key::Escape),
    (0x0020, Key::Space),
];

/// Current pointer position, for opening the wheel from the keyboard.
pub fn pointer_position() -> Option<(i32, i32)> {
    let (conn, screen) = x11rb::connect(None).ok()?;
    let root = conn.setup().roots[screen].root;
    let reply = conn.query_pointer(root).ok()?.reply().ok()?;
    Some((reply.root_x as i32, reply.root_y as i32))
}

/// Raise and focus a window the way a taskbar would. GNOME's focus-stealing
/// prevention lets these requests through, unlike a plain focus request from
/// an app that's running in the background.
pub fn activate(xid: u32) -> anyhow::Result<()> {
    use x11rb::protocol::xproto::{ClientMessageEvent, EventMask};
    let (conn, screen) = x11rb::connect(None)?;
    let root = conn.setup().roots[screen].root;
    let active = atom(&conn, "_NET_ACTIVE_WINDOW")?;
    // Source 2 = pager; timestamp 0 = now.
    let event = ClientMessageEvent::new(32, xid, active, [2u32, 0, 0, 0, 0]);
    conn.send_event(
        false,
        root,
        EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY,
        event,
    )?;
    conn.flush()?;
    Ok(())
}

/// The window that has keyboard focus, according to the window manager.
pub fn active_window() -> Option<u64> {
    let (conn, screen) = x11rb::connect(None).ok()?;
    let root = conn.setup().roots[screen].root;
    let prop = atom(&conn, "_NET_ACTIVE_WINDOW").ok()?;
    let reply = conn.get_property(false, root, prop, AtomEnum::WINDOW, 0, 1).ok()?.reply().ok()?;
    let mut values = reply.value32()?;
    values.next().map(u64::from)
}

/// Size of the whole X screen, in pixels.
pub fn screen_size() -> Option<(i32, i32)> {
    let (conn, screen) = x11rb::connect(None).ok()?;
    let scr = &conn.setup().roots[screen];
    Some((scr.width_in_pixels as i32, scr.height_in_pixels as i32))
}

/// A blurred-ready snapshot of a screen area (what the wheel will sit on),
/// scaled down by `shrink` and returned as a PNG data URL.
pub fn snapshot(x: i32, y: i32, w: u32, h: u32, shrink: u32) -> Option<String> {
    use x11rb::protocol::xproto::ImageFormat;
    let (conn, screen) = x11rb::connect(None).ok()?;
    let scr = &conn.setup().roots[screen];
    // Stay inside the screen.
    let x0 = x.clamp(0, scr.width_in_pixels as i32 - 1);
    let y0 = y.clamp(0, scr.height_in_pixels as i32 - 1);
    let w = (w as i32).min(scr.width_in_pixels as i32 - x0).max(1) as u16;
    let h = (h as i32).min(scr.height_in_pixels as i32 - y0).max(1) as u16;
    let reply = conn
        .get_image(ImageFormat::Z_PIXMAP, scr.root, x0 as i16, y0 as i16, w, h, !0)
        .ok()?
        .reply()
        .ok()?;
    if reply.depth < 24 || reply.data.len() < w as usize * h as usize * 4 {
        return None;
    }
    // 32 bits per pixel, little-endian BGRX, rows packed.
    crate::watch::frost(&reply.data, w as usize, h as usize, w as usize * 4, shrink)
}

/// What's under a point on screen, from the window manager's point of view.
#[derive(Debug, Clone, Default)]
pub struct WindowInfo {
    pub desktop: bool,
    pub class: String,
    pub title: String,
    /// GTK's D-Bus object path for the window, e.g. `/org/gnome/Nautilus/window/1`.
    pub gtk_path: Option<String>,
}

fn string_prop(conn: &RustConnection, win: Window, name: &str) -> Option<String> {
    let prop = atom(conn, name).ok()?;
    let reply = conn.get_property(false, win, prop, AtomEnum::ANY, 0, 1024).ok()?.reply().ok()?;
    if reply.value.is_empty() {
        return None;
    }
    Some(String::from_utf8_lossy(&reply.value).trim_end_matches('\0').to_string())
}

/// The topmost visible top-level window containing (x, y), skipping Kumquat's own.
pub fn window_at(x: i32, y: i32) -> Option<WindowInfo> {
    let (conn, screen) = x11rb::connect(None).ok()?;
    let root = conn.setup().roots[screen].root;
    let stacking = atom(&conn, "_NET_CLIENT_LIST_STACKING").ok()?;
    let list = conn
        .get_property(false, root, stacking, AtomEnum::WINDOW, 0, 4096)
        .ok()?
        .reply()
        .ok()?;
    let windows: Vec<Window> = list.value32()?.collect();
    let wm_type = atom(&conn, "_NET_WM_WINDOW_TYPE").ok()?;
    let type_desktop = atom(&conn, "_NET_WM_WINDOW_TYPE_DESKTOP").ok()?;

    for &win in windows.iter().rev() {
        let Ok(Ok(attrs)) = conn.get_window_attributes(win).map(|c| c.reply()) else { continue };
        if attrs.map_state != x11rb::protocol::xproto::MapState::VIEWABLE {
            continue;
        }
        let Ok(Ok(geo)) = conn.get_geometry(win).map(|c| c.reply()) else { continue };
        let Ok(Ok(pos)) = conn.translate_coordinates(win, root, 0, 0).map(|c| c.reply()) else { continue };
        let (wx, wy) = (pos.dst_x as i32, pos.dst_y as i32);
        if x < wx || y < wy || x >= wx + geo.width as i32 || y >= wy + geo.height as i32 {
            continue;
        }
        let class = string_prop(&conn, win, "WM_CLASS").unwrap_or_default().replace('\0', " ");
        if class.to_lowercase().contains("kumquat") {
            continue;
        }
        let desktop = conn
            .get_property(false, win, wm_type, AtomEnum::ATOM, 0, 16)
            .ok()
            .and_then(|c| c.reply().ok())
            .and_then(|r| r.value32().map(|mut v| v.any(|a| a == type_desktop)))
            .unwrap_or(false);
        return Some(WindowInfo {
            desktop,
            class,
            title: string_prop(&conn, win, "_NET_WM_NAME").unwrap_or_default(),
            gtk_path: string_prop(&conn, win, "_GTK_WINDOW_OBJECT_PATH"),
        });
    }
    // Nothing on top: the bare desktop.
    Some(WindowInfo { desktop: true, ..Default::default() })
}

pub fn spawn(on_signal: impl Fn(Signal) + Send + 'static) {
    thread::Builder::new()
        .name("x11-drag-watch".into())
        .spawn(move || {
            if let Err(err) = run(&on_signal) {
                log::error!("drag watcher stopped: {err:#}");
            }
        })
        .expect("spawn drag watcher");
}

struct Ctx {
    conn: RustConnection,
    root: Window,
    window: Window,
    xdnd: Atom,
    uri_list: Atom,
    prop: Atom,
    keycodes: Vec<(u8, Key)>,
}

fn atom(conn: &RustConnection, name: &str) -> anyhow::Result<Atom> {
    Ok(conn.intern_atom(false, name.as_bytes())?.reply()?.atom)
}

fn run(on_signal: &dyn Fn(Signal)) -> anyhow::Result<()> {
    let (conn, screen) = x11rb::connect(None)?;
    let root = conn.setup().roots[screen].root;
    conn.xfixes_query_version(5, 0)?.reply()?;

    let window = conn.generate_id()?;
    conn.create_window(
        x11rb::COPY_DEPTH_FROM_PARENT,
        window,
        root,
        -10,
        -10,
        1,
        1,
        0,
        WindowClass::INPUT_ONLY,
        x11rb::COPY_FROM_PARENT,
        &CreateWindowAux::new(),
    )?;

    let xdnd = atom(&conn, "XdndSelection")?;
    let uri_list = atom(&conn, "text/uri-list")?;
    let prop = atom(&conn, "KUMQUAT_DRAG")?;
    let keycodes = keycodes(&conn)?;

    conn.xfixes_select_selection_input(
        root,
        xdnd,
        SelectionEventMask::SET_SELECTION_OWNER
            | SelectionEventMask::SELECTION_WINDOW_DESTROY
            | SelectionEventMask::SELECTION_CLIENT_CLOSE,
    )?;
    conn.flush()?;

    let ctx = Ctx { conn, root, window, xdnd, uri_list, prop, keycodes };
    log::info!("watching X11 drags");

    loop {
        let event = ctx.conn.wait_for_event()?;
        if let Event::XfixesSelectionNotify(ev) = event {
            if ev.selection == ctx.xdnd
                && ev.subtype == SelectionEvent::SET_SELECTION_OWNER
                && ev.owner != x11rb::NONE
            {
                track_drag(&ctx, ev.selection_timestamp, on_signal)?;
            }
        }
    }
}

fn keycodes(conn: &RustConnection) -> anyhow::Result<Vec<(u8, Key)>> {
    let setup = conn.setup();
    let (min, max) = (setup.min_keycode, setup.max_keycode);
    let map = conn.get_keyboard_mapping(min, max - min + 1)?.reply()?;
    let per = map.keysyms_per_keycode as usize;
    let mut out = Vec::new();
    for (i, syms) in map.keysyms.chunks(per.max(1)).enumerate() {
        for (sym, key) in KEYSYMS {
            if syms.contains(&sym) {
                out.push((min + i as u8, key));
            }
        }
    }
    Ok(out)
}

fn track_drag(ctx: &Ctx, timestamp: u32, on_signal: &dyn Fn(Signal)) -> anyhow::Result<()> {
    let paths = fetch_paths(ctx, timestamp).unwrap_or_else(|err| {
        log::debug!("could not read drag contents: {err:#}");
        None
    });
    if matches!(&paths, Some(p) if p.is_empty()) {
        // Text or another non-file drag: let it be.
        return Ok(());
    }
    on_signal(Signal::DragStarted { paths });

    let start = Instant::now();
    let first = ctx.conn.query_pointer(ctx.root)?.reply()?;
    let mut prev_mask = first.mask;
    let mut prev_keys = ctx.conn.query_keymap()?.reply()?.keys;
    let mut released_polls = 0;

    loop {
        // Drain anything queued; a new drag simply continues this one.
        while ctx.conn.poll_for_event()?.is_some() {}

        let pointer = ctx.conn.query_pointer(ctx.root)?.reply()?;
        let buttons = KeyButMask::BUTTON1 | KeyButMask::BUTTON2 | KeyButMask::BUTTON3;
        if pointer.mask & buttons == KeyButMask::from(0u16) {
            released_polls += 1;
            if released_polls >= 2 && start.elapsed() > Duration::from_millis(80) {
                on_signal(Signal::DragEnded);
                return Ok(());
            }
        } else {
            released_polls = 0;
        }

        for (modifier, bit) in MODIFIERS {
            let now = pointer.mask & bit != KeyButMask::from(0u16);
            let before = prev_mask & bit != KeyButMask::from(0u16);
            if now && !before {
                on_signal(Signal::ModifierDown {
                    modifier,
                    x: pointer.root_x as i32,
                    y: pointer.root_y as i32,
                });
            }
        }
        prev_mask = pointer.mask;

        let keys = ctx.conn.query_keymap()?.reply()?.keys;
        for (code, key) in &ctx.keycodes {
            let byte = (*code / 8) as usize;
            let bit = 1u8 << (*code % 8);
            if keys[byte] & bit != 0 && prev_keys[byte] & bit == 0 {
                if *key == Key::Space {
                    // Space can open a wheel, like the modifiers.
                    on_signal(Signal::ModifierDown {
                        modifier: Modifier::Space,
                        x: pointer.root_x as i32,
                        y: pointer.root_y as i32,
                    });
                } else {
                    on_signal(Signal::Key(*key));
                }
            }
        }
        prev_keys = keys;

        if start.elapsed() > Duration::from_secs(600) {
            on_signal(Signal::DragEnded);
            return Ok(());
        }
        thread::sleep(Duration::from_millis(12));
    }
}

/// Ask the drag source for its `text/uri-list`, the same way a drop target would.
fn fetch_paths(ctx: &Ctx, timestamp: u32) -> anyhow::Result<Option<Vec<String>>> {
    ctx.conn
        .convert_selection(ctx.window, ctx.xdnd, ctx.uri_list, ctx.prop, timestamp)?;
    ctx.conn.flush()?;
    let deadline = Instant::now() + Duration::from_millis(300);
    while Instant::now() < deadline {
        match ctx.conn.poll_for_event()? {
            Some(Event::SelectionNotify(ev)) if ev.requestor == ctx.window => {
                if ev.property == x11rb::NONE {
                    return Ok(None);
                }
                let reply = ctx
                    .conn
                    .get_property(true, ctx.window, ctx.prop, AtomEnum::ANY, 0, 1 << 20)?
                    .reply()?;
                let text = String::from_utf8_lossy(&reply.value);
                return Ok(Some(parse_uri_list(&text)));
            }
            Some(_) => {}
            None => thread::sleep(Duration::from_millis(4)),
        }
    }
    Ok(None)
}

pub fn parse_uri_list(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|uri| {
            let rest = uri.strip_prefix("file://")?;
            // `file://host/path` — keep only the path.
            let path = if rest.starts_with('/') { rest } else { &rest[rest.find('/')?..] };
            Some(
                percent_encoding::percent_decode_str(path)
                    .decode_utf8_lossy()
                    .into_owned(),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_uri_lists() {
        let list = "file:///home/me/My%20Photo.png\r\n#comment\r\nfile://host/tmp/a.txt\r\nhttps://example.com\r\n";
        assert_eq!(parse_uri_list(list), vec!["/home/me/My Photo.png", "/tmp/a.txt"]);
    }
}

#[cfg(test)]
mod snapshot_probe {
    #[test]
    #[ignore]
    fn takes_a_snapshot() {
        let t = std::time::Instant::now();
        let shot = super::snapshot(100, 100, 460, 460, 4);
        println!("snapshot: {:?} bytes in {:?}", shot.as_ref().map(|s| s.len()), t.elapsed());
        assert!(shot.is_some());
    }
}
