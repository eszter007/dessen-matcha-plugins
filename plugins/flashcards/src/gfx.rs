//! Drawing with matcha's own fonts: text is composed into 1-bit canvases in app memory and drawn
//! as bitmaps, so CJK and Latin look as they do in matcha whatever fonts the firmware has.
use crate::dsn;
use alloc::{string::String, vec, vec::Vec};

/// A 1-bit image, set bit = ink, 32 pixels to a word (pixel x is bit 31 - x % 32).
pub struct Canvas {
    pub w: i32,
    pub h: i32,
    pub stride: usize,
    pub b: Vec<u32>,
}

impl Canvas {
    pub fn new(w: i32, h: i32) -> Canvas {
        let w = w.max(1);
        let h = h.max(1);
        let stride = ((w + 31) / 32) as usize;
        Canvas { w, h, stride, b: vec![0; stride * h as usize] }
    }
    pub fn set(&mut self, x: i32, y: i32) {
        if x >= 0 && y >= 0 && x < self.w && y < self.h {
            self.b[y as usize * self.stride + (x >> 5) as usize] |= 0x8000_0000 >> (x & 31);
        }
    }
    /// ORs up to 24 bits starting at bit `k` of a glyph bitmap into row y at x.
    fn or_bits(&mut self, x: i32, y: i32, bits: &[u8], k: usize, n: usize) {
        if y < 0 || y >= self.h || n == 0 || x <= -32 || x >= self.w {
            return;
        }
        let byte = |i: usize| -> u32 { *bits.get(i).unwrap_or(&0) as u32 };
        let p = k >> 3;
        let mut v = (byte(p) << 24 | byte(p + 1) << 16 | byte(p + 2) << 8 | byte(p + 3)) << (k & 7);
        v &= !0u32 << (32 - n);
        let mut x = x;
        if x < 0 {
            v <<= -x;
            x = 0;
        }
        let row = y as usize * self.stride;
        let i = (x >> 5) as usize;
        let o = (x & 31) as u32;
        if i < self.stride {
            self.b[row + i] |= v >> o;
        }
        if o != 0 && i + 1 < self.stride {
            self.b[row + i + 1] |= v << (32 - o);
        }
    }
}

/// Draws a canvas's ink in `color`, cropped to the screen (the firmware refuses a bitmap that leaves
/// it). dsn_bitmap takes bytes MSB first and its mask draws clear bits, so rows go out inverted.
pub fn blit(c: &Canvas, x: i32, y: i32, color: u32) {
    let (sw, sh) = crate::ui::screen();
    let x0 = x.max(0);
    let y0 = y.max(0);
    let x1 = (x + c.w).min(sw);
    let y1 = (y + c.h).min(sh);
    let w = x1 - x0;
    let h = y1 - y0;
    if w <= 0 || h <= 0 {
        return;
    }
    let bs = ((w + 7) / 8) as usize;
    let mut out = vec![0u8; bs * h as usize];
    let off = ((x0 - x) & 31) as u32;
    let k0 = ((x0 - x) >> 5) as usize;
    for r in 0..h as usize {
        let src = &c.b[(r + (y0 - y) as usize) * c.stride + k0..(r + (y0 - y) as usize + 1) * c.stride];
        let o = &mut out[r * bs..(r + 1) * bs];
        let mut i = 0;
        let mut wi = 0;
        while i < bs {
            let mut v = src[wi] << off;
            if off != 0 && wi + 1 < src.len() {
                v |= src[wi + 1] >> (32 - off);
            }
            let v = !v;
            for shift in [24, 16, 8, 0] {
                if i < bs {
                    o[i] = (v >> shift) as u8;
                    i += 1;
                }
            }
            wi += 1;
        }
    }
    dsn::bitmap(x0, y0, w, h, &out, dsn::BITMAP_MASK, color);
}

// ---------------------------------------------------------------- fonts
// fonts/<face>/<block>.bin from tools/matcha_fonts.py: u16 count, count x {u16 low, u8 w, u8 h,
// u16 advance (12.4), i16 left, i16 top, u32 offset}, then 1-bit bitmaps.
const FACE_DIR: [&str; 14] = ["s", "r10", "b10", "r12", "b12", "j8", "j12", "c8", "c12", "n14", "n14b", "k14", "hero", "n18b"];
const FACE_ASC: [i32; 14] = [16, 20, 20, 24, 24, 20, 29, 20, 29, 32, 32, 34, 101, 41];
const S: u8 = 0;
const R10: u8 = 1;
const B10: u8 = 2;
const R12: u8 = 3;
const B12: u8 = 4;
const J8: u8 = 5;
const J12: u8 = 6;
const C8: u8 = 7;
const C12: u8 = 8;
const N14: u8 = 9;
const N14B: u8 = 10;
const K14: u8 = 11;
const HERO: u8 = 12;
const N18B: u8 = 13;

