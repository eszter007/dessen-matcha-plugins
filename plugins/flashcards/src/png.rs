//! PNG to a dithered 1-bit image, at most `max_w` x `max_h`, for the cards' pictures. Non-interlaced
//! PNGs of any colour type; transparency is laid on white, as the reader shows it. The ordered
//! dither is the firmware's own (dsn_book_cover). Saved as u16 width, u16 height, then rows of
//! (width + 7) / 8 bytes, MSB first, set = ink.
use alloc::{vec, vec::Vec};

const BAYER: [[u8; 4]; 4] = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]];

fn be32(b: &[u8]) -> u32 { u32::from_be_bytes([b[0], b[1], b[2], b[3]]) }

/// The image as 8-bit gray on white, or None when it is not a PNG this can read.
fn decode(data: &[u8]) -> Option<(usize, usize, Vec<u8>)> {
    if data.len() < 33 || &data[..8] != b"\x89PNG\r\n\x1a\n" {
        return None;
    }
    let (mut w, mut h, mut depth, mut color) = (0usize, 0usize, 0u8, 0u8);
    let mut palette: Vec<[u8; 3]> = Vec::new();
    let mut alpha_pal: Vec<u8> = Vec::new();
    let mut idat = Vec::new();
    let mut p = 8;
    while p + 12 <= data.len() {
        let len = be32(&data[p..]) as usize;
        let kind = &data[p + 4..p + 8];
        let body = data.get(p + 8..p + 8 + len)?;
        match kind {
            b"IHDR" => {
                w = be32(body) as usize;
                h = be32(&body[4..]) as usize;
                depth = body[8];
                color = body[9];
                if body[12] != 0 {
                    return None; // interlaced
                }
            }
            b"PLTE" => palette = body.chunks_exact(3).map(|c| [c[0], c[1], c[2]]).collect(),
            b"tRNS" => alpha_pal = body.to_vec(),
            b"IDAT" => idat.extend_from_slice(body),
            b"IEND" => break,
            _ => {}
        }
        p += 12 + len;
    }
    if w == 0 || h == 0 || w > 4096 || h > 4096 {
        return None;
    }
    let raw = miniz_oxide::inflate::decompress_to_vec_zlib(&idat).ok()?;
    let channels = match color { 0 => 1, 2 => 3, 3 => 1, 4 => 2, 6 => 4, _ => return None };
    let bits = channels * depth as usize;
    let bpp = bits.div_ceil(8).max(1);
    let stride = (w * bits).div_ceil(8);
    if raw.len() < (stride + 1) * h {
        return None;
    }
    let mut prev = vec![0u8; stride];
    let mut cur = vec![0u8; stride];
    let mut gray = vec![255u8; w * h];
    for y in 0..h {
        let line = &raw[y * (stride + 1)..(y + 1) * (stride + 1)];
        let filter = line[0];
        for x in 0..stride {
            let a = if x >= bpp { cur[x - bpp] as i16 } else { 0 };
            let b = prev[x] as i16;
            let c = if x >= bpp { prev[x - bpp] as i16 } else { 0 };
            let v = line[1 + x] as i16;
            cur[x] = match filter {
                0 => v,
                1 => v + a,
                2 => v + b,
                3 => v + (a + b) / 2,
                4 => {
                    let pp = a + b - c;
                    let (pa, pb, pc) = ((pp - a).abs(), (pp - b).abs(), (pp - c).abs());
                    v + if pa <= pb && pa <= pc { a } else if pb <= pc { b } else { c }
                }
                _ => return None,
            } as u8;
        }
        let sample = |i: usize| -> u32 {
            // channel sample i of this row, scaled to 0..255
            match depth {
                8 => cur[i] as u32,
                16 => cur[i * 2] as u32,
                d => {
                    let bit = i * d as usize;
                    let v = (cur[bit / 8] >> (8 - d as usize - bit % 8)) & ((1u8 << d) - 1);
                    if color == 3 { v as u32 } else { v as u32 * 255 / ((1u32 << d) - 1) }
                }
            }
        };
        for x in 0..w {
            let (g, alpha) = match color {
                0 => (sample(x), 255),
                2 => {
                    let (r, gg, bb) = (sample(x * 3), sample(x * 3 + 1), sample(x * 3 + 2));
                    ((r * 77 + gg * 150 + bb * 29) >> 8, 255)
                }
                3 => {
                    let i = sample(x) as usize;
                    let c = palette.get(i).copied().unwrap_or([255, 255, 255]);
                    (((c[0] as u32) * 77 + (c[1] as u32) * 150 + (c[2] as u32) * 29) >> 8,
                     alpha_pal.get(i).copied().unwrap_or(255) as u32)
                }
                4 => (sample(x * 2), sample(x * 2 + 1)),
                _ => {
                    let (r, gg, bb) = (sample(x * 4), sample(x * 4 + 1), sample(x * 4 + 2));
                    ((r * 77 + gg * 150 + bb * 29) >> 8, sample(x * 4 + 3))
                }
            };
            gray[y * w + x] = ((g * alpha + 255 * (255 - alpha)) / 255) as u8;
        }
        core::mem::swap(&mut prev, &mut cur);
    }
    Some((w, h, gray))
}

/// The 1-bit file for a PNG, scaled down (box-averaged) to fit `max_w` x `max_h`.
pub fn to_bits(data: &[u8], max_w: usize, max_h: usize) -> Option<Vec<u8>> {
    let (w, h, gray) = decode(data)?;
    let scale_num = max_w.min(w * max_h / h.max(1)).min(w).max(1); // target width
    let tw = scale_num;
    let th = (h * tw / w).clamp(1, max_h.max(1));
    let row_bytes = tw.div_ceil(8);
    let mut out = Vec::with_capacity(4 + row_bytes * th);
    out.extend_from_slice(&(tw as u16).to_le_bytes());
    out.extend_from_slice(&(th as u16).to_le_bytes());
    for ty in 0..th {
        let y0 = ty * h / th;
        let y1 = ((ty + 1) * h / th).max(y0 + 1);
        let mut rowb = vec![0u8; row_bytes];
        for tx in 0..tw {
            let x0 = tx * w / tw;
            let x1 = ((tx + 1) * w / tw).max(x0 + 1);
            let mut sum = 0u32;
            for y in y0..y1 {
                for x in x0..x1 {
                    sum += gray[y * w + x] as u32;
                }
            }
            let g = sum / ((y1 - y0) * (x1 - x0)) as u32;
            if g < BAYER[ty & 3][tx & 3] as u32 * 16 + 8 {
                rowb[tx / 8] |= 0x80 >> (tx % 8);
            }
        }
        out.extend_from_slice(&rowb);
    }
    Some(out)
}
