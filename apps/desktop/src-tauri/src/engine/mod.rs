//! Turns a wheel item plus some files into new files.

pub mod archive;
pub mod av;
pub mod docs;
pub mod image;
pub mod pdf;
pub mod subs;

use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{bail, Context as _, Result};
use serde_json::Value;

use crate::catalog::{self, canonical_ext, ext_of, Kind};
use crate::settings::{self, Settings};

/// What a finished job produced.
pub enum Outcome {
    Files(Vec<PathBuf>),
    /// Text to show the user, e.g. the contents of a QR code.
    Message { title: String, body: String },
}

pub struct Ctx<'a> {
    pub settings: Settings,
    pub params: &'a Value,
    pub progress: &'a (dyn Fn(f32) + Sync),
}

impl Ctx<'_> {
    pub fn param_f64(&self, key: &str) -> Option<f64> {
        self.params.get(key).and_then(Value::as_f64)
    }
    pub fn param_str(&self, key: &str) -> Option<&str> {
        self.params.get(key).and_then(Value::as_str)
    }
}

/// File name without its extension (or double extension like `.tar.gz`).
pub fn stem(path: &Path) -> String {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "file".into());
    if path.is_dir() {
        return name;
    }
    let ext = ext_of(path);
    if ext.is_empty() {
        name
    } else {
        name[..name.len() - ext.len() - 1].to_string()
    }
}

/// Per-job output handling: an optional target folder (where the file was
/// dragged to) and hidden staging folders that results are written into, so
/// half-written files never show up in the user's folders.
struct Scope {
    id: String,
    target: Option<PathBuf>,
    /// (staging folder, folder its contents move to when the job succeeds)
    staged: Vec<(PathBuf, PathBuf)>,
}

thread_local! {
    static SCOPE: std::cell::RefCell<Option<Scope>> = const { std::cell::RefCell::new(None) };
}

/// Start staging outputs on this thread. `target` overrides where results go.
pub fn begin_job(target: Option<PathBuf>) {
    SCOPE.with(|s| {
        *s.borrow_mut() = Some(Scope { id: uuid::Uuid::new_v4().simple().to_string(), target, staged: vec![] });
    });
}

/// Move staged results into place and return their final paths.
/// `mover` does the actual move (e.g. to keep desktop metadata).
pub fn finish_job(outputs: Vec<PathBuf>, mover: &dyn Fn(&Path, &Path) -> std::io::Result<()>) -> Result<Vec<PathBuf>> {
    let scope = SCOPE.with(|s| s.borrow_mut().take());
    let Some(scope) = scope else { return Ok(outputs) };
    let mut moved: Vec<(PathBuf, PathBuf)> = Vec::new();
    for (staging, dest_dir) in &scope.staged {
        let mut entries: Vec<PathBuf> = std::fs::read_dir(staging)
            .map(|rd| rd.filter_map(|e| e.ok()).map(|e| e.path()).collect())
            .unwrap_or_default();
        entries.sort();
        for entry in entries {
            let dest = unique(dest_dir.join(entry.file_name().unwrap()));
            mover(&entry, &dest).with_context(|| format!("could not save {}", dest.display()))?;
            moved.push((entry, dest));
        }
        let _ = std::fs::remove_dir_all(staging);
    }
    Ok(outputs
        .into_iter()
        .map(|o| moved.iter().find(|(from, _)| *from == o).map(|(_, to)| to.clone()).unwrap_or(o))
        .collect())
}

/// Throw away anything a failed job wrote.
pub fn abort_job() {
    if let Some(scope) = SCOPE.with(|s| s.borrow_mut().take()) {
        for (staging, _) in scope.staged {
            let _ = std::fs::remove_dir_all(staging);
        }
    }
}

