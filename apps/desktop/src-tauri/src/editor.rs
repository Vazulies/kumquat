//! Editor windows for tools that need a choice first (crop area, trim range, …),
//! plus the preview commands those windows use.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;

use anyhow::Result;
use base64::Engine as _;
use serde::Serialize;
use tauri::{AppHandle, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

use crate::catalog::Item;
use crate::placement::Placement;
use crate::engine::{self, TempDir};

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorJob {
    pub item: Item,
    pub paths: Vec<String>,
    #[serde(skip)]
    pub placement: Option<Placement>,
}

static JOBS: Mutex<Option<HashMap<String, EditorJob>>> = Mutex::new(None);

pub fn open(app: &AppHandle, item: Item, paths: Vec<String>, placement: Option<Placement>) -> Result<()> {
    let label = format!("editor-{}", uuid::Uuid::new_v4().simple());
    let (w, h) = match item.editor.as_deref() {
        Some("params") => (420.0, 300.0),
        Some("media-trim") => (860.0, 520.0),
        Some("pdf-pages") => (900.0, 680.0),
        _ => (960.0, 720.0),
    };
    let file = paths
        .first()
        .and_then(|p| Path::new(p).file_name())
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let title = format!("{} — {}", item.label, file);
    JOBS.lock().unwrap().get_or_insert_with(HashMap::new).insert(label.clone(), EditorJob { item, paths, placement });
    WebviewWindowBuilder::new(app, &label, WebviewUrl::App("editor.html".into()))
        .title(title)
        .inner_size(w, h)
        .min_inner_size(380.0, 260.0)
        .center()
        .focused(true)
        .build()?;
    Ok(())
}

#[tauri::command]
pub fn editor_context(window: WebviewWindow) -> Option<EditorJob> {
    JOBS.lock().unwrap().as_ref()?.get(window.label()).cloned()
}

#[tauri::command]
pub fn editor_submit(app: AppHandle, window: WebviewWindow, params: serde_json::Value) -> Result<(), String> {
    let job = JOBS
        .lock()
        .unwrap()
        .as_mut()
        .and_then(|m| m.remove(window.label()))
        .ok_or("this editor has already finished")?;
    crate::jobs::run(app, job.item.id, job.paths, params, job.placement);
    let _ = window.close();
    Ok(())
}

#[tauri::command]
pub fn editor_cancel(window: WebviewWindow) {
    if let Some(m) = JOBS.lock().unwrap().as_mut() {
        m.remove(window.label());
    }
    let _ = window.close();
}

fn data_url(mime: &str, bytes: &[u8]) -> String {
    format!("data:{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImagePreview {
    src: String,
    width: u32,
    height: u32,
}

/// A browser-friendly preview of any image, plus the original's oriented size.
#[tauri::command]
pub async fn image_preview(path: String, max: u32) -> Result<ImagePreview, String> {
    let tmp = TempDir::new().map_err(|e| e.to_string())?;
    let dest = tmp.join("preview.png");
    let (width, height) = engine::image::preview(Path::new(&path), max, &dest).map_err(|e| e.to_string())?;
    let bytes = std::fs::read(&dest).map_err(|e| e.to_string())?;
    Ok(ImagePreview { src: data_url("image/png", &bytes), width, height })
}

#[tauri::command]
pub async fn media_info(path: String) -> Result<engine::av::Probe, String> {
    engine::av::probe(Path::new(&path)).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn media_frame(path: String, at: f64, width: u32) -> Result<String, String> {
    let tmp = TempDir::new().map_err(|e| e.to_string())?;
    let dest = tmp.join("frame.jpg");
    engine::av::frame(Path::new(&path), at, width, &dest).map_err(|e| e.to_string())?;
    let bytes = std::fs::read(&dest).map_err(|e| e.to_string())?;
    Ok(data_url("image/jpeg", &bytes))
}

#[tauri::command]
pub async fn media_waveform(path: String, buckets: usize) -> Result<Vec<f32>, String> {
    engine::av::waveform(Path::new(&path), buckets.clamp(16, 4000)).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn pdf_info(path: String) -> Result<usize, String> {
    engine::pdf::page_count(Path::new(&path)).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn pdf_page(path: String, index: usize, width: u32) -> Result<String, String> {
    let png = engine::pdf::render_page_png(Path::new(&path), index, width).map_err(|e| e.to_string())?;
    Ok(data_url("image/png", &png))
}

/// Drop the job of an editor the user closed with the window's close button.
pub fn forget(label: &str) {
    if let Some(m) = JOBS.lock().unwrap().as_mut() {
        m.remove(label);
    }
}
