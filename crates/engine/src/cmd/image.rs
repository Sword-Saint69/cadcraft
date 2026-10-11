//! Raster images and PDF import: IMAGEATTACH, IMAGEADJUST, IMAGECLIP, IMAGEFRAME,
//! IMAGEDETACH, PDFIMPORT and PDFATTACH, plus an ATTACH dispatcher.
//!
//! The engine stays I/O-agnostic: file bytes arrive via `path` (native builds read
//! the file) or base64 `data` + `name`. Pixel sizes come from minimal PNG/JPEG
//! header parsers here, so no image dependency is needed. PDF vector import goes
//! through the io hooks, which understand `.pdf`; IMAGE entities round-trip
//! through DXF/DWG as usual.

use std::sync::Arc;

use cadcraft_doc::{Common, Entity, EntityKind, Handle, Image};
use cadcraft_geom::{Vec2, Vec3};
use serde_json::{Value, json};

use super::file::{base64_decode, io};
use super::*;
use crate::{Result, Session};

/// Raster formats IMAGEATTACH accepts.
const RASTER_EXTS: &[&str] = &["png", "jpg", "jpeg"];
/// Most pixels per side accepted.
const MAX_SIDE_PX: u32 = 100_000;

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("imageattach", "Attach Raster Image...", run_imageattach)
            .menu(&["Insert", "Raster Image Reference..."])
            .alias(&["iat", "-imageattach"])
            .params("{path | data: base64, name?: \"x.png\"} (png/jpg/jpeg), at?: [x,y] (default 0,0), scale?: (default 1), width?: drawing units (default: pixel size x scale)} → {handle, size, path}"),
        CommandSpec::new("imageadjust", "Adjust Image", run_imageadjust)
            .menu(&["Modify", "Object", "Image", "Adjust..."])
            .alias(&["iad"])
            .params("{handle?: (default: selected image), brightness?: 0..100, contrast?: 0..100, fade?: 0..100} → {handle, brightness, contrast, fade}"),
        CommandSpec::new("imageclip", "Clip Image", run_imageclip)
            .menu(&["Modify", "Clip", "Image"])
            .alias(&["icl"])
            .params("{handle?:, boundary?: [[x,y],...] (drawing units, empty clears via delete), clip?: bool, delete?: true (remove boundary)} → {handle, clipping, vertices}"),
        CommandSpec::new("imageframe", "Image Frame", run_imageframe)
            .menu(&["Modify", "Object", "Image", "Frame"])
            .params("{mode?: 0|1|2 (default 1)} → {mode} (0 hide, 1 show, 2 show but not plotted)"),
        CommandSpec::new("imagetransparency", "Transparency", run_imagetransparency)
            .menu(&["Modify", "Object", "Image", "Transparency"])
            .alias(&["itr"])
            .params("{handle?:, on?: bool (default: toggle)} → {handle, transparency}"),
        CommandSpec::new("imagequality", "Quality", run_imagequality)
            .menu(&["Modify", "Object", "Image", "Quality"])
            .alias(&["iqu"])
            .params("{mode?: draft|high (default: show)} → {mode} (display quality of all images)"),
        CommandSpec::new("imagedetach", "Detach Image", run_imagedetach)
            .menu(&["Modify", "Object", "Image", "Detach"])
            .alias(&["-image"])
            .params("{handle? | name?: image definition name} (default: selected image)"),
        CommandSpec::new("pdfimport", "Import PDF...", run_pdfimport)
            .menu(&["File", "Import PDF..."])
            .alias(&["pdfimp"])
            .params("{path | data: base64, name?: \"x.pdf\"}, page?: 1-based (default: all pages), at?: [x,y] (default 0,0), scale?: (default 1)} → {imported, handles} (vector paths become LINEs on PDF_IMPORT; text and images are skipped)"),
        CommandSpec::new("pdfattach", "Attach PDF...", run_pdfattach)
            .menu(&["Insert", "PDF Underlay..."])
            .params("{path | data: base64, name?: \"x.pdf\"}, page?: (default 1), at?: [x,y], scale?:} → same as PDFIMPORT (PDFs import as vector linework; there is no separate underlay object yet)"),
        CommandSpec::new("attach", "Attach", run_attach)
            .menu(&["Insert", "Attach..."])
            .params("{path | data, name?} (dxf/dwg → IMPORT, pdf → PDFIMPORT, png/jpg/jpeg → IMAGEATTACH)"),
    ]
}

