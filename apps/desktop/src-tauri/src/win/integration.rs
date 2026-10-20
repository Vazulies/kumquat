//! Explorer integration: "Send to › Kumquat" for any selection, and a
//! "Kumquat" entry on the right-click menu of a single file or folder.

use std::path::PathBuf;

use anyhow::{bail, Context as _, Result};
use serde::Serialize;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
use windows::Win32::System::Registry::{RegDeleteTreeW, RegGetValueW, RegSetKeyValueW, HKEY_CURRENT_USER, REG_SZ, RRF_RT_ANY};

use super::wide;

/// Per-user right-click verbs, for files and for folders.
const VERB_KEYS: [&str; 2] = [r"Software\Classes\*\shell\Kumquat", r"Software\Classes\Directory\shell\Kumquat"];

fn send_to_link() -> Option<PathBuf> {
    // %APPDATA%\Microsoft\Windows\SendTo
    Some(dirs::config_dir()?.join(r"Microsoft\Windows\SendTo\Kumquat.lnk"))
}

fn set_value(key: &str, name: Option<&str>, value: &str) -> Result<()> {
    let key = wide(key);
    let name = name.map(wide);
    let data = wide(value);
    let err = unsafe {
        RegSetKeyValueW(
            HKEY_CURRENT_USER,
            PCWSTR(key.as_ptr()),
            name.as_ref().map(|n| PCWSTR(n.as_ptr())).unwrap_or(PCWSTR::null()),
            REG_SZ.0,
            Some(data.as_ptr() as *const _),
            (data.len() * 2) as u32,
        )
    };
    if err != ERROR_SUCCESS {
        bail!("could not write to the registry (error {})", err.0);
    }
    Ok(())
}

fn key_exists(key: &str) -> bool {
    let key = wide(key);
    let err = unsafe { RegGetValueW(HKEY_CURRENT_USER, PCWSTR(key.as_ptr()), PCWSTR::null(), RRF_RT_ANY, None, None, None) };
    err == ERROR_SUCCESS
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IntegrationStatus {
    installed: bool,
    managers: Vec<String>,
    shortcut: String,
    hint: String,
}

pub fn status() -> IntegrationStatus {
    let installed = send_to_link().map(|l| l.is_file()).unwrap_or(false) && VERB_KEYS.iter().all(|k| key_exists(k));
    IntegrationStatus {
        installed,
        managers: vec!["Explorer".into()],
        shortcut: String::new(),
        hint: "Select files in Explorer, then right-click › Send to › Kumquat (on Windows 11 it's under Show more options). \
               A single file or folder also gets a Kumquat entry of its own. Arrow keys pick, Enter applies, Tab switches wheels."
            .into(),
    }
}

pub fn install() -> Result<IntegrationStatus> {
    let exe = std::env::current_exe().context("can't find Kumquat's own path")?;
    let exe_str = exe.to_string_lossy();
    for key in VERB_KEYS {
        set_value(key, None, "Kumquat")?;
        set_value(key, Some("Icon"), &format!("\"{exe_str}\",0"))?;
        set_value(key, Some("MultiSelectModel"), "Single")?;
        set_value(&format!(r"{key}\command"), None, &format!("\"{exe_str}\" --wheel \"%1\""))?;
    }
    let link = send_to_link().context("can't find the Send to folder")?;
    if let Some(dir) = link.parent() {
        std::fs::create_dir_all(dir)?;
    }
    super::shell::create_shortcut(&link, &exe, "--wheel", "Open the Kumquat wheel for these files")
        .map_err(|e| anyhow::anyhow!("could not create the Send to shortcut: {e}"))?;
    Ok(status())
}

pub fn uninstall() -> Result<IntegrationStatus> {
    for key in VERB_KEYS {
        let wkey = wide(key);
        let err = unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, PCWSTR(wkey.as_ptr())) };
        if err != ERROR_SUCCESS && err != ERROR_FILE_NOT_FOUND {
            bail!("could not remove {key} from the registry (error {})", err.0);
        }
    }
    if let Some(link) = send_to_link().filter(|l| l.exists()) {
        std::fs::remove_file(link)?;
    }
    Ok(status())
}
