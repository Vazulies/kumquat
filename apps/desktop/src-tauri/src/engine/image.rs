//! Images, via ImageMagick (`magick`, or `convert` on older systems).

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context as _, Result};
use base64::Engine as _;

use super::{output_path, path_str, run, run_output, Ctx, Outcome, TempDir};
use crate::catalog::{canonical_ext, ext_of};

fn magick() -> Result<Command> {
    Ok(Command::new(crate::bins::require("magick")?))
}

/// Input arguments for ImageMagick: density for SVG, first frame for animations.
fn input_args(path: &Path, first_frame: bool) -> Vec<String> {
    let ext = canonical_ext(&ext_of(path)).to_string();
    let mut args = Vec::new();
    if ext == "svg" {
        args.extend(["-density".into(), "300".into(), "-background".into(), "none".into()]);
    }
    let spec = if first_frame && ext == "gif" {
        format!("{}[0]", path_str(path))
    } else {
        path_str(path)
    };
    args.push(spec);
    args.push("-auto-orient".into());
    args
}

fn quality_args(target: &str, quality: u8) -> Vec<String> {
    let q = quality.to_string();
    match target {
        "jpg" => vec!["-background".into(), "white".into(), "-alpha".into(), "remove".into(), "-alpha".into(), "off".into(), "-quality".into(), q],
        "bmp" => vec!["-background".into(), "white".into(), "-alpha".into(), "remove".into(), "-alpha".into(), "off".into()],
        "webp" | "heic" | "avif" => vec!["-quality".into(), q],
        "tiff" => vec!["-compress".into(), "lzw".into()],
        "pdf" => vec!["-quality".into(), q],
        _ => vec![],
    }
}

pub fn convert(path: &Path, ext: &str, target: &str, ctx: &Ctx) -> Result<Vec<PathBuf>> {
    if ext == "gif" && (target == "mp4" || target == "webm") {
        return Ok(vec![super::av::gif_to_video(path, target, ctx)?]);
    }
    let out = output_path(path, target, None);
    let mut cmd = magick()?;
    cmd.args(input_args(path, target != "gif" && target != "webp"));
    cmd.args(quality_args(target, ctx.settings.image_quality));
    cmd.arg(&out);
    run(&mut cmd).map_err(|e| explain(e, ext, target))?;
    Ok(vec![out])
}

/// Older ImageMagick builds often lack HEIC/AVIF support; say so plainly.
fn explain(err: anyhow::Error, from: &str, to: &str) -> anyhow::Error {
    let msg = err.to_string();
    if msg.contains("no decode delegate") || msg.contains("no encode delegate") || msg.contains("NoDecodeDelegate") || msg.contains("NoEncodeDelegate") {
        let fmt = if msg.contains("no encode") || msg.contains("NoEncode") { to } else { from };
        anyhow::anyhow!("This ImageMagick build can't handle {}. Run scripts/fetch-binaries.sh to get one that can.", fmt.to_uppercase())
    } else {
        err
    }
}

fn same_ext(path: &Path) -> String {
    canonical_ext(&ext_of(path)).to_string()
}

fn simple(path: &Path, suffix: &str, ops: &[&str], ctx: &Ctx) -> Result<PathBuf> {
    let ext = same_ext(path);
    let out_ext = if ext == "svg" { "png".to_string() } else { ext.clone() };
    let out = output_path(path, &out_ext, Some(suffix));
    let mut cmd = magick()?;
    cmd.args(input_args(path, false));
    cmd.args(ops);
    if matches!(out_ext.as_str(), "jpg" | "webp" | "heic" | "avif") {
        cmd.args(["-quality", &ctx.settings.image_quality.to_string()]);
    }
    cmd.arg(&out);
    run(&mut cmd).map_err(|e| explain(e, &ext, &out_ext))?;
    Ok(out)
}

pub fn compress(path: &Path, _ctx: &Ctx) -> Result<PathBuf> {
    let ext = same_ext(path);
    let (out_ext, ops): (&str, Vec<&str>) = match ext.as_str() {
        "jpg" => ("jpg", vec!["-strip", "-quality", "72", "-sampling-factor", "4:2:0", "-interlace", "JPEG"]),
        "png" => ("png", vec!["-strip", "-dither", "FloydSteinberg", "-colors", "256", "-define", "png:compression-level=9"]),
        "webp" => ("webp", vec!["-strip", "-quality", "60"]),
        "heic" => ("heic", vec!["-strip", "-quality", "55"]),
        "avif" => ("avif", vec!["-strip", "-quality", "50"]),
        "tiff" => ("tiff", vec!["-strip", "-compress", "zip"]),
        "bmp" => ("png", vec!["-strip", "-define", "png:compression-level=9"]),
        "gif" => ("gif", vec!["-layers", "optimize", "-fuzz", "4%"]),
        _ => bail!("{} is already a compact format", ext.to_uppercase()),
    };
    let out = output_path(path, out_ext, Some("compressed"));
    let mut cmd = magick()?;
    cmd.args(input_args(path, false));
    cmd.args(ops);
    cmd.arg(&out);
    run(&mut cmd).map_err(|e| explain(e, &ext, out_ext))?;
    let before = std::fs::metadata(path)?.len();
    let after = std::fs::metadata(&out)?.len();
    if after >= before {
        let _ = std::fs::remove_file(&out);
        bail!("{} is already as small as it gets", file_name(path));
    }
    Ok(out)
}