fn extension(path: &str) -> String {
    std::path::Path::new(path).extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default()
}

fn read_input(cmd: &str, p: &Value) -> Result<(Vec<u8>, String)> {
    if let Some(path) = str_param(p, "path") {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let bytes = std::fs::read(path).map_err(|e| bad(cmd, format!("{path}: {e}")))?;
            return Ok((bytes, path.to_string()));
        }
        #[cfg(target_arch = "wasm32")]
        {
            let _ = path;
            return Err(bad(cmd, "paths are not available on the web; pass `data`"));
        }
    }
    let data = str_param(p, "data").ok_or_else(|| bad(cmd, "`path` or `data` (base64) is required"))?;
    let bytes = base64_decode(data).ok_or_else(|| bad(cmd, "invalid base64"))?;
    Ok((bytes, str_param(p, "name").unwrap_or("image.png").to_string()))
}

fn at_scale(cmd: &str, p: &Value) -> Result<(Vec2, f64)> {
    let at = match p.get("at") {
        Some(v) => point_value(v).ok_or_else(|| bad(cmd, "`at` must be a point [x, y]"))?,
        None => Vec2::ZERO,
    };
    if !at.x.is_finite() || !at.y.is_finite() {
        return Err(bad(cmd, "`at` must be finite numbers"));
    }
    let scale = match p.get("scale") {
        Some(v) => {
            v.as_f64().filter(|k| k.is_finite() && (1e-9..=1e9).contains(&k.abs())).ok_or_else(|| bad(cmd, "`scale` must be a non-zero number"))?
        }
        None => 1.0,
    };
    Ok((at, scale))
}

/// PNG IHDR or JPEG SOF size (clean-room header parse; pixels never loaded).
fn raster_size(bytes: &[u8], ext: &str) -> Result<(u32, u32)> {
    match ext {
        "png" => {
            if bytes.len() < 24 || bytes[0..8] != [137, 80, 78, 71, 13, 10, 26, 10] || &bytes[12..16] != b"IHDR" {
                return Err(bad("imageattach", "not a PNG file (bad signature or IHDR)"));
            }
            let w = u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
            let h = u32::from_be_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);
            check_side(w, h)
        }
        "jpg" | "jpeg" => jpeg_size(bytes),
        e => Err(bad("imageattach", format!("`.{e}` is not a raster image (use png, jpg or jpeg)"))),
    }
}

fn check_side(w: u32, h: u32) -> Result<(u32, u32)> {
    if w == 0 || h == 0 || w > MAX_SIDE_PX || h > MAX_SIDE_PX {
        return Err(bad("imageattach", format!("image size {w}x{h} is not usable (1..={MAX_SIDE_PX} pixels per side)")));
    }
    Ok((w, h))
}

