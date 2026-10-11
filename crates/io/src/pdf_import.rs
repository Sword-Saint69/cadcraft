//! PDF import (PDFIMPORT, IMPORT of a .pdf): best-effort vector paths to linework.
//!
//! Clean-room and dependency-light: the file is scanned for page dictionaries
//! (`/MediaBox`), content streams are inflated with miniz_oxide when they are
//! `/FlateDecode`, and the path operators (`m l c v y h re`, `S s f F B n W`,
//! `q Q cm w J j M d ri gs`) become LINE and LWPOLYLINE entities. Text (`BT…ET`),
//! images (`Do`), shadings and annotations are skipped: the drawing keeps the
//! linework, not the text or the pixels. Anything the parser doesn't understand
//! is skipped, never fatal.
//!
//! Units: PDF points (1/72 inch) become inches in the new (imperial) drawing, so a
//! `page`-sized sheet measures right with no scaling. Multi-page files land side by
//! side (page 2 starts one page width right of page 1); `page` (1-based) imports one
//! page only.

use cadcraft_doc::{Common, Drawing, EntityKind, Layer, Line};
use cadcraft_geom::{Bounds2, Vec2, Vec3};

use crate::{IoError, Result};

/// Most pages, streams, operators and points (hostile files).
const MAX_PAGES: usize = 256;
const MAX_STREAMS: usize = 4096;
const MAX_OPS: usize = 500_000;
const MAX_POINTS: usize = 500_000;
/// Most bytes inflated from one content stream.
const MAX_INFLATED: usize = 32_000_000;
/// Segments per cubic curve when flattening.
const CURVE_STEPS: usize = 12;
/// Lines shorter than this (drawing units) merge into the polyline's next point.
const MIN_SEG: f64 = 1e-9;

/// Import a PDF file: `page` is 1-based (`None`: every page, laid side by side).
pub fn drawing_for_pdf(bytes: &[u8], name: &str, page: Option<usize>) -> Result<Drawing> {
    if !is_pdf(bytes) {
        return Err(IoError::Format("not a PDF file (bad header)".into()));
    }
    let pages = page_boxes(bytes)?;
    if pages.is_empty() {
        return Err(IoError::Format("this PDF has no pages with a MediaBox".into()));
    }
    let wanted: Vec<(usize, [f64; 4])> = match page {
        Some(n) => {
            if n == 0 || n > pages.len() {
                return Err(IoError::Format(format!("page {n} is out of range (this PDF has {} page(s))", pages.len())));
            }
            vec![(n - 1, pages[n - 1])]
        }
        None => pages.into_iter().enumerate().collect(),
    };
    let mut d = Drawing::new_imperial();
    d.layers.push(Layer::new("PDF_IMPORT"));
    let mut total = 0usize;
    let mut dx = 0.0f64;
    for (pi, b) in wanted {
        let w_pt = (b[2] - b[0]).max(0.0);
        let ents = page_entities(bytes, pi, b, dx)?;
        total += ents.len();
        for (layer, kind) in ents {
            let common = Common { layer, ..Common::default() };
            d.add(&cadcraft_doc::Space::Model, common, kind).map_err(|e| IoError::Format(e.to_string()))?;
        }
        dx += w_pt / 72.0 + 1.0;
    }
    if total == 0 {
        return Err(IoError::Format("no vector paths found (text and images are not imported; use IMAGEATTACH for a raster)".into()));
    }
    let _ = name;
    Ok(d)
}

fn is_pdf(bytes: &[u8]) -> bool {
    bytes.len() > 5 && bytes[0..5] == *b"%PDF-"
}

