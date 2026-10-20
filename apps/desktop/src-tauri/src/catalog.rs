//! What each wheel offers for a given selection of files.

use std::path::Path;

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Image,
    Audio,
    Video,
    Pdf,
    Document,
    Slides,
    Sheet,
    Subtitle,
    Archive,
    Folder,
    Other,
}

/// The extension of a file, lower-cased, with `tar.gz`-style double extensions kept.
pub fn ext_of(path: &Path) -> String {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    for double in ["tar.gz", "tar.xz", "tar.bz2"] {
        if name.ends_with(&format!(".{double}")) {
            return double.to_string();
        }
    }
    path.extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

/// Spellings that mean the same format.
pub fn canonical_ext(ext: &str) -> &str {
    match ext {
        "jpeg" | "jpe" => "jpg",
        "tif" => "tiff",
        "heif" => "heic",
        "aif" => "aiff",
        "htm" => "html",
        "markdown" => "md",
        "tgz" => "tar.gz",
        "m4v" => "mp4",
        "oga" => "ogg",
        other => other,
    }
}

pub fn kind_of(path: &Path) -> Kind {
    if path.is_dir() {
        return Kind::Folder;
    }
    match canonical_ext(&ext_of(path)) {
        "jpg" | "png" | "webp" | "heic" | "tiff" | "avif" | "bmp" | "gif" | "svg" => Kind::Image,
        "mp3" | "m4a" | "aac" | "wav" | "flac" | "ogg" | "opus" | "aiff" | "wma" => Kind::Audio,
        "mp4" | "mov" | "mkv" | "webm" | "avi" | "wmv" | "flv" | "mpg" | "mpeg" | "3gp" | "ts" => Kind::Video,
        "pdf" => Kind::Pdf,
        "docx" | "doc" | "odt" | "rtf" | "md" | "txt" | "epub" | "html" | "pages" => Kind::Document,
        "pptx" | "ppt" | "odp" | "key" => Kind::Slides,
        "xlsx" | "xls" | "ods" | "csv" | "tsv" | "numbers" => Kind::Sheet,
        "srt" | "vtt" => Kind::Subtitle,
        "zip" | "tar" | "tar.gz" | "gz" | "tar.xz" | "tar.bz2" | "rar" | "7z" => Kind::Archive,
        _ => Kind::Other,
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    /// `convert:<ext>`, `tool:<name>` or `action:<name>`.
    pub id: String,
    pub label: String,
    /// Short secondary line, e.g. "Lossless" or "Choose part".
    pub hint: String,
    /// Icon key the frontend knows how to draw.
    pub icon: String,
    /// Opens an editor window before running, when set.
    pub editor: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Catalog {
    pub kind: Option<Kind>,
    pub count: usize,
    pub title: String,
    pub convert: Vec<Item>,
    pub tools: Vec<Item>,
    pub actions: Vec<Item>,
}

fn convert(ext: &str, hint: &str) -> Item {
    Item {
        id: format!("convert:{ext}"),
        label: ext.to_uppercase(),
        hint: hint.to_string(),
        icon: "format".into(),
        editor: None,
    }
}

fn tool(id: &str, label: &str, hint: &str, icon: &str, editor: Option<&str>) -> Item {
    Item {
        id: format!("tool:{id}"),
        label: label.into(),
        hint: hint.into(),
        icon: icon.into(),
        editor: editor.map(Into::into),
    }
}

fn action(id: &str, label: &str, icon: &str) -> Item {
    Item {
        id: format!("action:{id}"),
        label: label.into(),
        hint: String::new(),
        icon: icon.into(),
        editor: None,
    }
}

/// Output formats for a kind, given the input extension.
pub fn outputs(kind: Kind, ext: &str) -> Vec<(&'static str, &'static str)> {
    let list: Vec<(&str, &str)> = match kind {
        Kind::Image => {
            let mut v = vec![
                ("jpg", "Photo"),
                ("png", "Lossless"),
                ("webp", "Web"),
                ("heic", "Small"),
                ("avif", "Smallest"),
                ("tiff", "Print"),
                ("bmp", "Bitmap"),
                ("pdf", "Document"),
            ];
            if ext == "gif" {
                v.push(("mp4", "Video"));
                v.push(("webm", "Web video"));
            }
            v
        }
        Kind::Audio => vec![
            ("mp3", "Everywhere"),
            ("m4a", "AAC"),
            ("wav", "Uncompressed"),
            ("flac", "Lossless"),
            ("ogg", "Vorbis"),
            ("opus", "Efficient"),
            ("aiff", "Uncompressed"),
            ("wma", "Windows"),
        ],
        Kind::Video => vec![
            ("mp4", "Everywhere"),
            ("mov", "QuickTime"),
            ("mkv", "Matroska"),
            ("webm", "Web"),
            ("avi", "Legacy"),
            ("wmv", "Windows"),
            ("gif", "Animation"),
            ("mp3", "Audio only"),
        ],
        Kind::Pdf => vec![
            ("docx", "Word"),
            ("pptx", "PowerPoint"),
            ("xlsx", "Excel"),
            ("jpg", "Every page"),
            ("png", "Every page"),
            ("epub", "E-book"),
            ("txt", "Plain text"),
            ("md", "Markdown"),
        ],
        Kind::Document => {
            let mut v = vec![
                ("docx", "Word"),
                ("odt", "OpenDocument"),
                ("pdf", "PDF"),
                ("md", "Markdown"),
                ("txt", "Plain text"),
                ("epub", "E-book"),
                ("html", "Web page"),
                ("rtf", "Rich text"),
            ];
            if ext == "txt" || ext == "md" {
                v.push(("jpg", "Image"));
                v.push(("png", "Image"));
            }
            v
        }
        Kind::Slides => vec![
            ("pdf", "PDF"),
            ("pptx", "PowerPoint"),
            ("odp", "OpenDocument"),
            ("jpg", "Every slide"),
            ("png", "Every slide"),
        ],
        Kind::Sheet => vec![
            ("xlsx", "Excel"),
            ("ods", "OpenDocument"),
            ("csv", "Comma"),
            ("tsv", "Tab"),
            ("pdf", "PDF"),
        ],
        Kind::Subtitle => vec![("srt", "SubRip"), ("vtt", "WebVTT"), ("txt", "Transcript")],
        Kind::Archive => vec![("zip", "Everywhere"), ("tar", "Unix"), ("tar.gz", "GZIP")],
        Kind::Folder | Kind::Other => vec![],
    };
    let canon = canonical_ext(ext);
    list.into_iter().filter(|(e, _)| *e != canon).collect()
}

fn tools_for(kind: Option<Kind>, count: usize, all_svg: bool) -> Vec<Item> {
    let mut t = Vec::new();
    match kind {
        Some(Kind::Image) => {
            t.push(tool("image.compress", "Compress", "Smaller file", "compress", None));
            t.push(tool("image.resize", "Resize", "Change size", "resize", Some("params")));
            if count == 1 {
                t.push(tool("image.crop", "Crop", "Choose area", "crop", Some("image-crop")));
                t.push(tool("image.annotate", "Annotate", "Draw & mark", "pen", Some("image-annotate")));
            }
            t.push(tool("image.rotate", "Rotate", "90° right", "rotate", None));
            t.push(tool("image.background", "Background", "Fill transparency", "fill", Some("params")));
            if !all_svg {
                t.push(tool("image.strip", "Clean metadata", "Remove EXIF & GPS", "shield", None));
            }
            t.push(tool("image.qr", "Read QR", "Copy what it says", "qr", None));
            if count > 1 {
                t.push(tool("image.combine", "Combine", "Into one PDF", "merge", None));
            }
        }
        Some(Kind::Pdf) => {
            t.push(tool("pdf.compress", "Compress", "Smaller file", "compress", None));
            if count > 1 {
                t.push(tool("pdf.merge", "Merge", "Into one PDF", "merge", None));
            }
            t.push(tool("pdf.split", "Split", "One file per page", "split", None));
            if count == 1 {
                t.push(tool("pdf.organize", "Organize", "Reorder & delete", "pages", Some("pdf-pages")));
                t.push(tool("pdf.redact", "Redact", "Black out text", "redact", Some("pdf-redact")));
            }
            t.push(tool("pdf.rotate", "Rotate", "All pages 90°", "rotate", None));
            t.push(tool("pdf.strip", "Clean metadata", "Remove author & info", "shield", None));
        }
        Some(Kind::Video) => {
            t.push(tool("video.compress", "Compress", "Smaller file", "compress", None));
            if count == 1 {
                t.push(tool("video.trim", "Trim", "Keep a part", "scissors", Some("media-trim")));
                t.push(tool("video.crop", "Crop", "Choose area", "crop", Some("video-crop")));
            }
            t.push(tool("video.speed", "Speed", "Faster or slower", "speed", Some("params")));
            t.push(tool("video.audio", "Extract audio", "Soundtrack only", "music", None));
            t.push(tool("video.mute", "Mute", "Remove sound", "mute", None));
            t.push(tool("video.rotate", "Rotate", "90° right", "rotate", None));
            t.push(tool("video.strip", "Clean metadata", "Remove tags", "shield", None));
        }
        Some(Kind::Audio) => {
            t.push(tool("audio.compress", "Compress", "Smaller file", "compress", None));
            if count == 1 {
                t.push(tool("audio.trim", "Trim", "Keep a part", "scissors", Some("media-trim")));
            }
            t.push(tool("audio.cleanup", "Clean up", "Less noise & hum", "sparkle", None));
            t.push(tool("audio.normalize", "Normalize", "Even loudness", "wave", None));
            t.push(tool("audio.speed", "Speed", "Faster or slower", "speed", Some("params")));
            t.push(tool("audio.strip", "Clean metadata", "Remove tags", "shield", None));
        }
        Some(Kind::Subtitle) => {
            t.push(tool("subtitle.shift", "Shift timing", "Earlier or later", "clock", Some("params")));
        }
        Some(Kind::Archive) => {
            t.push(tool("archive.extract", "Extract", "Unpack here", "extract", None));
        }
        _ => {}
    }
    if count > 1 || kind == Some(Kind::Folder) || kind.is_none() {
        t.push(tool("any.zip", "Compress to ZIP", "One archive", "zip", None));
    } else if !matches!(kind, Some(Kind::Archive)) {
        t.push(tool("any.zip", "ZIP", "Archive it", "zip", None));
    }
    t
}

fn actions() -> Vec<Item> {
    let mut items = vec![
        action("send", "Send to phone", "phone"),
        action("float", "Float", "pin"),
        action("copy", "Copy to…", "copy"),
        action("move", "Move to…", "move"),
        action("path", "Copy path", "link"),
        action("terminal", "Terminal", "terminal"),
    ];
    // Nothing to send through on Windows (see actions::send_to_phone).
    if cfg!(windows) {
        items.retain(|i| i.id != "action:send");
    }
    items
}

pub fn for_paths(paths: &[String]) -> Catalog {
    let kinds: Vec<Kind> = paths.iter().map(|p| kind_of(Path::new(p))).collect();
    let kind = match kinds.first() {
        Some(first) if kinds.iter().all(|k| k == first) => Some(*first),
        _ => None,
    };
    let count = paths.len();
    let title = match count {
        0 => "Nothing selected".to_string(),
        1 => Path::new(&paths[0])
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| paths[0].clone()),
        n => format!("{n} items"),
    };

    // Every file must share the input extension's outputs, so mixing a PNG with
    // a JPG still offers the formats they have in common.
    let convert_items = match kind {
        Some(k) if count > 0 => {
            let mut common: Option<Vec<(&str, &str)>> = None;
            for p in paths {
                let outs = outputs(k, canonical_ext(&ext_of(Path::new(p))));
                common = Some(match common {
                    None => outs,
                    Some(prev) => prev.into_iter().filter(|o| outs.iter().any(|x| x.0 == o.0)).collect(),
                });
            }
            common
                .unwrap_or_default()
                .into_iter()
                .map(|(e, h)| convert(e, h))
                .collect()
        }
        _ => vec![],
    };

    Catalog {
        kind,
        count,
        title,
        convert: convert_items,
        tools: tools_for(kind, count, count > 0 && paths.iter().all(|p| ext_of(Path::new(p)) == "svg")),
        actions: actions(),
    }
}

/// Look up a single item, so commands can check what they were asked to run.
pub fn find_item(paths: &[String], id: &str) -> Option<Item> {
    let c = for_paths(paths);
    c.convert
        .into_iter()
        .chain(c.tools)
        .chain(c.actions)
        .find(|i| i.id == id)
}

#[cfg(test)]
mod counts {
    use super::*;

    /// Totals quoted on the website: `cargo test site_counts -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn site_counts() {
        let inputs = [
            "jpg", "png", "webp", "heic", "tiff", "avif", "bmp", "gif", "svg", "mp3", "m4a", "aac", "wav", "flac", "ogg", "opus",
            "aiff", "wma", "mp4", "mov", "mkv", "webm", "avi", "wmv", "flv", "mpg", "mpeg", "3gp", "ts", "pdf", "docx", "doc",
            "odt", "rtf", "md", "txt", "epub", "html", "pages", "pptx", "ppt", "odp", "key", "xlsx", "xls", "ods", "csv", "tsv",
            "numbers", "srt", "vtt", "zip", "tar", "tar.gz", "gz", "tar.xz", "tar.bz2", "rar", "7z",
        ];
        let mut conversions = 0;
        let mut tools = std::collections::BTreeSet::new();
        for ext in inputs {
            let path = format!("/tmp/x.{ext}");
            let kind = kind_of(Path::new(&path));
            conversions += outputs(kind, ext).len();
            for count in [1, 2] {
                for t in tools_for(Some(kind), count, false) {
                    tools.insert(t.id);
                }
            }
        }
        for t in tools_for(Some(Kind::Folder), 1, false) {
            tools.insert(t.id);
        }
        println!("inputs {} conversions {conversions} tools {} actions {}", inputs.len(), tools.len(), actions().len());
    }
}
