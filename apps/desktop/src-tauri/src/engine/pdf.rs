//! PDFs: PDFium reads and renders pages, lopdf edits the file structure.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::{bail, Context as _, Result};
use lopdf::{dictionary, Dictionary, Document, Object, ObjectId, Stream};
use pdfium_render::prelude::*;

use super::{output_path, stem, Ctx, TempDir};

/// PDFium is not safe to use from two threads at once.
static PDFIUM_LOCK: Mutex<()> = Mutex::new(());

fn with_pdfium<T>(f: impl FnOnce(&Pdfium) -> Result<T>) -> Result<T> {
    let _guard = PDFIUM_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let bindings = match crate::bins::pdfium_library() {
        Some(lib) => Pdfium::bind_to_library(lib.to_string_lossy().as_ref()),
        None => Pdfium::bind_to_system_library(),
    }
    .map_err(|_| anyhow::anyhow!("PDFium is missing. It's needed to read PDF pages. Run scripts/fetch-binaries.sh."))?;
    let pdfium = Pdfium::new(bindings);
    f(&pdfium)
}

fn open<'a>(pdfium: &'a Pdfium, path: &Path) -> Result<PdfDocument<'a>> {
    pdfium
        .load_pdf_from_file(path, None)
        .map_err(|e| anyhow::anyhow!("could not open the PDF: {e}"))
}

fn render(page: &PdfPage, dpi: f32) -> Result<::image::DynamicImage> {
    // Keep huge pages (posters, maps) within a sane bitmap size.
    let scale = dpi / 72.0;
    let longest = page.width().value.max(page.height().value) * scale;
    let scale = if longest > 12000.0 { scale * 12000.0 / longest } else { scale };
    let config = PdfRenderConfig::new().scale_page_by_factor(scale);
    let bitmap = page
        .render_with_config(&config)
        .map_err(|e| anyhow::anyhow!("could not render a page: {e}"))?;
    Ok(bitmap.as_image())
}

pub fn page_count(path: &Path) -> Result<usize> {
    with_pdfium(|p| Ok(open(p, path)?.pages().len() as usize))
}

/// A PNG of one page, `width` pixels wide, for the page editors.
pub fn render_page_png(path: &Path, index: usize, width: u32) -> Result<Vec<u8>> {
    with_pdfium(|p| {
        let doc = open(p, path)?;
        let page = doc.pages().get(index as u16).map_err(|e| anyhow::anyhow!("no page {index}: {e}"))?;
        let config = PdfRenderConfig::new().set_target_width(width as i32);
        let img = page
            .render_with_config(&config)
            .map_err(|e| anyhow::anyhow!("could not render a page: {e}"))?
            .as_image();
        let mut out = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut out), ::image::ImageFormat::Png)?;
        Ok(out)
    })
}

fn page_texts(path: &Path) -> Result<Vec<String>> {
    with_pdfium(|p| {
        let doc = open(p, path)?;
        let mut texts = Vec::new();
        for page in doc.pages().iter() {
            let text = page.text().map(|t| t.all()).unwrap_or_default();
            texts.push(text.replace('\r', ""));
        }
        Ok(texts)
    })
}

/// Pages rendered to images: one file, or a folder when there are several.
/// `name_from` decides where results go and what they're called.
pub fn render_pages(path: &Path, name_from: &Path, ext: &str, dpi: u32, quality: u8, progress: &(dyn Fn(f32) + Sync)) -> Result<Vec<PathBuf>> {
    with_pdfium(|p| {
        let doc = open(p, path)?;
        let count = doc.pages().len() as usize;
        if count == 0 {
            bail!("the PDF has no pages");
        }
        let (dir, single) = if count == 1 {
            (None, output_path(name_from, ext, None))
        } else {
            let dir = output_path(name_from, "", Some("pages"));
            std::fs::create_dir_all(&dir)?;
            (Some(dir), PathBuf::new())
        };
        let digits = count.to_string().len().max(2);
        for (i, page) in doc.pages().iter().enumerate() {
            let img = render(&page, dpi as f32)?;
            let target = match &dir {
                Some(d) => d.join(format!("{}-{:0digits$}.{ext}", stem(name_from), i + 1)),
                None => single.clone(),
            };
            save_image(&img, &target, ext, quality)?;
            progress((i + 1) as f32 / count as f32);
        }
        Ok(vec![dir.unwrap_or(single)])
    })
}