/// MediaBox of every page, in PDF points: [x0, y0, x1, y1].
fn page_boxes(bytes: &[u8]) -> Result<Vec<[f64; 4]>> {
    // Pages are found by scanning for /Type /Page objects that are not /Pages.
    // Hostile-safe: bounded scan, bounded count, numbers only.
    let mut out = Vec::new();
    let mut i = 0usize;
    while i + 10 < bytes.len() && out.len() < MAX_PAGES {
        if bytes[i] == b'/'
            && starts_word(bytes, i + 1, "MediaBox")
            && let Some(b) = read_box(bytes, i + 9)
        {
            // Only keep boxes following a nearby /Type /Page (within 2KB before).
            let from = i.saturating_sub(2048);
            let window = &bytes[from..i.min(bytes.len())];
            if find_page_type(window) {
                out.push(b);
            }
        }
        i += 1;
    }
    // Fallback: any MediaBox when the /Type scan found nothing (flat files).
    if out.is_empty() {
        let mut i = 0usize;
        while i + 10 < bytes.len() && out.len() < MAX_PAGES {
            if bytes[i] == b'/'
                && starts_word(bytes, i + 1, "MediaBox")
                && let Some(b) = read_box(bytes, i + 9)
            {
                out.push(b);
            }
            i += 1;
        }
    }
    Ok(out)
}

fn find_page_type(window: &[u8]) -> bool {
    let mut i = 0usize;
    while i + 10 < window.len() {
        if window[i] == b'/' && starts_word(window, i + 1, "Type") {
            let mut j = i + 5;
            while j < window.len() && window[j].is_ascii_whitespace() {
                j += 1;
            }
            // /Pages also starts with "Page": require the next char not be 's'.
            if j < window.len() && window[j] == b'/' && starts_word(window, j + 1, "Page") {
                let after = j + 5;
                if after >= window.len() || window[after] != b's' {
                    return true;
                }
            }
        }
        i += 1;
    }
    false
}

fn starts_word(bytes: &[u8], at: usize, word: &str) -> bool {
    let w = word.as_bytes();
    if at + w.len() > bytes.len() {
        return false;
    }
    if &bytes[at..at + w.len()] != w {
        return false;
    }
    // Next byte must not be a name/letter continuation.
    if at + w.len() < bytes.len() {
        let c = bytes[at + w.len()];
        if c.is_ascii_alphanumeric() {
            return false;
        }
    }
    true
}

fn read_box(bytes: &[u8], mut i: usize) -> Option<[f64; 4]> {
    while i < bytes.len() && bytes[i] != b'[' {
        if bytes[i] == b'/' || bytes[i] == b'<' {
            return None;
        }
        i += 1;
    }
    if i >= bytes.len() {
        return None;
    }
    i += 1;
    let mut nums = [0.0f64; 4];
    for n in nums.iter_mut() {
        while i < bytes.len() && (bytes[i].is_ascii_whitespace()) {
            i += 1;
        }
        let (v, j) = read_number(bytes, i)?;
        *n = v;
        i = j;
    }
    if nums[2] > nums[0] && nums[3] > nums[1] && nums.iter().all(|v| v.is_finite() && v.abs() < 1e6) { Some(nums) } else { None }
}

fn read_number(bytes: &[u8], mut i: usize) -> Option<(f64, usize)> {
    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
        i += 1;
    }
    let start = i;
    if i < bytes.len() && (bytes[i] == b'+' || bytes[i] == b'-') {
        i += 1;
    }
    let mut digits = 0;
    let mut dot = false;
    while i < bytes.len() && (bytes[i].is_ascii_digit() || (!dot && bytes[i] == b'.')) {
        if bytes[i] == b'.' {
            dot = true;
        } else {
            digits += 1;
        }
        i += 1;
    }
    if digits == 0 {
        return None;
    }
    std::str::from_utf8(bytes.get(start..i)?).ok()?.parse::<f64>().ok().map(|v| (v, i))
}

/// Vector entities of page `pi` (0-based), shifted right by `dx` inches.
fn page_entities(bytes: &[u8], pi: usize, media: [f64; 4], dx: f64) -> Result<Vec<(String, EntityKind)>> {
    let streams = content_streams(bytes)?;
    if streams.is_empty() {
        return Ok(Vec::new());
    }
    // Page N uses stream N when the file has that many streams; else every stream.
    let picked: Vec<&[u8]> = if pi < streams.len() { vec![&streams[pi]] } else { streams.iter().map(|s| s.as_slice()).collect() };
    let mut out = Vec::new();
    for s in picked {
        let ops = inflate_if_needed(bytes, s);
        for (layer, kind) in interpret(&ops, media, dx) {
            if out.len() >= MAX_POINTS {
                break;
            }
            out.push((layer, kind));
        }
    }
    Ok(out)
}

