//! Finds the command-line programs that do the actual converting.
//!
//! Bundled copies in `resources/binaries/` win; otherwise we fall back to
//! whatever is installed on the system.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use serde::Serialize;

static RESOURCE_DIR: OnceLock<Option<PathBuf>> = OnceLock::new();

pub fn init(resource_dir: Option<PathBuf>) {
    let _ = RESOURCE_DIR.set(resource_dir);
}

/// Directories that may hold bundled binaries, most preferred first.
fn bundled_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(Some(res)) = RESOURCE_DIR.get() {
        dirs.push(res.join("binaries"));
    }
    if let Ok(dir) = std::env::var("KUMQUAT_BINARIES") {
        dirs.push(PathBuf::from(dir));
    }
    // During development, the repository's own resources folder.
    dirs.push(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../resources/binaries"));
    dirs
}

/// Each tool and the names it may go by, bundled name first.
#[cfg(not(windows))]
const TOOLS: &[(&str, &[&str], &str)] = &[
    ("ffmpeg", &["ffmpeg"], "Audio, video and GIF"),
    ("ffprobe", &["ffprobe"], "Reading media details"),
    ("magick", &["magick", "convert"], "Images (ImageMagick)"),
    ("pandoc", &["pandoc"], "Documents and e-books"),
    ("soffice", &["soffice", "libreoffice"], "Office files and PDF export (LibreOffice)"),
    ("7zz", &["7zz", "7z", "7za"], "RAR and 7z archives"),
];

/// On Windows, names are relative to the bundled folder and include `.exe`.
/// ImageMagick and 7-Zip keep their DLLs beside them in a folder of their own,
/// and `convert` is left out: it's Windows' disk converter there. LibreOffice's
/// `soffice.com` waits for conversions to finish, unlike `soffice.exe`.
#[cfg(windows)]
const TOOLS: &[(&str, &[&str], &str)] = &[
    ("ffmpeg", &["ffmpeg.exe"], "Audio, video and GIF"),
    ("ffprobe", &["ffprobe.exe"], "Reading media details"),
    ("magick", &["imagemagick/magick.exe", "magick.exe"], "Images (ImageMagick)"),
    ("pandoc", &["pandoc.exe"], "Documents and e-books"),
    ("soffice", &["libreoffice/program/soffice.com", "libreoffice/program/soffice.exe", "soffice.com", "soffice.exe"], "Office files and PDF export (LibreOffice)"),
    ("7zz", &["7zip/7z.exe", "7zz.exe", "7z.exe", "7za.exe"], "RAR and 7z archives"),
];

pub fn find(tool: &str) -> Option<PathBuf> {
    let names = TOOLS
        .iter()
        .find(|(id, _, _)| *id == tool)
        .map(|(_, names, _)| *names)
        .unwrap_or(&[]);
    for dir in bundled_dirs() {
        for name in names {
            let candidate = dir.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    let on_path = |name: &&str| {
        let name = Path::new(name).file_name()?;
        which::which(name).ok()
    };
    names.iter().find_map(on_path).or_else(|| installed(tool))
}

/// Where Windows installers put the tools that aren't usually on PATH.
#[cfg(windows)]
fn installed(tool: &str) -> Option<PathBuf> {
    let rels: &[&str] = match tool {
        "soffice" => &[r"LibreOffice\program\soffice.com", r"LibreOffice\program\soffice.exe"],
        "7zz" => &[r"7-Zip\7z.exe"],
        _ => return None,
    };
    ["ProgramFiles", "ProgramFiles(x86)", "ProgramW6432"]
        .iter()
        .filter_map(std::env::var_os)
        .flat_map(|dir| rels.iter().map(move |rel| PathBuf::from(&dir).join(rel)))
        .find(|p| p.is_file())
}

#[cfg(not(windows))]
fn installed(_tool: &str) -> Option<PathBuf> {
    None
}

pub fn require(tool: &str) -> anyhow::Result<PathBuf> {
    find(tool).ok_or_else(|| {
        let what = TOOLS
            .iter()
            .find(|(id, _, _)| *id == tool)
            .map(|(_, _, what)| *what)
            .unwrap_or(tool);
        let script = if cfg!(windows) { "scripts/fetch-binaries.ps1" } else { "scripts/fetch-binaries.sh" };
        anyhow::anyhow!("{tool} is missing. It's needed for {what}. Run {script} or install it.")
    })
}

/// The PDFium shared library, used to render and read PDF pages.
#[cfg(windows)]
pub fn pdfium_library() -> Option<PathBuf> {
    bundled_dirs().into_iter().map(|dir| dir.join("lib").join("pdfium.dll")).find(|p| p.is_file())
}

/// The PDFium shared library, used to render and read PDF pages.
#[cfg(not(windows))]
pub fn pdfium_library() -> Option<PathBuf> {
    for dir in bundled_dirs() {
        let candidate = dir.join("lib").join("libpdfium.so");
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    ["/usr/lib/libpdfium.so", "/usr/local/lib/libpdfium.so", "/usr/lib/x86_64-linux-gnu/libpdfium.so"]
        .iter()
        .map(PathBuf::from)
        .find(|p| p.is_file())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolStatus {
    pub id: String,
    pub purpose: String,
    pub path: Option<String>,
    pub bundled: bool,
}

pub fn status() -> Vec<ToolStatus> {
    let bundled_roots: Vec<PathBuf> = bundled_dirs()
        .into_iter()
        .filter_map(|d| d.canonicalize().ok())
        .collect();
    let is_bundled = |p: &Path| {
        p.canonicalize()
            .map(|c| bundled_roots.iter().any(|r| c.starts_with(r)))
            .unwrap_or(false)
    };
    let mut out: Vec<ToolStatus> = TOOLS
        .iter()
        .map(|(id, _, purpose)| {
            let path = find(id);
            ToolStatus {
                id: id.to_string(),
                purpose: purpose.to_string(),
                bundled: path.as_deref().map(is_bundled).unwrap_or(false),
                path: path.map(|p| p.display().to_string()),
            }
        })
        .collect();
    let pdfium = pdfium_library();
    out.push(ToolStatus {
        id: "pdfium".into(),
        purpose: "Reading and rendering PDF pages".into(),
        bundled: pdfium.as_deref().map(is_bundled).unwrap_or(false),
        path: pdfium.map(|p| p.display().to_string()),
    });
    out
}