fn final_dir(input: &Path) -> PathBuf {
    let s = settings::get();
    if !s.output_beside_original {
        if let Some(dir) = s.output_folder.filter(|d| !d.is_empty()) {
            let dir = PathBuf::from(dir);
            if std::fs::create_dir_all(&dir).is_ok() {
                return dir;
            }
        }
    }
    let target = SCOPE.with(|s| s.borrow().as_ref().and_then(|sc| sc.target.clone()));
    if let Some(dir) = target.filter(|d| tempfile_probe(d)) {
        return dir;
    }
    let parent = input.parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("."));
    if tempfile_probe(&parent) {
        parent
    } else {
        dirs::download_dir().unwrap_or_else(std::env::temp_dir)
    }
}

fn output_dir(input: &Path) -> PathBuf {
    let dest = final_dir(input);
    SCOPE.with(|s| {
        let mut s = s.borrow_mut();
        let Some(scope) = s.as_mut() else { return dest.clone() };
        if let Some((staging, _)) = scope.staged.iter().find(|(_, d)| *d == dest) {
            return staging.clone();
        }
        let staging = dest.join(format!(".kumquat-{}", scope.id));
        if std::fs::create_dir_all(&staging).is_err() {
            return dest.clone();
        }
        hide(&staging);
        scope.staged.push((staging.clone(), dest.clone()));
        staging
    })
}

/// Dot-folders are already hidden on Linux; Windows needs the attribute.
fn hide(_path: &Path) {
    #[cfg(windows)]
    unsafe {
        use windows::Win32::Storage::FileSystem::{SetFileAttributesW, FILE_ATTRIBUTE_HIDDEN};
        let wide = crate::win::wide(_path);
        let _ = SetFileAttributesW(windows::core::PCWSTR(wide.as_ptr()), FILE_ATTRIBUTE_HIDDEN);
    }
}

/// Keep helper programs from flashing a console window on Windows.
pub fn quiet(cmd: &mut Command) -> &mut Command {
    #[cfg(windows)]
    crate::win::no_console(cmd);
    cmd
}

fn tempfile_probe(dir: &Path) -> bool {
    let probe = dir.join(format!(".kumquat-{}", uuid::Uuid::new_v4()));
    let ok = std::fs::write(&probe, b"").is_ok();
    let _ = std::fs::remove_file(&probe);
    ok
}

/// Where a result should be written: next to the input, never overwriting.
pub fn output_path(input: &Path, ext: &str, suffix: Option<&str>) -> PathBuf {
    let base = match suffix {
        Some(s) => format!("{} ({s})", stem(input)),
        None => stem(input),
    };
    let name = if ext.is_empty() { base } else { format!("{base}.{ext}") };
    unique(output_dir(input).join(name))
}

/// `name.ext`, then `name 2.ext`, `name 3.ext`, …
pub fn unique(path: PathBuf) -> PathBuf {
    if !path.exists() {
        return path;
    }
    let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
    let ext = ext_of(&path);
    let stem = stem(&path);
    for n in 2.. {
        let name = if ext.is_empty() || path.is_dir() {
            format!("{stem} {n}")
        } else {
            format!("{stem} {n}.{ext}")
        };
        let candidate = dir.join(name);
        if !candidate.exists() {
            return candidate;
        }
    }
    unreachable!()
}

/// A scratch directory that is removed when dropped.
pub struct TempDir(pub PathBuf);

