//! Where results go when the wheel was opened mid-drag: the place the file
//! was being dragged to.

use std::path::{Path, PathBuf};
#[cfg(target_os = "linux")]
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};

use crate::watch::WindowInfo;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Placement {
    /// Folder the results are saved in.
    pub dir: PathBuf,
    /// Screen point to put desktop icons at, when `dir` is the desktop.
    pub desktop_point: Option<(i32, i32)>,
}

/// Work out the target from what's under the pointer at (x, y).
#[cfg(target_os = "linux")]
pub fn resolve(info: &WindowInfo, x: i32, y: i32) -> Option<Placement> {
    if info.desktop {
        let dir = dirs::desktop_dir()?;
        return dir.is_dir().then_some(Placement { dir, desktop_point: Some((x, y)) });
    }
    if info.class.contains("org.gnome.Nautilus") || info.class.to_lowercase().contains("nautilus") {
        let dir = nautilus_folder(info)?;
        return Some(Placement { dir, desktop_point: None });
    }
    None
}

/// Work out the target from what's under the pointer at (x, y).
#[cfg(windows)]
pub fn resolve(info: &WindowInfo, x: i32, y: i32) -> Option<Placement> {
    if info.desktop {
        let dir = dirs::desktop_dir()?;
        return dir.is_dir().then_some(Placement { dir, desktop_point: Some((x, y)) });
    }
    if info.class == "CabinetWClass" || info.class == "ExploreWClass" {
        let dir = crate::win::shell::explorer_folder(info.hwnd)?;
        return dir.is_dir().then_some(Placement { dir, desktop_point: None });
    }
    None
}

/// The folder a GNOME Files window is showing, via its D-Bus window list.
#[cfg(target_os = "linux")]
fn nautilus_folder(info: &WindowInfo) -> Option<PathBuf> {
    let gtk_path = info.gtk_path.as_deref()?;
    let out = Command::new("gdbus")
        .args([
            "call", "--session",
            "--dest", "org.freedesktop.FileManager1",
            "--object-path", "/org/freedesktop/FileManager1",
            "--method", "org.freedesktop.DBus.Properties.Get",
            "org.freedesktop.FileManager1", "OpenWindowsWithLocations",
        ])
        .stderr(Stdio::null())
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let folders = locations_for(&text, gtk_path);
    // Several tabs: the window title names the active one.
    let pick = folders
        .iter()
        .find(|f| f.file_name().map(|n| n.to_string_lossy() == info.title).unwrap_or(false))
        .or_else(|| folders.first())?;
    pick.is_dir().then(|| pick.clone())
}

/// Parse `({'/org/gnome/Nautilus/window/1': ['file:///home/me'], …},)`-style output.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn locations_for(text: &str, window: &str) -> Vec<PathBuf> {
    let key = format!("'{window}'");
    let Some(start) = text.find(&key) else { return vec![] };
    let rest = &text[start + key.len()..];
    let Some(open) = rest.find('[') else { return vec![] };
    let Some(close) = rest[open..].find(']') else { return vec![] };
    rest[open + 1..open + close]
        .split(',')
        .filter_map(|s| {
            let uri = s.trim().trim_matches('\'');
            let path = uri.strip_prefix("file://")?;
            Some(PathBuf::from(percent_encoding::percent_decode_str(path).decode_utf8_lossy().into_owned()))
        })
        .collect()
}

#[cfg(target_os = "linux")]
fn gio_set(path: &Path, args: &[&str]) {
    let _ = Command::new("gio")
        .arg("set")
        .args(args.iter().take(1))
        .arg(path)
        .args(args.iter().skip(1))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

/// Tell the desktop where to put a file's icon. Ubuntu's desktop places
/// files without a saved slot at their "drop position".
#[cfg(target_os = "linux")]
pub fn mark_drop_position(path: &Path, (x, y): (i32, i32)) {
    gio_set(path, &["-t", "string", "metadata::nautilus-drop-position", &format!("{x},{y}")]);
    gio_set(path, &["-t", "unset", "metadata::nautilus-icon-position"]);
}

/// Put a file's desktop icon at a screen point. Explorer has no per-file
/// metadata for this, so once the file is on the desktop, move its icon there.
#[cfg(windows)]
pub fn mark_drop_position(path: &Path, point: (i32, i32)) {
    let on_desktop = dirs::desktop_dir().zip(path.parent()).map(|(d, p)| d == p).unwrap_or(false);
    if on_desktop {
        crate::win::shell::place_on_desktop(path, point);
    }
}

/// Move a finished result into place, falling back to copying across drives.
#[cfg(windows)]
pub fn move_with_metadata(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::rename(from, to).or_else(|_| {
        std::fs::copy(from, to)?;
        std::fs::remove_file(from)
    })
}

/// Move a finished result into place, keeping its desktop metadata with it.
#[cfg(target_os = "linux")]
pub fn move_with_metadata(from: &Path, to: &Path) -> std::io::Result<()> {
    let status = Command::new("gio")
        .arg("move")
        .arg(from)
        .arg(to)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    match status {
        Ok(s) if s.success() && to.exists() => Ok(()),
        _ => std::fs::rename(from, to),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_nautilus_window_locations() {
        let out = "(<{'/org/gnome/Nautilus/window/1': ['file:///home/me/My%20Pics', 'file:///tmp'], '/org/gnome/Nautilus/window/2': ['file:///etc']}>,)";
        assert_eq!(
            locations_for(out, "/org/gnome/Nautilus/window/1"),
            vec![PathBuf::from("/home/me/My Pics"), PathBuf::from("/tmp")]
        );
        assert_eq!(locations_for(out, "/org/gnome/Nautilus/window/2"), vec![PathBuf::from("/etc")]);
        assert!(locations_for(out, "/nope").is_empty());
    }
}

#[cfg(all(test, target_os = "linux"))]
mod probe {
    /// Prints what's under the pointer right now: `cargo test probe -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn probe_pointer() {
        let (x, y) = crate::watch::pointer_position().unwrap();
        let info = crate::watch::window_at(x, y);
        println!("at {x},{y}: {info:?}");
        println!("placement: {:?}", info.and_then(|w| super::resolve(&w, x, y)));
        // The desktop itself, wherever it is in the stack.
        let desktop = super::resolve(&crate::watch::WindowInfo { desktop: true, ..Default::default() }, 100, 200);
        println!("desktop: {desktop:?}");
    }
}
