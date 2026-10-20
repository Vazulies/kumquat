//! End-to-end check of every conversion and tool against generated sample files.
//!
//! Needs the conversion programs (bundled or installed), so it's ignored by default:
//!   cargo test smoke -- --ignored --nocapture

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{json, Value};

use crate::catalog;
use crate::engine::{self, Ctx, Outcome, TempDir};
use crate::settings::Settings;

fn sh(cmd: &mut Command) {
    let out = cmd.output().expect("run sample generator");
    assert!(out.status.success(), "{:?} failed: {}", cmd, String::from_utf8_lossy(&out.stderr));
}

fn bin(name: &str) -> PathBuf {
    crate::bins::find(name).unwrap_or_else(|| panic!("{name} not found"))
}

fn samples(dir: &Path) -> Vec<PathBuf> {
    let ffmpeg = bin("ffmpeg");
    let magick = bin("magick");
    let pandoc = bin("pandoc");
    let soffice = bin("soffice");
    let p = |n: &str| dir.join(n);

    sh(Command::new(&magick).args(["-size", "640x400", "gradient:#ff8a1f-#2b2f3a", "-fill", "white", "-pointsize", "48", "-annotate", "+40+200", "Kumquat"]).arg(p("photo.jpg")));
    sh(Command::new(&magick).args(["-size", "300x300", "xc:none", "-fill", "#ff8a1f", "-draw", "circle 150,150 150,40"]).arg(p("logo.png")));
    sh(Command::new(&magick).args(["-size", "200x200", "xc:skyblue"]).arg(p("pic.heic")));
    std::fs::write(p("icon.svg"), r#"<svg xmlns="http://www.w3.org/2000/svg" width="64" height="64"><circle cx="32" cy="32" r="28" fill="orange"/></svg>"#).unwrap();
    sh(Command::new(&ffmpeg).args(["-y", "-loglevel", "error", "-f", "lavfi", "-i", "testsrc=duration=2:size=320x240:rate=10", "-vf", "fps=10"]).arg(p("anim.gif")));

    sh(Command::new(&ffmpeg).args(["-y", "-loglevel", "error", "-f", "lavfi", "-i", "testsrc=duration=3:size=640x360:rate=25", "-f", "lavfi", "-i", "sine=frequency=440:duration=3", "-c:v", "libx264", "-pix_fmt", "yuv420p", "-c:a", "aac", "-shortest"]).arg(p("clip.mp4")));
    sh(Command::new(&ffmpeg).args(["-y", "-loglevel", "error", "-f", "lavfi", "-i", "sine=frequency=330:duration=4", "-f", "lavfi", "-i", "anoisesrc=d=4:a=0.02", "-filter_complex", "amix=inputs=2"]).arg(p("voice.wav")));

    std::fs::write(p("notes.md"), "# Notes\n\nSome **bold** text and a list:\n\n- one\n- two\n\n| Item | Price |\n|---|---|\n| Tea | 3.50 |\n").unwrap();
    std::fs::write(p("plain.txt"), "Hello from a plain text file.\nSecond line.\n").unwrap();
    sh(Command::new(&pandoc).arg(p("notes.md")).arg("-o").arg(p("report.docx")));
    sh(Command::new(&pandoc).arg(p("notes.md")).arg("-o").arg(p("deck.pptx")));
    let lo_profile = format!("-env:UserInstallation=file://{}", dir.join("lo").display());
    sh(Command::new(&soffice).arg(&lo_profile).args(["--headless", "--convert-to", "pdf", "--outdir"]).arg(dir).arg(p("report.docx")));
    // A second page so page tools have something to work with.
    sh(Command::new(&magick).arg(p("photo.jpg")).arg(p("photo-page.pdf")));
    engine::pdf::merge(&[p("report.pdf"), p("photo-page.pdf")]).map(|m| std::fs::rename(m, p("two-pages.pdf")).unwrap()).unwrap();

    std::fs::write(p("prices.csv"), "item,price\n\"Tea, green\",3.5\nCoffee,4\n").unwrap();
    sh(Command::new(&soffice).arg(&lo_profile).args(["--headless", "--infilter=CSV:44,34,76", "--convert-to", "xlsx", "--outdir"]).arg(dir).arg(p("prices.csv")));

    std::fs::write(p("film.srt"), "1\n00:00:01,000 --> 00:00:02,500\nHello\n\n2\n00:00:03,000 --> 00:00:04,000\n<i>World</i>\n").unwrap();

    std::fs::create_dir_all(p("folder/sub")).unwrap();
    std::fs::write(p("folder/a.txt"), "a").unwrap();
    std::fs::write(p("folder/sub/b.txt"), "b").unwrap();
    let zip = engine::archive::zip_paths(&[p("folder")]).unwrap();
    std::fs::rename(zip, p("bundle.zip")).unwrap();
    if let Some(seven) = crate::bins::find("7zz") {
        sh(Command::new(seven).args(["a", "-bso0"]).arg(p("bundle.7z")).arg(p("folder")));
    }

    let mut v = vec![
        "photo.jpg", "logo.png", "pic.heic", "icon.svg", "anim.gif", "clip.mp4", "voice.wav", "notes.md", "plain.txt",
        "report.docx", "deck.pptx", "two-pages.pdf", "prices.csv", "prices.xlsx", "film.srt", "bundle.zip",
    ];
    if dir.join("bundle.7z").exists() {
        v.push("bundle.7z");
    }
    v.into_iter().map(p).collect()
}

/// Parameters a person would pick in each editor.
fn params_for(id: &str) -> Value {
    match id {
        "tool:image.resize" => json!({ "percent": 50 }),
        "tool:image.crop" => json!({ "x": 10, "y": 10, "width": 100, "height": 80 }),
        "tool:image.annotate" => {
            let png = std::fs::read(std::env::temp_dir().join("kumquat-overlay.png")).unwrap_or_default();
            use base64::Engine as _;
            json!({ "overlay": format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(png)) })
        }
        "tool:image.background" => json!({ "color": "#ffffff" }),
        "tool:video.speed" | "tool:audio.speed" => json!({ "factor": 2.0 }),
        "tool:video.trim" | "tool:audio.trim" => json!({ "start": 0.5, "end": 1.5 }),
        "tool:video.crop" => json!({ "x": 20, "y": 20, "width": 200, "height": 120 }),
        "tool:pdf.organize" => json!({ "pages": [{ "index": 1, "rotate": 90 }, { "index": 0, "rotate": 0 }] }),
        "tool:pdf.redact" => json!({ "boxes": [{ "page": 0, "x": 0.1, "y": 0.1, "w": 0.4, "h": 0.1 }] }),
        "tool:subtitle.shift" => json!({ "seconds": 1.5 }),
        _ => Value::Null,
    }
}

