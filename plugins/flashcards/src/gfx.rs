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
    pub fn get(&self, x: i32, y: i32) -> bool {
        x >= 0 && y >= 0 && x < self.w && y < self.h && self.b[y as usize * self.stride + (x >> 5) as usize] & (0x8000_0000 >> (x & 31)) != 0
    }
    /// The ink grown a pixel to the right: matcha's bold for a face that has no bold cut.
    pub fn embolden(&mut self) {
        for v in self.b.chunks_mut(self.stride) {
            let mut carry = 0u32;
            for w in v.iter_mut() {
                let o = *w;
                *w |= o >> 1 | carry;
                carry = o << 31;
            }
        }
    }
    /// Turned a quarter counter-clockwise: the top row becomes the left column, read bottom up.
    pub fn turned_ccw(&self) -> Canvas {
        let mut out = Canvas::new(self.h, self.w);
        for y in 0..self.h {
            for x in 0..self.w {
                if self.get(x, y) {
                    out.set(y, self.w - 1 - x);
                }
            }
        }
        out
    }
    /// The smallest box holding all ink, as (x, y, w, h); None for a blank canvas.
    pub fn ink_box(&self) -> Option<(i32, i32, i32, i32)> {
        let (mut x0, mut y0, mut x1, mut y1) = (self.w, self.h, -1, -1);
        for y in 0..self.h {
            for x in 0..self.w {
                if self.get(x, y) {
                    x0 = x0.min(x);
                    x1 = x1.max(x);
                    y0 = y0.min(y);
                    y1 = y1.max(y);
                }
            }
        }
        (x1 >= 0).then(|| (x0, y0, x1 - x0 + 1, y1 - y0 + 1))
    }
    pub fn fill_disk(&mut self, cx: i32, cy: i32, r: i32) {
        for dy in -r..=r {
            let mut dx = 0;
            while (dx + 1) * (dx + 1) + dy * dy <= r * r {
                dx += 1;
            }
            for x in cx - dx..=cx + dx {
                self.set(x, cy + dy);
            }
        }
    }
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
const FACE_DIR: [&str; 53] = [
    "s", "r10", "b10", "r12", "b12", "j8", "j12", "c8", "c12", "n14", "n14b", "k14", "hero", "n18b", "n12", "n12b", "n12i", "sp", "spk",
    "spmj", "spkj", "spkt", "spkk", "k14j", "k14t", "k14k", "sym",
    "n14i", "n18", "n24", "n24b", "k20", "k20j", "k20t", "k20k",
    "ar8", "ar10", "ar10b", "ar12", "ar12b", "ar14", "ar14b", "ar18", "ar24", "he12", "he14", "he14b", "he18", "he24",
    "k32", "k32j", "k32t", "k32k",
];
const FACE_ASC: [i32; 53] = [16, 20, 20, 24, 24, 20, 29, 20, 29, 32, 32, 34, 101, 41, 27, 27, 27, 89, 87, 67, 87, 87, 87, 34, 34, 34, 32,
    32, 41, 54, 54, 49, 49, 49, 49,
    24, 29, 29, 35, 35, 40, 40, 53, 69, 27, 31, 31, 41, 54,
    78, 78, 78, 78];
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
const N12: u8 = 14;
const N12B: u8 = 15;
const N12I: u8 = 16;
const SP: u8 = 17;
const SPK: u8 = 18;
const SPMJ: u8 = 19;
const SPKJ: u8 = 20;
const SPKT: u8 = 21;
const SPKK: u8 = 22;
const K14J: u8 = 23;
const K14T: u8 = 24;
const K14K: u8 = 25;
const SYM: u8 = 26;
#[allow(dead_code)]
const N14I: u8 = 27;
#[allow(dead_code)]
const N18: u8 = 28;
#[allow(dead_code)]
const N24: u8 = 29;
#[allow(dead_code)]
const N24B: u8 = 30;
const K20: u8 = 31;
const K20J: u8 = 32;
const K20T: u8 = 33;
const K20K: u8 = 34;
const AR8: u8 = 35;
const AR10: u8 = 36;
const AR10B: u8 = 37;
const AR12: u8 = 38;
const AR12B: u8 = 39;
const AR14: u8 = 40;
const AR14B: u8 = 41;
const AR18: u8 = 42;
const AR24: u8 = 43;
const HE12: u8 = 44;
const HE14: u8 = 45;
const HE14B: u8 = 46;
const HE18: u8 = 47;
const HE24: u8 = 48;
const K32: u8 = 49;
const K32J: u8 = 50;
const K32T: u8 = 51;
const K32K: u8 = 52;

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
// The spine card's smaller reader sizes: the back's senses and sentence, the book's title (italic).
pub const SERIF12: Style = Style { face: N12, fallback: [J12, C12], bold: false, line_h: 34 };
pub const SERIF12B: Style = Style { face: N12B, fallback: [J12, C12], bold: true, line_h: 34 };
pub const SERIF12I: Style = Style { face: N12I, fallback: [J12, C12], bold: false, line_h: 34 };
// The CJK face alone, for glyphs that are measured and placed one by one.
pub const CJK14: Style = Style { face: K14, fallback: [J12, C12], bold: false, line_h: 40 };
// The spine's word: Noto Serif Bold at 40 pt, and Noto Serif CJK Bold at 36 pt (big blocks, read
// only for the word and let go after it with forget_spine).
pub const SPINE: Style = Style { face: SP, fallback: [SPK, SPK], bold: true, line_h: 107 };
/// A card's language, read from its text: CJK faces come in a cut per language, as the shapes of
/// many characters differ between them.
#[derive(Clone, Copy, PartialEq)]
pub enum Script {
    Sc,
    Tc,
    Jp,
    Kr,
}
fn body_cjk(sc: Script) -> u8 {
    match sc { Script::Sc => K14, Script::Tc => K14T, Script::Jp => K14J, Script::Kr => K14K }
}
/// `st` with the language's CJK face in front of its fallbacks (or as its face, for a CJK style).
pub fn for_script(st: Style, sc: Script) -> Style {
    let f = body_cjk(sc);
    if st.face == K14 {
        return Style { face: f, ..st };
    }
    Style { fallback: [f, st.fallback[0]], ..st }
}
/// The spine's large CJK face for the language; `ruby` is the smaller cut beside a Japanese reading.
pub fn spine_cjk(sc: Script, ruby: bool) -> Style {
    let face = match sc {
        Script::Jp if ruby => SPMJ,
        Script::Jp => SPKJ,
        Script::Tc => SPKT,
        Script::Kr => SPKK,
        Script::Sc => SPK,
    };
    Style { face, fallback: [face, face], bold: true, line_h: if face == SPMJ { 81 } else { 104 } }
}
/// Lets go of the spine faces' big blocks once the word is drawn.
pub fn forget_spine() {
    let blocks = unsafe { &mut *core::ptr::addr_of_mut!(BLOCKS) };
    blocks.retain(|b| !matches!(b.face, SPK | SPMJ | SPKJ | SPKT | SPKK | K32 | K32J | K32T | K32K));
}
/// Whether `c` is an ideograph only Traditional Chinese uses (fonts/tc.bin: Big5, not GB2312).
pub fn traditional_only(c: char) -> bool {
    let cp = c as u32;
    if !(0x4E00..0xA000).contains(&cp) {
        return false;
    }
    let Some(b) = block_file("fonts/tc.bin") else { return false };
    b.get(((cp - 0x4E00) / 8) as usize).is_some_and(|v| v & (0x80 >> ((cp - 0x4E00) % 8)) != 0)
}
fn block_file(name: &str) -> Option<&'static [u8]> {
    static mut TC: Option<Vec<u8>> = None;
    let tc = unsafe { &mut *core::ptr::addr_of_mut!(TC) };
    if tc.is_none() {
        *tc = Some(crate::store::read(&crate::store::path(name)).unwrap_or_default());
    }
    tc.as_deref().filter(|d| !d.is_empty()).map(|d| unsafe { core::slice::from_raw_parts(d.as_ptr(), d.len()) })
}
pub const CJK12: Style = Style { face: J12, fallback: [C12, C12], bold: false, line_h: 34 };

