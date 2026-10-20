//! ZIP, TAR and GZIP in Rust; RAR and 7z through 7-Zip.

use std::fs::File;
use std::io::{BufReader, BufWriter};
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context as _, Result};
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;

use super::{output_path, run, stem, unique, TempDir};

/// Unpack an archive into `dest` (which must exist).
fn unpack(path: &Path, ext: &str, dest: &Path) -> Result<()> {
    match ext {
        "zip" => {
            let mut zip = zip::ZipArchive::new(BufReader::new(File::open(path)?))?;
            zip.extract(dest)?;
        }
        "tar" => tar::Archive::new(BufReader::new(File::open(path)?)).unpack(dest)?,
        "tar.gz" => tar::Archive::new(GzDecoder::new(BufReader::new(File::open(path)?))).unpack(dest)?,
        "gz" => {
            let mut out = File::create(dest.join(stem(path)))?;
            std::io::copy(&mut GzDecoder::new(BufReader::new(File::open(path)?)), &mut out)?;
        }
        "tar.xz" | "tar.bz2" => run(Command::new("tar").arg("-xf").arg(path).arg("-C").arg(dest))?,
        "rar" | "7z" => {
            let seven = crate::bins::require("7zz")?;
            run(Command::new(seven).arg("x").arg("-y").arg(format!("-o{}", dest.display())).arg(path))?;
        }
        other => bail!("can't open .{other} archives"),
    }
    Ok(())
}

pub fn extract(path: &Path) -> Result<PathBuf> {
    let ext = crate::catalog::canonical_ext(&crate::catalog::ext_of(path)).to_string();
    let tmp = TempDir::new()?;
    unpack(path, &ext, tmp.path())?;

    let entries: Vec<PathBuf> = std::fs::read_dir(tmp.path())?.filter_map(|e| e.ok()).map(|e| e.path()).collect();
    // An archive holding one folder (or one file) doesn't need another folder around it.
    let (source, name_from) = match entries.as_slice() {
        [single] => (single.clone(), single.clone()),
        [] => bail!("the archive is empty"),
        _ => (tmp.path().to_path_buf(), path.to_path_buf()),
    };
    let out = if source == tmp.path() {
        output_path(path, "", None)
    } else {
        let name = name_from.file_name().unwrap().to_string_lossy().to_string();
        let beside = output_path(path, "", None).with_file_name(&name);
        unique(beside)
    };
    move_tree(&source, &out)?;
    Ok(out)
}

fn move_tree(from: &Path, to: &Path) -> Result<()> {
    if std::fs::rename(from, to).is_ok() {
        return Ok(());
    }
    copy_tree(from, to)?;
    Ok(())
}

fn copy_tree(from: &Path, to: &Path) -> Result<()> {
    if from.is_dir() {
        std::fs::create_dir_all(to)?;
        for entry in std::fs::read_dir(from)? {
            let entry = entry?;
            copy_tree(&entry.path(), &to.join(entry.file_name()))?;
        }
    } else {
        std::fs::copy(from, to)?;
    }
    Ok(())
}

/// Every file under `root`, with its path relative to `base`.
fn walk(root: &Path, base: &Path, out: &mut Vec<(PathBuf, String)>) -> Result<()> {
    let rel = root.strip_prefix(base).unwrap_or(root).to_string_lossy().replace('\\', "/");
    if root.is_dir() {
        if !rel.is_empty() {
            out.push((root.to_path_buf(), format!("{rel}/")));
        }
        let mut entries: Vec<_> = std::fs::read_dir(root)?.filter_map(|e| e.ok()).map(|e| e.path()).collect();
        entries.sort();
        for e in entries {
            walk(&e, base, out)?;
        }
    } else {
        out.push((root.to_path_buf(), rel));
    }
    Ok(())
}

fn write_zip(items: &[PathBuf], base: &Path, out: &Path) -> Result<()> {
    let mut zip = zip::ZipWriter::new(BufWriter::new(File::create(out)?));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o644);
    let mut files = Vec::new();
    for item in items {
        walk(item, base, &mut files)?;
    }
    for (path, name) in files {
        if name.ends_with('/') {
            zip.add_directory(name.trim_end_matches('/'), options.unix_permissions(0o755))?;
        } else {
            zip.start_file(name, options)?;
            std::io::copy(&mut File::open(&path)?, &mut zip)?;
        }
    }
    zip.finish()?;
    Ok(())
}

fn write_tar<W: std::io::Write>(items: &[PathBuf], base: &Path, w: W) -> Result<W> {
    let mut tar = tar::Builder::new(w);
    tar.follow_symlinks(false);
    for item in items {
        let name = item.strip_prefix(base).unwrap_or(item);
        if item.is_dir() {
            tar.append_dir_all(name, item)?;
        } else {
            tar.append_path_with_name(item, name)?;
        }
    }
    Ok(tar.into_inner()?)
}

fn pack(items: &[PathBuf], base: &Path, target: &str, out: &Path) -> Result<()> {
    match target {
        "zip" => write_zip(items, base, out),
        "tar" => {
            write_tar(items, base, BufWriter::new(File::create(out)?))?;
            Ok(())
        }
        "tar.gz" => {
            let gz = GzEncoder::new(BufWriter::new(File::create(out)?), flate2::Compression::default());
            write_tar(items, base, gz)?.finish()?;
            Ok(())
        }
        other => bail!("can't create .{other} archives"),
    }
}

pub fn convert(path: &Path, ext: &str, target: &str) -> Result<Vec<PathBuf>> {
    let tmp = TempDir::new()?;
    unpack(path, ext, tmp.path())?;
    let mut items: Vec<PathBuf> = std::fs::read_dir(tmp.path())?.filter_map(|e| e.ok()).map(|e| e.path()).collect();
    items.sort();
    let out = output_path(path, target, None);
    pack(&items, tmp.path(), target, &out)?;
    Ok(vec![out])
}

pub fn zip_paths(paths: &[PathBuf]) -> Result<PathBuf> {
    let first = paths.first().context("nothing to compress")?;
    let base = first.parent().map(Path::to_path_buf).unwrap_or_default();
    let out = if paths.len() == 1 {
        output_path(first, "zip", None)
    } else {
        unique(output_path(first, "zip", None).with_file_name("Archive.zip"))
    };
    write_zip(paths, &base, &out)?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zips_and_extracts_round_trip() {
        let tmp = TempDir::new().unwrap();
        let folder = tmp.join("photos");
        std::fs::create_dir_all(folder.join("inner")).unwrap();
        std::fs::write(folder.join("a.txt"), "hello").unwrap();
        std::fs::write(folder.join("inner/b.txt"), "world").unwrap();

        let zip = tmp.join("photos.zip");
        write_zip(&[folder.clone()], tmp.path(), &zip).unwrap();
        let back = tmp.join("back");
        std::fs::create_dir_all(&back).unwrap();
        unpack(&zip, "zip", &back).unwrap();
        assert_eq!(std::fs::read_to_string(back.join("photos/inner/b.txt")).unwrap(), "world");

        let tgz = tmp.join("photos.tar.gz");
        pack(&[folder], tmp.path(), "tar.gz", &tgz).unwrap();
        let back2 = tmp.join("back2");
        std::fs::create_dir_all(&back2).unwrap();
        unpack(&tgz, "tar.gz", &back2).unwrap();
        assert_eq!(std::fs::read_to_string(back2.join("photos/a.txt")).unwrap(), "hello");
    }
}
