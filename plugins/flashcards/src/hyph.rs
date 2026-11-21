//! Liang hyphenation, ported from matcha-reader (lib/Epub/Epub/hyphenation).
//!
//! Tries come from `hyph/<lang>.bin` in the plugin folder (written by
//! tools/matcha_hyph.py): 12-byte header (magic "HYP1", root offset u32 LE,
//! min prefix, min suffix, alphabet 0=Latin 1=Cyrillic, reserved) followed by
//! matcha's serialized Typst-style binary trie, unchanged.

use alloc::string::String;
use alloc::vec::Vec;

const MAX_WORD_BYTES: usize = 160;
const MAX_WORD_CHARS: usize = 70;

struct Trie {
    lang: String,
    data: Option<Vec<u8>>,
}

static mut TRIES: Vec<Trie> = Vec::new();

struct Patterns<'a> {
    data: &'a [u8],
    root: usize,
    min_prefix: usize,
    min_suffix: usize,
    cyrillic: bool,
}

fn normalize_lang(lang: &str) -> String {
    let mut p = String::new();
    for c in lang.chars() {
        if c == '-' || c == '_' { break; }
        p.push(c.to_ascii_lowercase());
    }
    let m = match p.as_str() {
        "eng" => "en", "fra" | "fre" => "fr", "deu" | "ger" => "de", "rus" => "ru", "spa" => "es",
        "ita" => "it", "ukr" => "uk", "swe" => "sv", "fin" => "fi", _ => return p,
    };
    String::from(m)
}

fn patterns(lang: &str) -> Option<Patterns<'static>> {
    let lang = normalize_lang(lang);
    if lang.is_empty() || !lang.bytes().all(|b| b.is_ascii_lowercase()) { return None; }
    let tries = unsafe { &mut *core::ptr::addr_of_mut!(TRIES) };
    let idx = match tries.iter().position(|t| t.lang == lang) {
        Some(i) => i,
        None => {
            let data = crate::store::read(&crate::store::path(&alloc::format!("hyph/{}.bin", lang)))
                .filter(|d| d.len() > 12 && &d[..4] == b"HYP1");
            tries.push(Trie { lang, data });
            tries.len() - 1
        }
    };
    let d: &'static [u8] = tries[idx].data.as_deref()?;
    Some(Patterns {
        data: &d[12..],
        root: u32::from_le_bytes([d[4], d[5], d[6], d[7]]) as usize,
        min_prefix: d[8] as usize,
        min_suffix: d[9] as usize,
        cyrillic: d[10] == 1,
    })
}

// ---- character classes (HyphenationCommon.cpp) ----

fn to_lower_latin(cp: u32) -> u32 {
    if (b'A' as u32..=b'Z' as u32).contains(&cp) { return cp + 32; }
    if (0xC0..=0xD6).contains(&cp) || (0xD8..=0xDE).contains(&cp) { return cp + 0x20; }
    if ((0x100..=0x137).contains(&cp) && cp % 2 == 0)
        || ((0x139..=0x148).contains(&cp) && cp % 2 == 1)
        || ((0x14A..=0x177).contains(&cp) && cp % 2 == 0)
        || ((0x179..=0x17E).contains(&cp) && cp % 2 == 1)
    {
        return cp + 1;
    }
    match cp { 0x178 => 0xFF, 0x1E9E => 0xDF, _ => cp }
}

fn to_lower_cyrillic(cp: u32) -> u32 {
    if (0x410..=0x42F).contains(&cp) { return cp + 0x20; }
    if cp == 0x401 { return 0x451; }
    cp
}

fn is_latin(cp: u32) -> bool {
    (b'A' as u32..=b'Z' as u32).contains(&cp)
        || (b'a' as u32..=b'z' as u32).contains(&cp)
        || (0xC0..=0xD6).contains(&cp)
        || (0xD8..=0xF6).contains(&cp)
        || (0xF8..=0xFF).contains(&cp)
        || (0x100..=0x17F).contains(&cp)
        || cp == 0x1E9E
}