/// Raw (still possibly compressed) content streams: `stream … endstream` slices whose
/// dictionary mentions /FlateDecode or that look like content (bounded count).
fn content_streams(bytes: &[u8]) -> Result<Vec<Vec<u8>>> {
    let mut out: Vec<Vec<u8>> = Vec::new();
    let mut i = 0usize;
    while i + 6 < bytes.len() && out.len() < MAX_STREAMS {
        if bytes[i..].starts_with(b"stream") && (i + 6 >= bytes.len() || !bytes[i + 6].is_ascii_alphabetic()) {
            let mut j = i + 6;
            while j < bytes.len() && (bytes[j] == b'\r' || bytes[j] == b'\n') {
                j += 1;
            }
            if let Some(end) = find(bytes, j, b"endstream") {
                // Dictionary window before `stream` decides flateness later.
                out.push(bytes[j..end].to_vec());
                i = end + 9;
                continue;
            }
            break;
        }
        i += 1;
    }
    Ok(out)
}

fn find(bytes: &[u8], from: usize, needle: &[u8]) -> Option<usize> {
    bytes.get(from..)?.windows(needle.len()).position(|w| w == needle).map(|p| p + from)
}

fn inflate_if_needed(_bytes: &[u8], raw: &[u8]) -> Vec<u8> {
    // Heuristic: zlib streams start with 0x78 (most common); try inflating, else raw.
    if raw.len() > 2
        && raw[0] == 0x78
        && [0x01, 0x9C, 0xDA].contains(&raw[1])
        && let Some(de) = decompress(raw)
    {
        return de;
    }
    raw.to_vec()
}

fn decompress(raw: &[u8]) -> Option<Vec<u8>> {
    use miniz_oxide::inflate::decompress_to_vec_zlib_with_limit;
    // FlateDecode streams are zlib-wrapped.
    decompress_to_vec_zlib_with_limit(raw, MAX_INFLATED).ok()
}

/// Current graphics state while interpreting a content stream.
#[derive(Clone, Copy)]
struct GState {
    /// Current transformation matrix (a b c d e f).
    ctm: [f64; 6],
}

impl Default for GState {
    fn default() -> Self {
        GState { ctm: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0] }
    }
}

