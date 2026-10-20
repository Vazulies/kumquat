//! The actions wheel: things to do with files that don't change them.

use std::path::{Path, PathBuf};
#[cfg(target_os = "linux")]
use std::process::Stdio;
use std::process::Command;
use std::sync::Mutex;

use anyhow::{bail, Context as _, Result};
use tauri::AppHandle;
use tauri_plugin_dialog::DialogExt;

use crate::engine::{self, Outcome};

/// Kept alive for the life of the app so copied text stays on the X11 clipboard.
static CLIPBOARD: Mutex<Option<arboard::Clipboard>> = Mutex::new(None);

pub fn copy_text(text: &str) -> Result<()> {
    let mut guard = CLIPBOARD.lock().unwrap_or_else(|e| e.into_inner());
    if guard.is_none() {
        *guard = Some(arboard::Clipboard::new().context("no clipboard available")?);
    }
    guard.as_mut().unwrap().set_text(text.to_string())?;
    Ok(())
}

pub fn run(app: &AppHandle, name: &str, paths: &[PathBuf]) -> Result<Outcome> {
    match name {
        "path" => {
            let text = paths.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join("\n");
            copy_text(&text)?;
            Ok(Outcome::Message {
                title: if paths.len() == 1 { "Path copied".into() } else { "Paths copied".into() },
                body: text,
            })
        }
        "terminal" => {
            let first = paths.first().context("nothing selected")?;
            let dir = if first.is_dir() { first.clone() } else { first.parent().map(Path::to_path_buf).unwrap_or_default() };
            open_terminal(&dir)?;
            Ok(Outcome::Files(vec![]))
        }
        "copy" | "move" => {
            let title = if name == "copy" { "Copy to…" } else { "Move to…" };
            let Some(folder) = app.dialog().file().set_title(title).blocking_pick_folder() else {
                return Ok(Outcome::Files(vec![]));
            };
            let folder = folder.into_path().map_err(|e| anyhow::anyhow!("{e}"))?;
            let mut done = Vec::new();
            for p in paths {
                let dest = engine::unique(folder.join(p.file_name().context("bad file name")?));
                if name == "copy" {
                    copy_recursive(p, &dest)?;
                } else if std::fs::rename(p, &dest).is_err() {
                    copy_recursive(p, &dest)?;
                    if p.is_dir() {
                        std::fs::remove_dir_all(p)?;
                    } else {
                        std::fs::remove_file(p)?;
                    }
                }
                done.push(dest);
            }
            Ok(Outcome::Files(done))
        }
        "float" => {
            crate::float::open(app, paths.iter().map(|p| p.to_string_lossy().into_owned()).collect())?;
            Ok(Outcome::Files(vec![]))
        }
        "send" => send_to_phone(paths),
        other => bail!("unknown action {other}"),
    }
}