fn is_cyrillic(cp: u32) -> bool { (0x400..=0x52F).contains(&cp) }
fn is_alpha(cp: u32) -> bool { is_latin(cp) || is_cyrillic(cp) }
fn is_digit(cp: u32) -> bool { (b'0' as u32..=b'9' as u32).contains(&cp) }
fn is_apostrophe(cp: u32) -> bool { matches!(cp, 0x27 | 0x2018 | 0x2019) }
fn is_soft_hyphen(cp: u32) -> bool { cp == 0xAD }

fn is_punct(cp: u32) -> bool {
    matches!(cp, 0x2D | 0x2E | 0x2C | 0x21 | 0x3F | 0x3B | 0x3A | 0x22 | 0x27 | 0x29 | 0x28
        | 0xAB | 0xBB | 0x2018 | 0x2019 | 0x201A | 0x201C | 0x201D | 0x201E | 0xA0
        | 0x7B | 0x7D | 0x5B | 0x5D | 0x2F | 0x2039 | 0x203A | 0x2026)
}

fn is_explicit_hyphen(cp: u32) -> bool {
    matches!(cp, 0x2D | 0xAD | 0x58A | 0x2010..=0x2015 | 0x2043 | 0x207B | 0x208B | 0x2212
        | 0x2E17 | 0x2E3A | 0x2E3B | 0xFE58 | 0xFE63 | 0xFF0D | 0x5F | 0x2026)
}

fn is_separator(cp: u32) -> bool { is_explicit_hyphen(cp) || is_apostrophe(cp) }

#[derive(Clone, Copy)]
struct Cp { value: u32, off: usize }

fn compose(prev: u32, mark: u32) -> u32 {
    const T: &[(u32, &[(u8, u32)])] = &[
        (0x300, &[(b'A', 0xC0), (b'a', 0xE0), (b'E', 0xC8), (b'e', 0xE8), (b'I', 0xCC), (b'i', 0xEC),
                  (b'O', 0xD2), (b'o', 0xF2), (b'U', 0xD9), (b'u', 0xF9)]),
        (0x301, &[(b'A', 0xC1), (b'a', 0xE1), (b'E', 0xC9), (b'e', 0xE9), (b'I', 0xCD), (b'i', 0xED),
                  (b'O', 0xD3), (b'o', 0xF3), (b'U', 0xDA), (b'u', 0xFA), (b'Y', 0xDD), (b'y', 0xFD),
                  (b'C', 0x106), (b'c', 0x107), (b'N', 0x143), (b'n', 0x144), (b'S', 0x15A), (b's', 0x15B),
                  (b'Z', 0x179), (b'z', 0x17A)]),
        (0x302, &[(b'A', 0xC2), (b'a', 0xE2), (b'E', 0xCA), (b'e', 0xEA), (b'I', 0xCE), (b'i', 0xEE),
                  (b'O', 0xD4), (b'o', 0xF4), (b'U', 0xDB), (b'u', 0xFB)]),
        (0x303, &[(b'A', 0xC3), (b'a', 0xE3), (b'N', 0xD1), (b'n', 0xF1)]),
        (0x308, &[(b'A', 0xC4), (b'a', 0xE4), (b'E', 0xCB), (b'e', 0xEB), (b'I', 0xCF), (b'i', 0xEF),
                  (b'O', 0xD6), (b'o', 0xF6), (b'U', 0xDC), (b'u', 0xFC), (b'Y', 0x178), (b'y', 0xFF)]),
        (0x307, &[(b'Z', 0x17B), (b'z', 0x17C)]),
        (0x327, &[(b'C', 0xC7), (b'c', 0xE7)]),
        (0x328, &[(b'A', 0x104), (b'a', 0x105), (b'E', 0x118), (b'e', 0x119)]),
    ];
    for (m, pairs) in T {
        if *m == mark {
            for (b, c) in pairs.iter() {
                if *b as u32 == prev { return *c; }
            }
        }
    }
    0
}

fn collect(word: &str) -> Vec<Cp> {
    let mut cps: Vec<Cp> = Vec::with_capacity(word.len());
    for (off, ch) in word.char_indices() {
        let cp = ch as u32;
        if let Some(last) = cps.last_mut() {
            let c = compose(last.value, cp);
            if c != 0 { last.value = c; continue; }
        }
        cps.push(Cp { value: cp, off });
    }
    cps
}

