//! Right-to-left text: UAX#9 bidi reordering and Arabic contextual shaping,
//! ported from matcha-reader's MiniBidi (lib/MiniBidi: minibidi.c, BidiUtils.cpp).
//!
//! MiniBidi is derived from minibidi.c in mintty (https://github.com/mintty/mintty),
//! original author Ahmad Khalifa (www.arabeyes.org), mintty changes by Thomas Wolff,
//! MIT licence; see matcha-reader licenses/borrowed-code.txt. The class and
//! mirror tables are matcha's bidiclasses.t / bidi_pairs.t (Unicode data, Unicode
//! licence), presentation forms from Unicode ArabicShaping.txt / UnicodeData.txt.
//!
//! Differences from matcha:
//! - no BIDI_MAX_LINE limit: working buffers are Vecs, so lines of any length work
//!   (embedding depth is capped at 125 as in UAX#9, the C code had no cap).
//! - the plugin's bold markers \u{1} (on) and \u{2} (off) are removed before the
//!   algorithm runs; each character remembers whether it was bold, and after
//!   reordering the markers are emitted again wherever the bold state changes in
//!   visual order. A bold word therefore stays bold after it is reversed, and a
//!   line that starts or ends inside bold keeps that state at its edges (a line
//!   whose first marker is \u{2} is taken to start bold).
//! - like matcha, do_bidi runs before do_shape (shaping reads visual neighbours,
//!   which gives the same forms as shaping in logical order); the output drops
//!   the Lam-Alef placeholder and ZWJ/ZWNJ, puts RTL combining marks after their
//!   base, and writes the Hebrew divine name YHWH as 'ה, as BidiUtils does.

use alloc::{string::String, vec::Vec};

// Bidi classes (UAX#9)
const L: u8 = 0;
const LRE: u8 = 1;
const LRO: u8 = 2;
const R: u8 = 3;
const AL: u8 = 4;
const RLE: u8 = 5;
const RLO: u8 = 6;
const PDF: u8 = 7;
const EN: u8 = 8;
const ES: u8 = 9;
const ET: u8 = 10;
const AN: u8 = 11;
const CS: u8 = 12;
const NSM: u8 = 13;
const BN: u8 = 14;
const B: u8 = 15;
const S: u8 = 16;
const WS: u8 = 17;
const ON: u8 = 18;
const LRI: u8 = 19;
const RLI: u8 = 20;
const FSI: u8 = 21;
const PDI: u8 = 22;