fn save_image(img: &::image::DynamicImage, target: &Path, ext: &str, quality: u8) -> Result<()> {
    if ext == "jpg" {
        let rgb = flatten_white(img);
        let file = std::fs::File::create(target)?;
        let mut enc = ::image::codecs::jpeg::JpegEncoder::new_with_quality(std::io::BufWriter::new(file), quality);
        enc.encode_image(&rgb)?;
    } else {
        img.save_with_format(target, ::image::ImageFormat::Png)?;
    }
    Ok(())
}

fn flatten_white(img: &::image::DynamicImage) -> ::image::RgbImage {
    let rgba = img.to_rgba8();
    let mut rgb = ::image::RgbImage::new(rgba.width(), rgba.height());
    for (x, y, px) in rgba.enumerate_pixels() {
        let a = px[3] as u32;
        let mix = |c: u8| ((c as u32 * a + 255 * (255 - a)) / 255) as u8;
        rgb.put_pixel(x, y, ::image::Rgb([mix(px[0]), mix(px[1]), mix(px[2])]));
    }
    rgb
}

pub fn convert(path: &Path, target: &str, ctx: &Ctx) -> Result<Vec<PathBuf>> {
    match target {
        "jpg" | "png" => render_pages(path, path, target, ctx.settings.pdf_dpi, ctx.settings.image_quality, ctx.progress),
        "txt" => {
            let out = output_path(path, "txt", None);
            std::fs::write(&out, page_texts(path)?.join("\n\n"))?;
            Ok(vec![out])
        }
        "md" => {
            let out = output_path(path, "md", None);
            std::fs::write(&out, to_markdown(&page_texts(path)?))?;
            Ok(vec![out])
        }
        "epub" => {
            let tmp = TempDir::new()?;
            let md = tmp.join(format!("{}.md", stem(path)));
            std::fs::write(&md, to_markdown(&page_texts(path)?))?;
            let out = output_path(path, "epub", None);
            super::docs::pandoc(&md, &out, &["--metadata", &format!("title={}", stem(path))])?;
            Ok(vec![out])
        }
        "docx" => {
            if crate::bins::find("soffice").is_some() {
                Ok(vec![super::docs::soffice_to(path, path, "docx", "docx:MS Word 2007 XML", Some("writer_pdf_import"))?])
            } else {
                let tmp = TempDir::new()?;
                let md = tmp.join("text.md");
                std::fs::write(&md, to_markdown(&page_texts(path)?))?;
                let out = output_path(path, "docx", None);
                super::docs::pandoc(&md, &out, &[])?;
                Ok(vec![out])
            }
        }
        "pptx" => Ok(vec![super::docs::soffice_to(path, path, "pptx", "pptx:Impress MS PowerPoint 2007 XML", Some("impress_pdf_import"))?]),
        "xlsx" => Ok(vec![to_xlsx(path)?]),
        other => bail!("PDF to {other} isn't supported"),
    }
}

/// Plain page text, lightly shaped into Markdown paragraphs.
fn to_markdown(pages: &[String]) -> String {
    let mut out = String::new();
    for (i, page) in pages.iter().enumerate() {
        if i > 0 {
            out.push_str("\n\n---\n\n");
        }
        let mut para = String::new();
        for line in page.lines() {
            let line = line.trim();
            if line.is_empty() {
                if !para.is_empty() {
                    out.push_str(para.trim());
                    out.push_str("\n\n");
                    para.clear();
                }
                continue;
            }
            // A hyphen at the end of a line joins the word with the next line.
            if let Some(stripped) = line.strip_suffix('-') {
                para.push_str(stripped);
            } else {
                para.push_str(line);
                para.push(' ');
            }
        }
        if !para.is_empty() {
            out.push_str(para.trim());
            out.push('\n');
        }
    }
    out
}

