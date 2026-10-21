//! User settings, stored as JSON in ~/.config/kumquat/settings.json.

use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};

/// A modifier key that opens one of the wheels while a file is being dragged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Modifier {
    Shift,
    Control,
    Alt,
    Super,
    /// Not a modifier, but free during a drag and never clashes with layout switching.
    Space,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    pub convert_key: Modifier,
    pub tools_key: Modifier,
    pub actions_key: Modifier,
    /// Where results go: next to the original file, or into `output_folder`.
    pub output_beside_original: bool,
    pub output_folder: Option<String>,
    /// 1–100, used for JPEG, WebP, AVIF and HEIC output.
    pub image_quality: u8,
    /// x264 constant rate factor (lower is better quality).
    pub video_crf: u8,
    /// Resolution used when PDF pages and slides become images.
    pub pdf_dpi: u32,
    pub reveal_after: bool,
    pub notifications: bool,
    /// When paused, dragging files never opens the wheel.
    pub paused: bool,
    pub onboarded: bool,
    /// Start-at-login is switched on once, on first run; after that it's the user's call.
    pub autostart_set: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            convert_key: Modifier::Shift,
            tools_key: Modifier::Control,
            actions_key: Modifier::Space,
            output_beside_original: true,
            output_folder: None,
            image_quality: 90,
            video_crf: 20,
            pdf_dpi: 300,
            reveal_after: false,
            notifications: true,
            paused: false,
            onboarded: false,
            autostart_set: false,
        }
    }
}

fn path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("kumquat")
        .join("settings.json")
}

fn store() -> &'static Mutex<Settings> {
    static STORE: OnceLock<Mutex<Settings>> = OnceLock::new();
    STORE.get_or_init(|| {
        let loaded = std::fs::read_to_string(path())
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        Mutex::new(loaded)
    })
}

pub fn get() -> Settings {
    store().lock().unwrap().clone()
}

pub fn set(new: Settings) -> anyhow::Result<()> {
    let p = path();
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&p, serde_json::to_string_pretty(&new)?)?;
    *store().lock().unwrap() = new;
    Ok(())
}

pub fn update(f: impl FnOnce(&mut Settings)) -> anyhow::Result<Settings> {
    let mut s = get();
    f(&mut s);
    set(s.clone())?;
    Ok(s)
}