static CLASSES: &[(u32, u32, u8)] = &[
    (0x0000, 0x0008, BN),
    (0x0009, 0x0009, S),
    (0x000A, 0x000A, B),
    (0x000B, 0x000B, S),
    (0x000C, 0x000C, WS),
    (0x000D, 0x000D, B),
    (0x000E, 0x001B, BN),
    (0x001C, 0x001E, B),
    (0x001F, 0x001F, S),
    (0x0020, 0x0020, WS),
    (0x0023, 0x0025, ET),
    (0x002B, 0x002B, ES),
    (0x002C, 0x002C, CS),
    (0x002D, 0x002D, ES),
    (0x002E, 0x002F, CS),
    (0x0030, 0x0039, EN),
    (0x003A, 0x003A, CS),
    (0x0041, 0x005A, L),
    (0x0061, 0x007A, L),
    (0x007F, 0x0084, BN),
    (0x0085, 0x0085, B),
    (0x0086, 0x009F, BN),
    (0x00A0, 0x00A0, CS),
    (0x00A2, 0x00A5, ET),
    (0x00AA, 0x00AA, L),
    (0x00AD, 0x00AD, BN),
    (0x00B0, 0x00B1, ET),
    (0x00B2, 0x00B3, EN),
    (0x00B5, 0x00B5, L),
    (0x00B9, 0x00B9, EN),
    (0x00BA, 0x00BA, L),
    (0x00C0, 0x00D6, L),
    (0x00D8, 0x00F6, L),
    (0x00F8, 0x02B8, L),
    (0x0300, 0x036F, NSM),
    (0x0400, 0x04FF, L),
    (0x0500, 0x052F, L),
    (0x0591, 0x05A1, NSM),
    (0x05A3, 0x05B9, NSM),
    (0x05BB, 0x05BD, NSM),
    (0x05BE, 0x05BE, R),
    (0x05BF, 0x05BF, NSM),
    (0x05C0, 0x05C0, R),
    (0x05C1, 0x05C2, NSM),
    (0x05C3, 0x05C3, R),
    (0x05C4, 0x05C4, NSM),
    (0x05D0, 0x05EA, R),
    (0x05F0, 0x05F4, R),
    (0x0600, 0x0605, AN),
    (0x0608, 0x0608, AL),
    (0x0609, 0x060A, ET),
    (0x060B, 0x060B, AL),
    (0x060C, 0x060C, CS),
    (0x060D, 0x060D, AL),
    (0x0610, 0x061A, NSM),
    (0x061B, 0x061F, AL),
    (0x0620, 0x064A, AL),
    (0x064B, 0x065F, NSM),
    (0x0660, 0x0669, AN),
    (0x066A, 0x066A, ET),
    (0x066B, 0x066C, AN),
    (0x066D, 0x066F, AL),
    (0x0670, 0x0670, NSM),
    (0x0671, 0x06D5, AL),
    (0x06D6, 0x06DC, NSM),
    (0x06DD, 0x06DD, AN),
    (0x06DF, 0x06E4, NSM),
    (0x06E5, 0x06E6, AL),
    (0x06E7, 0x06E8, NSM),
    (0x06EA, 0x06ED, NSM),
    (0x06EE, 0x06EF, AL),
    (0x06F0, 0x06F9, EN),
    (0x06FA, 0x06FF, AL),
    (0x1E00, 0x1EFF, L),
    (0x200B, 0x200D, BN),
    (0x200E, 0x200E, L),
    (0x200F, 0x200F, R),
    (0x2028, 0x2028, WS),
    (0x2029, 0x2029, B),
    (0x202A, 0x202A, LRE),
    (0x202B, 0x202B, RLE),
    (0x202C, 0x202C, PDF),
    (0x202D, 0x202D, LRO),
    (0x202E, 0x202E, RLO),
    (0x202F, 0x202F, WS),
    (0x2060, 0x2063, BN),
    (0x2066, 0x2066, LRI),
    (0x2067, 0x2067, RLI),
    (0x2068, 0x2068, FSI),
    (0x2069, 0x2069, PDI),
    (0x206A, 0x206F, BN),
    (0xFB50, 0xFBC2, AL),
    (0xFBD3, 0xFBFF, AL),
    (0xFE70, 0xFE74, AL),
    (0xFE76, 0xFEFC, AL),
    (0xFEFF, 0xFEFF, BN),
];

const BRACKX: u8 = 0;
const BRACKO: u8 = 1;
const BRACKC: u8 = 2;

static PAIRS: &[(u32, u32, u8)] = &[
    (0x0028, 0x0029, BRACKO),
    (0x0029, 0x0028, BRACKC),
    (0x003C, 0x003E, BRACKO),
    (0x003E, 0x003C, BRACKC),
    (0x005B, 0x005D, BRACKO),
    (0x005D, 0x005B, BRACKC),
    (0x007B, 0x007D, BRACKO),
    (0x007D, 0x007B, BRACKC),
    (0x00AB, 0x00BB, BRACKX),
    (0x00BB, 0x00AB, BRACKX),
    (0x2018, 0x2019, BRACKX),
    (0x2019, 0x2018, BRACKX),
    (0x201C, 0x201D, BRACKX),
    (0x201D, 0x201C, BRACKX),
    (0x2039, 0x203A, BRACKO),
    (0x203A, 0x2039, BRACKC),
];

const LIG_PLACEHOLDER: u32 = 0xFFFF;
const ZWNJ: u8 = 0x01;
const ZWJ: u8 = 0x02;
const MAX_DEPTH: u8 = 125;

fn bidi_class(ch: u32) -> u8 {
    match CLASSES.binary_search_by(|&(a, b, _)| {
        if ch < a {
            core::cmp::Ordering::Greater
        } else if ch > b {
            core::cmp::Ordering::Less
        } else {
            core::cmp::Ordering::Equal
        }
    }) {
        Ok(i) => CLASSES[i].2,
        Err(_) => ON,
    }
}

fn find_pair(c: u32) -> Option<&'static (u32, u32, u8)> {
    PAIRS.binary_search_by_key(&c, |p| p.0).ok().map(|i| &PAIRS[i])
}

