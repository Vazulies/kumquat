//! Documents, slides and spreadsheets, via pandoc and LibreOffice.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

use anyhow::{bail, Context as _, Result};

use super::{output_path, run, stem, Ctx, TempDir};
use crate::catalog::{kind_of, Kind};

/// LibreOffice refuses to run twice with the same profile, so conversions queue up.
static SOFFICE_LOCK: Mutex<()> = Mutex::new(());

pub fn pandoc(input: &Path, out: &Path, extra: &[&str]) -> Result<()> {
    let bin = crate::bins::require("pandoc")?;
    let mut cmd = Command::new(bin);
    cmd.arg(input).arg("-o").arg(out).args(extra);
    // Relative paths (like --extract-media) are relative to the output.
    if let Some(dir) = out.parent() {
        cmd.current_dir(dir);
    }
    if let Some(dir) = input.parent() {
        cmd.arg("--resource-path").arg(dir);
    }
    run(&mut cmd)
}

/// Move a file, falling back to copy+delete across filesystems.
pub fn move_file(from: &Path, to: &Path) -> Result<()> {
    if std::fs::rename(from, to).is_err() {
        std::fs::copy(from, to).with_context(|| format!("could not write {}", to.display()))?;
        let _ = std::fs::remove_file(from);
    }
    Ok(())
}

/// `file:///home/me/x` or `file:///C:/Users/me/x`, as LibreOffice wants it.
fn file_url(path: &Path) -> String {
    let p = path.display().to_string();
    let p = if cfg!(windows) { p.replace('\\', "/") } else { p };
    if p.starts_with('/') { format!("file://{p}") } else { format!("file:///{p}") }
}

/// Convert with LibreOffice into a scratch folder and return the produced file.
fn soffice_raw(input: &Path, filter: &str, infilter: Option<&str>, tmp: &TempDir) -> Result<PathBuf> {
    let bin = crate::bins::require("soffice")?;
    let profile = std::env::temp_dir().join("kumquat").join("lo-profile");
    let _guard = SOFFICE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut cmd = Command::new(bin);
    cmd.arg(format!("-env:UserInstallation={}", file_url(&profile)))
        .args(["--headless", "--norestore", "--nologo", "--nodefault", "--nolockcheck"]);
    if let Some(inf) = infilter {
        cmd.arg(format!("--infilter={inf}"));
    }
    cmd.args(["--convert-to", filter, "--outdir"]).arg(tmp.path()).arg(input);
    run(&mut cmd)?;
    let produced = std::fs::read_dir(tmp.path())?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .find(|p| p.is_file())
        .context("LibreOffice didn't produce a file. Is the input damaged?")?;
    Ok(produced)
}

/// LibreOffice conversion straight to the final output location, named after `name_from`.
pub fn soffice_to(input: &Path, name_from: &Path, out_ext: &str, filter: &str, infilter: Option<&str>) -> Result<PathBuf> {
    let tmp = TempDir::new()?;
    let produced = soffice_raw(input, filter, infilter, &tmp)?;
    let out = output_path(name_from, out_ext, None);
    move_file(&produced, &out)?;
    Ok(out)
}

/// LibreOffice conversion into a scratch folder, for multi-step pipelines.
fn soffice_temp(input: &Path, filter: &str, infilter: Option<&str>, tmp: &TempDir) -> Result<PathBuf> {
    let sub = TempDir::new()?;
    let produced = soffice_raw(input, filter, infilter, &sub)?;
    let dest = tmp.join(produced.file_name().unwrap());
    move_file(&produced, &dest)?;
    Ok(dest)
}

fn csv_infilter(ext: &str) -> Option<&'static str> {
    match ext {
        "csv" => Some("CSV:44,34,76"),
        "tsv" => Some("CSV:9,34,76"),
        _ => None,
    }
}

pub fn convert(path: &Path, ext: &str, target: &str, ctx: &Ctx) -> Result<Vec<PathBuf>> {
    match kind_of(path) {
        Kind::Document => document(path, ext, target, ctx),
        Kind::Slides => slides(path, target, ctx),
        Kind::Sheet => sheet(path, ext, target),
        _ => bail!("not a document"),
    }
}

fn pandoc_reader(ext: &str) -> Option<&'static str> {
    match ext {
        "docx" => Some("docx"),
        "odt" => Some("odt"),
        "md" | "txt" => Some("markdown"),
        "epub" => Some("epub"),
        "html" => Some("html"),
        "rtf" => Some("rtf"),
        _ => None,
    }
}