/// Interpret content bytes into entities. Coordinates: PDF points with y up from the
/// MediaBox origin → inches (÷72), shifted by `dx`, y kept as-is (PDF y-up matches CAD y-up).
fn interpret(data: &[u8], media: [f64; 4], dx: f64) -> Vec<(String, EntityKind)> {
    let mut out = Vec::new();
    let mut stack: Vec<GState> = vec![GState::default()];
    let mut cur: Vec<Vec2> = Vec::new();
    let mut sub_start: Option<Vec2> = None;
    let mut in_text = false;
    let mut operands: Vec<f64> = Vec::new();
    let mut ops = 0usize;

    let mut i = 0usize;
    while i < data.len() {
        let c = data[i];
        if c == b'%' {
            while i < data.len() && data[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if c == b'(' {
            i = skip_literal(data, i);
            continue;
        }
        if c == b'<' {
            if i + 1 < data.len() && data[i + 1] == b'<' {
                i = skip_nested(data, i, b'<', b'>');
            } else {
                i = skip_until(data, i, b'>');
            }
            continue;
        }
        if c == b'[' {
            i = skip_nested(data, i, b'[', b']');
            operands.clear();
            continue;
        }
        if c == b'/' {
            i += 1;
            while i < data.len() && !data[i].is_ascii_whitespace() && !b"()<>[]{}/%".contains(&data[i]) {
                i += 1;
            }
            operands.clear();
            // A name alone is an operand (e.g. /F1 in `… /F1 Do`); keep parsing.
            // `Do` with no numbers draws an image: skip (no operands to keep anyway).
            continue;
        }
        if c.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        if c.is_ascii_digit() || c == b'+' || c == b'-' || c == b'.' {
            if let Some((v, j)) = read_number(data, i) {
                if operands.len() < 64 {
                    operands.push(v);
                }
                i = j;
                continue;
            }
            i += 1;
            continue;
        }
        // An operator: letters (and * ' ").
        if c.is_ascii_alphabetic() || c == b'*' || c == b'\'' || c == b'"' {
            let start = i;
            while i < data.len() && (data[i].is_ascii_alphabetic() || data[i] == b'*' || data[i] == b'\'' || data[i] == b'"') {
                i += 1;
            }
            let op = &data[start..i];
            ops += 1;
            if ops > MAX_OPS || out.len() > MAX_POINTS {
                break;
            }
            apply_op(op, &operands, &mut stack, &mut cur, &mut sub_start, &mut in_text, media, dx, &mut out);
            operands.clear();
            continue;
        }
        i += 1;
    }
    flush_path(&mut cur, &mut sub_start, false, &mut out);
    out
}

#[allow(clippy::too_many_arguments)]
fn apply_op(
    op: &[u8],
    args: &[f64],
    stack: &mut Vec<GState>,
    cur: &mut Vec<Vec2>,
    sub_start: &mut Option<Vec2>,
    in_text: &mut bool,
    media: [f64; 4],
    dx: f64,
    out: &mut Vec<(String, EntityKind)>,
) {
    let g = || stack.last().copied().unwrap_or_default();
    let map = |x: f64, y: f64| map_pt(g().ctm, x, y, media, dx);
    match op {
        b"BT" => *in_text = true,
        b"ET" => *in_text = false,
        _ if *in_text => {}
        b"q" => {
            let s = g();
            if stack.len() < 64 {
                stack.push(s);
            }
        }
        b"Q" => {
            if stack.len() > 1 {
                stack.pop();
            }
        }
        b"cm" => {
            if args.len() >= 6 {
                let m = [args[0], args[1], args[2], args[3], args[4], args[5]];
                if let Some(s) = stack.last_mut() {
                    s.ctm = mul(m, s.ctm);
                }
            }
        }
        b"m" => {
            if args.len() >= 2 {
                flush_path(cur, sub_start, false, out);
                let p = map(args[0], args[1]);
                cur.push(p);
                *sub_start = Some(p);
            }
        }
        b"l" => {
            if args.len() >= 2 {
                let p = map(args[0], args[1]);
                if cur.is_empty() {
                    *sub_start = Some(p);
                }
                push_pt(cur, p);
            }
        }
        b"c" => {
            if args.len() >= 6 && !cur.is_empty() {
                let p0 = *cur.last().unwrap_or(&Vec2::ZERO);
                let p1 = map(args[0], args[1]);
                let p2 = map(args[2], args[3]);
                let p3 = map(args[4], args[5]);
                for k in 1..=CURVE_STEPS {
                    let t = k as f64 / CURVE_STEPS as f64;
                    push_pt(cur, cubic(p0, p1, p2, p3, t));
                }
            } else if args.len() >= 6 {
                let p3 = map(args[4], args[5]);
                cur.push(map(args[0], args[1]));
                for k in 1..=CURVE_STEPS {
                    let t = k as f64 / CURVE_STEPS as f64;
                    let p0 = cur[0];
                    push_pt(cur, cubic(p0, map(args[0], args[1]), map(args[2], args[3]), p3, t));
                }
                *sub_start = Some(cur[0]);
            }
        }
        b"v" | b"y" => {
            if args.len() >= 4 {
                let p0 = cur.last().copied().unwrap_or_else(|| map(args[0], args[1]));
                let p3 = map(args[2], args[3]);
                let (p1, p2) = if op == b"v" { (p0, map(args[0], args[1])) } else { (map(args[0], args[1]), p3) };
                for k in 1..=CURVE_STEPS {
                    let t = k as f64 / CURVE_STEPS as f64;
                    push_pt(cur, cubic(p0, p1, p2, p3, t));
                }
            }
        }
        b"h" => {
            if let Some(s) = *sub_start {
                push_pt(cur, s);
            }
        }
        b"re" => {
            if args.len() >= 4 {
                flush_path(cur, sub_start, false, out);
                let (x, y, w, h) = (args[0], args[1], args[2], args[3]);
                if w.abs() < 1e9 && h.abs() < 1e9 {
                    let pts = [map(x, y), map(x + w, y), map(x + w, y + h), map(x, y + h), map(x, y)];
                    emit_poly(&pts, out);
                }
            }
        }
        b"S" | b"s" | b"f" | b"F" | b"B" | b"b" | b"n" => {
            if (op == b"s" || op == b"b")
                && let Some(s) = *sub_start
            {
                push_pt(cur, s);
            }
            let stroke = op == b"S" || op == b"s" || op == b"B" || op == b"b";
            flush_path(cur, sub_start, stroke, out);
        }
        // Stroke/fill attributes, clipping, images, shadings: parsed and ignored.
        b"w" | b"J" | b"j" | b"M" | b"d" | b"ri" | b"gs" | b"W" | b"Do" | b"sh" => {}
        _ => {}
    }
}

fn push_pt(cur: &mut Vec<Vec2>, p: Vec2) {
    if !p.x.is_finite() || !p.y.is_finite() {
        return;
    }
    if cur.len() >= MAX_POINTS {
        return;
    }
    if let Some(&last) = cur.last()
        && (last.x - p.x).abs() + (last.y - p.y).abs() < MIN_SEG
    {
        return;
    }
    cur.push(p);
}

fn flush_path(cur: &mut Vec<Vec2>, sub: &mut Option<Vec2>, _stroke: bool, out: &mut Vec<(String, EntityKind)>) {
    if cur.len() < 2 {
        cur.clear();
        *sub = None;
        return;
    }
    emit_poly(cur, out);
    cur.clear();
    *sub = None;
}

fn emit_poly(pts: &[Vec2], out: &mut Vec<(String, EntityKind)>) {
    if pts.len() == 2 {
        let (a, b) = (pts[0], pts[1]);
        out.push(("PDF_IMPORT".to_string(), EntityKind::Line(Line { a: Vec3::new(a.x, a.y, 0.0), b: Vec3::new(b.x, b.y, 0.0) })));
        return;
    }
    // Longer paths become LWPOLYLINEs via DXF round trip; store as LINE strips when short
    // is wasteful — instead chain LINEs (simple, always renders).
    // To keep entity counts sane, emit one LINE per segment pair.
    for w in pts.windows(2) {
        if out.len() >= MAX_POINTS {
            break;
        }
        out.push(("PDF_IMPORT".to_string(), EntityKind::Line(Line { a: Vec3::new(w[0].x, w[0].y, 0.0), b: Vec3::new(w[1].x, w[1].y, 0.0) })));
    }
}

fn cubic(p0: Vec2, p1: Vec2, p2: Vec2, p3: Vec2, t: f64) -> Vec2 {
    let u = 1.0 - t;
    Vec2::new(
        u * u * u * p0.x + 3.0 * u * u * t * p1.x + 3.0 * u * t * t * p2.x + t * t * t * p3.x,
        u * u * u * p0.y + 3.0 * u * u * t * p1.y + 3.0 * u * t * t * p2.y + t * t * t * p3.y,
    )
}

fn mul(a: [f64; 6], b: [f64; 6]) -> [f64; 6] {
    [
        a[0] * b[0] + a[1] * b[2],
        a[0] * b[1] + a[1] * b[3],
        a[2] * b[0] + a[3] * b[2],
        a[2] * b[1] + a[3] * b[3],
        a[4] * b[0] + a[5] * b[2] + b[4],
        a[4] * b[1] + a[5] * b[3] + b[5],
    ]
}

fn map_pt(ctm: [f64; 6], x: f64, y: f64, media: [f64; 4], dx: f64) -> Vec2 {
    if !x.is_finite() || !y.is_finite() {
        return Vec2::ZERO;
    }
    let xc = x.clamp(-1e6, 1e6);
    let yc = y.clamp(-1e6, 1e6);
    let px = ctm[0] * xc + ctm[2] * yc + ctm[4];
    let py = ctm[1] * xc + ctm[3] * yc + ctm[5];
    // Points are relative to the MediaBox origin; inches; pages laid along +x.
    Vec2::new((px - media[0]) / 72.0 + dx, (py - media[1]) / 72.0)
}

fn skip_literal(data: &[u8], mut i: usize) -> usize {
    // data[i] == '('; skip balanced parens with backslash escapes.
    let mut depth = 0usize;
    while i < data.len() {
        match data[i] {
            b'\\' => i += 2,
            b'(' => {
                depth += 1;
                i += 1;
            }
            b')' => {
                depth = depth.saturating_sub(1);
                i += 1;
                if depth == 0 {
                    break;
                }
            }
            _ => i += 1,
        }
        if depth == 0 && i > 0 {
            break;
        }
    }
    i
}

fn skip_until(data: &[u8], mut i: usize, end: u8) -> usize {
    while i < data.len() && data[i] != end {
        i += 1;
    }
    (i + 1).min(data.len())
}

fn skip_nested(data: &[u8], mut i: usize, open: u8, close: u8) -> usize {
    let mut depth = 0usize;
    while i < data.len() {
        if data[i] == open {
            depth += 1;
        } else if data[i] == close {
            depth = depth.saturating_sub(1);
            if depth == 0 {
                return (i + 1).min(data.len());
            }
        } else if open == b'<' && data[i] == b'(' {
            i = skip_literal(data, i);
            continue;
        }
        i += 1;
    }
    i
}

/// Tight bounds of imported linework (for messages), in drawing units.
pub fn imported_bounds(d: &Drawing) -> Option<Bounds2> {
    let b = d.extents(&cadcraft_doc::Space::Model);
    if b.is_empty() { None } else { Some(b) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pdf_one_line() -> Vec<u8> {
        // One page, one stroked line, uncompressed content.
        let content = b"0 0 1 RG 2 w 72 72 m 288 288 l S";
        let mut pdf = format!(
            "%PDF-1.4\n1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj\n2 0 obj<</Type/Pages/Kids[3 0 R]/Count 1>>endobj\n3 0 obj<</Type/Page/Parent 2 0 R/MediaBox[0 0 612 792]/Contents 4 0 R>>endobj\n4 0 obj<</Length {}>>stream\n",
            content.len()
        )
        .into_bytes();
        pdf.extend(content);
        pdf.extend(b"\nendstream\nendobj\ntrailer<</Root 1 0 R>>\n%%EOF");
        pdf
    }

    #[test]
    fn imports_a_line() {
        let d = drawing_for_pdf(&pdf_one_line(), "a.pdf", None).unwrap();
        assert_eq!(d.model.len(), 1);
        match &d.model.iter().next().unwrap().kind {
            EntityKind::Line(l) => {
                assert!((l.a.x - 1.0).abs() < 1e-9 && (l.b.x - 4.0).abs() < 1e-9);
            }
            k => panic!("expected LINE, got {k:?}"),
        }
    }

    #[test]
    fn rejects_non_pdf_and_bad_page() {
        assert!(drawing_for_pdf(b"nope", "a.pdf", None).is_err());
        let pdf = pdf_one_line();
        assert!(drawing_for_pdf(&pdf, "a.pdf", Some(2)).is_err());
        assert!(drawing_for_pdf(&pdf, "a.pdf", Some(0)).is_err());
    }

    #[test]
    fn imports_flate_compressed_content() {
        use miniz_oxide::deflate::compress_to_vec_zlib;
        let content = b"72 72 m 288 288 l S";
        let compressed = compress_to_vec_zlib(content, 6);
        let mut pdf = format!(
            "%PDF-1.4\n1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj\n2 0 obj<</Type/Pages/Kids[3 0 R]/Count 1>>endobj\n3 0 obj<</Type/Page/Parent 2 0 R/MediaBox[0 0 612 792]/Contents 4 0 R>>endobj\n4 0 obj<</Length {}/Filter/FlateDecode>>stream\n",
            compressed.len()
        )
        .into_bytes();
        pdf.extend(&compressed);
        pdf.extend(b"\nendstream\nendobj\ntrailer<</Root 1 0 R>>\n%%EOF");
        let d = drawing_for_pdf(&pdf, "a.pdf", None).unwrap();
        assert_eq!(d.model.len(), 1);
    }
}