fn jpeg_size(bytes: &[u8]) -> Result<(u32, u32)> {
    if bytes.len() < 4 || bytes[0] != 0xFF || bytes[1] != 0xD8 {
        return Err(bad("imageattach", "not a JPEG file (bad SOI)"));
    }
    let mut i = 2usize;
    while i + 4 <= bytes.len() {
        if bytes[i] != 0xFF {
            i += 1;
            continue;
        }
        while i < bytes.len() && bytes[i] == 0xFF {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        let marker = bytes[i];
        i += 1;
        if marker == 0xD8 || marker == 0xD9 || (0xD0..=0xD7).contains(&marker) || marker == 0x01 {
            continue;
        }
        if i + 2 > bytes.len() {
            break;
        }
        let len = usize::from(u16::from_be_bytes([bytes[i], bytes[i + 1]]));
        if len < 2 || i + len > bytes.len() {
            break;
        }
        let is_sof = matches!(marker, 0xC0..=0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF);
        if is_sof {
            if len < 8 {
                break;
            }
            let h = u16::from_be_bytes([bytes[i + 3], bytes[i + 4]]);
            let w = u16::from_be_bytes([bytes[i + 5], bytes[i + 6]]);
            return check_side(u32::from(w), u32::from(h));
        }
        i += len;
    }
    Err(bad("imageattach", "not a JPEG file (no size marker found)"))
}

fn def_name(path: &str) -> String {
    let file = std::path::Path::new(path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| path.to_string());
    let stem = file.rsplit_once('.').map(|(s, _)| s).unwrap_or(&file);
    let stem = stem.trim();
    if stem.is_empty() { "Image".to_string() } else { stem.chars().take(64).collect() }
}

/// Build the IMAGE entity for `bytes` (a PNG/JPEG): `width` drawing units wide
/// (default: one unit per pixel, times `scale`), moved to `at`.
fn make_image(cmd: &str, bytes: &[u8], name: &str, at: Vec2, scale: f64, width: Option<f64>) -> Result<Image> {
    let ext = extension(name);
    if !RASTER_EXTS.contains(&ext.as_str()) {
        return Err(bad(cmd, format!("`.{ext}` is not a raster image (use png, jpg or jpeg)")));
    }
    let (w, h) = raster_size(bytes, &ext)?;
    let aspect = f64::from(h) / f64::from(w);
    let w_units = match width {
        Some(w) if w.is_finite() && w > 0.0 && w <= 1e12 => w,
        Some(_) => return Err(bad(cmd, "`width` must be a positive number")),
        None => f64::from(w) * scale,
    };
    let h_units = w_units * aspect;
    if !(h_units.is_finite() && h_units > 0.0 && h_units <= 1e12) {
        return Err(bad(cmd, "the image is too large to attach at this size"));
    }
    let file = std::path::Path::new(name).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| name.to_string());
    Ok(Image {
        insert: Vec3::new(at.x, at.y, 0.0),
        u: Vec3::new(w_units / f64::from(w), 0.0, 0.0),
        v: Vec3::new(0.0, h_units / f64::from(h), 0.0),
        size: Vec2::new(f64::from(w), f64::from(h)),
        path: file,
        name: def_name(name),
        ..Image::default()
    })
}

fn run_imageattach(s: &mut Session, p: &Value) -> Result<Value> {
    let (bytes, name) = read_input("imageattach", p)?;
    let (at, scale) = at_scale("imageattach", p)?;
    let width = p.get("width").and_then(Value::as_f64);
    let img = make_image("imageattach", &bytes, &name, at, scale, width)?;
    let (w, h) = (img.size.x, img.size.y);
    let space = s.space();
    let common = Common::default();
    let handle = s.doc_mut()?.add(&space, common, EntityKind::Image(img)).map_err(|e| bad("imageattach", e.to_string()))?;
    s.set_selection(vec![handle]);
    Ok(json!({
        "handle": handle.hex(),
        "size": [w, h],
        "path": name,
        "message": format!("Attached {name} ({w}x{h} px)"),
    }))
}

fn selected_image(s: &Session, p: &Value) -> Result<Handle> {
    if let Some(h) = p.get("handle").and_then(Value::as_str).and_then(Handle::parse_hex) {
        let d = s.doc()?;
        if d.entity(h).is_some_and(|e| matches!(e.kind, EntityKind::Image(_))) {
            return Ok(h);
        }
        return Err(bad("image", format!("no image {}", h.hex())));
    }
    let d = s.doc()?;
    for h in s.selection() {
        if d.entity(h).is_some_and(|e| matches!(e.kind, EntityKind::Image(_))) {
            return Ok(h);
        }
    }
    let space = s.space();
    let store = d.space(&space).ok_or_else(|| bad("image", "no such space"))?;
    store
        .iter()
        .find(|e| matches!(e.kind, EntityKind::Image(_)))
        .map(|e| e.handle)
        .ok_or_else(|| bad("image", "no image selected (select an IMAGE first)"))
}

fn pct(cmd: &str, p: &Value, key: &str) -> Result<Option<u8>> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => {
            let n = v.as_f64().ok_or_else(|| bad(cmd, format!("`{key}` must be 0..100")))?;
            if !n.is_finite() || !(0.0..=100.0).contains(&n) {
                return Err(bad(cmd, format!("`{key}` must be 0..100")));
            }
            Ok(Some(n.round() as u8))
        }
    }
}

