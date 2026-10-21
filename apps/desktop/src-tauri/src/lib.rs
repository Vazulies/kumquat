mod actions;
mod bins;
mod catalog;
mod editor;
mod engine;
mod float;
#[cfg(target_os = "linux")]
mod integration;
mod jobs;
mod placement;
mod settings;
#[cfg(test)]
mod smoke;
mod watch;
mod wheel;
#[cfg(windows)]
mod win;
#[cfg(target_os = "linux")]
mod x11watch;

#[cfg(windows)]
use win::integration;

use std::path::{Path, PathBuf};

use serde_json::Value;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager, RunEvent, WindowEvent};
use tauri_plugin_autostart::MacosLauncher;
#[cfg(not(windows))]
use tauri_plugin_autostart::ManagerExt as _;
use tauri_plugin_dialog::DialogExt;

use crate::wheel::{Mode, WheelKind};

/// Frontend errors end up in the same log as everything else.
#[tauri::command]
fn js_log(window: tauri::WebviewWindow, message: String) {
    log::warn!("[{}] {message}", window.label());
}

#[tauri::command]
fn get_catalog(paths: Vec<String>) -> catalog::Catalog {
    catalog::for_paths(&paths)
}

#[tauri::command]
async fn run_item(app: AppHandle, id: String, paths: Vec<String>, params: Option<Value>) -> Result<(), String> {
    let item = catalog::find_item(&paths, &id).ok_or("that isn't available for these files")?;
    // The wheel page closes itself: mid-drag it must stay up to catch the drop.
    let placement = wheel::placement();
    match (&item.editor, params) {
        (Some(_), None) => editor::open(&app, item, paths, placement).map_err(|e| e.to_string()),
        (_, params) => {
            jobs::run(app, id, paths, params.unwrap_or(Value::Null), placement);
            Ok(())
        }
    }
}

#[tauri::command]
fn wheel_repaint(app: AppHandle) {
    wheel::repaint(&app);
}

#[tauri::command]
fn wheel_close(app: AppHandle) {
    wheel::close(&app);
}

#[tauri::command]
fn get_settings() -> settings::Settings {
    settings::get()
}

#[tauri::command]
fn save_settings(app: AppHandle, value: settings::Settings) -> Result<settings::Settings, String> {
    settings::set(value.clone()).map_err(|e| e.to_string())?;
    refresh_tray(&app);
    Ok(value)
}

#[tauri::command]
fn tool_status() -> Vec<bins::ToolStatus> {
    bins::status()
}

#[tauri::command]
fn integration_status() -> integration::IntegrationStatus {
    integration::status()
}

#[tauri::command]
fn set_integration(on: bool) -> Result<integration::IntegrationStatus, String> {
    if on { integration::install() } else { integration::uninstall() }.map_err(|e| e.to_string())
}

/// Start at login: the autostart plugin on Linux (it knows about AppImages),
/// Kumquat's own per-user entry on Windows.
fn autostart_enabled(app: &AppHandle) -> bool {
    #[cfg(windows)]
    {
        let _ = app;
        win::autostart::is_enabled()
    }
    #[cfg(not(windows))]
    {
        app.autolaunch().is_enabled().unwrap_or(false)
    }
}

fn set_autostart_on(app: &AppHandle, on: bool) -> Result<(), String> {
    #[cfg(windows)]
    {
        let _ = app;
        if on { win::autostart::enable() } else { win::autostart::disable() }.map_err(|e| e.to_string())
    }
    #[cfg(not(windows))]
    {
        let launcher = app.autolaunch();
        if on { launcher.enable() } else { launcher.disable() }.map_err(|e| e.to_string())
    }
}

#[tauri::command]
fn get_autostart(app: AppHandle) -> bool {
    autostart_enabled(&app)
}

#[tauri::command]
fn set_autostart(app: AppHandle, on: bool) -> Result<bool, String> {
    set_autostart_on(&app, on)?;
    Ok(on)
}

/// Whether drags can be watched in this session (not on Wayland).
fn drag_wheel_available() -> bool {
    cfg!(windows) || (std::env::var("XDG_SESSION_TYPE").as_deref() != Ok("wayland") && std::env::var("DISPLAY").is_ok())
}

#[tauri::command]
fn session_info() -> Value {
    let session = if cfg!(windows) { "windows".into() } else { std::env::var("XDG_SESSION_TYPE").unwrap_or_default() };
    serde_json::json!({
        "sessionType": session,
        "platform": std::env::consts::OS,
        "fileManager": if cfg!(windows) { "Explorer" } else { "Files" },
        "dragWheel": drag_wheel_available(),
    })
}

/// Kumquat should be ready whenever the computer is: start at login by default.
/// Done once, so turning it off in Settings sticks. Development builds are
/// skipped, or they'd register a temporary binary.
fn enable_autostart_once(app: &AppHandle) {
    if cfg!(debug_assertions) || settings::get().autostart_set {
        return;
    }
    match set_autostart_on(app, true) {
        Ok(()) => log::info!("start at login enabled"),
        Err(err) => log::warn!("could not enable start at login: {err}"),
    }
    let _ = settings::update(|s| s.autostart_set = true);
}

fn show_settings(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("settings") {
        let _ = app.run_on_main_thread(move || {
            let _ = w.show();
            let _ = w.unminimize();
            let _ = w.set_focus();
            wheel::activate_window(&w);
        });
    }
}

