# Kumquat

Convert and edit files right where they are, on Linux and Windows. Start dragging a file, tap a key, and drop it on what you want:

| Key while dragging | Wheel | What's on it |
|---|---|---|
| **Shift** | Convert | Images, audio, video, PDF, documents, slides, spreadsheets, subtitles, archives |
| **Ctrl** | Tools | Compress, resize, crop, annotate, trim, speed, clean up audio, merge/split/organize/redact PDFs, extract, ZIP… |
| **Space** | Actions | Send to phone, Float, Copy to…, Move to…, Copy path, Terminal |

Results land where you were dragging to: tap the key over the desktop and the new file appears at that spot; over a Files window, it's saved into that folder; anywhere else, next to the original. Nothing is ever overwritten, and half-finished files never show up (they're written to a hidden folder and moved into place when done). Keys can be changed in Settings.

Without dragging: select files in Files and press **F4** (or right-click › Scripts › Kumquat), or use the tray icon's *Choose files…*. Then arrow keys pick, Enter applies, Tab or the modifier keys switch wheels. On Windows, it's right-click › Send to › Kumquat instead (see [Windows](#windows)).

## Running it

```bash
scripts/fetch-binaries.sh        # from the repo root, once
cd apps/desktop
npm install
npm run tauri dev
```

`scripts/fetch-binaries.sh` downloads ffmpeg, ImageMagick (with HEIC/AVIF), pandoc, 7-Zip and PDFium into `resources/binaries/`, which get bundled into the app. Add `--with-libreoffice` to bundle LibreOffice too; otherwise the system's LibreOffice is used for office formats. Anything not bundled falls back to what's installed.

Packages: `npm run tauri build` makes a `.deb` and an AppImage on Linux (the AppImage needs `patchelf`), and an NSIS installer on Windows.

## How the drag wheel works

On X11, any app that starts a drag takes ownership of the `XdndSelection` selection. Kumquat watches for that with XFixes, asks the drag source for its file list, and polls the modifier keys while the drag lasts. When the convert/tools/actions key is tapped, a transparent always-on-top window appears under the pointer and becomes the drop target.

Wayland doesn't let apps watch other apps' drags, so on Wayland sessions only the file manager shortcut and tray work.

## Windows

The Windows build is new and has only been compiled, not run; see the checklist below before shipping it.

```powershell
powershell -ExecutionPolicy Bypass -File scripts\fetch-binaries.ps1   # from the repo root, once
cd apps\desktop
npm install
npm run tauri dev
npm run tauri build   # NSIS installer: src-tauri\target\release\bundle\nsis\Kumquat_<version>_x64-setup.exe
```

Needs the usual Tauri prerequisites (Visual Studio Build Tools with the C++ workload, WebView2, which Windows 10/11 already has). `tauri.windows.conf.json` switches the bundle to a per-user NSIS installer. `fetch-binaries.ps1` puts `ffmpeg.exe`, `ffprobe.exe`, `pandoc.exe`, `imagemagick\magick.exe`, `7zip\7z.exe` (+ `7z.dll`, for RAR) and `lib\pdfium.dll` into `resources\binaries\`. LibreOffice isn't bundled; an installed copy in Program Files is found automatically.

How it differs from Linux:

| | Linux (X11) | Windows |
|---|---|---|
| Noticing a drag | XFixes on `XdndSelection` | A mouse button is held and the shell's drag image window (`SysDragImage`) is on screen |
| Which files | Asked from the drag source up front | Arrive when the drag enters the wheel (it's an OLE drop target) |
| Keys | X pointer mask + keymap polling | `GetAsyncKeyState` polling |
| Wheel backdrop | `GetImage` on the root window | GDI `BitBlt` from the screen |
| Wheel not stealing focus | GTK `accept-focus` | `WS_EX_NOACTIVATE` (Tauri's `set_focusable`) |
| Saving where you drop | Nautilus D-Bus window list; desktop icon via `gio` metadata | Explorer folder via `IShellWindows` (active tab wins); desktop icon moved with `IFolderView::SelectAndPositionItems` |
| Without dragging | Nautilus/Nemo/Caja script + F4 | *Send to › Kumquat* shortcut, plus a *Kumquat* right-click entry for a single file or folder (per-user registry, under *Show more options* on Windows 11) |
| Show in file manager | `FileManager1.ShowItems` | `SHOpenFolderAndSelectItems` |
| Terminal | `$TERMINAL`, gnome-terminal, … | Windows Terminal, else PowerShell, else cmd |
| Send to phone | KDE Connect / GSConnect | Not offered |
| Notifications | Progress bar for long jobs | One toast when done (toasts can't be updated) |

Settings live in `%APPDATA%\kumquat\settings.json`.

### Windows status

Verified (on Linux, Oct 2026):

- `cargo check` and `cargo clippy --all-targets` pass for `x86_64-pc-windows-msvc` (every dependency type-checks; see below for how).
- `fetch-binaries.ps1` parses under PowerShell 7; its ffmpeg, pandoc and PDFium steps run end to end, and the 7-Zip installer and ImageMagick portable archive contain `7z.exe`/`7z.dll` and `magick.exe` at the root as the script expects.
- Linux is unchanged: `cargo test` passes and the X11 snapshot still works.

Untested, needs a real Windows 10/11 machine:

- [ ] `npm run tauri build` produces and installs `Kumquat_<version>_x64-setup.exe`; the app starts in the tray without a console window.
- [ ] Drag detection: dragging files from Explorer, the desktop, a browser's downloads list; tapping Shift/Ctrl/Space opens the wheel under the pointer. Apps that drag without a shell drag image won't be noticed. Text drags with a drag image open an empty wheel.
- [ ] The wheel doesn't take focus mid-drag, receives the drop (paths come through WebView2's drop handling), and closes after the drag ends; Esc/arrows/Enter during a drag.
- [ ] The frosted backdrop on mixed-DPI and multi-monitor setups (negative coordinates).
- [ ] Keyboard-mode wheel (Send to, tray) gets focus and closes when focus moves elsewhere.
- [ ] Saving where you drop: Explorer window (including Windows 11 tabs), the desktop (icon ends up at the pointer unless auto-arrange is on), OneDrive-redirected Desktop.
- [ ] Staging folders are hidden while a job runs; results move across drives.
- [ ] Send to › Kumquat with several files opens one wheel; the single-file right-click entry; turning the integration off removes both.
- [ ] Bundled tools run without flashing console windows; LibreOffice conversions with the `file:///C:/…` profile URL; `tar` for `.tar.xz`/`.tar.bz2`.
- [ ] Toasts show as "Kumquat" in the installed build (relies on the installer giving the Start menu shortcut the `app.kumquat.desktop` AppUserModelID) and the *Show in Explorer* button works; dev builds show as PowerShell.
- [ ] `cargo test smoke -- --ignored` with the Windows binaries.

CI: `.github/workflows/windows.yml` runs on `windows-latest` for pushes to main, pull requests and `v*` tags. It fetches the tools, runs `cargo test --lib`, builds the installer and uploads it as the `kumquat-windows-installer` artifact; a `v*` tag also attaches it to a draft release. The conversion smoke tests run there too but may fail without LibreOffice on the runner, so they don't fail the job. That proves the installer builds; the checklist above still needs a person at a Windows machine.

Checking the Windows build from Linux: `rustup target add x86_64-pc-windows-msvc`, then `cargo check --target x86_64-pc-windows-msvc` in `apps/desktop/src-tauri`. Tauri's build script wants `llvm-rc` for the exe's icon resource; `cargo check` never links, so any stand-in on `PATH` that answers `/?` with "Resource Converter" and creates the `/fo` file is enough. Linking needs the MSVC libraries (e.g. via `cargo xwin`), or just build on Windows.

## Layout

```
apps/desktop/
  wheel.html, settings.html, editor.html, float.html   one page per window
  src/            TypeScript UI (wheel, settings, editors, float shelf)
  src-tauri/src/
    watch.rs      drag signals shared by both platforms
    x11watch.rs   drag detection on X11
    win/          Windows: drag detection, Explorer/shell, Send to integration
    wheel.rs      wheel window state
    catalog.rs    what each wheel offers for given files
    engine/       image, av, pdf, docs, subs, archive
    jobs.rs       background runs + notifications
    actions.rs    send, float, copy/move, path, terminal
    editor.rs     editor windows and their preview commands
    integration.rs file manager script + F4 shortcut
resources/binaries/   bundled programs (fetched, not committed)
scripts/fetch-binaries.sh, fetch-binaries.ps1
```

## Tests

```bash
cd apps/desktop/src-tauri
cargo test                                # unit tests
cargo test smoke -- --ignored --nocapture # every conversion and tool on generated samples
```

## License

MIT. See [LICENSE](LICENSE).