fn run_imageadjust(s: &mut Session, p: &Value) -> Result<Value> {
    let h = selected_image(s, p)?;
    let (b, c, f) = (pct("imageadjust", p, "brightness")?, pct("imageadjust", p, "contrast")?, pct("imageadjust", p, "fade")?);
    if b.is_none() && c.is_none() && f.is_none() {
        return Err(bad("imageadjust", "nothing to adjust (give brightness, contrast or fade)"));
    }
    let d = s.doc_mut()?;
    // Model space, layouts, then block definitions.
    let mut out = None;
    let spaces_first: bool = d
        .modify_entity(h, |e| {
            if let EntityKind::Image(i) = &mut e.kind {
                if let Some(v) = b {
                    i.brightness = v;
                }
                if let Some(v) = c {
                    i.contrast = v;
                }
                if let Some(v) = f {
                    i.fade = v;
                }
                out = Some((i.brightness, i.contrast, i.fade));
            }
        })
        .is_ok();
    if !spaces_first {
        for blk in d.blocks.values_mut() {
            if Arc::make_mut(blk).entities.modify(h, |e| {
                if let EntityKind::Image(i) = &mut e.kind {
                    if let Some(v) = b {
                        i.brightness = v;
                    }
                    if let Some(v) = c {
                        i.contrast = v;
                    }
                    if let Some(v) = f {
                        i.fade = v;
                    }
                    out = Some((i.brightness, i.contrast, i.fade));
                }
            }) {
                break;
            }
        }
    }
    match out {
        Some((bb, cc, ff)) => Ok(json!({ "handle": h.hex(), "brightness": bb, "contrast": cc, "fade": ff })),
        None => Err(bad("imageadjust", "no such image")),
    }
}

fn run_imageclip(s: &mut Session, p: &Value) -> Result<Value> {
    let h = selected_image(s, p)?;
    let delete = p.get("delete").and_then(Value::as_bool).unwrap_or(false);
    let clip = p.get("clip").and_then(Value::as_bool);
    let boundary: Option<Vec<Vec2>> = match p.get("boundary") {
        None | Some(Value::Null) => None,
        Some(v) => {
            let a = v.as_array().ok_or_else(|| bad("imageclip", "`boundary` must be a list of points"))?;
            if a.len() > 10_000 {
                return Err(bad("imageclip", "too many boundary points (at most 10000)"));
            }
            if a.len() < 2 {
                return Err(bad("imageclip", "`boundary` needs at least 2 points"));
            }
            let mut pts = Vec::with_capacity(a.len());
            for q in a {
                let pt = point_value(q).ok_or_else(|| bad("imageclip", "`boundary` must be a list of points"))?;
                if !pt.x.is_finite() || !pt.y.is_finite() {
                    return Err(bad("imageclip", "`boundary` points must be finite numbers"));
                }
                pts.push(pt);
            }
            Some(pts)
        }
    };
    if delete {
        with_image_mut(s, h, |i| {
            i.clip = Vec::new();
            i.clipping = false;
        })?;
        return Ok(json!({ "handle": h.hex(), "clipping": false, "vertices": 0 }));
    }
    let (vertices, clipping) = match (boundary, clip) {
        (Some(b), c) => {
            let n = b.len();
            let on = c.unwrap_or(true);
            with_image_mut(s, h, |i| {
                i.clip = b.clone();
                i.clipping = on;
            })?;
            (n, on)
        }
        (None, Some(c)) => {
            let n = with_image_mut(s, h, |i| i.clipping = c)?;
            (n, c)
        }
        (None, None) => return Err(bad("imageclip", "give `boundary`, `clip`, or `delete: true`")),
    };
    Ok(json!({ "handle": h.hex(), "clipping": clipping, "vertices": vertices }))
}

fn with_image_mut(s: &mut Session, h: Handle, mut f: impl FnMut(&mut Image)) -> Result<usize> {
    let d = s.doc_mut()?;
    let mut vertices = None;
    let mut done = d
        .modify_entity(h, |e| {
            if let EntityKind::Image(i) = &mut e.kind {
                f(i);
                vertices = Some(i.clip.len());
            }
        })
        .is_ok()
        && vertices.is_some();
    if !done {
        for blk in d.blocks.values_mut() {
            if Arc::make_mut(blk).entities.modify(h, |e| {
                if let EntityKind::Image(i) = &mut e.kind {
                    f(i);
                    vertices = Some(i.clip.len());
                }
            }) {
                done = true;
                break;
            }
        }
    }
    if done { Ok(vertices.unwrap_or(0)) } else { Err(bad("imageclip", "no such image")) }
}