fn copy_recursive(from: &Path, to: &Path) -> Result<()> {
    if from.is_dir() {
        std::fs::create_dir_all(to)?;
        for entry in std::fs::read_dir(from)? {
            let entry = entry?;
            copy_recursive(&entry.path(), &to.join(entry.file_name()))?;
        }
    } else {
        std::fs::copy(from, to).with_context(|| format!("could not copy {}", from.display()))?;
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn open_terminal(dir: &Path) -> Result<()> {
    let mut candidates: Vec<String> = Vec::new();
    if let Ok(t) = std::env::var("TERMINAL") {
        candidates.push(t);
    }
    candidates.extend(
        ["x-terminal-emulator", "gnome-terminal", "kgx", "konsole", "xfce4-terminal", "tilix", "alacritty", "kitty", "wezterm", "foot", "xterm"]
            .map(String::from),
    );
    for term in candidates {
        let Ok(bin) = which::which(&term) else { continue };
        let mut cmd = Command::new(&bin);
        cmd.current_dir(dir);
        let real = bin.canonicalize().unwrap_or(bin.clone());
        let real_name = real.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        if real_name.starts_with("gnome-terminal") || real_name.starts_with("xfce4-terminal") || real_name == "tilix" {
            cmd.arg(format!("--working-directory={}", dir.display()));
        } else if real_name == "konsole" {
            cmd.arg("--workdir").arg(dir);
        }
        cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
        if cmd.spawn().is_ok() {
            return Ok(());
        }
    }
    bail!("no terminal app found")
}

/// Windows Terminal if it's there, else PowerShell, else the command prompt.
#[cfg(windows)]
fn open_terminal(dir: &Path) -> Result<()> {
    use std::os::windows::process::CommandExt;
    const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;
    if let Ok(wt) = which::which("wt") {
        if Command::new(wt).arg("-d").arg(dir).spawn().is_ok() {
            return Ok(());
        }
    }
    for shell in ["pwsh", "powershell", "cmd"] {
        let Ok(bin) = which::which(shell) else { continue };
        if Command::new(bin).current_dir(dir).creation_flags(CREATE_NEW_CONSOLE).spawn().is_ok() {
            return Ok(());
        }
    }
    bail!("no terminal app found")
}

#[cfg(target_os = "linux")]
const URI_SAFE: &percent_encoding::AsciiSet =
    &percent_encoding::NON_ALPHANUMERIC.remove(b'/').remove(b'-').remove(b'_').remove(b'.').remove(b'~');

#[cfg(target_os = "linux")]
fn file_uri(p: &Path) -> String {
    let path = p.to_string_lossy();
    format!("file://{}", percent_encoding::utf8_percent_encode(&path, URI_SAFE))
}

/// Windows has no phone-sharing command line to call; the item is hidden there.
#[cfg(windows)]
fn send_to_phone(_paths: &[PathBuf]) -> Result<Outcome> {
    bail!("Sending to a phone isn't available on Windows yet. Try Phone Link's own Send option in Explorer.")
}

/// Share through KDE Connect, or GSConnect on GNOME.
#[cfg(target_os = "linux")]
fn send_to_phone(paths: &[PathBuf]) -> Result<Outcome> {
    let files: Vec<&PathBuf> = paths.iter().filter(|p| p.is_file()).collect();
    if files.is_empty() {
        bail!("only files can be sent, not folders");
    }

    if let Ok(kdc) = which::which("kdeconnect-cli") {
        let ids = engine::run_output(Command::new(&kdc).args(["-a", "--id-only"]))?;
        let Some(id) = ids.lines().map(str::trim).find(|l| !l.is_empty()) else {
            bail!("no phone is connected in KDE Connect");
        };
        for f in &files {
            engine::run(Command::new(&kdc).args(["-d", id, "--share"]).arg(f))?;
        }
        return Ok(sent(files.len()));
    }

    if let Ok(gdbus) = which::which("gdbus") {
        let dest = "org.gnome.Shell.Extensions.GSConnect";
        if let Ok(objects) = engine::run_output(Command::new(&gdbus).args([
            "call", "--session", "--dest", dest,
            "--object-path", "/org/gnome/Shell/Extensions/GSConnect",
            "--method", "org.freedesktop.DBus.ObjectManager.GetManagedObjects",
        ])) {
            let device = connected_gsconnect_device(&objects).context("no phone is connected in GSConnect")?;
            for f in &files {
                let arg = format!("[<('{}', false)>]", file_uri(f).replace('\'', "\\'"));
                engine::run(Command::new(&gdbus).args([
                    "call", "--session", "--dest", dest, "--object-path", &device,
                    "--method", "org.gtk.Actions.Activate", "shareFile", &arg, "{}",
                ]))?;
            }
            return Ok(sent(files.len()));
        }
    }

    bail!("To send files to your phone, install GSConnect (GNOME) or KDE Connect and pair your phone.")
}

#[cfg(target_os = "linux")]
fn sent(n: usize) -> Outcome {
    Outcome::Message {
        title: "Sent to your phone".into(),
        body: if n == 1 { "1 file is on its way.".into() } else { format!("{n} files are on their way.") },
    }
}

/// Find a connected device's object path in GetManagedObjects' text output.
#[cfg(target_os = "linux")]
fn connected_gsconnect_device(objects: &str) -> Option<String> {
    let marker = "'/org/gnome/Shell/Extensions/GSConnect/Device/";
    let mut rest = objects;
    while let Some(i) = rest.find(marker) {
        let after = &rest[i + 1..];
        let end = after.find('\'')?;
        let path = &after[..end];
        let section_end = after[end..].find(marker).map(|j| end + j).unwrap_or(after.len());
        let section = &after[end..section_end];
        if section.contains("'Connected': <true>") && section.contains("'Paired': <true>") {
            return Some(path.to_string());
        }
        rest = &after[end..];
    }
    None
}

/// Show files in Explorer, selected.
#[cfg(windows)]
pub fn reveal(paths: &[PathBuf]) {
    crate::win::shell::reveal(paths);
}

#[cfg(windows)]
pub fn open_uri(target: &str) {
    crate::win::shell::open(target);
}

/// Show files in the file manager, selected.
#[cfg(target_os = "linux")]
pub fn reveal(paths: &[PathBuf]) {
    let uris: Vec<String> = paths.iter().map(|p| format!("'{}'", file_uri(p).replace('\'', "\\'"))).collect();
    let ok = which::which("gdbus")
        .ok()
        .and_then(|gdbus| {
            Command::new(gdbus)
                .args([
                    "call", "--session", "--dest", "org.freedesktop.FileManager1",
                    "--object-path", "/org/freedesktop/FileManager1",
                    "--method", "org.freedesktop.FileManager1.ShowItems",
                    &format!("[{}]", uris.join(", ")), "",
                ])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .ok()
        })
        .map(|s| s.success())
        .unwrap_or(false);
    if !ok {
        if let Some(dir) = paths.first().and_then(|p| p.parent()) {
            let _ = Command::new("xdg-open").arg(dir).spawn();
        }
    }
}

#[cfg(target_os = "linux")]
pub fn open_uri(target: &str) {
    let _ = Command::new("xdg-open").arg(target).stdout(Stdio::null()).stderr(Stdio::null()).spawn();
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    #[test]
    fn finds_connected_gsconnect_device() {
        let out = "({objectpath '/org/gnome/Shell/Extensions/GSConnect/Device/aaa': {'org.gnome.Shell.Extensions.GSConnect.Device': {'Connected': <false>, 'Paired': <true>}}, objectpath '/org/gnome/Shell/Extensions/GSConnect/Device/bbb': {'org.gnome.Shell.Extensions.GSConnect.Device': {'Connected': <true>, 'Paired': <true>}}},)";
        assert_eq!(connected_gsconnect_device(out).as_deref(), Some("/org/gnome/Shell/Extensions/GSConnect/Device/bbb"));
    }

    #[test]
    fn encodes_file_uris() {
        assert_eq!(file_uri(Path::new("/home/me/My File.png")), "file:///home/me/My%20File.png");
    }
}