fn document(path: &Path, ext: &str, target: &str, ctx: &Ctx) -> Result<Vec<PathBuf>> {
    let tmp = TempDir::new()?;
    let title = stem(path);

    // Things only LibreOffice reads (Word 97, Pages) become DOCX first.
    let (source, source_ext) = if pandoc_reader(ext).is_none() {
        (soffice_temp(path, "docx:MS Word 2007 XML", None, &tmp)?, "docx".to_string())
    } else {
        (path.to_path_buf(), ext.to_string())
    };

    let office_native = matches!(source_ext.as_str(), "docx" | "odt" | "rtf" | "txt" | "html");
    match target {
        "pdf" => {
            if office_native {
                Ok(vec![soffice_to(&source, path, "pdf", "pdf", None)?])
            } else {
                let docx = tmp.join(format!("{title}.docx"));
                pandoc(&source, &docx, &["-f", pandoc_reader(&source_ext).unwrap()])?;
                Ok(vec![soffice_to(&docx, path, "pdf", "pdf", None)?])
            }
        }
        "jpg" | "png" => {
            let pdf = if office_native {
                soffice_temp(&source, "pdf", None, &tmp)?
            } else {
                let docx = tmp.join(format!("{title}.docx"));
                pandoc(&source, &docx, &["-f", "markdown"])?;
                soffice_temp(&docx, "pdf", None, &tmp)?
            };
            super::pdf::render_pages(&pdf, path, target, ctx.settings.pdf_dpi.min(200), ctx.settings.image_quality, ctx.progress)
        }
        "docx" | "odt" | "rtf" if source_ext == "txt" && crate::bins::find("soffice").is_some() => {
            let filter = match target {
                "docx" => "docx:MS Word 2007 XML",
                "odt" => "odt",
                _ => "rtf",
            };
            Ok(vec![soffice_to(&source, path, target, filter, None)?])
        }
        "md" if source_ext == "txt" => {
            let out = output_path(path, "md", None);
            std::fs::copy(&source, &out)?;
            Ok(vec![out])
        }
        _ => {
            let out = output_path(path, target, None);
            let reader = pandoc_reader(&source_ext).unwrap();
            let mut args: Vec<String> = vec!["-f".into(), reader.into()];
            match target {
                "txt" => args.extend(["-t".into(), "plain".into(), "--wrap=none".into()]),
                "md" => {
                    args.extend(["-t".into(), "gfm".into(), "--wrap=none".into()]);
                    args.push(format!("--extract-media={} media", stem(&out)));
                }
                "html" => args.extend([
                    "-s".into(),
                    "--embed-resources".into(),
                    "--metadata".into(),
                    format!("pagetitle={title}"),
                ]),
                "epub" => args.extend(["--metadata".into(), format!("title={title}")]),
                "rtf" => args.push("-s".into()),
                _ => {}
            }
            let refs: Vec<&str> = args.iter().map(String::as_str).collect();
            pandoc(&source, &out, &refs)?;
            Ok(vec![out])
        }
    }
}

fn slides(path: &Path, target: &str, ctx: &Ctx) -> Result<Vec<PathBuf>> {
    match target {
        "pdf" => Ok(vec![soffice_to(path, path, "pdf", "pdf:impress_pdf_Export", None)?]),
        "pptx" => Ok(vec![soffice_to(path, path, "pptx", "pptx:Impress MS PowerPoint 2007 XML", None)?]),
        "odp" => Ok(vec![soffice_to(path, path, "odp", "odp", None)?]),
        "jpg" | "png" => {
            let tmp = TempDir::new()?;
            let pdf = soffice_temp(path, "pdf:impress_pdf_Export", None, &tmp)?;
            super::pdf::render_pages(&pdf, path, target, ctx.settings.pdf_dpi.min(200), ctx.settings.image_quality, ctx.progress)
        }
        other => bail!("slides to {other} isn't supported"),
    }
}

fn sheet(path: &Path, ext: &str, target: &str) -> Result<Vec<PathBuf>> {
    let infilter = csv_infilter(ext);
    match (ext, target) {
        ("csv", "tsv") | ("tsv", "csv") => {
            let text = std::fs::read_to_string(path)?;
            let (from, to) = if ext == "csv" { (',', '\t') } else { ('\t', ',') };
            let out = output_path(path, target, None);
            std::fs::write(&out, write_delimited(&parse_delimited(&text, from), to))?;
            Ok(vec![out])
        }
        (_, "csv") => Ok(vec![soffice_to(path, path, "csv", "csv:Text - txt - csv (StarCalc):44,34,76", infilter)?]),
        (_, "tsv") => Ok(vec![soffice_to(path, path, "tsv", "csv:Text - txt - csv (StarCalc):9,34,76", infilter)?]),
        (_, "xlsx") => Ok(vec![soffice_to(path, path, "xlsx", "xlsx:Calc MS Excel 2007 XML", infilter)?]),
        (_, "ods") => Ok(vec![soffice_to(path, path, "ods", "ods", infilter)?]),
        (_, "pdf") => Ok(vec![soffice_to(path, path, "pdf", "pdf:calc_pdf_Export", infilter)?]),
        (_, other) => bail!("spreadsheet to {other} isn't supported"),
    }
}

/// RFC 4180-style parsing: quotes may wrap delimiters, newlines and doubled quotes.
pub fn parse_delimited(text: &str, delim: char) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if quoted {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    field.push('"');
                    chars.next();
                } else {
                    quoted = false;
                }
            } else {
                field.push(c);
            }
        } else if c == '"' && field.is_empty() {
            quoted = true;
        } else if c == delim {
            row.push(std::mem::take(&mut field));
        } else if c == '\n' {
            row.push(std::mem::take(&mut field));
            rows.push(std::mem::take(&mut row));
        } else if c != '\r' {
            field.push(c);
        }
    }
    if !field.is_empty() || !row.is_empty() {
        row.push(field);
        rows.push(row);
    }
    rows
}

pub fn write_delimited(rows: &[Vec<String>], delim: char) -> String {
    let mut out = String::new();
    for row in rows {
        let cells: Vec<String> = row
            .iter()
            .map(|f| {
                if f.contains(delim) || f.contains('"') || f.contains('\n') {
                    format!("\"{}\"", f.replace('"', "\"\""))
                } else {
                    f.clone()
                }
            })
            .collect();
        out.push_str(&cells.join(&delim.to_string()));
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_csv_to_tsv() {
        let csv = "name,note\n\"Smith, J\",\"said \"\"hi\"\"\"\n";
        let rows = parse_delimited(csv, ',');
        assert_eq!(rows, vec![vec!["name", "note"], vec!["Smith, J", "said \"hi\""]]);
        assert_eq!(write_delimited(&rows, '\t'), "name\tnote\nSmith, J\t\"said \"\"hi\"\"\"\n");
    }
}