/// IMAGE transparency flag (display bit 8) of one image.
fn run_imagetransparency(s: &mut Session, p: &Value) -> Result<Value> {
    let h = selected_image(s, p)?;
    let on = match p.get("on") {
        None | Some(Value::Null) => None,
        Some(v) => Some(v.as_bool().ok_or_else(|| bad("imagetransparency", "`on` must be true or false"))?),
    };
    let mut state = false;
    // Read first (toggle needs the current flag).
    let cur = s.doc()?.entity(h).and_then(|e| match &e.kind {
        EntityKind::Image(i) => Some(i.display & 8 != 0),
        _ => None,
    });
    let next = on.unwrap_or(!cur.unwrap_or(false));
    with_image_mut(s, h, |i| {
        if next {
            i.display |= 8;
        } else {
            i.display &= !8;
        }
        state = i.display & 8 != 0;
    })?;
    Ok(json!({ "handle": h.hex(), "transparency": state }))
}

/// Display quality of all images (RASTERVARIABLES code 71: 0 draft, 1 high).
fn run_imagequality(s: &mut Session, p: &Value) -> Result<Value> {
    let mode = match str_param(p, "mode") {
        None => None,
        Some(m) if m.eq_ignore_ascii_case("draft") => Some(0i64),
        Some(m) if m.eq_ignore_ascii_case("high") => Some(1i64),
        Some(m) => {
            let _ = m;
            return Err(bad("imagequality", "`mode` must be draft or high"));
        }
    };
    let d = s.doc_mut()?;
    if let Some(m) = mode {
        d.header.set_i64("RASTERQUALITY", m);
    }
    let cur = d.header.i64("RASTERQUALITY", 1);
    Ok(json!({ "mode": if cur == 0 { "draft" } else { "high" } }))
}
/// IMAGEFRAME header variable (0 hide, 1 show, 2 show but not plotted).
fn run_imageframe(s: &mut Session, p: &Value) -> Result<Value> {
    let mode = match p.get("mode") {
        None | Some(Value::Null) => 1i64,
        Some(v) => {
            let n = v.as_i64().ok_or_else(|| bad("imageframe", "`mode` must be 0, 1 or 2"))?;
            if !(0..=2).contains(&n) {
                return Err(bad("imageframe", "`mode` must be 0 (hide), 1 (show) or 2 (show but not plotted)"));
            }
            n
        }
    };
    s.doc_mut()?.header.set_i64("IMAGEFRAME", mode);
    Ok(json!({ "mode": mode }))
}

fn run_imagedetach(s: &mut Session, p: &Value) -> Result<Value> {
    let d = s.doc()?;
    let target: Option<Handle> = match p.get("handle").and_then(Value::as_str).and_then(Handle::parse_hex) {
        Some(h) => Some(h),
        None => s.selection().iter().copied().find(|h| d.entity(*h).is_some_and(|e| matches!(e.kind, EntityKind::Image(_)))),
    };
    let name = str_param(p, "name").map(|n| n.to_ascii_uppercase());
    if target.is_none() && name.is_none() {
        let any = d.model.iter().find(|e| matches!(e.kind, EntityKind::Image(_))).map(|e| e.handle);
        if let Some(h) = any {
            return run_imagedetach(s, &json!({ "handle": h.hex() }));
        }
        return Err(bad("imagedetach", "no image selected (select an IMAGE first, or give `handle` or `name`)"));
    }
    let d = s.doc_mut()?;
    let mut removed = 0usize;
    if let Some(h) = target {
        if d.remove_entity(h).is_some_and(|e| matches!(e.kind, EntityKind::Image(_))) {
            removed += 1;
        } else {
            for b in d.blocks.values_mut() {
                if Arc::make_mut(b).entities.remove(h).is_some_and(|e| matches!(e.kind, EntityKind::Image(_))) {
                    removed += 1;
                    break;
                }
            }
        }
    } else if let Some(n) = name {
        let matches_def =
            |e: &Entity| matches!(&e.kind, EntityKind::Image(i) if i.name.to_ascii_uppercase() == n || i.path.to_ascii_uppercase() == n);
        let hs: Vec<Handle> = d.model.iter().filter(|e| matches_def(e)).map(|e| e.handle).collect();
        for h in hs {
            if d.remove_entity(h).is_some() {
                removed += 1;
            }
        }
        for l in 0..d.layouts.len() {
            let hs: Vec<Handle> =
                d.layouts.get(l).map(|l| l.entities.iter().filter(|e| matches_def(e)).map(|e| e.handle).collect()).unwrap_or_default();
            for h in hs {
                if d.remove_entity(h).is_some() {
                    removed += 1;
                }
            }
        }
    }
    if removed == 0 {
        return Err(bad("imagedetach", "no such image"));
    }
    s.set_selection(Vec::new());
    Ok(json!({ "removed": removed }))
}