fn trim(cps: &mut Vec<Cp>) {
    if cps.is_empty() { return; }
    if cps.len() >= 3 {
        let mut end = cps.len() as isize - 1;
        while end >= 0 && is_punct(cps[end as usize].value) { end -= 1; }
        let mut pos = end;
        if pos >= 0 && is_digit(cps[pos as usize].value) {
            while pos >= 0 && is_digit(cps[pos as usize].value) { pos -= 1; }
            if pos >= 0 && cps[pos as usize].value == b'[' as u32 && end - pos > 1 {
                cps.truncate(pos as usize);
            }
        }
    }
    let lead = cps.iter().take_while(|c| is_punct(c.value)).count();
    cps.drain(..lead);
    while cps.last().map_or(false, |c| is_punct(c.value)) { cps.pop(); }
}

// ---- Liang trie walk (LiangHyphenation.cpp) ----

#[derive(Clone, Copy)]
struct State { addr: usize, stride: u8, count: usize, trans: usize, targets: usize, levels: usize, levels_len: usize }

fn decode_state(d: &[u8], addr: usize) -> Option<State> {
    if addr >= d.len() { return None; }
    let rem = d.len() - addr;
    let mut pos = 0;
    let header = d[addr];
    pos += 1;
    let has_levels = header >> 7 != 0;
    let mut stride = (header >> 5) & 3;
    if stride == 0 { stride = 1; }
    let mut count = (header & 0x1F) as usize;
    if count == 31 {
        if pos >= rem { return None; }
        count = d[addr + pos] as usize;
        pos += 1;
    }
    let (mut levels, mut levels_len) = (0, 0);
    if has_levels {
        if pos + 1 >= rem { return None; }
        let hi = d[addr + pos] as usize;
        let lo = d[addr + pos + 1] as usize;
        pos += 2;
        let off = (hi << 4) | (lo >> 4);
        levels_len = lo & 0xF;
        if off + levels_len > d.len() || off < 4 { return None; }
        levels = off - 4;
    }
    if pos + count > rem { return None; }
    let trans = addr + pos;
    pos += count;
    if pos + count * stride as usize > rem { return None; }
    Some(State { addr, stride, count, trans, targets: addr + pos, levels, levels_len })
}

fn decode_delta(b: &[u8], stride: u8) -> i32 {
    match stride {
        1 => b[0] as i8 as i32,
        2 => i16::from_be_bytes([b[0], b[1]]) as i32,
        _ => (((b[0] as i32) << 16) | ((b[1] as i32) << 8) | b[2] as i32) - (1 << 23),
    }
}

fn transition(d: &[u8], s: &State, letter: u8) -> Option<State> {
    for i in 0..s.count {
        if d[s.trans + i] != letter { continue; }
        let p = s.targets + i * s.stride as usize;
        let next = s.addr as i64 + decode_delta(&d[p..], s.stride) as i64;
        if next < 0 || next as usize >= d.len() { return None; }
        return decode_state(d, next as usize);
    }
    None
}

fn push_utf8(cp: u32, bytes: &mut Vec<u8>) -> bool {
    let Some(c) = char::from_u32(cp) else { return false };
    let mut buf = [0u8; 4];
    let s = c.encode_utf8(&mut buf);
    if bytes.len() + s.len() > MAX_WORD_BYTES { return false; }
    bytes.extend_from_slice(s.as_bytes());
    true
}

