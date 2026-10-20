//! File manager integration: a "Kumquat" script with a keyboard shortcut, the
//! Linux stand-in for pressing Enter on a file.

use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use anyhow::{Context as _, Result};
use serde::Serialize;

pub const SHORTCUT: &str = "F4";
const MARKER: &str = "# Installed by Kumquat";

fn script(exe: &str) -> String {
    format!(
        r#"#!/bin/sh
{MARKER}
# Opens the Kumquat wheel for the selected files.
SELECTED="${{NAUTILUS_SCRIPT_SELECTED_FILE_PATHS:-${{NEMO_SCRIPT_SELECTED_FILE_PATHS:-$CAJA_SCRIPT_SELECTED_FILE_PATHS}}}}"
if [ -n "$SELECTED" ]; then
  IFS='
'
  set -- $SELECTED
fi
exec "{exe}" --wheel "$@"
"#
    )
}

/// Script folders of the file managers that are installed.
fn script_dirs() -> Vec<(&'static str, PathBuf)> {
    let data = dirs::data_dir().unwrap_or_default();
    let config = dirs::config_dir().unwrap_or_default();
    let mut out = Vec::new();
    if which::which("nautilus").is_ok() {
        out.push(("Files", data.join("nautilus/scripts")));
    }
    if which::which("nemo").is_ok() {
        out.push(("Nemo", data.join("nemo/scripts")));
    }
    if which::which("caja").is_ok() {
        out.push(("Caja", config.join("caja/scripts")));
    }
    out
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IntegrationStatus {
    installed: bool,
    managers: Vec<String>,
    shortcut: String,
    /// How to use it, for the settings page.
    hint: String,
}

pub fn status() -> IntegrationStatus {
    let dirs = script_dirs();
    let installed = !dirs.is_empty() && dirs.iter().all(|(_, d)| d.join("Kumquat").is_file());
    let managers: Vec<String> = dirs.iter().map(|(n, _)| n.to_string()).collect();
    let hint = if managers.is_empty() {
        "No supported file manager (Files, Nemo or Caja) was found.".to_string()
    } else {
        format!(
            "Select files in {} and press {SHORTCUT}, or right-click › Scripts › Kumquat. Arrow keys pick, Enter applies, Tab switches wheels.",
            managers.join(", ")
        )
    };
    IntegrationStatus { installed, managers, shortcut: SHORTCUT.into(), hint }
}

pub fn install() -> Result<IntegrationStatus> {
    let exe = std::env::current_exe().context("can't find Kumquat's own path")?;
    // An AppImage runs from a temporary mount; point at the AppImage itself.
    let exe = std::env::var("APPIMAGE").map(PathBuf::from).unwrap_or(exe);
    let body = script(&exe.to_string_lossy());
    for (_, dir) in script_dirs() {
        std::fs::create_dir_all(&dir)?;
        let path = dir.join("Kumquat");
        std::fs::write(&path, &body)?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))?;
    }
    // GNOME Files reads keyboard shortcuts for scripts from this file.
    if which::which("nautilus").is_ok() {
        let accels = dirs::config_dir().unwrap_or_default().join("nautilus/scripts-accels");
        let current = std::fs::read_to_string(&accels).unwrap_or_default();
        if !current.lines().any(|l| l.split_whitespace().nth(1) == Some("Kumquat")) {
            std::fs::create_dir_all(accels.parent().unwrap())?;
            let mut next = current;
            if !next.is_empty() && !next.ends_with('\n') {
                next.push('\n');
            }
            next.push_str(&format!("{SHORTCUT} Kumquat\n"));
            std::fs::write(&accels, next)?;
        }
    }
    Ok(status())
}

pub fn uninstall() -> Result<IntegrationStatus> {
    for (_, dir) in script_dirs() {
        let path = dir.join("Kumquat");
        if std::fs::read_to_string(&path).map(|s| s.contains(MARKER)).unwrap_or(false) {
            std::fs::remove_file(&path)?;
        }
    }
    let accels = dirs::config_dir().unwrap_or_default().join("nautilus/scripts-accels");
    if let Ok(current) = std::fs::read_to_string(&accels) {
        let kept: Vec<&str> = current
            .lines()
            .filter(|l| l.split_whitespace().nth(1) != Some("Kumquat"))
            .collect();
        std::fs::write(&accels, kept.join("\n") + if kept.is_empty() { "" } else { "\n" })?;
    }
    Ok(status())
}