pub fn resize(path: &Path, ctx: &Ctx) -> Result<PathBuf> {
    let geometry = if let Some(w) = ctx.param_f64("width").filter(|w| *w > 0.0) {
        format!("{}x", w.round() as u32)
    } else {
        format!("{}%", ctx.param_f64("percent").unwrap_or(50.0).clamp(1.0, 1000.0))
    };
    simple(path, "resized", &["-resize", &geometry], ctx)
}

pub fn crop(path: &Path, ctx: &Ctx) -> Result<PathBuf> {
    let get = |k: &str| ctx.param_f64(k).map(|v| v.round().max(0.0) as u32);
    let (Some(x), Some(y), Some(w), Some(h)) = (get("x"), get("y"), get("width"), get("height")) else {
        bail!("no crop area chosen");
    };
    if w == 0 || h == 0 {
        bail!("the crop area is empty");
    }
    let geometry = format!("{w}x{h}+{x}+{y}");
    simple(path, "cropped", &["-crop", &geometry, "+repage"], ctx)
}

pub fn annotate(path: &Path, ctx: &Ctx) -> Result<PathBuf> {
    let data = ctx.param_str("overlay").context("nothing was drawn")?;
    let b64 = data.split_once(',').map(|(_, b)| b).unwrap_or(data);
    let png = base64::engine::general_purpose::STANDARD.decode(b64)?;
    let tmp = TempDir::new()?;
    let overlay = tmp.join("overlay.png");
    std::fs::write(&overlay, png)?;
    let ext = same_ext(path);
    let out_ext = if ext == "svg" || ext == "gif" { "png".to_string() } else { ext.clone() };
    let out = output_path(path, &out_ext, Some("annotated"));
    let mut cmd = magick()?;
    cmd.args(input_args(path, true));
    cmd.arg(&overlay);
    cmd.args(["-compose", "over", "-composite"]);
    cmd.args(quality_args(&out_ext, ctx.settings.image_quality));
    cmd.arg(&out);
    run(&mut cmd).map_err(|e| explain(e, &ext, &out_ext))?;
    Ok(out)
}

pub fn rotate(path: &Path, ctx: &Ctx) -> Result<PathBuf> {
    let degrees = ctx.param_f64("degrees").unwrap_or(90.0).to_string();
    simple(path, "rotated", &["-rotate", &degrees], ctx)
}

pub fn background(path: &Path, ctx: &Ctx) -> Result<PathBuf> {
    let color = ctx.param_str("color").unwrap_or("#ffffff").to_string();
    if !color.starts_with('#') || !color[1..].chars().all(|c| c.is_ascii_hexdigit()) {
        bail!("not a colour: {color}");
    }
    simple(path, "background", &["-background", &color, "-alpha", "remove", "-alpha", "off"], ctx)
}

pub fn strip_metadata(path: &Path, ctx: &Ctx) -> Result<PathBuf> {
    let ext = same_ext(path);
    let out = output_path(path, &ext, Some("clean"));
    let bytes = std::fs::read(path)?;
    let stripped = match ext.as_str() {
        "jpg" if !needs_orientation(path) => strip_jpeg(&bytes),
        "png" => strip_png(&bytes),
        _ => None,
    };
    match stripped {
        Some(clean) => std::fs::write(&out, clean)?,
        None => {
            let mut cmd = magick()?;
            cmd.args(input_args(path, false));
            cmd.arg("-strip");
            cmd.args(quality_args(&ext, ctx.settings.image_quality.max(92)));
            cmd.arg(&out);
            run(&mut cmd).map_err(|e| explain(e, &ext, &ext))?;
        }
    }
    Ok(out)
}

/// Whether removing EXIF would visibly rotate the photo.
fn needs_orientation(path: &Path) -> bool {
    let Ok(mut cmd) = magick() else { return false };
    cmd.args(["-ping", "-format", "%[orientation]"]).arg(path).arg("info:");
    match run_output(&mut cmd) {
        Ok(o) => {
            let o = o.trim();
            !(o.is_empty() || o == "TopLeft" || o == "Undefined")
        }
        Err(_) => true,
    }
}

/// Drop EXIF/XMP (APP1), IPTC (APP13) and comments without re-encoding.
fn strip_jpeg(data: &[u8]) -> Option<Vec<u8>> {
    if data.len() < 4 || data[0] != 0xFF || data[1] != 0xD8 {
        return None;
    }
    let mut out = vec![0xFF, 0xD8];
    let mut i = 2;
    while i + 4 <= data.len() {
        if data[i] != 0xFF {
            return None;
        }
        let marker = data[i + 1];
        if marker == 0xDA {
            out.extend_from_slice(&data[i..]);
            return Some(out);
        }
        let len = u16::from_be_bytes([data[i + 2], data[i + 3]]) as usize;
        let end = i + 2 + len;
        if end > data.len() {
            return None;
        }
        let drop = matches!(marker, 0xE1 | 0xED | 0xFE);
        if !drop {
            out.extend_from_slice(&data[i..end]);
        }
        i = end;
    }
    None
}