fn liang(cps: &[Cp], p: &Patterns) -> Vec<usize> {
    let mut out = Vec::new();
    if cps.is_empty() { return out; }
    // Augmented word: '.' + lowercase letters + '.'
    let mut bytes: Vec<u8> = Vec::with_capacity(cps.len() * 2 + 2);
    let mut char_off: Vec<usize> = Vec::with_capacity(cps.len() + 2);
    char_off.push(0);
    bytes.push(b'.');
    for c in cps {
        let ok = if p.cyrillic { is_cyrillic(c.value) } else { is_latin(c.value) };
        if !ok || char_off.len() >= MAX_WORD_CHARS - 1 { return out; }
        char_off.push(bytes.len());
        let lower = if p.cyrillic { to_lower_cyrillic(c.value) } else { to_lower_latin(c.value) };
        if !push_utf8(lower, &mut bytes) { return out; }
    }
    if char_off.len() >= MAX_WORD_CHARS || bytes.len() >= MAX_WORD_BYTES { return out; }
    char_off.push(bytes.len());
    bytes.push(b'.');
    let nchars = char_off.len();
    let mut byte_to_char = alloc::vec![-1i32; bytes.len()];
    for (i, &o) in char_off.iter().enumerate() { byte_to_char[o] = i as i32; }

    let d = p.data;
    let Some(root) = decode_state(d, p.root) else { return out };
    let mut scores = alloc::vec![0u8; nchars];
    for &start in &char_off {
        let mut st = root;
        for cur in start..bytes.len() {
            let Some(next) = transition(d, &st, bytes[cur]) else { break };
            st = next;
            if st.levels_len == 0 { continue; }
            let mut offset = 0;
            for i in 0..st.levels_len {
                let packed = d[st.levels + i];
                offset += (packed / 10) as usize;
                let level = packed % 10;
                let split = start + offset;
                if split >= bytes.len() { continue; }
                let b = byte_to_char[split];
                if b < 2 || b + 2 > nchars as i32 { continue; }
                let s = &mut scores[b as usize];
                if level > *s { *s = level; }
            }
        }
    }
    let n = cps.len();
    if n < 2 { return out; }
    for bi in 1..n {
        if bi < p.min_prefix || n - bi < p.min_suffix { continue; }
        if bi + 1 >= nchars { break; }
        if scores[bi + 1] & 1 != 0 { out.push(bi); }
    }
    out
}

// ---- Hyphenator::breakOffsets (Hyphenator.cpp, includeFallback = false) ----

fn segment_breaks(cps: &[Cp], p: &Patterns, out: &mut Vec<(usize, bool)>) {
    let mut seg = 0;
    for i in 0..=cps.len() {
        if i < cps.len() && !is_separator(cps[i].value) { continue; }
        if i > seg {
            for idx in liang(&cps[seg..i], p) {
                if idx > 0 && idx < i - seg { out.push((cps[seg + idx].off, true)); }
            }
        }
        seg = i + 1;
    }
}

fn apostrophe_breaks(cps: &[Cp], out: &mut Vec<(usize, bool)>) {
    let mut seg = 0;
    for i in 0..cps.len() {
        if !is_separator(cps[i].value) { continue; }
        if is_apostrophe(cps[i].value) && i > 0 && i + 1 < cps.len()
            && is_alpha(cps[i - 1].value) && is_alpha(cps[i + 1].value)
        {
            let left = cps[seg..i].iter().filter(|c| is_alpha(c.value)).count();
            let right = cps[i + 1..].iter().take_while(|c| !is_separator(c.value)).filter(|c| is_alpha(c.value)).count();
            if left >= 3 && right >= 3 { out.push((cps[i + 1].off, false)); }
        }
        seg = i + 1;
    }
}

fn sort_dedupe(v: &mut Vec<(usize, bool)>) {
    v.sort();
    v.dedup_by(|a, b| a.0 == b.0);
}

/// Byte offsets inside `word` where it may break, ascending; `true` means a
/// visible hyphen must be inserted (pattern breaks, soft hyphens), `false`
/// means the break follows an existing visible separator (hard hyphen, or an
/// apostrophe between 3+ letter segments). Mirrors matcha's
/// `Hyphenator::breakOffsets(word, false)`. Unknown language or missing trie
/// file: only explicit-hyphen/apostrophe breaks.
pub fn break_offsets(word: &str, lang: &str) -> Vec<(usize, bool)> {
    if word.is_empty() { return Vec::new(); }
    let mut cps = collect(word);
    trim(&mut cps);
    let pat = patterns(lang);
    let has_apos = cps.iter().any(|c| is_apostrophe(c.value));

    let mut explicit: Vec<(usize, bool)> = Vec::new();
    for i in 1..cps.len().saturating_sub(1) {
        let cp = cps[i].value;
        if is_explicit_hyphen(cp) && is_alpha(cps[i - 1].value) && is_alpha(cps[i + 1].value) {
            explicit.push((cps[i + 1].off, is_soft_hyphen(cp)));
        }
    }
    if !explicit.is_empty() {
        if let Some(p) = &pat { segment_breaks(&cps, p, &mut explicit); }
        if has_apos { apostrophe_breaks(&cps, &mut explicit); }
        sort_dedupe(&mut explicit);
        return explicit;
    }
    if has_apos {
        let mut v = Vec::new();
        if let Some(p) = &pat { segment_breaks(&cps, p, &mut v); }
        apostrophe_breaks(&cps, &mut v);
        sort_dedupe(&mut v);
        return v;
    }
    match &pat {
        Some(p) => liang(&cps, p).into_iter().map(|i| (cps[i].off, true)).collect(),
        None => Vec::new(),
    }
}

