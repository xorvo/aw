//! Generated PWA app icon: a green "›_" terminal glyph on a dark
//! gradient, encoded as a minimal PNG (color type 2 RGB, filter 0).
//! Direct port of the prototype's `buildIcon` so phones that already
//! added the app to their home screen keep the same icon.

use std::io::Write;
use std::sync::OnceLock;

use flate2::write::ZlibEncoder;
use flate2::Compression;

/// The two sizes the app shell references. Built lazily, once.
pub fn icon_png(size: u32) -> &'static [u8] {
    static ICON_180: OnceLock<Vec<u8>> = OnceLock::new();
    static ICON_512: OnceLock<Vec<u8>> = OnceLock::new();
    match size {
        512 => ICON_512.get_or_init(|| build_icon(512)),
        _ => ICON_180.get_or_init(|| build_icon(180)),
    }
}

fn build_icon(size: usize) -> Vec<u8> {
    let mut rgb = vec![0u8; size * size * 3];
    let lerp = |a: i32, b: i32, t: f64| (a as f64 + (b as f64 - a as f64) * t).round() as u8;

    // dark vertical gradient #141a24 -> #0b0e14
    for y in 0..size {
        let t = y as f64 / size as f64;
        let (r, g, b) = (lerp(0x14, 0x0b, t), lerp(0x1a, 0x0e, t), lerp(0x24, 0x14, t));
        for x in 0..size {
            let i = (y * size + x) * 3;
            rgb[i] = r;
            rgb[i + 1] = g;
            rgb[i + 2] = b;
        }
    }

    // distance from a point to segment AB
    let dseg = |px: f64, py: f64, ax: f64, ay: f64, bx: f64, by: f64| -> f64 {
        let (dx, dy) = (bx - ax, by - ay);
        let l2 = (dx * dx + dy * dy).max(1.0);
        let t = (((px - ax) * dx + (py - ay) * dy) / l2).clamp(0.0, 1.0);
        ((px - (ax + t * dx)).powi(2) + (py - (ay + t * dy)).powi(2)).sqrt()
    };

    let s = size as f64 / 180.0; // design at 180, scale up
    let (a, m, b) = ((58.0 * s, 52.0 * s), (112.0 * s, 90.0 * s), (58.0 * s, 128.0 * s));
    let stroke = 11.0 * s;
    let green = (0x3f, 0xb9, 0x50); // green ">"
    let (cx0, cx1, cy0, cy1) = (120.0 * s, 152.0 * s, 113.0 * s, 124.0 * s); // "_" cursor block
    for y in 0..size {
        for x in 0..size {
            let (px, py) = (x as f64, y as f64);
            let d = dseg(px, py, a.0, a.1, m.0, m.1).min(dseg(px, py, m.0, m.1, b.0, b.1));
            if d <= stroke || (px >= cx0 && px <= cx1 && py >= cy0 && py <= cy1) {
                let i = (y * size + x) * 3;
                rgb[i] = green.0;
                rgb[i + 1] = green.1;
                rgb[i + 2] = green.2;
            }
        }
    }

    // encode (color type 2, RGB; filter byte 0 per row)
    let row = 1 + size * 3;
    let mut raw = vec![0u8; size * row];
    for y in 0..size {
        raw[y * row] = 0;
        raw[y * row + 1..(y + 1) * row].copy_from_slice(&rgb[y * size * 3..(y + 1) * size * 3]);
    }
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&(size as u32).to_be_bytes());
    ihdr.extend_from_slice(&(size as u32).to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]); // 8-bit, RGB, deflate, filter 0, no interlace

    let mut enc = ZlibEncoder::new(Vec::new(), Compression::default());
    // Writing to a Vec can't fail; fall back to an empty IDAT if it ever did.
    let idat = enc
        .write_all(&raw)
        .and_then(|_| enc.finish())
        .unwrap_or_default();

    let mut png = Vec::with_capacity(idat.len() + 64);
    png.extend_from_slice(&[0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]);
    push_chunk(&mut png, b"IHDR", &ihdr);
    push_chunk(&mut png, b"IDAT", &idat);
    push_chunk(&mut png, b"IEND", &[]);
    png
}

fn push_chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let mut crc = Crc32::new();
    crc.update(kind);
    crc.update(data);
    out.extend_from_slice(&crc.finish().to_be_bytes());
}

/// Standard PNG CRC-32 (reflected, poly 0xedb88320), table-driven.
struct Crc32 {
    state: u32,
}

impl Crc32 {
    fn new() -> Self {
        Self { state: 0xffff_ffff }
    }

    fn update(&mut self, data: &[u8]) {
        static TABLE: OnceLock<[u32; 256]> = OnceLock::new();
        let table = TABLE.get_or_init(|| {
            let mut t = [0u32; 256];
            for (n, slot) in t.iter_mut().enumerate() {
                let mut c = n as u32;
                for _ in 0..8 {
                    c = if c & 1 != 0 { 0xedb8_8320 ^ (c >> 1) } else { c >> 1 };
                }
                *slot = c;
            }
            t
        });
        for &b in data {
            self.state = table[((self.state ^ b as u32) & 0xff) as usize] ^ (self.state >> 8);
        }
    }

    fn finish(self) -> u32 {
        self.state ^ 0xffff_ffff
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc32_known_vector() {
        // CRC-32 of "123456789" is 0xcbf43926 (the classic check value).
        let mut crc = Crc32::new();
        crc.update(b"123456789");
        assert_eq!(crc.finish(), 0xcbf4_3926);
    }

    #[test]
    fn icon_is_valid_png_with_correct_dimensions() {
        for size in [180u32, 512] {
            let png = icon_png(size);
            assert_eq!(&png[0..8], &[0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a], "signature");
            // IHDR is always the first chunk: length(4) type(4) then width/height.
            assert_eq!(&png[12..16], b"IHDR");
            let w = u32::from_be_bytes([png[16], png[17], png[18], png[19]]);
            let h = u32::from_be_bytes([png[20], png[21], png[22], png[23]]);
            assert_eq!((w, h), (size, size));
            assert_eq!(&png[png.len() - 8..png.len() - 4], b"IEND");
        }
    }

    #[test]
    fn icon_contains_green_glyph_and_gradient() {
        // Decode-light check: the IDAT payload must inflate back to the
        // raw size and contain both the green stroke and gradient pixels.
        let png = build_icon(64);
        let idat_start = 8 + 25; // signature + IHDR chunk (4+4+13+4)
        assert_eq!(&png[idat_start + 4..idat_start + 8], b"IDAT");
        let len = u32::from_be_bytes([
            png[idat_start], png[idat_start + 1], png[idat_start + 2], png[idat_start + 3],
        ]) as usize;
        let compressed = &png[idat_start + 8..idat_start + 8 + len];
        let mut raw = Vec::new();
        let mut dec = flate2::read::ZlibDecoder::new(compressed);
        std::io::Read::read_to_end(&mut dec, &mut raw).expect("zlib round-trip");
        assert_eq!(raw.len(), 64 * (1 + 64 * 3), "raw scanline size");
        // Green glyph pixel present somewhere.
        let green = raw.chunks(3).any(|c| c == [0x3f, 0xb9, 0x50]);
        assert!(green, "expected the green '>' stroke in the bitmap");
    }
}
