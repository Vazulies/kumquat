//! Runs wheel items in the background and reports back with notifications.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
#[cfg(target_os = "linux")]
use std::time::Duration;
use std::time::Instant;

use anyhow::{bail, Result};
use notify_rust::{Notification, NotificationHandle, Timeout};
use serde_json::Value;
use tauri::AppHandle;

use crate::engine::{self, Ctx, Outcome};
use crate::placement::{self, Placement};
use crate::{actions, catalog, settings};

pub fn icon_path() -> &'static str {
    static ICON: OnceLock<String> = OnceLock::new();
    ICON.get_or_init(|| {
        let path = dirs::cache_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join("kumquat")
            .join("icon.png");
        if !path.exists() {
            let _ = std::fs::create_dir_all(path.parent().unwrap());
            let _ = std::fs::write(&path, include_bytes!("../icons/128x128.png"));
        }
        path.to_string_lossy().into_owned()
    })
}

fn base_notification(summary: &str, body: &str) -> Notification {
    let mut n = Notification::new();
    n.appname("Kumquat").summary(summary).body(body).icon(icon_path());
    // Toasts need the AppUserModelID the installer gives the Start menu shortcut;
    // unpackaged dev builds borrow PowerShell's so they still show.
    #[cfg(windows)]
    if !cfg!(debug_assertions) {
        n.app_id("app.kumquat.desktop");
    }
    n
}

/// What the "show the result" button says.
const SHOW_LABEL: &str = if cfg!(windows) { "Show in Explorer" } else { "Show in Files" };

/// Take a progress notification off screen.
fn dismiss(handle: Option<NotificationHandle>) {
    #[cfg(target_os = "linux")]
    if let Some(h) = handle {
        h.close();
    }
    #[cfg(windows)]
    let _ = handle;
}

/// A notification that appears only for jobs taking more than a moment.
#[cfg_attr(windows, allow(dead_code))]
struct Progress {
    title: String,
    started: Instant,
    handle: Mutex<Option<NotificationHandle>>,
    last: Mutex<f32>,
}

impl Progress {
    fn new(title: String) -> Self {
        Self { title, started: Instant::now(), handle: Mutex::new(None), last: Mutex::new(-1.0) }
    }

    /// Windows toasts can't be updated in place, so long jobs just report when done.
    #[cfg(windows)]
    fn update(&self, _fraction: f32) {}

    #[cfg(target_os = "linux")]
    fn update(&self, fraction: f32) {
        if !settings::get().notifications || self.started.elapsed() < Duration::from_millis(1500) {
            return;
        }
        let mut last = self.last.lock().unwrap();
        if fraction - *last < 0.05 && *last >= 0.0 {
            return;
        }
        *last = fraction;
        let body = format!("{}% done", (fraction * 100.0).round() as u32);
        let mut handle = self.handle.lock().unwrap();
        match handle.as_mut() {
            Some(h) => {
                h.body(&body);
                h.hint(notify_rust::Hint::CustomInt("value".into(), (fraction * 100.0) as i32));
                let _ = h.update();
            }
            None => {
                let mut n = base_notification(&self.title, &body);
                n.timeout(Timeout::Never)
                    .hint(notify_rust::Hint::CustomInt("value".into(), (fraction * 100.0) as i32))
                    .hint(notify_rust::Hint::Transient(true));
                *handle = n.show().ok();
            }
        }
    }

    fn take(&self) -> Option<NotificationHandle> {
        self.handle.lock().unwrap().take()
    }
}

/// Short human description, e.g. "photo.png → WEBP" or "Compress video.mp4".
fn describe(id: &str, paths: &[String]) -> String {
    let what = match paths {
        [one] => std::path::Path::new(one)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default(),
        many => format!("{} items", many.len()),
    };
    if let Some(ext) = id.strip_prefix("convert:") {
        return format!("{what} → {}", ext.to_uppercase());
    }
    let label = catalog::find_item(paths, id).map(|i| i.label).unwrap_or_else(|| id.to_string());
    format!("{label}: {what}")
}