#[test]
#[ignore]
fn smoke() {
    let tmp = TempDir::new().unwrap();
    let dir = tmp.path();
    let files = samples(dir);

    // An overlay for the annotate tool, the size of photo.jpg.
    sh(Command::new(bin("magick")).args(["-size", "640x400", "xc:none", "-stroke", "red", "-strokewidth", "6", "-draw", "line 20,20 300,300"]).arg(std::env::temp_dir().join("kumquat-overlay.png")));

    let settings = Settings::default();
    let noop = |_: f32| {};
    let mut failures = Vec::new();
    let mut ran = 0;

    for file in &files {
        let paths = vec![file.to_string_lossy().into_owned()];
        let cat = catalog::for_paths(&paths);
        let items: Vec<_> = cat.convert.iter().chain(cat.tools.iter()).collect();
        for item in items {
            if item.id == "tool:image.qr" {
                continue;
            }
            let params = params_for(&item.id);
            let ctx = Ctx { settings: settings.clone(), params: &params, progress: &noop };
            let result = if let Some(target) = item.id.strip_prefix("convert:") {
                engine::convert(file, target, &ctx).map(Outcome::Files)
            } else {
                engine::tool(item.id.strip_prefix("tool:").unwrap(), std::slice::from_ref(file), &ctx)
            };
            ran += 1;
            let name = file.file_name().unwrap().to_string_lossy();
            match result {
                Ok(Outcome::Files(out)) => {
                    let ok = out.iter().all(|o| o.exists() && (o.is_dir() || std::fs::metadata(o).map(|m| m.len() > 0).unwrap_or(false)));
                    let shown: Vec<String> = out.iter().map(|o| o.file_name().unwrap().to_string_lossy().into_owned()).collect();
                    println!("{} {:<14} {:<22} → {}", if ok { "ok  " } else { "FAIL" }, name, item.id, shown.join(", "));
                    if !ok {
                        failures.push(format!("{name} {}: output missing or empty", item.id));
                    }
                }
                Ok(Outcome::Message { title, body }) => println!("ok   {name:<14} {:<22} → {title}: {body}", item.id),
                Err(e) => {
                    // "already as small as it gets" is a correct answer for tiny samples.
                    let benign = e.to_string().contains("already");
                    println!("{} {name:<14} {:<22} ✗ {e}", if benign { "skip" } else { "FAIL" }, item.id);
                    if !benign {
                        failures.push(format!("{name} {}: {e}", item.id));
                    }
                }
            }
        }
    }

    // Multi-file tools.
    let ctx = Ctx { settings: settings.clone(), params: &Value::Null, progress: &noop };
    for (tool, picks) in [
        ("image.combine", vec!["photo.jpg", "logo.png"]),
        ("pdf.merge", vec!["two-pages.pdf", "two-pages.pdf"]),
        ("any.zip", vec!["photo.jpg", "folder"]),
    ] {
        let paths: Vec<PathBuf> = picks.iter().map(|p| dir.join(p)).collect();
        ran += 1;
        match engine::tool(tool, &paths, &ctx) {
            Ok(_) => println!("ok   {:<14} {tool}", "(multi)"),
            Err(e) => {
                println!("FAIL {:<14} {tool} ✗ {e}", "(multi)");
                failures.push(format!("{tool}: {e}"));
            }
        }
    }

    // Spot-check a few results for real.
    let organized = dir.join("two-pages (organized).pdf");
    let doc = lopdf::Document::load(&organized).expect("organized pdf");
    assert_eq!(doc.get_pages().len(), 2);
    let shifted = std::fs::read_to_string(dir.join("film (shifted).srt")).unwrap();
    assert!(shifted.contains("00:00:02,500 --> 00:00:04,000"), "{shifted}");
    let trimmed = engine::av::probe(&dir.join("clip (trimmed).mp4")).unwrap();
    assert!((trimmed.duration.unwrap() - 1.0).abs() < 0.2, "trimmed to {:?}", trimmed.duration);
    let cropped = engine::av::probe(&dir.join("clip (cropped).mp4")).unwrap();
    assert_eq!((cropped.width, cropped.height), (Some(200), Some(120)));

    println!("\n{ran} runs, {} failures", failures.len());
    assert!(failures.is_empty(), "failures:\n{}", failures.join("\n"));
}