/// One worksheet per page; cells are split on runs of two or more spaces or tabs.
fn to_xlsx(path: &Path) -> Result<PathBuf> {
    let pages = page_texts(path)?;
    let mut book = rust_xlsxwriter::Workbook::new();
    for (i, text) in pages.iter().enumerate() {
        let sheet = book.add_worksheet();
        sheet.set_name(format!("Page {}", i + 1))?;
        for (r, line) in text.lines().enumerate() {
            for (c, cell) in split_cells(line).iter().enumerate() {
                match cell.replace(',', "").parse::<f64>() {
                    Ok(n) if !cell.is_empty() => sheet.write_number(r as u32, c as u16, n)?,
                    _ => sheet.write_string(r as u32, c as u16, cell.as_str())?,
                };
            }
        }
    }
    let out = output_path(path, "xlsx", None);
    book.save(&out)?;
    Ok(out)
}

fn split_cells(line: &str) -> Vec<String> {
    let mut cells = Vec::new();
    let mut cur = String::new();
    let mut spaces = 0;
    for ch in line.chars() {
        if ch == '\t' {
            spaces = 2;
            continue;
        }
        if ch == ' ' {
            spaces += 1;
            continue;
        }
        if spaces >= 2 && !cur.is_empty() {
            cells.push(std::mem::take(&mut cur));
        } else if spaces == 1 && !cur.is_empty() {
            cur.push(' ');
        }
        spaces = 0;
        cur.push(ch);
    }
    if !cur.is_empty() {
        cells.push(cur);
    }
    cells
}

// ---------------------------------------------------------------- lopdf edits

fn load(path: &Path) -> Result<Document> {
    let doc = Document::load(path).with_context(|| format!("could not read {}", path.display()))?;
    if doc.is_encrypted() {
        bail!("this PDF is password-protected");
    }
    Ok(doc)
}

fn save(doc: &mut Document, out: &Path) -> Result<()> {
    doc.prune_objects();
    doc.renumber_objects();
    doc.compress();
    doc.save(out).with_context(|| format!("could not write {}", out.display()))?;
    Ok(())
}

const INHERITABLE: [&[u8]; 4] = [b"MediaBox", b"CropBox", b"Resources", b"Rotate"];

/// Copy attributes a page inherits from its parents onto the page itself, so the
/// page can be moved to a different parent safely.
fn pin_inherited(doc: &mut Document, page: ObjectId) {
    let mut found: Vec<(Vec<u8>, Object)> = Vec::new();
    let mut parent = doc
        .get_dictionary(page)
        .ok()
        .and_then(|d| d.get(b"Parent").ok())
        .and_then(|p| p.as_reference().ok());
    let mut guard = 0;
    while let Some(pid) = parent {
        guard += 1;
        if guard > 64 {
            break;
        }
        let Ok(dict) = doc.get_dictionary(pid) else { break };
        for key in INHERITABLE {
            if !found.iter().any(|(k, _)| k == key) {
                if let Ok(v) = dict.get(key) {
                    found.push((key.to_vec(), v.clone()));
                }
            }
        }
        parent = dict.get(b"Parent").ok().and_then(|p| p.as_reference().ok());
    }
    if let Ok(dict) = doc.get_dictionary_mut(page) {
        for (key, value) in found {
            if !dict.has(&key) {
                dict.set(key, value);
            }
        }
    }
}

fn rotation(doc: &Document, page: ObjectId) -> i64 {
    doc.get_dictionary(page)
        .ok()
        .and_then(|d| d.get(b"Rotate").ok())
        .and_then(|r| r.as_i64().ok())
        .unwrap_or(0)
}

fn root_pages(doc: &Document) -> Result<ObjectId> {
    Ok(doc.catalog()?.get(b"Pages")?.as_reference()?)
}

/// Replace the page tree with a flat list of pages, in this order.
fn set_pages(doc: &mut Document, pages: &[ObjectId]) -> Result<()> {
    let root = root_pages(doc)?;
    for id in pages {
        if let Ok(d) = doc.get_dictionary_mut(*id) {
            d.set("Parent", root);
        }
    }
    let dict = doc.get_dictionary_mut(root)?;
    dict.set("Kids", pages.iter().map(|id| Object::Reference(*id)).collect::<Vec<_>>());
    dict.set("Count", pages.len() as i64);
    for key in INHERITABLE {
        dict.remove(key);
    }
    Ok(())
}

