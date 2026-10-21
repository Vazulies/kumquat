//! Start at login on Windows: a per-user `Run` entry with the path quoted.
//!
//! The autostart plugin writes machine-wide when it has admin rights and
//! leaves the path unquoted, which breaks for user names with spaces, so
//! Windows does it here instead.

use anyhow::{bail, Context as _, Result};
use windows::core::PCWSTR;
use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
use windows::Win32::System::Registry::{
    RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, REG_SZ, RRF_RT_REG_SZ,
};

use super::wide;

const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
/// The name the plugin used too, so this replaces anything it wrote.
const NAME: &str = "Kumquat";

pub fn enable() -> Result<()> {
    let exe = std::env::current_exe().context("can't find Kumquat's own path")?;
    let data = wide(format!("\"{}\" --background", exe.display()));
    let (key, name) = (wide(RUN), wide(NAME));
    let err = unsafe {
        RegSetKeyValueW(
            HKEY_CURRENT_USER,
            PCWSTR(key.as_ptr()),
            PCWSTR(name.as_ptr()),
            REG_SZ.0,
            Some(data.as_ptr() as *const _),
            (data.len() * 2) as u32,
        )
    };
    if err != ERROR_SUCCESS {
        bail!("could not add Kumquat to startup apps (error {})", err.0);
    }
    remove_machine_wide();
    Ok(())
}

pub fn disable() -> Result<()> {
    let (key, name) = (wide(RUN), wide(NAME));
    let err = unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, PCWSTR(key.as_ptr()), PCWSTR(name.as_ptr())) };
    if err != ERROR_SUCCESS && err != ERROR_FILE_NOT_FOUND {
        bail!("could not remove Kumquat from startup apps (error {})", err.0);
    }
    remove_machine_wide();
    Ok(())
}

pub fn is_enabled() -> bool {
    let (key, name) = (wide(RUN), wide(NAME));
    let err = unsafe { RegGetValueW(HKEY_CURRENT_USER, PCWSTR(key.as_ptr()), PCWSTR(name.as_ptr()), RRF_RT_REG_SZ, None, None, None) };
    err == ERROR_SUCCESS
}

/// Clear an entry an earlier version may have written for all users. Needs
/// admin rights, so failing here is fine.
fn remove_machine_wide() {
    let (key, name) = (wide(RUN), wide(NAME));
    let _ = unsafe { RegDeleteKeyValueW(HKEY_LOCAL_MACHINE, PCWSTR(key.as_ptr()), PCWSTR(name.as_ptr())) };
}