struct Block {
    face: u8,
    blk: u8,
    data: Option<Vec<u8>>,
}
static mut BLOCKS: Vec<Block> = Vec::new();
const BLOCKS_MAX: usize = 256 * 1024;
/// Lets go of the loaded glyph blocks once they pass BLOCKS_MAX (they are read again when
/// needed). Only between cards and screens: a glyph drawn earlier points into its block.
pub fn trim_glyph_cache() {
    let blocks = unsafe { &mut *core::ptr::addr_of_mut!(BLOCKS) };
    let size: usize = blocks.iter().map(|b| b.data.as_ref().map_or(0, |d| d.len())).sum();
    if size > BLOCKS_MAX {
        *blocks = Vec::new();
    }
}

fn block(face: u8, blk: u8) -> Option<&'static [u8]> {
    // Single-threaded app: the cache is only touched from here.
    let blocks = unsafe { &mut *core::ptr::addr_of_mut!(BLOCKS) };
    if let Some(b) = blocks.iter().find(|b| b.face == face && b.blk == blk) {
        return b.data.as_deref().map(|d| unsafe { core::slice::from_raw_parts(d.as_ptr(), d.len()) });
    }
    let path = crate::store::path(&alloc::format!("fonts/{}/{:02x}.bin", face_dir(face), blk));
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
    // The Simplified Chinese face stands for the CJK face of the text's language (set_script).
    let lang = |f: u8| match f {
        K14 => body_cjk(unsafe { SCRIPT }),
        K20 => match unsafe { SCRIPT } { Script::Sc => K20, Script::Tc => K20T, Script::Jp => K20J, Script::Kr => K20K },
        K32 => match unsafe { SCRIPT } { Script::Sc => K32, Script::Tc => K32T, Script::Jp => K32J, Script::Kr => K32K },
        SPK => match unsafe { SCRIPT } { Script::Sc => SPK, Script::Tc => SPKT, Script::Jp => SPKJ, Script::Kr => SPKK },
        _ => f,
    };
    if let Some(g) = glyph_in(lang(st.face), cp) {
        return Some((g, false));
    }
    if let Some(g) = st.fallback.iter().find_map(|&f| glyph_in(lang(f), cp)) {
        return Some((g, true));
    }
    // Hangul where a style has no Korean face, then matcha's symbols (IPA, stars).
    if (0x1100..0x1200).contains(&cp) || (0x3130..0x3190).contains(&cp) || (0xAC00..0xD7A4).contains(&cp) {
        return glyph_in(K14K, cp).map(|g| (g, true));
    }
    // Arabic and Hebrew: Noto Sans Arabic / Hebrew at about the style's size.
    let h = st.line_h;
    if (0x0600..0x0780).contains(&cp) || (0xFB50..0xFE00).contains(&cp) || (0xFE70..0xFF00).contains(&cp) {
        let f = match h {
            ..=20 => AR8,
            21..=26 => if st.bold { AR10B } else { AR10 },
            27..=36 => if st.bold { AR12B } else { AR12 },
            37..=44 => if st.bold { AR14B } else { AR14 },
            45..=60 => AR18,
            _ => AR24,
        };
        return glyph_in(f, cp).map(|g| (g, false));
    }
    if (0x0590..0x0600).contains(&cp) || (0xFB1D..0xFB50).contains(&cp) {
        let f = match h {
            ..=36 => HE12,
            37..=44 => if st.bold { HE14B } else { HE14 },
            45..=60 => HE18,
            _ => HE24,
        };
        return glyph_in(f, cp).map(|g| (g, false));
    }
    glyph_in(SYM, cp).map(|g| (g, true))
}