pub fn rotate_all(path: &Path) -> Result<PathBuf> {
    let mut doc = load(path)?;
    let pages: Vec<ObjectId> = doc.get_pages().into_values().collect();
    for id in &pages {
        pin_inherited(&mut doc, *id);
        let r = (rotation(&doc, *id) + 90).rem_euclid(360);
        doc.get_dictionary_mut(*id)?.set("Rotate", r);
    }
    let out = output_path(path, "pdf", Some("rotated"));
    save(&mut doc, &out)?;
    Ok(out)
}

pub fn organize(path: &Path, ctx: &Ctx) -> Result<PathBuf> {
    let order = ctx.params.get("pages").and_then(|p| p.as_array()).context("no page order given")?;
    let mut doc = load(path)?;
    let all: Vec<ObjectId> = doc.get_pages().into_values().collect();
    let mut chosen = Vec::new();
    for entry in order {
        let index = entry.get("index").and_then(|v| v.as_u64()).context("bad page entry")? as usize;
        let turn = entry.get("rotate").and_then(|v| v.as_i64()).unwrap_or(0);
        let id = *all.get(index).with_context(|| format!("no page {}", index + 1))?;
        if chosen.contains(&id) {
            bail!("page {} appears twice", index + 1);
        }
        pin_inherited(&mut doc, id);
        if turn != 0 {
            let r = (rotation(&doc, id) + turn).rem_euclid(360);
            doc.get_dictionary_mut(id)?.set("Rotate", r);
        }
        chosen.push(id);
    }
    if chosen.is_empty() {
        bail!("every page was removed");
    }
    set_pages(&mut doc, &chosen)?;
    let out = output_path(path, "pdf", Some("organized"));
    save(&mut doc, &out)?;
    Ok(out)
}

pub fn split(path: &Path) -> Result<PathBuf> {
    let doc = load(path)?;
    let pages: Vec<ObjectId> = doc.get_pages().into_values().collect();
    if pages.len() < 2 {
        bail!("the PDF has only one page");
    }
    let dir = output_path(path, "", Some("pages"));
    std::fs::create_dir_all(&dir)?;
    let digits = pages.len().to_string().len().max(2);
    for (i, page) in pages.iter().enumerate() {
        let mut single = doc.clone();
        pin_inherited(&mut single, *page);
        set_pages(&mut single, &[*page])?;
        let out = dir.join(format!("{}-{:0digits$}.pdf", stem(path), i + 1));
        save(&mut single, &out)?;
    }
    Ok(dir)
}

pub fn merge(paths: &[PathBuf]) -> Result<PathBuf> {
    let first = paths.first().context("no PDFs")?;
    let mut max_id = 1;
    let mut objects: BTreeMap<ObjectId, Object> = BTreeMap::new();
    let mut page_ids = Vec::new();
    for path in paths {
        let mut doc = load(path)?;
        doc.renumber_objects_with(max_id);
        max_id = doc.max_id + 1;
        let pages: Vec<ObjectId> = doc.get_pages().into_values().collect();
        for id in &pages {
            pin_inherited(&mut doc, *id);
        }
        page_ids.extend(pages);
        objects.extend(doc.objects);
    }
    let pages_id = (max_id, 0);
    let catalog_id = (max_id + 1, 0);
    for id in &page_ids {
        if let Some(Object::Dictionary(d)) = objects.get_mut(id) {
            d.set("Parent", pages_id);
        }
    }
    let mut doc = Document::with_version("1.7");
    doc.objects = objects;
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => page_ids.iter().map(|id| Object::Reference(*id)).collect::<Vec<_>>(),
            "Count" => page_ids.len() as i64,
        }),
    );
    doc.objects.insert(
        catalog_id,
        Object::Dictionary(dictionary! { "Type" => "Catalog", "Pages" => pages_id }),
    );
    doc.max_id = max_id + 1;
    doc.trailer.set("Root", catalog_id);
    let out = output_path(first, "pdf", Some("merged"));
    save(&mut doc, &out)?;
    Ok(out)
}