/// Drop text, EXIF and timestamp chunks from a PNG.
fn strip_png(data: &[u8]) -> Option<Vec<u8>> {
    const SIG: &[u8] = b"\x89PNG\r\n\x1a\n";
    if !data.starts_with(SIG) {
        return None;
    }
    let mut out = SIG.to_vec();
    let mut i = SIG.len();
    while i + 12 <= data.len() {
        let len = u32::from_be_bytes(data[i..i + 4].try_into().ok()?) as usize;
        let end = i + 12 + len;
        if end > data.len() {
            return None;
        }
        let kind = &data[i + 4..i + 8];
        if !matches!(kind, b"tEXt" | b"zTXt" | b"iTXt" | b"eXIf" | b"tIME") {
            out.extend_from_slice(&data[i..end]);
        }
        i = end;
    }
    Some(out)
}

pub fn read_qr(paths: &[PathBuf]) -> Result<Outcome> {
    let mut found = Vec::new();
    for path in paths {
        let img = load_luma(path)?;
        let mut prepared = rqrr::PreparedImage::prepare(img);
        for grid in prepared.detect_grids() {
            if let Ok((_, content)) = grid.decode() {
                found.push(content);
            }
        }
    }
    if found.is_empty() {
        bail!("No QR code found");
    }
    let body = found.join("\n");
    crate::actions::copy_text(&body)?;
    Ok(Outcome::Message { title: "QR code copied".into(), body })
}

fn load_luma(path: &Path) -> Result<::image::GrayImage> {
    if let Ok(img) = ::image::open(path) {
        return Ok(img.to_luma8());
    }
    // Formats the image crate can't read (HEIC, AVIF, SVG) go through ImageMagick.
    let tmp = TempDir::new()?;
    let png = tmp.join("qr.png");
    let mut cmd = magick()?;
    cmd.args(input_args(path, true)).arg(&png);
    run(&mut cmd)?;
    Ok(::image::open(&png)?.to_luma8())
}

pub fn combine_pdf(paths: &[PathBuf], ctx: &Ctx) -> Result<PathBuf> {
    let first = paths.first().context("no images")?;
    let out = output_path(first, "pdf", Some("combined"));
    let mut cmd = magick()?;
    for p in paths {
        cmd.args(input_args(p, true));
    }
    cmd.args(["-quality", &ctx.settings.image_quality.to_string()]);
    cmd.arg(&out);
    run(&mut cmd)?;
    Ok(out)
}

/// A PNG preview (at most `max` px on the long side) plus the oriented size of the original.
pub fn preview(path: &Path, max: u32, dest: &Path) -> Result<(u32, u32)> {
    let mut cmd = magick()?;
    cmd.args(input_args(path, true));
    cmd.args(["-format", "%w %h\n", "-write", "info:-"]);
    cmd.args(["-resize", &format!("{max}x{max}>")]);
    cmd.arg(dest);
    let out = run_output(&mut cmd)?;
    let mut nums = out.split_whitespace().filter_map(|n| n.parse::<u32>().ok());
    match (nums.next(), nums.next()) {
        (Some(w), Some(h)) => Ok((w, h)),
        _ => bail!("could not read the image size"),
    }
}

fn file_name(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_png_text_chunks() {
        let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
        let chunk = |kind: &[u8], data: &[u8]| {
            let mut c = (data.len() as u32).to_be_bytes().to_vec();
            c.extend_from_slice(kind);
            c.extend_from_slice(data);
            c.extend_from_slice(&[0, 0, 0, 0]);
            c
        };
        png.extend(chunk(b"IHDR", &[0; 13]));
        png.extend(chunk(b"tEXt", b"Author\0me"));
        png.extend(chunk(b"IEND", &[]));
        let out = strip_png(&png).unwrap();
        assert!(!out.windows(4).any(|w| w == b"tEXt"));
        assert!(out.windows(4).any(|w| w == b"IEND"));
    }

    #[test]
    fn strips_jpeg_exif() {
        let jpeg = [
            0xFF, 0xD8, // SOI
            0xFF, 0xE1, 0x00, 0x04, b'E', b'x', // APP1
            0xFF, 0xDB, 0x00, 0x03, 0x01, // DQT
            0xFF, 0xDA, 0x00, 0x02, 0x11, 0x22, 0xFF, 0xD9,
        ];
        let out = strip_jpeg(&jpeg).unwrap();
        assert_eq!(out, vec![0xFF, 0xD8, 0xFF, 0xDB, 0x00, 0x03, 0x01, 0xFF, 0xDA, 0x00, 0x02, 0x11, 0x22, 0xFF, 0xD9]);
    }
}