/// 0: not a bracket, c: opening bracket, opener: closing bracket.
fn bracket(c: u32) -> u32 {
    match find_pair(c) {
        Some(&(_, _, t)) if t == BRACKO => c,
        Some(&(_, to, t)) if t == BRACKC => to,
        _ => 0,
    }
}

fn mirror(c: u32) -> u32 {
    find_pair(c).map(|p| p.1).unwrap_or(c)
}

fn is_ni(bc: u8) -> bool {
    matches!(bc, B | S | WS | ON | FSI | LRI | RLI | PDI)
}

/// Any Hebrew or Arabic-script letter (strong R or AL).
pub fn has_rtl(s: &str) -> bool {
    s.chars().any(|c| matches!(bidi_class(c as u32), R | AL))
}

/// Base direction of a text: true when its first strong character is R or AL
/// (BidiUtils::detectParagraphLevel without the 64-character probe limit).
pub fn paragraph_rtl(s: &str) -> bool {
    for c in s.chars() {
        match bidi_class(c as u32) {
            R | AL => return true,
            L => return false,
            _ => {}
        }
    }
    false
}

#[derive(Clone, Copy)]
struct BChar {
    wc: u32,
    index: usize,
    joiners: u8,
    bold: bool,
}

// ── Arabic shaping (mintty do_shape with matcha's extensions) ──────────────

const SL: u8 = 0;
const SR: u8 = 1;
const SD: u8 = 2;
const SU: u8 = 3;
const SC: u8 = 4;

const SHAPE_FIRST: u32 = 0x621;
const SHAPE_LAST: u32 = 0x64A;

static SHAPETYPES: [(u8, u8); 42] = [
    (SU, 0x80), (SR, 0x81), (SR, 0x83), (SR, 0x85), (SR, 0x87), (SD, 0x89), (SR, 0x8D),
    (SD, 0x8F), (SR, 0x93), (SD, 0x95), (SD, 0x99), (SD, 0x9D), (SD, 0xA1), (SD, 0xA5),
    (SR, 0xA9), (SR, 0xAB), (SR, 0xAD), (SR, 0xAF), (SD, 0xB1), (SD, 0xB5), (SD, 0xB9),
    (SD, 0xBD), (SD, 0xC1), (SD, 0xC5), (SD, 0xC9), (SD, 0xCD), (SU, 0), (SU, 0),
    (SU, 0), (SU, 0), (SU, 0), (SC, 0), (SD, 0xD1), (SD, 0xD5), (SD, 0xD9),
    (SD, 0xDD), (SD, 0xE1), (SD, 0xE5), (SD, 0xE9), (SR, 0xED), (SR, 0xEF), (SD, 0xF1),
];

static XJOINTYPES: &[(u32, u32, u8)] = &[
    (0x0620, 0x0620, SD),
    (0x066E, 0x066F, SD),
    (0x0671, 0x0673, SR),
    (0x0675, 0x0677, SR),
    (0x0678, 0x0687, SD),
    (0x0688, 0x0699, SR),
    (0x069A, 0x06BF, SD),
    (0x06C0, 0x06C0, SR),
    (0x06C1, 0x06C2, SD),
    (0x06C3, 0x06CB, SR),
    (0x06CC, 0x06CC, SD),
    (0x06CD, 0x06CD, SR),
    (0x06CE, 0x06CE, SD),
    (0x06CF, 0x06CF, SR),
    (0x06D0, 0x06D1, SD),
    (0x06D2, 0x06D3, SR),
    (0x06D5, 0x06D5, SR),
    (0x06EE, 0x06EF, SR),
    (0x06FA, 0x06FC, SD),
    (0x06FF, 0x06FF, SD),
    (0x200C, 0x200C, SU),
    (0x200D, 0x200D, SC),
];