pub fn strip_metadata(path: &Path) -> Result<PathBuf> {
    let mut doc = load(path)?;
    doc.trailer.remove(b"Info");
    if let Ok(cat) = doc.catalog_mut() {
        cat.remove(b"Metadata");
        cat.remove(b"PieceInfo");
    }
    let out = output_path(path, "pdf", Some("clean"));
    save(&mut doc, &out)?;
    Ok(out)
}

fn name_of(obj: Option<&Object>) -> Option<&[u8]> {
    match obj {
        Some(Object::Name(n)) => Some(n.as_slice()),
        Some(Object::Array(a)) if a.len() == 1 => a[0].as_name().ok(),
        _ => None,
    }
}

/// Re-encode large embedded photos as smaller JPEGs; text and vectors stay intact.
pub fn compress(path: &Path) -> Result<PathBuf> {
    let mut doc = load(path)?;
    let ids: Vec<ObjectId> = doc.objects.keys().cloned().collect();
    for id in ids {
        let Some(Object::Stream(stream)) = doc.objects.get(&id) else { continue };
        if let Some(new) = shrink_image(stream) {
            doc.objects.insert(id, Object::Stream(new));
        }
    }
    let out = output_path(path, "pdf", Some("compressed"));
    save(&mut doc, &out)?;
    let before = std::fs::metadata(path)?.len();
    let after = std::fs::metadata(&out)?.len();
    if after >= before {
        let _ = std::fs::remove_file(&out);
        bail!("this PDF is already as small as it gets");
    }
    Ok(out)
}

fn shrink_image(stream: &Stream) -> Option<Stream> {
    let dict = &stream.dict;
    if dict.get(b"Subtype").ok()?.as_name().ok()? != b"Image" {
        return None;
    }
    if dict.get(b"ImageMask").and_then(|m| m.as_bool()).unwrap_or(false) {
        return None;
    }
    if stream.content.len() < 64 * 1024 {
        return None;
    }
    let filter = name_of(dict.get(b"Filter").ok());
    let width = dict.get(b"Width").ok()?.as_i64().ok()? as u32;
    let height = dict.get(b"Height").ok()?.as_i64().ok()? as u32;
    let img = match filter {
        Some(b"DCTDecode") => ::image::load_from_memory_with_format(&stream.content, ::image::ImageFormat::Jpeg).ok()?,
        Some(b"FlateDecode") => {
            if dict.get(b"BitsPerComponent").ok()?.as_i64().ok()? != 8 {
                return None;
            }
            let raw = stream.decompressed_content().ok()?;
            match dict.get(b"ColorSpace").ok()?.as_name().ok()? {
                b"DeviceRGB" => ::image::DynamicImage::ImageRgb8(::image::RgbImage::from_raw(width, height, raw)?),
                b"DeviceGray" => ::image::DynamicImage::ImageLuma8(::image::GrayImage::from_raw(width, height, raw)?),
                _ => return None,
            }
        }
        _ => return None,
    };
    let longest = img.width().max(img.height());
    let img = if longest > 2000 {
        img.resize(2000, 2000, ::image::imageops::FilterType::Lanczos3)
    } else {
        img
    };
    let gray = matches!(img, ::image::DynamicImage::ImageLuma8(_));
    let mut bytes = Vec::new();
    let mut enc = ::image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, 65);
    if gray {
        enc.encode_image(&img.to_luma8()).ok()?;
    } else {
        enc.encode_image(&img.to_rgb8()).ok()?;
    }
    // Only worth it for a real saving; Flate images may be line art that JPEG would blur.
    let worth = if filter == Some(b"DCTDecode") { 0.85 } else { 0.5 };
    if bytes.len() as f64 > stream.content.len() as f64 * worth {
        return None;
    }
    let mut new_dict = Dictionary::new();
    for (k, v) in dict.iter() {
        if !matches!(k.as_slice(), b"Filter" | b"DecodeParms" | b"Length" | b"Width" | b"Height" | b"ColorSpace" | b"Decode" | b"BitsPerComponent") {
            new_dict.set(k.clone(), v.clone());
        }
    }
    new_dict.set("Filter", "DCTDecode");
    new_dict.set("Width", img.width() as i64);
    new_dict.set("Height", img.height() as i64);
    new_dict.set("BitsPerComponent", 8);
    new_dict.set("ColorSpace", if gray { "DeviceGray" } else { "DeviceRGB" });
    let mut new = Stream::new(new_dict, bytes);
    new.allows_compression = false;
    Some(new)
}