/// A text style: its face, the faces it falls back to, and whether borrowed glyphs are thickened.
#[derive(Clone, Copy)]
pub struct Style {
    face: u8,
    fallback: [u8; 2],
    bold: bool,
    pub line_h: i32,
}
// matcha's SMALL, UI_10, UI_12 (regular and bold), the reader's serif for card text, and the
// NotoSerif 18 of the numeral and of "All caught up".
pub const SMALL: Style = Style { face: S, fallback: [J8, C8], bold: false, line_h: 19 };
pub const UI10: Style = Style { face: R10, fallback: [J8, C8], bold: false, line_h: 24 };
pub const UI10B: Style = Style { face: B10, fallback: [J8, C8], bold: true, line_h: 24 };
pub const UI12: Style = Style { face: R12, fallback: [J12, C12], bold: false, line_h: 29 };
pub const UI12B: Style = Style { face: B12, fallback: [J12, C12], bold: true, line_h: 29 };
pub const CARD: Style = Style { face: N14, fallback: [K14, J12], bold: false, line_h: 40 };
pub const CARDB: Style = Style { face: N14B, fallback: [K14, J12], bold: true, line_h: 40 };
pub const HERO_NUM: Style = Style { face: HERO, fallback: [HERO, HERO], bold: false, line_h: 128 };
pub const SERIF18B: Style = Style { face: N18B, fallback: [J12, C12], bold: true, line_h: 51 };

struct Block {
    face: u8,
    blk: u8,
    data: Option<Vec<u8>>,
}
static mut BLOCKS: Vec<Block> = Vec::new();

fn block(face: u8, blk: u8) -> Option<&'static [u8]> {
    // Single-threaded app: the cache is only touched from here.
    let blocks = unsafe { &mut *core::ptr::addr_of_mut!(BLOCKS) };
    if let Some(b) = blocks.iter().find(|b| b.face == face && b.blk == blk) {
        return b.data.as_deref().map(|d| unsafe { core::slice::from_raw_parts(d.as_ptr(), d.len()) });
    }
    let path = crate::store::path(&alloc::format!("fonts/{}/{:02x}.bin", FACE_DIR[face as usize], blk));
    let data = crate::store::read(&path).filter(|d| d.len() > 2);
    blocks.push(Block { face, blk, data });
    blocks.last().unwrap().data.as_deref().map(|d| unsafe { core::slice::from_raw_parts(d.as_ptr(), d.len()) })
}

struct Glyph {
    w: usize,
    h: usize,
    adv: i32,
    left: i32,
    top: i32,
    bits: &'static [u8],
}
fn glyph_in(face: u8, cp: u32) -> Option<Glyph> {
    if cp > 0xFFFF {
        return None;
    }
    let b = block(face, (cp >> 8) as u8)?;
    let n = u16::from_le_bytes([b[0], b[1]]) as usize;
    let want = (cp & 0xff) as u16;
    let (mut lo, mut hi) = (0isize, n as isize - 1);
    while lo <= hi {
        let mid = ((lo + hi) / 2) as usize;
        let e = &b[2 + mid * 14..2 + mid * 14 + 14];
        let have = u16::from_le_bytes([e[0], e[1]]);
        if have < want {
            lo = mid as isize + 1;
        } else if have > want {
            hi = mid as isize - 1;
        } else {
            let off = 2 + n * 14 + u32::from_le_bytes([e[10], e[11], e[12], e[13]]) as usize;
            return Some(Glyph {
                w: e[2] as usize,
                h: e[3] as usize,
                adv: u16::from_le_bytes([e[4], e[5]]) as i32,
                left: i16::from_le_bytes([e[6], e[7]]) as i32,
                top: i16::from_le_bytes([e[8], e[9]]) as i32,
                bits: &b[off.min(b.len())..],
            });
        }
    }
    None
}
/// The glyph for `c`, and whether a fallback face supplied it.
fn glyph(st: Style, c: char) -> Option<(Glyph, bool)> {
    let cp = c as u32;
    if let Some(g) = glyph_in(st.face, cp) {
        return Some((g, false));
    }
    st.fallback.iter().find_map(|&f| glyph_in(f, cp)).map(|g| (g, true))
}