// (letter, isolated form, number of forms: 2 = isolated+final, 4 = all)
static XSHAPEFORMS: &[(u32, u32, u8)] = &[
    (0x0671, 0xFB50, 2), (0x0679, 0xFB66, 4), (0x067A, 0xFB5E, 4), (0x067B, 0xFB52, 4),
    (0x067E, 0xFB56, 4), (0x067F, 0xFB62, 4), (0x0680, 0xFB5A, 4), (0x0683, 0xFB76, 4),
    (0x0684, 0xFB72, 4), (0x0686, 0xFB7A, 4), (0x0687, 0xFB7E, 4), (0x0688, 0xFB88, 2),
    (0x068C, 0xFB84, 2), (0x068D, 0xFB82, 2), (0x068E, 0xFB86, 2), (0x0691, 0xFB8C, 2),
    (0x0698, 0xFB8A, 2), (0x06A4, 0xFB6A, 4), (0x06A6, 0xFB6E, 4), (0x06A9, 0xFB8E, 4),
    (0x06AD, 0xFBD3, 4), (0x06AF, 0xFB92, 4), (0x06B1, 0xFB9A, 4), (0x06B3, 0xFB96, 4),
    (0x06BA, 0xFB9E, 2), (0x06BB, 0xFBA0, 4), (0x06BE, 0xFBAA, 4), (0x06C0, 0xFBA4, 2),
    (0x06C1, 0xFBA6, 4), (0x06C5, 0xFBE0, 2), (0x06C6, 0xFBD9, 2), (0x06C7, 0xFBD7, 2),
    (0x06C8, 0xFBDB, 2), (0x06C9, 0xFBE2, 2), (0x06CB, 0xFBDE, 2), (0x06CC, 0xFBFC, 4),
    (0x06D0, 0xFBE4, 4), (0x06D2, 0xFBAE, 2), (0x06D3, 0xFBB0, 2),
];

const ISOLATED: u8 = 0;
const FINAL: u8 = 1;
const INITIAL: u8 = 2;
const MEDIAL: u8 = 3;

fn stype(c: u32) -> u8 {
    if (SHAPE_FIRST..=SHAPE_LAST).contains(&c) {
        return SHAPETYPES[(c - SHAPE_FIRST) as usize].0;
    }
    for &(a, b, t) in XJOINTYPES {
        if c < a {
            break;
        }
        if c <= b {
            return t;
        }
    }
    SU
}

fn shape_form(c: u32, form: u8) -> u32 {
    if (SHAPE_FIRST..=SHAPE_LAST).contains(&c) {
        let fb = SHAPETYPES[(c - SHAPE_FIRST) as usize].1 as u32;
        return if fb != 0 { 0xFE00 + fb + form as u32 } else { c };
    }
    match XSHAPEFORMS.binary_search_by_key(&c, |e| e.0) {
        Ok(i) if form < XSHAPEFORMS[i].2 => XSHAPEFORMS[i].1 + form as u32,
        _ => c,
    }
}

fn joins_left(t: u8) -> bool {
    t == SL || t == SD || t == SC
}
fn joins_right(t: u8) -> bool {
    t == SR || t == SD || t == SC
}

/// Shapes a line that is already in visual order (line[i-1] is the left neighbour).
fn do_shape(line: &[BChar]) -> Vec<BChar> {
    let count = line.len();
    let mut to: Vec<BChar> = line.to_vec();
    let next = |i: usize| {
        let mut j = i + 1;
        while j < count && bidi_class(line[j].wc) == NSM {
            j += 1;
        }
        j
    };
    let prev = |i: usize| -> Option<usize> {
        let mut j = i;
        while j > 0 {
            j -= 1;
            if bidi_class(line[j].wc) != NSM {
                return Some(j);
            }
        }
        None
    };
    for i in 0..count {
        let wc = line[i].wc;
        match stype(wc) {
            SR => {
                let nx = next(i);
                let t = if nx < count { stype(line[nx].wc) } else { SU };
                to[i].wc = shape_form(wc, if joins_left(t) { FINAL } else { ISOLATED });
            }
            SD => {
                let nx = next(i);
                let pv = prev(i);
                let mut t = if nx < count { stype(line[nx].wc) } else { SU };
                if wc == 0x644 {
                    let lig = match pv.map(|p| line[p].wc) {
                        Some(0x622) => Some((0xFEF6, 0xFEF5)),
                        Some(0x623) => Some((0xFEF8, 0xFEF7)),
                        Some(0x625) => Some((0xFEFA, 0xFEF9)),
                        Some(0x627) => Some((0xFEFC, 0xFEFB)),
                        _ => None,
                    };
                    if let (Some((joined, alone)), Some(p)) = (lig, pv) {
                        to[i].wc = if joins_left(t) { joined } else { alone };
                        to[p].wc = LIG_PLACEHOLDER;
                        continue;
                    }
                }
                let pt = || pv.map(|p| stype(line[p].wc)).unwrap_or(SU);
                let joiners = line[i].joiners & 0xF;
                let prevjoiners = line[i].joiners >> 4;
                let form = if prevjoiners == ZWNJ {
                    if joins_right(pt()) { INITIAL } else { ISOLATED }
                } else if prevjoiners == (ZWJ | ZWNJ) {
                    MEDIAL
                } else if prevjoiners == ZWJ {
                    FINAL
                } else if joiners & ZWNJ != 0 {
                    if joins_left(t) { FINAL } else { ISOLATED }
                } else if joins_left(t) {
                    t = pt();
                    if joins_right(t) { MEDIAL } else { FINAL }
                } else {
                    t = pt();
                    if joins_right(t) { INITIAL } else { ISOLATED }
                };
                to[i].wc = shape_form(wc, form);
            }
            _ => {}
        }
    }
    to
}