/// Black out areas for good: affected pages become images with the boxes burnt in.
pub fn redact(path: &Path, ctx: &Ctx) -> Result<PathBuf> {
    let boxes = ctx.params.get("boxes").and_then(|b| b.as_array()).context("nothing marked to redact")?;
    let mut by_page: BTreeMap<usize, Vec<[f32; 4]>> = BTreeMap::new();
    for b in boxes {
        let num = |k: &str| b.get(k).and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
        let page = b.get("page").and_then(|v| v.as_u64()).context("bad redaction box")? as usize;
        by_page.entry(page).or_default().push([num("x"), num("y"), num("w"), num("h")]);
    }
    if by_page.is_empty() {
        bail!("nothing marked to redact");
    }

    let dpi = 200.0;
    let rendered = with_pdfium(|p| {
        let doc = open(p, path)?;
        let mut out = Vec::new();
        for (index, rects) in &by_page {
            let page = doc.pages().get(*index as u16).map_err(|e| anyhow::anyhow!("no page {}: {e}", index + 1))?;
            let mut img = flatten_white(&render(&page, dpi)?);
            let (w, h) = (img.width() as f32, img.height() as f32);
            for [x, y, bw, bh] in rects {
                let x0 = (x * w).clamp(0.0, w) as u32;
                let y0 = (y * h).clamp(0.0, h) as u32;
                let x1 = ((x + bw) * w).clamp(0.0, w).ceil() as u32;
                let y1 = ((y + bh) * h).clamp(0.0, h).ceil() as u32;
                for yy in y0..y1.min(img.height()) {
                    for xx in x0..x1.min(img.width()) {
                        img.put_pixel(xx, yy, ::image::Rgb([0, 0, 0]));
                    }
                }
            }
            let mut jpeg = Vec::new();
            ::image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 90).encode_image(&img)?;
            out.push((*index, img.width(), img.height(), jpeg));
        }
        Ok(out)
    })?;

    let mut doc = load(path)?;
    let pages: Vec<ObjectId> = doc.get_pages().into_values().collect();
    for (index, px_w, px_h, jpeg) in rendered {
        let page_id = *pages.get(index).context("page out of range")?;
        let (w, h) = (px_w as f32 * 72.0 / dpi, px_h as f32 * 72.0 / dpi);
        let mut image = Stream::new(
            dictionary! {
                "Type" => "XObject",
                "Subtype" => "Image",
                "Width" => px_w as i64,
                "Height" => px_h as i64,
                "ColorSpace" => "DeviceRGB",
                "BitsPerComponent" => 8,
                "Filter" => "DCTDecode",
            },
            jpeg,
        );
        image.allows_compression = false;
        let image_id = doc.add_object(image);
        let content = format!("q {w:.2} 0 0 {h:.2} 0 0 cm /Redacted Do Q");
        let content_id = doc.add_object(Stream::new(Dictionary::new(), content.into_bytes()));
        let page = doc.get_dictionary_mut(page_id)?;
        page.set("MediaBox", vec![0.into(), 0.into(), Object::Real(w), Object::Real(h)]);
        page.remove(b"CropBox");
        page.remove(b"Annots");
        page.set("Rotate", 0);
        page.set("Resources", dictionary! { "XObject" => dictionary! { "Redacted" => image_id } });
        page.set("Contents", content_id);
    }
    doc.trailer.remove(b"Info");
    let out = output_path(path, "pdf", Some("redacted"));
    save(&mut doc, &out)?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_cells_on_wide_gaps() {
        assert_eq!(split_cells("Name  Price\tQty"), vec!["Name", "Price", "Qty"]);
        assert_eq!(split_cells("Big apple   1,200"), vec!["Big apple", "1,200"]);
    }

    #[test]
    fn joins_hyphenated_lines() {
        let md = to_markdown(&["A sen-\ntence here.\n\nNext".to_string()]);
        assert_eq!(md, "A sentence here.\n\nNext\n");
    }
}