/// Let the user pick files, then open the wheel for them.
fn choose_files(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        let Some(files) = app.dialog().file().set_title("Choose files for Kumquat").blocking_pick_files() else {
            return;
        };
        let paths: Vec<String> = files
            .into_iter()
            .filter_map(|f| f.into_path().ok())
            .map(|p| p.to_string_lossy().into_owned())
            .collect();
        if !paths.is_empty() {
            wheel::open_wheel(&app, WheelKind::Convert, Mode::Keyboard, paths, None);
        }
    });
}

/// Command line: `kumquat [--wheel [convert|tools|actions]] FILE…`, `--settings`, `--background`.
fn handle_args(app: &AppHandle, args: &[String], cwd: &Path, first_launch: bool) {
    let mut kind = WheelKind::Convert;
    let mut files = Vec::new();
    let mut wants_settings = false;
    let mut background = false;
    let mut iter = args.iter().peekable();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--background" => background = true,
            "--settings" => wants_settings = true,
            "--wheel" => {
                if let Some(next) = iter.peek() {
                    let parsed = match next.as_str() {
                        "convert" => Some(WheelKind::Convert),
                        "tools" => Some(WheelKind::Tools),
                        "actions" => Some(WheelKind::Actions),
                        _ => None,
                    };
                    if let Some(k) = parsed {
                        kind = k;
                        iter.next();
                    }
                }
            }
            a if a.starts_with("--") => {}
            path => {
                let p = PathBuf::from(path.strip_prefix("file://").unwrap_or(path));
                let p = if p.is_absolute() { p } else { cwd.join(p) };
                if p.exists() {
                    files.push(p.to_string_lossy().into_owned());
                }
            }
        }
    }
    if !files.is_empty() {
        wheel::open_wheel(app, kind, Mode::Keyboard, files, None);
    } else if wants_settings || (!background && (!first_launch || !settings::get().onboarded)) {
        show_settings(app);
    }
}

fn refresh_tray(app: &AppHandle) {
    if let Some(tray) = app.tray_by_id("main") {
        let _ = tray.set_menu(build_menu(app).ok());
    }
}

fn build_menu(app: &AppHandle) -> tauri::Result<Menu<tauri::Wry>> {
    let paused = settings::get().paused;
    let choose = MenuItem::with_id(app, "choose", "Choose files…", true, None::<&str>)?;
    let pause = CheckMenuItem::with_id(app, "pause", "Pause drag wheel", true, paused, None::<&str>)?;
    let prefs = MenuItem::with_id(app, "settings", "Settings…", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit Kumquat", true, None::<&str>)?;
    Menu::with_items(app, &[&choose, &pause, &PredefinedMenuItem::separator(app)?, &prefs, &quit])
}

fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let icon = tauri::image::Image::from_bytes(include_bytes!("../icons/tray.png"))?;
    TrayIconBuilder::with_id("main")
        .icon(icon)
        .tooltip("Kumquat")
        .menu(&build_menu(app)?)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "choose" => choose_files(app),
            "pause" => {
                let _ = settings::update(|s| s.paused = !s.paused);
                refresh_tray(app);
            }
            "settings" => show_settings(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .build(app)?;
    Ok(())
}

pub fn run() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("kumquat_lib=info")).init();

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, argv, cwd| {
            handle_args(app, argv.get(1..).unwrap_or(&[]), Path::new(&cwd), false);
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_drag::init())
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, Some(vec!["--background"])))
        .on_page_load(|webview, payload| {
            log::debug!("{} loaded {}", webview.label(), payload.url());
        })
        .invoke_handler(tauri::generate_handler![
            js_log,
            get_catalog,
            run_item,
            wheel_close,
            wheel_repaint,
            get_settings,
            save_settings,
            tool_status,
            integration_status,
            set_integration,
            get_autostart,
            set_autostart,
            session_info,
            editor::editor_context,
            editor::editor_submit,
            editor::editor_cancel,
            editor::image_preview,
            editor::media_info,
            editor::media_frame,
            editor::media_waveform,
            editor::pdf_info,
            editor::pdf_page,
            float::float_context,
            float::float_close,
        ])
        .setup(|app| {
            bins::init(app.path().resource_dir().ok());
            let handle = app.handle().clone();
            wheel::prepare(&handle);
            build_tray(&handle)?;

            if drag_wheel_available() {
                let h = handle.clone();
                watch::spawn(move |signal| wheel::handle_signal(&h, signal));
            } else {
                log::warn!("not an X11 session: the drag wheel is off; use the file manager shortcut instead");
            }

            enable_autostart_once(&handle);

            let args: Vec<String> = std::env::args().skip(1).collect();
            let cwd = std::env::current_dir().unwrap_or_default();
            handle_args(&handle, &args, &cwd, true);
            Ok(())
        })
        .on_window_event(|window, event| match event {
            WindowEvent::CloseRequested { api, .. } if window.label() == "settings" => {
                api.prevent_close();
                let _ = window.hide();
            }
            WindowEvent::Destroyed => {
                editor::forget(window.label());
                float::forget(window.label());
            }
            _ => {}
        })
        .build(tauri::generate_context!())
        .expect("error while building Kumquat")
        .run(|_app, event| {
            // Keep running in the tray when windows close; only "Quit" exits.
            if let RunEvent::ExitRequested { api, code, .. } = event {
                if code.is_none() {
                    api.prevent_exit();
                }
            }
        });
}