/// Guess the language of `text` from its characters. This is only a cheap
/// heuristic (no dictionary, no statistics): distinctive letters pick a
/// language with a trie, everything else falls back to "en".
/// The language when the text tells it (letters only one language uses, or two of its common
/// words); None for a short text that could be anything.
pub fn guess_lang_sure(text: &str) -> Option<&'static str> {
    let has = |set: &str| text.chars().any(|c| set.contains(c));
    let cyr = text.chars().any(|c| is_cyrillic(c as u32));
    if cyr {
        return Some(if has("іїєґІЇЄҐ") { "uk" } else { "ru" });
    }
    if has("ąęłńśźżĄĘŁŃŚŹŻ") { return Some("pl"); }
    // Common short words decide most card sentences, before the letters do.
    const WORDS: [(&str, &[&str]); 8] = [
        ("de", &["der", "die", "das", "und", "ist", "nicht", "ich", "ein", "eine", "von", "mit", "zu", "auf", "sie", "es"]),
        ("en", &["the", "and", "is", "of", "to", "in", "it", "you", "that", "was", "for", "on", "are", "with"]),
        ("fr", &["le", "la", "les", "et", "est", "un", "une", "des", "du", "je", "pas", "que", "dans", "il", "elle"]),
        ("es", &["el", "la", "los", "las", "y", "es", "un", "una", "que", "de", "en", "no", "por", "con", "para"]),
        ("it", &["il", "la", "e", "è", "un", "una", "che", "di", "non", "per", "con", "sono", "gli", "lo"]),
        ("pt", &["o", "a", "os", "as", "e", "é", "um", "uma", "que", "de", "não", "em", "para", "com", "do", "da"]),
        ("sv", &["och", "är", "en", "ett", "det", "att", "jag", "inte", "på", "med", "som", "för"]),
        ("fi", &["ja", "on", "ei", "se", "että", "hän", "minä", "kun", "mutta", "oli", "ovat"]),
    ];
    // Each word lower-cased once, then counted against every list.
    let words: Vec<String> = text.split(|c: char| !c.is_alphabetic()).filter(|w| !w.is_empty() && w.len() <= 12)
        .map(|w| w.to_lowercase()).collect();
    let mut best = ("", 0);
    for (lang, list) in WORDS {
        let n = words.iter().filter(|w| list.contains(&w.as_str())).count();
        if n > best.1 {
            best = (lang, n);
        }
    }
    if best.1 >= 2 {
        return Some(best.0);
    }
    if has("ßẞ") { return Some("de"); }
    if has("ñÑ¿¡") { return Some("es"); }
    if has("ãõÃÕ") { return Some("pt"); }
    if has("åÅ") { return Some(if has("üÜ") { "de" } else { "sv" }); }
    if has("üÜ") { return Some("de"); }
    if has("äöÄÖ") {
        // Finnish doubles vowels and uses ä/ö heavily; Swedish/German share ä/ö.
        let lower: String = text.chars().flat_map(|c| c.to_lowercase()).collect();
        if ["ää", "öö", "aa", "ii", "uu", "yy"].iter().any(|p| lower.contains(p)) { return Some("fi"); }
        return Some("de");
    }
    if has("çœæÇŒÆëËîÎêÊâÂ") { return Some("fr"); }
    if has("ìòÌÒ") { return Some("it"); }
    if has("àèùÀÈÙ") { return Some("fr"); }
    if has("áéíóúÁÉÍÓÚ") { return Some("es"); }
    None
}
