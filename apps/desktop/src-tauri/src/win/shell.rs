//! Explorer and the Windows shell: which folder a window shows, putting desktop
//! icons where the file was dropped, showing files, and shortcuts.

use std::path::{Path, PathBuf};
use std::time::Duration;

use windows::core::{Interface, PCWSTR};
use windows::Win32::Foundation::{HWND, POINT};
use windows::Win32::Graphics::Gdi::ScreenToClient;
use windows::Win32::System::Com::{CoCreateInstance, IDispatch, IPersistFile, IServiceProvider, CLSCTX_ALL};
use windows::Win32::System::Variant::{VARIANT, VT_I4};
use windows::Win32::UI::Shell::Common::ITEMIDLIST;
use windows::Win32::UI::Shell::{
    IFolderView, IPersistFolder2, IShellBrowser, IShellFolder, IShellLinkW, IShellWindows, IWebBrowser2,
    ILCreateFromPathW, ILFree, SHGetPathFromIDListW, SHOpenFolderAndSelectItems, ShellExecuteW, ShellLink,
    ShellWindows, SID_STopLevelBrowser, SVSI_POSITIONITEM, SWC_DESKTOP, SWFO_NEEDDISPATCH,
};
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

use super::{wide, Com};

fn int_variant(i: i32) -> VARIANT {
    let mut v = VARIANT::default();
    unsafe {
        (*v.Anonymous.Anonymous).vt = VT_I4;
        (*v.Anonymous.Anonymous).Anonymous.lVal = i;
    }
    v
}

/// The view (folder contents) of an Explorer window or the desktop.
fn folder_view(browser: &IDispatch) -> windows::core::Result<(IFolderView, HWND)> {
    unsafe {
        let provider: IServiceProvider = browser.cast()?;
        let shell: IShellBrowser = provider.QueryService(&SID_STopLevelBrowser)?;
        let view = shell.QueryActiveShellView()?;
        let window = view.GetWindow()?;
        Ok((view.cast()?, window))
    }
}

fn view_path(view: &IFolderView) -> Option<PathBuf> {
    unsafe {
        let folder: IPersistFolder2 = view.GetFolder().ok()?;
        let pidl = folder.GetCurFolder().ok()?;
        let mut buf = [0u16; 260];
        let ok = SHGetPathFromIDListW(pidl, &mut buf).as_bool();
        ILFree(Some(pidl));
        ok.then(|| PathBuf::from(super::from_wide(&buf)))
    }
}

/// The folder an Explorer window (`CabinetWClass`) is showing. With tabs, one
/// window holds several views; the visible one is the active tab.
pub fn explorer_folder(window: u64) -> Option<PathBuf> {
    let _com = Com::init();
    unsafe {
        let windows: IShellWindows = CoCreateInstance(&ShellWindows, None, CLSCTX_ALL).ok()?;
        let mut fallback = None;
        for i in 0..windows.Count().ok()? {
            let Ok(item) = windows.Item(&int_variant(i)) else { continue };
            let Ok(browser) = item.cast::<IWebBrowser2>() else { continue };
            if browser.HWND().map(|h| h.0 as usize as u64).ok() != Some(window) {
                continue;
            }
            let Ok((view, view_window)) = folder_view(&item) else { continue };
            let Some(path) = view_path(&view) else { continue };
            if windows::Win32::UI::WindowsAndMessaging::IsWindowVisible(view_window).as_bool() {
                return Some(path);
            }
            fallback.get_or_insert(path);
        }
        fallback
    }
}

/// Put a desktop icon at a screen point, the way a drop there would have.
/// Explorer only learns about new files a moment after they appear, so this
/// tries again for a little while.
pub fn place_on_desktop(path: &Path, (x, y): (i32, i32)) {
    let Some(name) = path.file_name().map(|n| n.to_os_string()) else { return };
    std::thread::spawn(move || {
        let _com = Com::init();
        for delay in [300, 700, 1500] {
            std::thread::sleep(Duration::from_millis(delay));
            match position_desktop_item(&name, x, y) {
                Ok(()) => return,
                Err(err) => log::debug!("desktop icon not placed yet: {err}"),
            }
        }
    });
}

fn position_desktop_item(name: &std::ffi::OsStr, x: i32, y: i32) -> windows::core::Result<()> {
    unsafe {
        let windows: IShellWindows = CoCreateInstance(&ShellWindows, None, CLSCTX_ALL)?;
        let mut hwnd = 0i32;
        let desktop = windows.FindWindowSW(
            &int_variant(0), // CSIDL_DESKTOP
            &VARIANT::default(),
            SWC_DESKTOP,
            &mut hwnd,
            SWFO_NEEDDISPATCH,
        )?;
        let (view, view_window) = folder_view(&desktop)?;
        let folder: IShellFolder = view.GetFolder()?;
        let name = wide(name);
        let mut child: *mut ITEMIDLIST = std::ptr::null_mut();
        folder.ParseDisplayName(HWND::default(), None, PCWSTR(name.as_ptr()), None, &mut child, std::ptr::null_mut())?;
        // Icon positions are the item's top-left corner in the view; centre it on the point.
        let mut spacing = POINT::default();
        let _ = view.GetSpacing(&mut spacing);
        let mut pt = POINT { x: x - spacing.x / 2, y: y - spacing.y / 2 };
        let _ = ScreenToClient(view_window, &mut pt);
        let items = [child as *const ITEMIDLIST];
        let result = view.SelectAndPositionItems(1, items.as_ptr(), Some(&pt), SVSI_POSITIONITEM.0 as u32);
        ILFree(Some(child));
        result
    }
}

/// Show files in Explorer, selected. Files in other folders than the first are left out.
pub fn reveal(paths: &[PathBuf]) {
    let Some(dir) = paths.first().and_then(|p| p.parent()) else { return };
    let _com = Com::init();
    unsafe {
        let folder = ILCreateFromPathW(PCWSTR(wide(dir).as_ptr()));
        if folder.is_null() {
            return;
        }
        let items: Vec<*mut ITEMIDLIST> = paths
            .iter()
            .filter(|p| p.parent() == Some(dir))
            .map(|p| ILCreateFromPathW(PCWSTR(wide(p).as_ptr())))
            .filter(|p| !p.is_null())
            .collect();
        let refs: Vec<*const ITEMIDLIST> = items.iter().map(|p| *p as *const _).collect();
        if SHOpenFolderAndSelectItems(folder, Some(&refs), 0).is_err() {
            open(&dir.to_string_lossy());
        }
        for p in items {
            ILFree(Some(p));
        }
        ILFree(Some(folder));
    }
}

/// Open a file, folder or URL with its default app.
pub fn open(target: &str) {
    let target = wide(target);
    unsafe {
        ShellExecuteW(None, windows::core::w!("open"), PCWSTR(target.as_ptr()), None, None, SW_SHOWNORMAL);
    }
}

/// Write a `.lnk` shortcut.
pub fn create_shortcut(lnk: &Path, target: &Path, args: &str, description: &str) -> windows::core::Result<()> {
    let _com = Com::init();
    unsafe {
        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_ALL)?;
        link.SetPath(PCWSTR(wide(target).as_ptr()))?;
        link.SetArguments(PCWSTR(wide(args).as_ptr()))?;
        link.SetDescription(PCWSTR(wide(description).as_ptr()))?;
        link.SetIconLocation(PCWSTR(wide(target).as_ptr()), 0)?;
        link.cast::<IPersistFile>()?.Save(PCWSTR(wide(lnk).as_ptr()), true)
    }
}