// ── UAX#9 (mintty do_bidi, matcha variant) ─────────────────────────────────

fn flip_runs(line: &mut [BChar], levels: &[u8], tlevel: u8) {
    let count = line.len();
    let (mut i, mut j) = (0usize, 0usize);
    while i < count && j < count {
        let mut s = i;
        while s < count && tlevel > levels[s] {
            s += 1;
        }
        i = s;
        j = s;
        while i < count && tlevel <= levels[i] {
            i += 1;
        }
        let mut k = i;
        while k > 0 && k - 1 > j {
            k -= 1;
            line.swap(k, j);
            j += 1;
        }
    }
}

fn least_greater_odd(x: u8) -> u8 {
    (x + 1) | 1
}
fn least_greater_even(x: u8) -> u8 {
    (x + 2) & !1
}

/// Reorders `line` (logical order) into visual order in place, mirroring
/// characters in RTL runs. Returns the paragraph level.
fn do_bidi(autodir: bool, mut paragraph_level: u8, line: &mut [BChar]) -> u8 {
    let count = line.len();
    if count == 0 {
        return paragraph_level;
    }
    // P2/P3
    let mut isolate_level = 0i32;
    let mut res_level: i32 = -1;
    let mut has_rtl = false;
    for c in line.iter() {
        let t = bidi_class(c.wc);
        if t == LRI || t == RLI || t == FSI {
            has_rtl = true;
            isolate_level += 1;
        } else if t == PDI {
            has_rtl = true;
            if isolate_level > 0 {
                isolate_level -= 1;
            }
        } else if isolate_level == 0 {
            if t == R || t == AL {
                has_rtl = true;
                if res_level < 0 {
                    res_level = 1;
                }
                break;
            } else if matches!(t, RLE | LRE | RLO | LRO | PDF) {
                has_rtl = true;
                if res_level >= 0 {
                    break;
                }
            } else if t == L {
                if res_level < 0 {
                    res_level = 0;
                }
            } else if t == AN {
                has_rtl = true;
            }
        }
    }
    if autodir && res_level >= 0 {
        paragraph_level = res_level as u8;
    }
    if !has_rtl && paragraph_level == 0 {
        return 0;
    }

    let mut types: Vec<u8> = alloc::vec![0; count];
    let mut levels: Vec<u8> = alloc::vec![0; count];
    let mut skip: Vec<bool> = alloc::vec![false; count];

    // X1-X8
    let mut cur_emb = paragraph_level;
    let mut cur_ovr = ON;
    let mut cur_iso = false;
    isolate_level = 0;
    let mut dss: Vec<(u8, u8, bool)> = Vec::new();
    dss.push((cur_emb, cur_ovr, cur_iso));
    fn pop(dss: &mut Vec<(u8, u8, bool)>) -> (u8, u8, bool) {
        dss.pop();
        *dss.last().unwrap_or(&(0, ON, false))
    }

    for i in 0..count {
        let mut tt = bidi_class(line[i].wc);
        levels[i] = cur_emb;
        if tt == FSI {
            let mut lvl = 0;
            tt = LRI;
            for k in i + 1..count {
                let kt = bidi_class(line[k].wc);
                if kt == FSI || kt == RLI || kt == LRI {
                    lvl += 1;
                } else if kt == PDI {
                    if lvl > 0 {
                        lvl -= 1;
                    } else {
                        break;
                    }
                } else if kt == R || kt == AL {
                    tt = RLI;
                    break;
                } else if kt == L {
                    break;
                }
            }
        }
        // Embedding beyond MAX_DEPTH keeps the current level (C had no cap).
        let odd = |e: u8| if least_greater_odd(e) <= MAX_DEPTH { least_greater_odd(e) } else { e };
        let even = |e: u8| if least_greater_even(e) <= MAX_DEPTH { least_greater_even(e) } else { e };
        match tt {
            RLE | LRE | RLO | LRO => {
                cur_emb = if tt == RLE || tt == RLO { odd(cur_emb) } else { even(cur_emb) };
                cur_ovr = match tt {
                    RLO => R,
                    LRO => L,
                    _ => ON,
                };
                cur_iso = false;
                dss.push((cur_emb, cur_ovr, cur_iso));
            }
            RLI | LRI => {
                cur_emb = if tt == RLI { odd(cur_emb) } else { even(cur_emb) };
                if cur_ovr != ON {
                    tt = cur_ovr;
                }
                isolate_level += 1;
                cur_ovr = ON;
                cur_iso = true;
                dss.push((cur_emb, cur_ovr, cur_iso));
            }
            PDF => {
                if !cur_iso && dss.len() >= 2 {
                    (cur_emb, cur_ovr, cur_iso) = pop(&mut dss);
                }
                levels[i] = cur_emb;
            }
            PDI => {
                if isolate_level > 0 {
                    while !cur_iso && !dss.is_empty() {
                        cur_iso = pop(&mut dss).2;
                    }
                    (cur_emb, cur_ovr, cur_iso) = pop(&mut dss);
                    isolate_level -= 1;
                }
                if cur_ovr != ON {
                    tt = cur_ovr;
                }
                levels[i] = cur_emb;
            }
            _ => {
                if cur_ovr != ON {
                    tt = cur_ovr;
                }
            }
        }
        types[i] = tt;
    }

    // X9
    for i in 0..count {
        if matches!(types[i], RLE | LRE | RLO | LRO | PDF | BN) {
            types[i] = NSM;
            skip[i] = true;
        }
    }
    let sor = if paragraph_level & 1 != 0 { R } else { L };
    // W1
    if types[0] == NSM {
        types[0] = sor;
    }
    for i in 1..count {
        if types[i] == NSM {
            types[i] = match types[i - 1] {
                LRI | RLI | FSI | PDI => ON,
                t => t,
            };
        }
    }
    // W2
    for i in 0..count {
        if types[i] == EN {
            for j in (0..i).rev() {
                let t = types[j];
                if t == AL {
                    types[i] = AN;
                    break;
                }
                if t == R || t == L {
                    break;
                }
            }
        }
    }
    // W3
    for t in types.iter_mut() {
        if *t == AL {
            *t = R;
        }
    }
    // W4
    for i in 1..count.saturating_sub(1) {
        if types[i] == ES || types[i] == CS {
            let mut p = i as isize - 1;
            while p >= 0 && skip[p as usize] {
                p -= 1;
            }
            let mut n = i + 1;
            while n < count && skip[n] {
                n += 1;
            }
            if p >= 0 && n < count {
                let (tp, tn) = (types[p as usize], types[n]);
                if types[i] == ES && tp == EN && tn == EN {
                    types[i] = EN;
                }
                if types[i] == CS {
                    if tp == EN && tn == EN {
                        types[i] = EN;
                    }
                    if tp == AN && tn == AN {
                        types[i] = AN;
                    }
                }
            }
        }
    }
    // W5
    for i in 0..count {
        if skip[i] || types[i] != ET {
            continue;
        }
        for j in i..count {
            if skip[j] || types[j] == ET {
                continue;
            }
            if types[j] == EN {
                types[i] = EN;
            }
            break;
        }
    }
    for i in (0..count).rev() {
        if skip[i] || types[i] != ET {
            continue;
        }
        for j in (0..=i).rev() {
            if skip[j] || types[j] == ET {
                continue;
            }
            if types[j] == EN {
                types[i] = EN;
            }
            break;
        }
    }
    // W6
    for t in types.iter_mut() {
        if *t == ES || *t == ET || *t == CS {
            *t = ON;
        }
    }
    // W7
    let mut last_strong = sor;
    for i in 0..count {
        if skip[i] {
            continue;
        }
        if types[i] == L || types[i] == R {
            last_strong = types[i];
        }
        if types[i] == EN && last_strong == L {
            types[i] = L;
        }
    }
    // N0
    {
        let e = sor;
        let o = if e == L { R } else { L };
        const BRACKET_STACK: usize = 63;
        let mut openers: Vec<(u32, usize)> = Vec::new();
        let strong = |t: u8| if t == EN || t == AN { R } else { t };
        for i in 0..count {
            if skip[i] {
                continue;
            }
            let bc = bracket(line[i].wc);
            if bc == 0 {
                continue;
            }
            if bc == line[i].wc {
                if openers.len() < BRACKET_STACK {
                    openers.push((bc, i));
                }
                continue;
            }
            let Some(k) = openers.iter().rposition(|o| o.0 == bc) else { continue };
            let open_pos = openers[k].1;
            openers.truncate(k);
            let (mut found_e, mut found_o) = (false, false);
            for m in open_pos + 1..i {
                if skip[m] {
                    continue;
                }
                let t = strong(types[m]);
                if t == R || t == AL || t == L {
                    let d = if t == L { L } else { R };
                    if d == e {
                        found_e = true;
                    } else {
                        found_o = true;
                    }
                }
            }
            let dir = if found_e {
                e
            } else if found_o {
                let mut ctx = e;
                for m in (0..open_pos).rev() {
                    if skip[m] {
                        continue;
                    }
                    let t = strong(types[m]);
                    if t == R || t == AL {
                        ctx = R;
                        break;
                    } else if t == L {
                        ctx = L;
                        break;
                    }
                }
                if ctx == o { o } else { e }
            } else {
                continue;
            };
            types[open_pos] = dir;
            types[i] = dir;
            for m in open_pos + 1..i {
                if is_ni(types[m]) {
                    types[m] = dir;
                }
            }
        }
    }
    // N1
    let mut i = 0;
    while i < count {
        if skip[i] || !is_ni(types[i]) {
            i += 1;
            continue;
        }
        let mut end = i;
        while end + 1 < count && (skip[end + 1] || is_ni(types[end + 1])) {
            end += 1;
        }
        let strong = |t: u8| if t == EN || t == AN { R } else { t };
        let mut prev_strong = sor;
        for j in (0..i).rev() {
            if skip[j] {
                continue;
            }
            let t = strong(types[j]);
            if t == R || t == L {
                prev_strong = t;
                break;
            }
        }
        let mut next_strong = sor;
        for j in end + 1..count {
            if skip[j] {
                continue;
            }
            let t = strong(types[j]);
            if t == R || t == L {
                next_strong = t;
                break;
            }
        }
        if prev_strong == next_strong {
            for t in types[i..=end].iter_mut() {
                *t = prev_strong;
            }
        }
        i = end + 1;
    }
    // N2
    for i in 0..count {
        if is_ni(types[i]) {
            types[i] = if levels[i] & 1 != 0 { R } else { L };
        }
    }
    // I1/I2
    for i in 0..count {
        if skip[i] {
            continue;
        }
        if levels[i] & 1 == 0 {
            if types[i] == R {
                levels[i] += 1;
            } else if types[i] == AN || types[i] == EN {
                levels[i] += 2;
            }
        } else if types[i] == L || types[i] == EN || types[i] == AN {
            levels[i] += 1;
        }
    }
    // L1
    for i in (0..count).rev() {
        if skip[i] {
            continue;
        }
        if matches!(types[i], WS | S | B) {
            levels[i] = paragraph_level;
        } else {
            break;
        }
    }
    for i in 0..count {
        if types[i] == S {
            levels[i] = paragraph_level;
            for j in (0..i).rev() {
                if skip[j] {
                    continue;
                }
                if types[j] == WS || types[j] == BN {
                    levels[j] = paragraph_level;
                } else {
                    break;
                }
            }
        }
    }
    // L4
    for i in 0..count {
        if levels[i] & 1 != 0 {
            line[i].wc = mirror(line[i].wc);
        }
    }
    // L2
    let mut max_level = paragraph_level;
    let mut min_odd = 255u8;
    for &l in levels.iter() {
        max_level = max_level.max(l);
        if l & 1 != 0 && l < min_odd {
            min_odd = l;
        }
    }
    let mut level = max_level as i32;
    while level >= min_odd as i32 {
        flip_runs(line, &levels, level as u8);
        level -= 1;
    }
    paragraph_level
}

