//! SRT and WebVTT subtitles.

use std::path::{Path, PathBuf};

use anyhow::{bail, Result};

use super::output_path;
use crate::catalog::{canonical_ext, ext_of};

#[derive(Debug, Clone, PartialEq)]
pub struct Cue {
    pub start: i64,
    pub end: i64,
    pub text: String,
}

/// `01:02:03,456`, `01:02:03.456` or `02:03.456` → milliseconds.
fn parse_time(t: &str) -> Option<i64> {
    let t = t.trim().replace(',', ".");
    let (hms, ms) = t.split_once('.').unwrap_or((&t, "0"));
    let parts: Vec<i64> = hms.split(':').map(|p| p.parse().ok()).collect::<Option<_>>()?;
    let secs = match parts.as_slice() {
        [h, m, s] => h * 3600 + m * 60 + s,
        [m, s] => m * 60 + s,
        _ => return None,
    };
    let ms: i64 = format!("{ms:0<3}")[..3].parse().ok()?;
    Some(secs * 1000 + ms)
}

fn format_time(ms: i64, sep: char) -> String {
    let ms = ms.max(0);
    format!("{:02}:{:02}:{:02}{sep}{:03}", ms / 3_600_000, ms / 60_000 % 60, ms / 1000 % 60, ms % 1000)
}

pub fn parse(text: &str) -> Vec<Cue> {
    let text = text.trim_start_matches('\u{feff}').replace("\r\n", "\n");
    let mut cues = Vec::new();
    for block in text.split("\n\n") {
        let lines: Vec<&str> = block.lines().collect();
        let Some(timing) = lines.iter().position(|l| l.contains("-->")) else { continue };
        let (a, b) = lines[timing].split_once("-->").unwrap();
        // WebVTT may put cue settings after the end time.
        let b = b.split_whitespace().next().unwrap_or("");
        let (Some(start), Some(end)) = (parse_time(a), parse_time(b)) else { continue };
        cues.push(Cue { start, end, text: lines[timing + 1..].join("\n") });
    }
    cues
}

pub fn to_srt(cues: &[Cue]) -> String {
    let mut out = String::new();
    for (i, c) in cues.iter().enumerate() {
        out.push_str(&format!("{}\n{} --> {}\n{}\n\n", i + 1, format_time(c.start, ','), format_time(c.end, ','), c.text));
    }
    out
}

pub fn to_vtt(cues: &[Cue]) -> String {
    let mut out = String::from("WEBVTT\n\n");
    for c in cues {
        out.push_str(&format!("{} --> {}\n{}\n\n", format_time(c.start, '.'), format_time(c.end, '.'), c.text));
    }
    out
}

/// Just the words, without styling tags.
pub fn to_text(cues: &[Cue]) -> String {
    let mut out = String::new();
    let mut last = String::new();
    for c in cues {
        let mut clean = String::new();
        let mut depth = 0;
        for ch in c.text.chars() {
            match ch {
                '<' | '{' => depth += 1,
                '>' | '}' if depth > 0 => depth -= 1,
                _ if depth == 0 => clean.push(ch),
                _ => {}
            }
        }
        let line = clean.split_whitespace().collect::<Vec<_>>().join(" ");
        if !line.is_empty() && line != last {
            out.push_str(&line);
            out.push('\n');
            last = line;
        }
    }
    out
}

fn render(cues: &[Cue], ext: &str) -> String {
    match ext {
        "vtt" => to_vtt(cues),
        "txt" => to_text(cues),
        _ => to_srt(cues),
    }
}

fn read(path: &Path) -> Result<Vec<Cue>> {
    let bytes = std::fs::read(path)?;
    let cues = parse(&String::from_utf8_lossy(&bytes));
    if cues.is_empty() {
        bail!("no subtitles found in this file");
    }
    Ok(cues)
}

pub fn convert(path: &Path, _ext: &str, target: &str) -> Result<Vec<PathBuf>> {
    let cues = read(path)?;
    let out = output_path(path, target, None);
    std::fs::write(&out, render(&cues, target))?;
    Ok(vec![out])
}

pub fn shift(path: &Path, seconds: f64) -> Result<PathBuf> {
    let offset = (seconds * 1000.0).round() as i64;
    let cues: Vec<Cue> = read(path)?
        .into_iter()
        .map(|c| Cue { start: (c.start + offset).max(0), end: (c.end + offset).max(0), ..c })
        .collect();
    let ext = canonical_ext(&ext_of(path)).to_string();
    let out = output_path(path, &ext, Some("shifted"));
    std::fs::write(&out, render(&cues, &ext))?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_between_formats() {
        let srt = "1\n00:00:01,500 --> 00:00:03,000\n<i>Hello</i>\n\n2\n00:01:00,000 --> 00:01:02,250\nWorld\n";
        let cues = parse(srt);
        assert_eq!(cues.len(), 2);
        assert_eq!(cues[0].start, 1500);
        let vtt = to_vtt(&cues);
        assert!(vtt.starts_with("WEBVTT\n\n00:00:01.500 --> 00:00:03.000\n<i>Hello</i>"));
        assert_eq!(parse(&vtt), cues);
        assert_eq!(to_text(&cues), "Hello\nWorld\n");
    }

    #[test]
    fn reads_short_vtt_times_with_settings() {
        let cues = parse("WEBVTT\n\n00:05.000 --> 00:07.000 align:start\nHi\n");
        assert_eq!(cues[0].start, 5000);
        assert_eq!(cues[0].end, 7000);
    }
}