fn pdf_page(cmd: &str, p: &Value) -> Result<Option<usize>> {
    match p.get("page") {
        None | Some(Value::Null) => Ok(None),
        Some(v) => {
            let n = v.as_u64().ok_or_else(|| bad(cmd, "`page` must be a page number (1-based)"))?;
            if n == 0 || n > 256 {
                return Err(bad(cmd, "`page` must be 1..=256"));
            }
            Ok(Some(n as usize))
        }
    }
}

fn run_pdfimport(s: &mut Session, p: &Value) -> Result<Value> {
    let hooks = io().ok_or_else(|| bad("pdfimport", "file formats are not available in this build"))?;
    let (bytes, name) = read_input("pdfimport", p)?;
    let ext = extension(&name);
    if ext != "pdf" {
        return Err(bad("pdfimport", format!("`.{ext}` is not a PDF file (use IMPORT for dxf/dwg, IMAGEATTACH for png/jpg/jpeg)")));
    }
    let page = pdf_page("pdfimport", p)?;
    let (at, scale) = at_scale("pdfimport", p)?;
    // One page when asked (the io hook imports every page otherwise).
    let src = match page {
        Some(n) => {
            let read_page = super::file::read_pdf_page().ok_or_else(|| bad("pdfimport", "single-page PDF import is not available in this build"))?;
            read_page(&bytes, &name, n).map_err(|e| bad("pdfimport", e))?
        }
        None => (hooks.read)(&bytes, &name).map_err(|e| bad("pdfimport", e))?,
    };
    let file = std::path::Path::new(&name).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or(name.clone());
    let mut r = super::exchange::import_drawing_for(s, &src, at, scale, &file)?;
    if let Some(o) = r.as_object_mut() {
        o.insert("note".into(), json!("vector paths only: text, dimensions, images and shadings are skipped; AI vectorization of raster pages is not available yet — attach the page as an image and trace it"));
    }
    Ok(r)
}

fn run_pdfattach(s: &mut Session, p: &Value) -> Result<Value> {
    // No separate underlay object exists yet: a PDF attaches as its vector linework.
    run_pdfimport(s, p)
}