pub fn run(app: AppHandle, id: String, paths: Vec<String>, params: Value, placement: Option<Placement>) {
    std::thread::spawn(move || {
        let title = describe(&id, &paths);
        log::info!("starting {title} (placement: {placement:?})");
        let progress = Progress::new(title.clone());
        let report = |f: f32| progress.update(f);
        let files: Vec<PathBuf> = paths.iter().map(PathBuf::from).collect();
        engine::begin_job(placement.as_ref().map(|p| p.dir.clone()));
        let result = execute(&app, &id, &files, &params, &report).and_then(|outcome| match outcome {
            Outcome::Files(out) => {
                let point = placement.as_ref().and_then(|p| p.desktop_point);
                let mover = |from: &Path, to: &Path| match point {
                    Some(pt) => {
                        placement::mark_drop_position(from, pt);
                        placement::move_with_metadata(from, to)?;
                        placement::mark_drop_position(to, pt);
                        Ok(())
                    }
                    None => std::fs::rename(from, to),
                };
                engine::finish_job(out, &mover).map(Outcome::Files)
            }
            other => {
                engine::abort_job();
                Ok(other)
            }
        });
        if result.is_err() {
            engine::abort_job();
        }
        finish(&title, &progress, result);
    });
}

fn execute(app: &AppHandle, id: &str, paths: &[PathBuf], params: &Value, report: &(dyn Fn(f32) + Sync)) -> Result<Outcome> {
    if paths.is_empty() {
        bail!("no files were given");
    }
    for p in paths {
        if !p.exists() {
            bail!("{} no longer exists", p.display());
        }
    }
    if let Some(target) = id.strip_prefix("convert:") {
        let mut out = Vec::new();
        let n = paths.len() as f32;
        for (i, p) in paths.iter().enumerate() {
            let sub = move |f: f32| report((i as f32 + f) / n);
            let ctx = Ctx { settings: settings::get(), params, progress: &sub };
            out.extend(engine::convert(p, target, &ctx)?);
        }
        return Ok(Outcome::Files(out));
    }
    if let Some(tool) = id.strip_prefix("tool:") {
        let ctx = Ctx { settings: settings::get(), params, progress: report };
        return engine::tool(tool, paths, &ctx);
    }
    if let Some(action) = id.strip_prefix("action:") {
        return actions::run(app, action, paths);
    }
    bail!("unknown item {id}")
}

fn finish(title: &str, progress: &Progress, result: Result<Outcome>) {
    let s = settings::get();
    let handle = progress.take();
    let (summary, body, files, link) = match result {
        Ok(Outcome::Files(files)) if files.is_empty() => {
            dismiss(handle);
            return;
        }
        Ok(Outcome::Files(files)) => {
            log::info!("finished {title}: {files:?}");
            if s.reveal_after {
                actions::reveal(&files);
            }
            let body = match files.as_slice() {
                [one] => format!("Saved {}", one.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()),
                many => format!("Saved {} files", many.len()),
            };
            ("Done".to_string(), body, files, None)
        }
        Ok(Outcome::Message { title, body }) => {
            let link = (body.starts_with("http://") || body.starts_with("https://")).then(|| body.clone());
            (title, body, vec![], link)
        }
        Err(err) => {
            log::warn!("{title} failed: {err:#}");
            (format!("Couldn't finish {title}"), format!("{err}"), vec![], None)
        }
    };
    if !s.notifications {
        dismiss(handle);
        return;
    }
    let summary = if summary == "Done" { title.to_string() } else { summary };

    let mut n = base_notification(&summary, &body);
    n.timeout(Timeout::Milliseconds(6000));
    // On Windows every action is a button, so there's no separate "default" one.
    if !files.is_empty() && !s.reveal_after {
        if cfg!(target_os = "linux") {
            n.action("default", SHOW_LABEL);
        }
        n.action("show", SHOW_LABEL);
    }
    if link.is_some() {
        if cfg!(target_os = "linux") {
            n.action("default", "Open");
        }
        n.action("open", "Open link");
    }
    // Replace the progress notification rather than stacking a second one.
    #[cfg(target_os = "linux")]
    let shown = match handle {
        Some(mut h) => {
            *h = n;
            let _ = h.update();
            Some(h)
        }
        None => n.show().ok(),
    };
    #[cfg(windows)]
    let shown = {
        dismiss(handle);
        n.show().ok()
    };
    if let Some(h) = shown {
        if files.is_empty() && link.is_none() {
            return;
        }
        h.wait_for_action(|action| match action {
            "default" | "show" if !files.is_empty() => actions::reveal(&files),
            "default" | "open" => {
                if let Some(url) = &link {
                    actions::open_uri(url);
                }
            }
            _ => {}
        });
    }
}
