//! Audio and video, via ffmpeg.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{bail, Context as _, Result};
use serde::Serialize;

use super::{output_path, path_str, run_ffmpeg, run_output, Ctx};
use crate::catalog::{canonical_ext, ext_of, kind_of, Kind};

#[derive(Debug, Default, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Probe {
    pub duration: Option<f64>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub video_codec: Option<String>,
    pub audio_codec: Option<String>,
}

pub fn probe(path: &Path) -> Result<Probe> {
    let ffprobe = crate::bins::require("ffprobe")?;
    let out = run_output(
        Command::new(ffprobe)
            .args(["-v", "error", "-show_entries", "format=duration:stream=codec_type,codec_name,width,height:stream_side_data=rotation", "-of", "json"])
            .arg(path),
    )?;
    let v: serde_json::Value = serde_json::from_str(&out)?;
    let mut p = Probe {
        duration: v["format"]["duration"].as_str().and_then(|d| d.parse().ok()),
        ..Default::default()
    };
    for s in v["streams"].as_array().into_iter().flatten() {
        let name = s["codec_name"].as_str().map(String::from);
        match s["codec_type"].as_str() {
            Some("video") if p.video_codec.is_none() => {
                // Cover art in audio files shows up as a one-frame video stream.
                if matches!(name.as_deref(), Some("mjpeg" | "png")) && kind_of(path) == Kind::Audio {
                    continue;
                }
                p.video_codec = name;
                p.width = s["width"].as_u64().map(|w| w as u32);
                p.height = s["height"].as_u64().map(|h| h as u32);
                let rotated = s["side_data_list"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .any(|d| d["rotation"].as_i64().map(|r| r.abs() == 90).unwrap_or(false));
                if rotated {
                    std::mem::swap(&mut p.width, &mut p.height);
                }
            }
            Some("audio") if p.audio_codec.is_none() => p.audio_codec = name,
            _ => {}
        }
    }
    Ok(p)
}

fn s(v: &[&str]) -> Vec<String> {
    v.iter().map(|x| x.to_string()).collect()
}

const EVEN: &str = "scale=trunc(iw/2)*2:trunc(ih/2)*2";

/// Encoder settings for a video container.
fn video_args(target: &str, crf: u8) -> Vec<String> {
    let crf = crf.to_string();
    match target {
        "webm" => {
            let vp9 = (crf.parse::<u8>().unwrap_or(20) + 12).clamp(15, 50).to_string();
            s(&["-c:v", "libvpx-vp9", "-crf", &vp9, "-b:v", "0", "-pix_fmt", "yuv420p", "-row-mt", "1", "-deadline", "good", "-cpu-used", "4", "-c:a", "libopus", "-b:a", "128k"])
        }
        "avi" => s(&["-c:v", "mpeg4", "-q:v", "3", "-c:a", "libmp3lame", "-q:a", "3"]),
        "wmv" => s(&["-c:v", "wmv2", "-b:v", "6M", "-c:a", "wmav2", "-b:a", "192k"]),
        "mp4" | "mov" => s(&["-c:v", "libx264", "-preset", "medium", "-crf", &crf, "-pix_fmt", "yuv420p", "-c:a", "aac", "-b:a", "192k", "-movflags", "+faststart"]),
        _ => s(&["-c:v", "libx264", "-preset", "medium", "-crf", &crf, "-pix_fmt", "yuv420p", "-c:a", "aac", "-b:a", "192k"]),
    }
}

/// Encoder settings for an audio format.
fn audio_args(target: &str) -> Vec<String> {
    match target {
        "mp3" => s(&["-c:a", "libmp3lame", "-q:a", "2"]),
        "m4a" | "aac" => s(&["-c:a", "aac", "-b:a", "256k"]),
        "wav" => s(&["-c:a", "pcm_s16le"]),
        "flac" => s(&["-c:a", "flac"]),
        "ogg" => s(&["-c:a", "libvorbis", "-q:a", "6"]),
        "opus" => s(&["-c:a", "libopus", "-b:a", "160k"]),
        "aiff" => s(&["-c:a", "pcm_s16be"]),
        "wma" => s(&["-c:a", "wmav2", "-b:a", "192k"]),
        _ => vec![],
    }
}

fn is_video_encode_target(ext: &str) -> bool {
    matches!(ext, "mp4" | "mov" | "mkv" | "webm" | "avi" | "wmv")
}

/// The container to re-encode a video into when we keep "the same format".
fn same_video_ext(path: &Path) -> String {
    let ext = canonical_ext(&ext_of(path)).to_string();
    if is_video_encode_target(&ext) {
        ext
    } else {
        "mp4".into()
    }
}

fn same_audio_ext(path: &Path) -> String {
    let ext = canonical_ext(&ext_of(path)).to_string();
    if audio_args(&ext).is_empty() {
        "m4a".into()
    } else {
        ext
    }
}

fn base_args(input: &Path) -> Vec<String> {
    vec!["-i".into(), path_str(input)]
}

pub fn convert(path: &Path, target: &str, ctx: &Ctx) -> Result<Vec<PathBuf>> {
    let info = probe(path)?;
    let out = output_path(path, target, None);
    let out_s = path_str(&out);
    let is_audio_target = !audio_args(target).is_empty() && !is_video_encode_target(target);

    if is_audio_target {
        if info.audio_codec.is_none() {
            bail!("{} has no sound", file_name(path));
        }
        let mut args = base_args(path);
        args.extend(s(&["-vn", "-map_metadata", "0"]));
        args.extend(audio_args(target));
        args.push(out_s);
        run_ffmpeg(&args, info.duration, ctx.progress)?;
        return Ok(vec![out]);
    }

    if target == "gif" {
        let mut args = base_args(path);
        args.extend(s(&[
            "-vf",
            "fps=12,scale='min(720,iw)':-2:flags=lanczos,split[a][b];[a]palettegen=stats_mode=diff[p];[b][p]paletteuse=dither=bayer:bayer_scale=4",
            "-loop",
            "0",
        ]));
        args.push(out_s);
        run_ffmpeg(&args, info.duration, ctx.progress)?;
        return Ok(vec![out]);
    }

    if info.video_codec.is_none() {
        bail!("{} has no video", file_name(path));
    }

    // Changing only the container is instant and lossless when the codecs fit.
    if can_copy(target, &info) {
        let mut args = base_args(path);
        args.extend(s(&["-map", "0:v:0", "-map", "0:a?", "-c", "copy", "-map_metadata", "0"]));
        if target == "mp4" || target == "mov" {
            args.extend(s(&["-movflags", "+faststart"]));
            if info.video_codec.as_deref() == Some("hevc") {
                args.extend(s(&["-tag:v", "hvc1"]));
            }
        }
        args.push(out_s.clone());
        if run_ffmpeg(&args, info.duration, ctx.progress).is_ok() {
            return Ok(vec![out]);
        }
        let _ = std::fs::remove_file(&out);
    }

    let mut args = base_args(path);
    args.extend(s(&["-map", "0:v:0", "-map", "0:a?", "-map_metadata", "0"]));
    if target != "webm" {
        args.extend(s(&["-vf", EVEN]));
    }
    args.extend(video_args(target, ctx.settings.video_crf));
    args.push(out_s);
    run_ffmpeg(&args, info.duration, ctx.progress)?;
    Ok(vec![out])
}

fn can_copy(target: &str, info: &Probe) -> bool {
    let v = info.video_codec.as_deref().unwrap_or("");
    let a = info.audio_codec.as_deref();
    match target {
        "mkv" => true,
        "mp4" | "mov" => {
            matches!(v, "h264" | "hevc" | "mpeg4" | "av1")
                && a.map(|a| matches!(a, "aac" | "mp3" | "alac" | "ac3")).unwrap_or(true)
        }
        "webm" => matches!(v, "vp8" | "vp9" | "av1") && a.map(|a| matches!(a, "opus" | "vorbis")).unwrap_or(true),
        _ => false,
    }
}

pub fn gif_to_video(path: &Path, target: &str, ctx: &Ctx) -> Result<PathBuf> {
    let info = probe(path).unwrap_or_default();
    let out = output_path(path, target, None);
    let mut args = base_args(path);
    if target == "webm" {
        args.extend(video_args("webm", ctx.settings.video_crf));
    } else {
        args.extend(s(&["-vf", EVEN]));
        args.extend(video_args("mp4", ctx.settings.video_crf));
    }
    args.push(path_str(&out));
    run_ffmpeg(&args, info.duration, ctx.progress)?;
    Ok(out)
}

pub fn video_compress(path: &Path, ctx: &Ctx) -> Result<PathBuf> {
    let info = probe(path)?;
    let out = output_path(path, "mp4", Some("compressed"));
    let mut args = base_args(path);
    args.extend(s(&[
        "-map", "0:v:0", "-map", "0:a?",
        "-vf", "scale='if(gt(iw,ih),min(1920,iw),-2)':'if(gt(iw,ih),-2,min(1920,ih))',scale=trunc(iw/2)*2:trunc(ih/2)*2",
        "-c:v", "libx264", "-preset", "medium", "-crf", "28", "-pix_fmt", "yuv420p",
        "-c:a", "aac", "-b:a", "128k", "-movflags", "+faststart",
    ]));
    args.push(path_str(&out));
    run_ffmpeg(&args, info.duration, ctx.progress)?;
    check_smaller(path, &out)?;
    Ok(out)
}

pub fn audio_compress(path: &Path, ctx: &Ctx) -> Result<PathBuf> {
    let info = probe(path)?;
    let ext = canonical_ext(&ext_of(path)).to_string();
    let (out_ext, codec): (&str, Vec<String>) = match ext.as_str() {
        "mp3" => ("mp3", s(&["-c:a", "libmp3lame", "-b:a", "128k"])),
        "ogg" => ("ogg", s(&["-c:a", "libvorbis", "-q:a", "3"])),
        "opus" => ("opus", s(&["-c:a", "libopus", "-b:a", "64k"])),
        "wma" => ("wma", s(&["-c:a", "wmav2", "-b:a", "96k"])),
        "m4a" | "aac" => ("m4a", s(&["-c:a", "aac", "-b:a", "128k"])),
        _ => ("m4a", s(&["-c:a", "aac", "-b:a", "160k"])),
    };
    let out = output_path(path, out_ext, Some("compressed"));
    let mut args = base_args(path);
    args.extend(s(&["-vn", "-map_metadata", "0"]));
    args.extend(codec);
    args.push(path_str(&out));
    run_ffmpeg(&args, info.duration, ctx.progress)?;
    check_smaller(path, &out)?;
    Ok(out)
}

fn check_smaller(input: &Path, out: &Path) -> Result<()> {
    let before = std::fs::metadata(input)?.len();
    let after = std::fs::metadata(out)?.len();
    if after >= before {
        let _ = std::fs::remove_file(out);
        bail!("{} is already as small as it gets", file_name(input));
    }
    Ok(())
}

/// Encoder arguments to write `path`'s own format again.
fn reencode_args(path: &Path, ctx: &Ctx) -> (String, Vec<String>) {
    if kind_of(path) == Kind::Audio {
        let ext = same_audio_ext(path);
        let args = audio_args(&ext);
        (ext, args)
    } else {
        let ext = same_video_ext(path);
        let mut args = Vec::new();
        if ext != "webm" {
            args.extend(s(&["-vf", EVEN]));
        }
        args.extend(video_args(&ext, ctx.settings.video_crf));
        (ext, args)
    }
}

pub fn trim(path: &Path, ctx: &Ctx) -> Result<PathBuf> {
    let start = ctx.param_f64("start").unwrap_or(0.0).max(0.0);
    let end = ctx.param_f64("end").context("no end point chosen")?;
    if end <= start {
        bail!("the end must come after the start");
    }
    let (ext, enc) = reencode_args(path, ctx);
    let out = output_path(path, &ext, Some("trimmed"));
    let mut args = s(&["-ss", &format!("{start:.3}")]);
    args.extend(base_args(path));
    args.extend(s(&["-t", &format!("{:.3}", end - start), "-map_metadata", "0"]));
    if kind_of(path) == Kind::Audio {
        args.extend(s(&["-map", "0:a:0", "-vn"]));
    } else {
        args.extend(s(&["-map", "0:v:0", "-map", "0:a?"]));
    }
    args.extend(enc);
    args.push(path_str(&out));
    run_ffmpeg(&args, Some(end - start), ctx.progress)?;
    Ok(out)
}

pub fn video_crop(path: &Path, ctx: &Ctx) -> Result<PathBuf> {
    let info = probe(path)?;
    let even = |k: &str| ctx.param_f64(k).map(|v| ((v.max(0.0) as u32) / 2) * 2);
    let (Some(x), Some(y), Some(w), Some(h)) = (even("x"), even("y"), even("width"), even("height")) else {
        bail!("no crop area chosen");
    };
    if w < 2 || h < 2 {
        bail!("the crop area is too small");
    }
    let ext = same_video_ext(path);
    let out = output_path(path, &ext, Some("cropped"));
    let mut args = base_args(path);
    args.extend(s(&["-map", "0:v:0", "-map", "0:a?", "-vf", &format!("crop={w}:{h}:{x}:{y}"), "-map_metadata", "0"]));
    args.extend(video_args(&ext, ctx.settings.video_crf));
    args.push(path_str(&out));
    run_ffmpeg(&args, info.duration, ctx.progress)?;
    Ok(out)
}

fn atempo_chain(mut factor: f64) -> String {
    let mut parts = Vec::new();
    while factor > 2.0 {
        parts.push("atempo=2.0".to_string());
        factor /= 2.0;
    }
    while factor < 0.5 {
        parts.push("atempo=0.5".to_string());
        factor /= 0.5;
    }
    parts.push(format!("atempo={factor:.4}"));
    parts.join(",")
}

pub fn speed(path: &Path, ctx: &Ctx) -> Result<PathBuf> {
    let factor = ctx.param_f64("factor").unwrap_or(2.0);
    if !(0.1..=10.0).contains(&factor) {
        bail!("speed must be between 0.1× and 10×");
    }
    let info = probe(path)?;
    let (ext, enc) = reencode_args(path, ctx);
    let label = format!("{}x", trim_float(factor));
    let out = output_path(path, &ext, Some(&label));
    let mut args = base_args(path);
    if info.video_codec.is_some() && kind_of(path) == Kind::Video {
        let vf = format!("setpts=PTS/{factor},{EVEN}");
        args.extend(s(&["-map", "0:v:0", "-map", "0:a?", "-filter:v", &vf]));
        if info.audio_codec.is_some() {
            args.extend(s(&["-filter:a", &atempo_chain(factor)]));
        }
        // `reencode_args` adds its own -vf; ours already includes the even-size scale.
        let mut skip = false;
        for a in enc {
            if skip {
                skip = false;
                continue;
            }
            if a == "-vf" {
                skip = true;
                continue;
            }
            args.push(a);
        }
    } else {
        args.extend(s(&["-vn", "-filter:a", &atempo_chain(factor)]));
        args.extend(enc);
    }
    args.push(path_str(&out));
    run_ffmpeg(&args, info.duration.map(|d| d / factor), ctx.progress)?;
    Ok(out)
}

fn trim_float(v: f64) -> String {
    let s = format!("{v:.2}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

pub fn extract_audio(path: &Path, ctx: &Ctx) -> Result<PathBuf> {
    let info = probe(path)?;
    let Some(codec) = info.audio_codec.as_deref() else {
        bail!("{} has no sound", file_name(path));
    };
    let copy_ext = match codec {
        "aac" => Some("m4a"),
        "mp3" => Some("mp3"),
        "opus" => Some("opus"),
        "vorbis" => Some("ogg"),
        "flac" => Some("flac"),
        _ => None,
    };
    let ext = copy_ext.unwrap_or("mp3");
    let out = output_path(path, ext, None);
    let mut args = base_args(path);
    args.extend(s(&["-vn", "-map", "0:a:0", "-map_metadata", "0"]));
    if copy_ext.is_some() {
        args.extend(s(&["-c:a", "copy"]));
    } else {
        args.extend(audio_args("mp3"));
    }
    args.push(path_str(&out));
    run_ffmpeg(&args, info.duration, ctx.progress)?;
    Ok(out)
}

pub fn mute(path: &Path, ctx: &Ctx) -> Result<PathBuf> {
    let info = probe(path)?;
    let ext = canonical_ext(&ext_of(path)).to_string();
    let out = output_path(path, &ext, Some("muted"));
    let mut args = base_args(path);
    args.extend(s(&["-map", "0:v", "-c", "copy", "-an", "-map_metadata", "0"]));
    args.push(path_str(&out));
    run_ffmpeg(&args, info.duration, ctx.progress)?;
    Ok(out)
}

pub fn video_rotate(path: &Path, ctx: &Ctx) -> Result<PathBuf> {
    let info = probe(path)?;
    let ext = same_video_ext(path);
    let out = output_path(path, &ext, Some("rotated"));
    let mut args = base_args(path);
    args.extend(s(&["-map", "0:v:0", "-map", "0:a?", "-vf", &format!("transpose=1,{EVEN}"), "-map_metadata", "0"]));
    args.extend(video_args(&ext, ctx.settings.video_crf));
    args.push(path_str(&out));
    run_ffmpeg(&args, info.duration, ctx.progress)?;
    Ok(out)
}

pub fn strip_metadata(path: &Path, ctx: &Ctx) -> Result<PathBuf> {
    let info = probe(path)?;
    let ext = canonical_ext(&ext_of(path)).to_string();
    let out = output_path(path, &ext, Some("clean"));
    let mut args = base_args(path);
    args.extend(s(&["-map", "0", "-map_metadata", "-1", "-map_chapters", "-1", "-c", "copy", "-fflags", "+bitexact"]));
    args.push(path_str(&out));
    run_ffmpeg(&args, info.duration, ctx.progress)?;
    Ok(out)
}

pub fn audio_filter(path: &Path, ctx: &Ctx, suffix: &str, filter: &str) -> Result<PathBuf> {
    let info = probe(path)?;
    let ext = same_audio_ext(path);
    let out = output_path(path, &ext, Some(suffix));
    let mut args = base_args(path);
    args.extend(s(&["-vn", "-map_metadata", "0", "-af", filter]));
    args.extend(audio_args(&ext));
    args.push(path_str(&out));
    run_ffmpeg(&args, info.duration, ctx.progress)?;
    Ok(out)
}

/// One frame as a JPEG, for the crop and trim editors.
pub fn frame(path: &Path, at: f64, width: u32, dest: &Path) -> Result<()> {
    let ffmpeg = crate::bins::require("ffmpeg")?;
    super::run(
        Command::new(ffmpeg)
            .args(["-hide_banner", "-loglevel", "error", "-y", "-ss", &format!("{:.3}", at.max(0.0))])
            .arg("-i")
            .arg(path)
            .args(["-frames:v", "1", "-vf", &format!("scale={width}:-2"), "-q:v", "4"])
            .arg(dest),
    )
}

/// Loudness peaks (0–1) across the file, for drawing a waveform.
pub fn waveform(path: &Path, buckets: usize) -> Result<Vec<f32>> {
    let ffmpeg = crate::bins::require("ffmpeg")?;
    let mut child = super::quiet(&mut Command::new(ffmpeg))
        .args(["-hide_banner", "-loglevel", "error", "-i"])
        .arg(path)
        .args(["-vn", "-ac", "1", "-ar", "4000", "-f", "s16le", "-"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let mut raw = Vec::new();
    child.stdout.take().unwrap().read_to_end(&mut raw)?;
    child.wait()?;
    let samples: Vec<i16> = raw.chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]])).collect();
    if samples.is_empty() {
        return Ok(vec![0.0; buckets]);
    }
    let per = (samples.len() / buckets).max(1);
    let mut peaks: Vec<f32> = samples
        .chunks(per)
        .map(|c| c.iter().map(|s| (*s as f32).abs()).fold(0.0, f32::max) / 32768.0)
        .collect();
    peaks.truncate(buckets);
    let max = peaks.iter().cloned().fold(0.0001, f32::max);
    Ok(peaks.into_iter().map(|p| p / max).collect())
}

fn file_name(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chains_atempo_outside_its_range() {
        assert_eq!(atempo_chain(4.0), "atempo=2.0,atempo=2.0000");
        assert_eq!(atempo_chain(0.25), "atempo=0.5,atempo=0.5000");
        assert_eq!(atempo_chain(1.5), "atempo=1.5000");
    }
}