static mut SCRIPT: Script = Script::Sc;
/// Sets the language whose CJK cut text is drawn in, until set again.
pub fn set_script(sc: Script) { unsafe { SCRIPT = sc } }
/// The language of a text: kana mean Japanese, Hangul Korean, an ideograph only Traditional
/// Chinese uses Traditional, else Simplified (cards and decks carry no language of their own).
pub fn detect_script<'a>(texts: impl Iterator<Item = &'a str> + Clone) -> Script {
    let any = |f: &dyn Fn(char) -> bool| texts.clone().any(|t| t.chars().any(f));
    let cp = |c: char| c as u32;
    if any(&|c| (0x3041..=0x30FF).contains(&cp(c))) {
        Script::Jp
    } else if any(&|c| (0xAC00..=0xD7A3).contains(&cp(c)) || (0x3130..=0x318F).contains(&cp(c))) {
        Script::Kr
    } else if any(&|c| traditional_only(c)) {
        Script::Tc
    } else {
        Script::Sc
    }
}

/// The face's ascender: how far below a line's top its baseline sits.
/// The first face of the card renderer's table (cardfaces.rs), after the built-in ones.
pub const CARD_FACE0: u8 = 53;
fn face_dir(face: u8) -> &'static str {
    if face >= CARD_FACE0 { crate::cardfaces::FACES[(face - CARD_FACE0) as usize].0 } else { FACE_DIR[face as usize] }
}
fn face_asc(face: u8) -> i32 {
    if face >= CARD_FACE0 { crate::cardfaces::FACES[(face - CARD_FACE0) as usize].1 } else { FACE_ASC[face as usize] }
}
/// A card renderer style: family 0 serif / 1 sans, size index 0-7, bold, italic; CJK text falls
/// back to the language's CJK face of about that size.
pub fn card_style(family: usize, size: usize, bold: bool, italic: bool) -> Style {
    let i = (family.min(1) * 8 + size.min(7)) * 4 + if bold && italic { 3 } else if italic { 2 } else if bold { 1 } else { 0 };
    let (_, _, line_h) = crate::cardfaces::FACES[i];
    // Large CJK (a word over its sentence): the 32 pt sans, or the 36 pt serif when bold.
    let cjk = match size { 0..=2 => K14, 3..=5 => K20, _ => if bold { SPK } else { K32 } };
    Style { face: CARD_FACE0 + i as u8, fallback: [cjk, K14], bold, line_h }
}
pub fn same_face(a: Style, b: Style) -> bool { a.face == b.face }
pub fn ascent(st: Style) -> i32 { face_asc(st.face) }

pub fn text_width(st: Style, s: &str) -> i32 {
    // Arabic measures in its joined forms, as it is drawn.
    if s.chars().any(|c| (0x0600..0x0780).contains(&(c as u32))) {
        return text_width_raw(st, &crate::rtl::visual(s));
    }
    text_width_raw(st, s)
}
fn text_width_raw(st: Style, s: &str) -> i32 {
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
        let gy = y + face_asc(st.face) - g.top;
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
    // Hebrew, Arabic, Persian: shaped and put in the order they are drawn in.
    let visual = crate::rtl::visual(s);
    let s = visual.as_str();
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
