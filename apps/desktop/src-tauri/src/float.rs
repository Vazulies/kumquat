//! Float: a small always-on-top shelf that holds files until you drag them out.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;

use anyhow::Result;
use serde::Serialize;
use tauri::{AppHandle, PhysicalPosition, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

use crate::engine::TempDir;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Floating {
    paths: Vec<String>,
    /// PNG file used as the drag image.
    icon: String,
    /// Data URL preview for images and videos.
    preview: Option<String>,
    #[serde(skip)]
    thumbnail: Option<String>,
}

static FLOATS: Mutex<Option<HashMap<String, Floating>>> = Mutex::new(None);

pub fn open(app: &AppHandle, paths: Vec<String>) -> Result<()> {
    let label = format!("float-{}", uuid::Uuid::new_v4().simple());
    let thumbnail = paths.first().and_then(|p| thumbnail(Path::new(p)));
    let preview = thumbnail.as_ref().and_then(|t| std::fs::read(t).ok()).map(|b| {
        use base64::Engine as _;
        format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(b))
    });
    let icon = thumbnail.clone().unwrap_or_else(|| crate::jobs::icon_path().to_string());
    FLOATS
        .lock()
        .unwrap()
        .get_or_insert_with(HashMap::new)
        .insert(label.clone(), Floating { paths, icon, preview, thumbnail });
    let window = WebviewWindowBuilder::new(app, &label, WebviewUrl::App("float.html".into()))
        .title("Kumquat Float")
        .inner_size(172.0, 196.0)
        .resizable(false)
        .decorations(false)
        .transparent(true)
        .always_on_top(true)
        .skip_taskbar(true)
        .visible(false)
        .build()?;
    if let Some((x, y)) = crate::watch::pointer_position() {
        let _ = window.set_position(PhysicalPosition::new(x + 24, y - 98));
    }
    window.show()?;
    Ok(())
}

/// A small preview for images and videos; other files show their type instead.
fn thumbnail(path: &Path) -> Option<String> {
    use crate::catalog::{kind_of, Kind};
    let tmp = TempDir::new().ok()?;
    let dest = tmp.join("thumb.png");
    match kind_of(path) {
        Kind::Image => {
            crate::engine::image::preview(path, 256, &dest).ok()?;
        }
        Kind::Video => {
            let jpg = tmp.join("thumb.jpg");
            crate::engine::av::frame(path, 1.0, 256, &jpg).ok()?;
            std::fs::rename(&jpg, &dest).ok()?;
        }
        _ => return None,
    }
    let keep = dirs::cache_dir()?.join("kumquat").join(format!("float-{}.png", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir_all(keep.parent()?).ok()?;
    std::fs::copy(&dest, &keep).ok()?;
    Some(keep.to_string_lossy().into_owned())
}

#[tauri::command]
pub fn float_context(window: WebviewWindow) -> Option<Floating> {
    FLOATS.lock().unwrap().as_ref()?.get(window.label()).cloned()
}

#[tauri::command]
pub fn float_close(window: WebviewWindow) {
    forget(window.label());
    let _ = window.close();
}

pub fn forget(label: &str) {
    if let Some(f) = FLOATS.lock().unwrap().as_mut().and_then(|m| m.remove(label)) {
        if let Some(t) = f.thumbnail {
            let _ = std::fs::remove_file(t);
        }
    }
}
