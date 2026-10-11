//! Raster image files (PNG, JPEG) as drawings, and their pixel sizes.
//!
//! Clean-room header parsers (no image dependency): PNG's IHDR chunk and JPEG's SOF
//! markers. Anything else (or anything truncated) is an error, never a panic. The
//! pixels themselves are not loaded; the IMAGE entity points at the file like an
//! IMAGEATTACH would, so the drawing saves through DXF/DWG as usual.

use cadcraft_doc::{Common, Drawing, Entity, EntityKind, Image, Space};
use cadcraft_geom::{Vec2, Vec3};

use crate::{IoError, Result};

/// Image formats IMPORT/OPEN accept as raster files.
pub const RASTER_EXTS: &[&str] = &["png", "jpg", "jpeg"];
/// Most pixels per side accepted (hostile files).
pub const MAX_SIDE_PX: u32 = 100_000;

/// Pixel size of a raster file, or an error describing why it is not one.
pub fn raster_size(bytes: &[u8], name: &str) -> Result<(u32, u32)> {
    let ext = std::path::Path::new(name).extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    match ext.as_str() {
        "png" => png_size(bytes),
        "jpg" | "jpeg" => jpeg_size(bytes),
        e => Err(IoError::Unsupported(e.to_string())),
    }
}

/// A drawing holding one IMAGE entity for a raster file: `w` x `h` drawing units at
/// the origin (one unit per pixel), on layer 0, pointing at `name`'s file name.
pub fn drawing_for_raster(bytes: &[u8], name: &str) -> Result<Drawing> {
    let (w, h) = raster_size(bytes, name)?;
    let file =
        std::path::Path::new(name).file_name().map(|n| n.to_string_lossy().to_string()).filter(|n| !n.is_empty()).unwrap_or_else(|| name.to_string());
    let stem = file.rsplit_once('.').map(|(s, _)| s).unwrap_or(&file);
    let stem = stem.trim();
    let def_name = if stem.is_empty() { "Image".to_string() } else { stem.to_string() };
    let mut d = Drawing::new_imperial();
    let img = Image {
        insert: Vec3::ZERO,
        u: Vec3::new(1.0, 0.0, 0.0),
        v: Vec3::new(0.0, 1.0, 0.0),
        size: Vec2::new(f64::from(w), f64::from(h)),
        path: file,
        name: def_name,
        ..Image::default()
    };
    let handle = d.new_handle();
    d.model.push(Entity::new(handle, EntityKind::Image(img)));
    d.ensure_layer(&Common::default().layer);
    let _ = &Space::Model;
    Ok(d)
}

fn png_size(bytes: &[u8]) -> Result<(u32, u32)> {
    // Signature + IHDR length/type/width/height: 8 + 4 + 4 + 4 + 4.
    if bytes.len() < 24 || bytes[0..8] != [137, 80, 78, 71, 13, 10, 26, 10] || &bytes[12..16] != b"IHDR" {
        return Err(IoError::Format("not a PNG file (bad signature or IHDR)".into()));
    }
    let w = u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
    let h = u32::from_be_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);
    check_side(w, h, "PNG")
}

fn jpeg_size(bytes: &[u8]) -> Result<(u32, u32)> {
    if bytes.len() < 4 || bytes[0] != 0xFF || bytes[1] != 0xD8 {
        return Err(IoError::Format("not a JPEG file (bad SOI)".into()));
    }
    // Scan markers: each (except SOI/RSTn/SOI/TEM) is FF xx + u16 length.
    let mut i = 2usize;
    while i + 4 <= bytes.len() {
        if bytes[i] != 0xFF {
            i += 1;
            continue;
        }
        // Skip padding FF bytes.
        while i < bytes.len() && bytes[i] == 0xFF {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        let marker = bytes[i];
        i += 1;
        // Standalone markers without length.
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
        // SOF0-SOF3, SOF5-SOF7, SOF9-SOF11, SOF13-SOF15 hold width/height.
        let is_sof = matches!(marker, 0xC0..=0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF);
        if is_sof {
            if len < 8 {
                break;
            }
            let h = u16::from_be_bytes([bytes[i + 3], bytes[i + 4]]);
            let w = u16::from_be_bytes([bytes[i + 5], bytes[i + 6]]);
            return check_side(u32::from(w), u32::from(h), "JPEG");
        }
        i += len;
    }
    Err(IoError::Format("not a JPEG file (no size marker found)".into()))
}

fn check_side(w: u32, h: u32, what: &str) -> Result<(u32, u32)> {
    if w == 0 || h == 0 || w > MAX_SIDE_PX || h > MAX_SIDE_PX {
        return Err(IoError::Format(format!("{what} image size {w}x{h} is not usable (1..={MAX_SIDE_PX} pixels per side)")));
    }
    Ok((w, h))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(w: u32, h: u32) -> Vec<u8> {
        let mut b = vec![137, 80, 78, 71, 13, 10, 26, 10];
        b.extend([0, 0, 0, 13]);
        b.extend(b"IHDR");
        b.extend(w.to_be_bytes());
        b.extend(h.to_be_bytes());
        b.extend([8, 2, 0, 0, 0]);
        b
    }

    #[test]
    fn png_header_sizes() {
        assert_eq!(raster_size(&png(320, 200), "a.png").unwrap(), (320, 200));
        assert!(raster_size(b"nope", "a.png").is_err());
        assert!(raster_size(&png(0, 10), "a.png").is_err());
    }

    #[test]
    fn drawing_holds_one_image() {
        let d = drawing_for_raster(&png(4, 2), "plan.PNG").unwrap();
        assert_eq!(d.model.len(), 1);
        match &d.model.iter().next().unwrap().kind {
            EntityKind::Image(i) => {
                assert_eq!((i.size.x, i.size.y), (4.0, 2.0));
                assert_eq!(i.path, "plan.PNG");
            }
            k => panic!("expected IMAGE, got {k:?}"),
        }
    }
}