impl TempDir {
    pub fn new() -> Result<Self> {
        let dir = std::env::temp_dir().join("kumquat").join(uuid::Uuid::new_v4().to_string());
        std::fs::create_dir_all(&dir)?;
        Ok(Self(dir))
    }
    pub fn path(&self) -> &Path {
        &self.0
    }
    pub fn join(&self, p: impl AsRef<Path>) -> PathBuf {
        self.0.join(p)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Run a program, turning a failure into an error that includes its stderr.
pub fn run(cmd: &mut Command) -> Result<()> {
    let program = cmd.get_program().to_string_lossy().to_string();
    log::debug!("running {:?}", cmd);
    let out = quiet(cmd)
        .stdin(Stdio::null())
        .output()
        .with_context(|| format!("could not start {program}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        let tail: Vec<&str> = err.lines().rev().take(6).collect();
        let tail: Vec<&str> = tail.into_iter().rev().collect();
        bail!("{} failed: {}", short_name(&program), tail.join(" ").trim());
    }
    Ok(())
}

pub fn run_output(cmd: &mut Command) -> Result<String> {
    let program = cmd.get_program().to_string_lossy().to_string();
    let out = quiet(cmd)
        .stdin(Stdio::null())
        .output()
        .with_context(|| format!("could not start {program}"))?;
    if !out.status.success() {
        bail!("{} failed: {}", short_name(&program), String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn short_name(program: &str) -> String {
    Path::new(program)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| program.to_string())
}

/// Run ffmpeg with `-progress`, reporting 0–1 progress against `duration` seconds.
pub fn run_ffmpeg(args: &[String], duration: Option<f64>, progress: &(dyn Fn(f32) + Sync)) -> Result<()> {
    let ffmpeg = crate::bins::require("ffmpeg")?;
    let mut cmd = Command::new(ffmpeg);
    cmd.args(["-hide_banner", "-nostdin", "-y", "-loglevel", "error", "-progress", "pipe:1", "-nostats"]);
    cmd.args(args);
    log::debug!("running {:?}", cmd);
    let mut child = quiet(&mut cmd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("could not start ffmpeg")?;
    let mut stderr = child.stderr.take().unwrap();
    let err_reader = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = stderr.read_to_string(&mut s);
        s
    });
    if let Some(stdout) = child.stdout.take() {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if let (Some(total), Some(v)) = (duration, line.strip_prefix("out_time_us=")) {
                if let Ok(us) = v.trim().parse::<f64>() {
                    if total > 0.0 {
                        progress(((us / 1e6) / total).clamp(0.0, 1.0) as f32);
                    }
                }
            }
        }
    }
    let status = child.wait()?;
    let err = err_reader.join().unwrap_or_default();
    if !status.success() {
        let tail: Vec<&str> = err.lines().rev().take(4).collect();
        let tail: Vec<&str> = tail.into_iter().rev().collect();
        bail!("ffmpeg failed: {}", tail.join(" ").trim());
    }
    Ok(())
}

pub fn path_str(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

/// Convert one file to `target` (an extension from the catalog).
pub fn convert(path: &Path, target: &str, ctx: &Ctx) -> Result<Vec<PathBuf>> {
    let ext = ext_of(path);
    let ext = canonical_ext(&ext);
    match catalog::kind_of(path) {
        Kind::Image => image::convert(path, ext, target, ctx),
        Kind::Audio | Kind::Video => av::convert(path, target, ctx),
        Kind::Pdf => pdf::convert(path, target, ctx),
        Kind::Document | Kind::Slides | Kind::Sheet => docs::convert(path, ext, target, ctx),
        Kind::Subtitle => subs::convert(path, ext, target),
        Kind::Archive => archive::convert(path, ext, target),
        Kind::Folder | Kind::Other => bail!("Kumquat can't convert this kind of file"),
    }
}

/// Run a `tool:` item.
pub fn tool(name: &str, paths: &[PathBuf], ctx: &Ctx) -> Result<Outcome> {
    let each = |f: &dyn Fn(&Path) -> Result<PathBuf>| -> Result<Outcome> {
        let mut out = Vec::new();
        for (i, p) in paths.iter().enumerate() {
            out.push(f(p)?);
            (ctx.progress)((i + 1) as f32 / paths.len() as f32);
        }
        Ok(Outcome::Files(out))
    };
    match name {
        "image.compress" => each(&|p| image::compress(p, ctx)),
        "image.resize" => each(&|p| image::resize(p, ctx)),
        "image.crop" => each(&|p| image::crop(p, ctx)),
        "image.annotate" => each(&|p| image::annotate(p, ctx)),
        "image.rotate" => each(&|p| image::rotate(p, ctx)),
        "image.background" => each(&|p| image::background(p, ctx)),
        "image.strip" => each(&|p| image::strip_metadata(p, ctx)),
        "image.qr" => image::read_qr(paths),
        "image.combine" => Ok(Outcome::Files(vec![image::combine_pdf(paths, ctx)?])),

        "pdf.compress" => each(&|p| pdf::compress(p)),
        "pdf.merge" => Ok(Outcome::Files(vec![pdf::merge(paths)?])),
        "pdf.split" => each(&|p| pdf::split(p)),
        "pdf.organize" => each(&|p| pdf::organize(p, ctx)),
        "pdf.redact" => each(&|p| pdf::redact(p, ctx)),
        "pdf.rotate" => each(&|p| pdf::rotate_all(p)),
        "pdf.strip" => each(&|p| pdf::strip_metadata(p)),

        "video.compress" => each(&|p| av::video_compress(p, ctx)),
        "video.trim" | "audio.trim" => each(&|p| av::trim(p, ctx)),
        "video.crop" => each(&|p| av::video_crop(p, ctx)),
        "video.speed" | "audio.speed" => each(&|p| av::speed(p, ctx)),
        "video.audio" => each(&|p| av::extract_audio(p, ctx)),
        "video.mute" => each(&|p| av::mute(p, ctx)),
        "video.rotate" => each(&|p| av::video_rotate(p, ctx)),
        "video.strip" | "audio.strip" => each(&|p| av::strip_metadata(p, ctx)),

        "audio.compress" => each(&|p| av::audio_compress(p, ctx)),
        "audio.cleanup" => each(&|p| av::audio_filter(p, ctx, "cleaned", "highpass=f=80,lowpass=f=12000,afftdn=nf=-25,loudnorm=I=-16:TP=-1.5:LRA=11")),
        "audio.normalize" => each(&|p| av::audio_filter(p, ctx, "normalized", "loudnorm=I=-16:TP=-1.5:LRA=11")),

        "subtitle.shift" => each(&|p| subs::shift(p, ctx.param_f64("seconds").unwrap_or(0.0))),
        "archive.extract" => each(&|p| archive::extract(p)),
        "any.zip" => Ok(Outcome::Files(vec![archive::zip_paths(paths)?])),
        other => bail!("unknown tool {other}"),
    }
}

#[cfg(test)]
mod staging_tests {
    use super::*;

    #[test]
    fn stages_outputs_then_moves_them_into_the_target() {
        let tmp = TempDir::new().unwrap();
        let src = tmp.join("in/photo.png");
        std::fs::create_dir_all(src.parent().unwrap()).unwrap();
        std::fs::write(&src, b"x").unwrap();
        let target = tmp.join("dropped-here");
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(target.join("photo.jpg"), b"existing").unwrap();

        begin_job(Some(target.clone()));
        let out = output_path(&src, "jpg", None);
        assert!(out.parent().unwrap().file_name().unwrap().to_string_lossy().starts_with(".kumquat-"));
        std::fs::write(&out, b"new").unwrap();
        let done = finish_job(vec![out], &|a, b| std::fs::rename(a, b)).unwrap();

        // Lands in the drop target, without overwriting what's there.
        assert_eq!(done, vec![target.join("photo 2.jpg")]);
        assert_eq!(std::fs::read(target.join("photo.jpg")).unwrap(), b"existing");
        let leftovers: Vec<_> = std::fs::read_dir(&target).unwrap().filter_map(|e| e.ok()).map(|e| e.file_name()).collect();
        assert_eq!(leftovers.len(), 2, "staging folder should be gone: {leftovers:?}");
    }

    #[test]
    fn failed_jobs_leave_nothing_behind() {
        let tmp = TempDir::new().unwrap();
        let src = tmp.join("a.png");
        std::fs::write(&src, b"x").unwrap();
        begin_job(None);
        let out = output_path(&src, "jpg", None);
        std::fs::write(&out, b"partial").unwrap();
        abort_job();
        assert_eq!(std::fs::read_dir(tmp.path()).unwrap().count(), 1);
    }
}