pub fn text_width(st: Style, s: &str) -> i32 {
    let mut acc = 0;
    let mut extra = 0;
    for c in s.chars() {
        if let Some((g, fb)) = glyph(st, c) {
            acc += g.adv;
            extra = (fb && st.bold) as i32;
        }
    }
    ((acc + 8) >> 4) + extra
}

/// Draws one line into a canvas with its top at y; bold styles thicken borrowed CJK glyphs by a pixel.
pub fn draw_text(c: &mut Canvas, x: i32, y: i32, st: Style, s: &str) {
    let mut acc = 0;
    for ch in s.chars() {
        let Some((g, fb)) = glyph(st, ch) else { continue };
        let gx = x + ((acc + 8) >> 4) + g.left;
        let gy = y + FACE_ASC[st.face as usize] - g.top;
        for yy in 0..g.h {
            let mut part = 0;
            while part < g.w {
                let n = (g.w - part).min(24);
                let k = yy * g.w + part;
                c.or_bits(gx + part as i32, gy + yy as i32, g.bits, k, n);
                if fb && st.bold {
                    c.or_bits(gx + part as i32 + 1, gy + yy as i32, g.bits, k, n);
                }
                part += 24;
            }
        }
        acc += g.adv;
    }
}

/// One line of text straight to the screen, in `color`.
pub fn text(x: i32, y: i32, st: Style, s: &str, color: u32) {
    if s.is_empty() {
        return;
    }
    let mut c = Canvas::new(text_width(st, s) + 4, st.line_h + 12);
    draw_text(&mut c, 0, 0, st, s);
    blit(&c, x, y, color);
}
pub fn text_centered(cx: i32, y: i32, st: Style, s: &str, color: u32) {
    text(cx - text_width(st, s) / 2, y, st, s, color);
}

/// `s` cut to fit `max_w`, ending in "…" when it had to be cut.
pub fn truncated(st: Style, s: &str, max_w: i32) -> String {
    if text_width(st, s) <= max_w {
        return String::from(s);
    }
    let mut out = String::from(s);
    while !out.is_empty() {
        out.pop();
        let t = String::from(out.trim_end()) + "…";
        if text_width(st, &t) <= max_w {
            return t;
        }
    }
    String::from("…")
}

fn is_wide(c: char) -> bool { c as u32 >= 0x2E80 }
// Closing punctuation stays on the line before it (kinsoku), as matcha's card text does.
fn no_line_start(c: char) -> bool { "、。，．！？）」』】〉》〕・ー…‥ぁぃぅぇぉっゃゅょァィゥェォッャュョ,.!?;:)]}".contains(c) }

/// Splits text into lines of at most `max_w`: Latin breaks at spaces, CJK between characters.
pub fn wrap(st: Style, s: &str, max_w: i32) -> Vec<String> {
    let mut lines = Vec::new();
    for para in s.split('\n') {
        let mut line = String::new();
        let mut word = String::new();
        let flush_word = |line: &mut String, word: &mut String, lines: &mut Vec<String>| {
            if word.is_empty() {
                return;
            }
            let candidate = alloc::format!("{}{}", line, word);
            if line.is_empty() || text_width(st, candidate.trim_end()) <= max_w {
                *line = candidate;
            } else {
                lines.push(String::from(line.trim_end()));
                *line = String::from(word.trim_start());
            }
            // A single word wider than the line is cut where it fills it.
            while text_width(st, line.trim_end()) > max_w && line.chars().count() > 1 {
                let mut head = String::new();
                for c in line.chars() {
                    let t = alloc::format!("{}{}", head, c);
                    if text_width(st, &t) > max_w && !head.is_empty() {
                        break;
                    }
                    head = t;
                }
                let rest = String::from(&line[head.len()..]);
                lines.push(head);
                *line = rest;
            }
            word.clear();
        };
        let mut prev: Option<char> = None;
        for c in para.chars() {
            if c == ' ' {
                word.push(c);
                flush_word(&mut line, &mut word, &mut lines);
            } else if (is_wide(c) && !no_line_start(c)) || prev.is_some_and(is_wide) && !no_line_start(c) {
                flush_word(&mut line, &mut word, &mut lines);
                word.push(c);
            } else {
                word.push(c);
            }
            prev = Some(c);
        }
        flush_word(&mut line, &mut word, &mut lines);
        lines.push(String::from(line.trim_end()));
    }
    lines
}

/// A 24 or 32 px icon (row-major, MSB first, 0 = ink) straight to the screen.
pub fn icon(x: i32, y: i32, size: i32, bits: &[u8], color: u32) {
    dsn::bitmap(x, y, size, size, bits, dsn::BITMAP_MASK, color);
}