// ── BidiUtils::applyBidiVisual ─────────────────────────────────────────────

fn is_transparent_mark(cp: u32) -> bool {
    cp >= 0x0591 && bidi_class(cp) == NSM
}

fn filtered(cp: u32) -> bool {
    cp == LIG_PLACEHOLDER || cp == 0x200C || cp == 0x200D
}

const BOLD_ON: char = '\u{1}';
const BOLD_OFF: char = '\u{2}';

/// One wrapped line in logical order -> the order to draw it left to right,
/// with Arabic letters replaced by their contextual presentation forms.
/// Paragraph direction comes from the line's first strong character.
pub fn visual(line: &str) -> String {
    if !has_rtl(line) {
        return String::from(line);
    }
    // Bold state at the start: on when the first marker closes bold.
    let start_bold = line.chars().find(|&c| c == BOLD_ON || c == BOLD_OFF) == Some(BOLD_OFF);
    let mut bold = start_bold;
    let mut v: Vec<BChar> = Vec::with_capacity(line.len());
    let mut last_base: Option<usize> = None;
    let mut pending: u8 = 0;
    for ch in line.chars() {
        if ch == BOLD_ON {
            bold = true;
            continue;
        }
        if ch == BOLD_OFF {
            bold = false;
            continue;
        }
        let cp = ch as u32;
        let n = v.len();
        let mut c = BChar { wc: cp, index: n, joiners: 0, bold };
        if cp == 0x200C || cp == 0x200D {
            let j = if cp == 0x200D { ZWJ } else { ZWNJ };
            if let Some(b) = last_base {
                v[b].joiners |= j;
            }
            pending |= j;
        } else {
            c.joiners = pending << 4;
            pending = 0;
            last_base = Some(n);
        }
        v.push(c);
    }
    let end_bold = bold;

    do_bidi(true, 0, &mut v);
    let shaped = do_shape(&v);
    let count = shaped.len();

    let mut out = String::with_capacity(line.len() + 8);
    let mut state = start_bold;
    let mut emit = |out: &mut String, c: &BChar| {
        if c.bold != state {
            out.push(if c.bold { BOLD_ON } else { BOLD_OFF });
            state = c.bold;
        }
        if let Some(ch) = char::from_u32(c.wc) {
            out.push(ch);
        }
    };
    const YHWH: [u32; 4] = [0x05D4, 0x05D5, 0x05D4, 0x05D9]; // הוהי in visual order
    let mut i = 0;
    while i < count {
        let cp = shaped[i].wc;
        if filtered(cp) {
            i += 1;
            continue;
        }
        if cp == 0x05D4 || is_transparent_mark(cp) {
            let mut end = i;
            let mut matched = 0;
            while end < count && matched < 4 {
                if !is_transparent_mark(shaped[end].wc) {
                    matched = if shaped[end].wc == YHWH[matched] { matched + 1 } else { 5 };
                }
                end += 1;
            }
            if matched == 4 {
                let b = BChar { wc: '\'' as u32, ..shaped[i] };
                emit(&mut out, &b);
                emit(&mut out, &BChar { wc: 0x05D4, ..b });
                i = end;
                continue;
            }
        }
        if !is_transparent_mark(cp) {
            emit(&mut out, &shaped[i]);
            i += 1;
            continue;
        }
        let mut j = i;
        while j < count && (filtered(shaped[j].wc) || is_transparent_mark(shaped[j].wc)) {
            j += 1;
        }
        if j < count && shaped[j].index < shaped[i].index {
            emit(&mut out, &shaped[j]);
            for k in (i..j).rev() {
                if is_transparent_mark(shaped[k].wc) {
                    let m = BChar { bold: shaped[j].bold, ..shaped[k] };
                    emit(&mut out, &m);
                }
            }
            i = j + 1;
        } else {
            for k in i..j {
                if is_transparent_mark(shaped[k].wc) {
                    emit(&mut out, &shaped[k]);
                }
            }
            i = j;
        }
    }
    if state != end_bold {
        out.push(if end_bold { BOLD_ON } else { BOLD_OFF });
    }
    out
}