fn run_attach(s: &mut Session, p: &Value) -> Result<Value> {
    // Dispatch by file type so ATTACH does what the toolbar promises.
    let name = str_param(p, "path").map(|s| s.to_string()).or_else(|| str_param(p, "name").map(|s| s.to_string())).unwrap_or_default();
    match extension(&name).as_str() {
        "dxf" | "dwg" | "dwt" => s.execute("import", p),
        "pdf" => s.execute("pdfimport", p),
        "png" | "jpg" | "jpeg" => s.execute("imageattach", p),
        _ if name.trim().is_empty() => Err(bad("attach", "give a file name (ATTACH needs dxf/dwg, pdf or png/jpg/jpeg)")),
        e => Err(bad("attach", format!("`.{e}` can't be attached (use dxf/dwg, pdf or png/jpg/jpeg; DWF/DGN underlays are not available yet)"))),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::super::file::{IoHooks, set_io, set_read_pdf_page};
    use super::*;
    use crate::Session;

    fn png_bytes(w: u32, h: u32) -> Vec<u8> {
        let mut b = vec![137, 80, 78, 71, 13, 10, 26, 10];
        b.extend([0, 0, 0, 13]);
        b.extend(b"IHDR");
        b.extend(w.to_be_bytes());
        b.extend(h.to_be_bytes());
        b.extend([8, 2, 0, 0, 0]);
        b
    }

    fn hooks() {
        set_io(IoHooks { read: |b, name| cadcraft_io_public::read(b, name), write: |d, name| cadcraft_io_public::write(d, name), plot: None });
        set_read_pdf_page(|b, name, n| cadcraft_io_public::read_pdf_page(b, name, n));
    }

    // The engine crate can't depend on the io crate (layering: io sits above the
    // engine); tests use synthetic drawings instead of real file reads.
    mod test_drawings {
        pub fn read(b: &[u8], name: &str) -> std::result::Result<cadcraft_doc::Drawing, String> {
            let ext = std::path::Path::new(name).extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
            match ext.as_str() {
                "png" | "jpg" | "jpeg" => Err("not wired in unit tests (use imageattach)".into()),
                _ => {
                    let _ = b;
                    Ok(cadcraft_doc::Drawing::new_imperial())
                }
            }
        }
        pub fn write(_d: &cadcraft_doc::Drawing, name: &str) -> std::result::Result<Vec<u8>, String> {
            Ok(format!("{name}:0").into_bytes())
        }
        pub fn read_pdf_page(_b: &[u8], _name: &str, _n: usize) -> std::result::Result<cadcraft_doc::Drawing, String> {
            Ok(cadcraft_doc::Drawing::new_imperial())
        }
    }

    // The engine crate can't depend on the io crate (layering: io sits above the
    // engine); tests reach synthetic drawings through tiny shims.
    mod cadcraft_io_public {
        pub fn read(b: &[u8], name: &str) -> std::result::Result<cadcraft_doc::Drawing, String> {
            super::test_drawings::read(b, name)
        }
        pub fn write(d: &cadcraft_doc::Drawing, name: &str) -> std::result::Result<Vec<u8>, String> {
            super::test_drawings::write(d, name)
        }
        pub fn read_pdf_page(b: &[u8], name: &str, n: usize) -> std::result::Result<cadcraft_doc::Drawing, String> {
            super::test_drawings::read_pdf_page(b, name, n)
        }
    }

    #[test]
    fn attach_adjust_clip_frame_detach() {
        hooks();
        let mut s = Session::new();
        let data = super::super::file::base64_encode(&png_bytes(8, 4));
        let r = s.execute("imageattach", &json!({ "data": data, "name": "plan.png", "at": [10.0, 5.0], "scale": 2.0 })).unwrap();
        let h = r["handle"].as_str().unwrap().to_string();
        assert_eq!(r["size"], json!([8.0, 4.0]));
        // One unit per pixel, scaled by 2: U is 2 units/px over 8 px = 16 units wide.
        let e = s.doc().unwrap().entity(Handle::parse_hex(&h).unwrap()).unwrap().clone();
        match &e.kind {
            EntityKind::Image(i) => {
                assert!((i.u.x - 2.0).abs() < 1e-12 && (i.insert.x - 10.0).abs() < 1e-12);
            }
            k => panic!("expected IMAGE, got {k:?}"),
        }
        let r = s.execute("imageadjust", &json!({ "handle": h, "brightness": 70, "fade": 10 })).unwrap();
        assert_eq!((r["brightness"].clone(), r["fade"].clone()), (json!(70), json!(10)));
        let r = s.execute("imageclip", &json!({ "handle": h, "boundary": [[10, 5], [26, 13]] })).unwrap();
        assert_eq!((r["clipping"].clone(), r["vertices"].clone()), (json!(true), json!(2)));
        let r = s.execute("imageframe", &json!({ "mode": 2 })).unwrap();
        assert_eq!(r["mode"], 2);
        assert!(s.execute("imageframe", &json!({ "mode": 7 })).is_err());
        let r = s.execute("imagetransparency", &json!({ "handle": h })).unwrap();
        assert_eq!(r["transparency"], json!(true), "toggles on");
        let r = s.execute("imagetransparency", &json!({ "handle": h, "on": false })).unwrap();
        assert_eq!(r["transparency"], json!(false));
        let r = s.execute("imagequality", &json!({ "mode": "draft" })).unwrap();
        assert_eq!(r["mode"], "draft");
        assert!(s.execute("imagequality", &json!({ "mode": "blurry" })).is_err());
        let r = s.execute("imagedetach", &json!({ "handle": h })).unwrap();
        assert_eq!(r["removed"], 1);
        assert!(s.execute("imageadjust", &json!({ "handle": h })).is_err());
    }

    #[test]
    fn attach_dispatches_by_type() {
        hooks();
        let mut s = Session::new();
        let data = super::super::file::base64_encode(&png_bytes(2, 2));
        let r = s.execute("attach", &json!({ "data": data, "name": "a.jpg" })).unwrap_err().to_string();
        assert!(r.contains("not a JPEG"), "{r}");
        let r = s.execute("attach", &json!({ "data": super::super::file::base64_encode(&png_bytes(2, 2)), "name": "a.png" })).unwrap();
        assert!(r["handle"].as_str().is_some());
        assert!(s.execute("attach", &json!({ "name": "a.dgn" })).is_err());
    }
}
