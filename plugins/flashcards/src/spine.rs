//! matcha's card for a word mined while reading (FlashcardReviewActivity's drawSpine, drawFront,
//! drawFrontVertical and drawBack): a ruled index card with the word along a spine, the sentence it
//! was saved from beside it, and on the back the reading, the senses and the sentence again.
//! Chinese and Japanese stand upright down the spine; other scripts run up it, as on a book's back.
//! A Japanese card is bound on the right and its front reads in columns.
//!
//! matcha scales its reader font freely; here the faces come in fixed sizes: the spine's word has
//! its own large serif faces, and shrinks to a smaller one when it does not fit.
use crate::dsn;
use crate::gfx::{self, Canvas, Style, CARD, CARDB, CJK14, CJK12, SERIF12, SERIF12B, SERIF12I, SERIF18B, SPINE, Script};

/// The language of the card's Latin or Cyrillic text, guessed for its hyphenation patterns.
static mut LANG: &str = "en";
fn lang() -> &'static str { unsafe { LANG } }
/// The card being drawn's language (one card at a time: set by draw).
static mut SCRIPT: Script = Script::Sc;
/// `st` with the card's language's CJK faces.
fn s(st: Style) -> Style { gfx::for_script(st, unsafe { SCRIPT }) }
use crate::store::Mined;
use alloc::{string::String, vec::Vec};

pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

const MAX_SPINE_CHARS: usize = 10;
fn book() -> Style { s(SERIF12I) }

fn is_kana(c: char) -> bool { (0x3041..=0x30FF).contains(&(c as u32)) }
fn is_cjk(c: char) -> bool { c as u32 >= 0x2E80 }
/// The card's language from its word and sentence.
pub fn script(m: &Mined) -> Script { gfx::detect_script([m.word.as_str(), m.sentence.as_str()].into_iter()) }

/// Draws the whole card inside the double rule's inner rectangle.
pub fn draw(m: &Mined, inner: &Rect, back: bool, deck_lang: &str) {
    let sc = script(m);
    unsafe { SCRIPT = sc };
    gfx::set_script(sc);
    let all = alloc::format!("{} {} {}", m.word, m.sentence, m.definition);
    unsafe { LANG = crate::hyph::guess_lang_sure(&all).unwrap_or_else(|| crate::cardhtml::lang_static(deck_lang)) };
    let japanese = sc == Script::Jp;
    let spine_w = (inner.w * 26 / 100).clamp(90, 160);
    let spine_x = if japanese { inner.x + inner.w - spine_w } else { inner.x };
    let rule_x = if japanese { spine_x - 1 } else { inner.x + spine_w };
    dsn::fill(rule_x, inner.y, 1, inner.h, dsn::BLACK);
    draw_spine(m, &Rect { x: spine_x, y: inner.y, w: spine_w, h: inner.h }, back && japanese);
    let body = Rect {
        x: if japanese { inner.x + 22 } else { inner.x + spine_w + 24 },
        y: inner.y + 26,
        w: inner.w - spine_w - 46,
        h: inner.h - 48,
    };
    if back {
        draw_back(m, &body, japanese);
    } else if japanese {
        draw_front_vertical(m, &body);
    } else {
        draw_front(m, &body);
    }
}

// ---------------------------------------------------------------- glyphs
/// One character's ink, with its ink box.
fn glyph(st: Style, c: char) -> (Canvas, (i32, i32, i32, i32)) {
    let mut s = String::new();
    s.push(c);
    let mut cv = Canvas::new(gfx::text_width(st, &s) + 6, st.line_h + 8);
    gfx::draw_text(&mut cv, 1, 0, st, &s);
    let ib = cv.ink_box().unwrap_or((0, 0, cv.w, cv.h));
    (cv, ib)
}
/// Draws a canvas so its ink box is centred on (cx, cy).
fn blit_centered(cv: &Canvas, ib: (i32, i32, i32, i32), cx: i32, cy: i32) {
    gfx::blit(cv, cx - ib.0 - ib.2 / 2, cy - ib.1 - ib.3 / 2, dsn::BLACK);
}

// ---------------------------------------------------------------- the spine
fn draw_spine(m: &Mined, spine: &Rect, ruby: bool) {
    let chars: Vec<char> = m.word.chars().collect();
    let Some(&first) = chars.first() else { return };
    if is_cjk(first) {
        // Upright, one character under the other, as a vertical book sets it.
        let n = chars.len().min(MAX_SPINE_CHARS);
        let cut = chars.len() > n;
        let ruby = ruby && !m.reading.is_empty();
        let word_w = if ruby { spine.w * 64 / 100 } else { spine.w };
        let step = (word_w * if ruby { 90 } else { 80 } / 100).min((spine.h - 48) / n as i32);
        let top = spine.y + (spine.h - n as i32 * step) / 2;
        let word_x = if ruby { spine.x + spine.w * 4 / 100 } else { spine.x };
        // The large face's em is about 75 px, the one beside a reading about 58 px; a shorter step takes
        // the card face.
        let st = if step >= 80 { gfx::spine_cjk(unsafe { SCRIPT }, false) } else if step >= 60 && unsafe { SCRIPT } == Script::Jp { gfx::spine_cjk(Script::Jp, true) } else { s(CJK14) };
        let mut ink_right = word_x + word_w / 2;
        for (i, &c) in chars[..n].iter().enumerate() {
            let (cv, ib) = glyph(st, c);
            blit_centered(&cv, ib, word_x + word_w / 2, top + i as i32 * step + step / 2);
            ink_right = ink_right.max(word_x + word_w / 2 - ib.2 / 2 + ib.2);
        }
        gfx::forget_spine();
        if ruby && !cut {
            // Furigana stand close against the characters' right edge, as in a vertical book.
            draw_ruby(m, &chars[..n], spine, ink_right + 5, top, step);
        }
        return;
    }
    // Other scripts run up the spine, as on an English-language book's back: in the large serif
    // bold when the word fits, else the card's serif bold, else cut.
    let room = spine.h - 48;
    let st = if gfx::text_width(SPINE, &m.word) <= room { SPINE } else { s(SERIF18B) };
    let word = gfx::truncated(st, &m.word, room);
    let mut cv = Canvas::new(gfx::text_width(st, &word) + 4, st.line_h + 8);
    gfx::draw_text(&mut cv, 1, 0, st, &word);
    let cv = cv.turned_ccw();
    // Centred on the letters' x-height band rather than all their ink, which leans to the ascenders.
    let ib = cv.ink_box().unwrap_or((0, 0, cv.w, cv.h));
    let nudge = ib.2 / 16;
    gfx::blit(&cv, spine.x + (spine.w - ib.2) / 2 - ib.0 + nudge, spine.y + (spine.h - ib.3) / 2 - ib.1, dsn::BLACK);
}

struct Seg {
    start: usize,
    count: usize,
    text: Vec<char>,
}
/// Anki's furigana syntax (木[こ]漏[も]れ 日[び]) over the word's characters; None when its bases
/// do not spell the word.
fn parse_furigana(furigana: &str, chars: &[char]) -> Option<Vec<Seg>> {
    if !furigana.contains('[') {
        return None;
    }
    let mut spelled: Vec<char> = Vec::new();
    let mut base_start = 0;
    let mut segs = Vec::new();
    let mut it = furigana.chars().peekable();
    while let Some(c) = it.next() {
        if c == ' ' {
            base_start = spelled.len();
        } else if c == '[' {
            let mut text = Vec::new();
            for r in it.by_ref() {
                if r == ']' {
                    break;
                }
                text.push(r);
            }
            if base_start == spelled.len() {
                return None;
            }
            segs.push(Seg { start: base_start, count: spelled.len() - base_start, text });
            base_start = spelled.len();
        } else {
            spelled.push(c);
        }
    }
    (spelled.as_slice() == chars && !segs.is_empty()).then_some(segs)
}
/// The reading split over the word's kanji runs by the kana the word itself spells
/// (木漏れ日 / こもれび -> 木漏:こも, 日:び); None when the reading does not follow the word.
fn split_reading(reading: &[char], chars: &[char]) -> Option<Vec<Seg>> {
    let same = |a: char, b: char| {
        let f = |c: char| if (0x30A1..=0x30F6).contains(&(c as u32)) { char::from_u32(c as u32 - 0x60).unwrap_or(c) } else { c };
        f(a) == f(b)
    };
    let mut segs = Vec::new();
    let (mut pos, mut i) = (0, 0);
    while i < chars.len() {
        if is_kana(chars[i]) {
            if pos >= reading.len() || !same(reading[pos], chars[i]) {
                return None;
            }
            pos += 1;
            i += 1;
            continue;
        }
        let mut j = i;
        while j < chars.len() && !is_kana(chars[j]) {
            j += 1;
        }
        let mut end = reading.len();
        if j < chars.len() {
            end = pos + 1;
            while end < reading.len() && !same(reading[end], chars[j]) {
                end += 1;
            }
            if end >= reading.len() {
                return None;
            }
        }
        if end <= pos {
            return None;
        }
        segs.push(Seg { start: i, count: j - i, text: reading[pos..end].to_vec() });
        pos = end;
        i = j;
    }
    (pos == reading.len() && !reading.is_empty()).then_some(segs)
}
/// On a Japanese back the reading stands beside the word, as furigana do in vertical text.
fn draw_ruby(m: &Mined, chars: &[char], spine: &Rect, col_x: i32, top: i32, step: i32) {
    let reading: Vec<char> = m.reading.chars().filter(|&c| c != ' ').collect();
    if reading.len() > 32 {
        return;
    }
    let segs = parse_furigana(&m.furigana, chars)
        .or_else(|| split_reading(&reading, chars))
        .unwrap_or_else(|| alloc::vec![Seg { start: 0, count: chars.len(), text: reading.clone() }]);
    let total: usize = segs.iter().map(|s| s.text.len()).sum();
    let base = (spine.w * 26 / 100).min((spine.h - 48) / total.max(1) as i32).max(8);
    let cx = col_x + gfx::text_width(CJK12, "あ") / 2;
    for g in &segs {
        if g.text.is_empty() {
            continue;
        }
        let span = g.count as i32 * step;
        let cell = base.min(span * 3 / 2 / g.text.len() as i32).max(8);
        let mut y = (top + g.start as i32 * step + (span - g.text.len() as i32 * cell) / 2).max(spine.y + 8);
        for &c in &g.text {
            let mut kana = String::new();
            kana.push(c);
            let mut cv = Canvas::new(gfx::text_width(CJK12, &kana) + 4, CJK12.line_h + 6);
            gfx::draw_text(&mut cv, 1, 0, CJK12, &kana);
            if let Some(ib) = cv.ink_box() {
                blit_centered(&cv, ib, cx, y + cell / 2);
            }
            y += cell;
        }
    }
}

// ---------------------------------------------------------------- rich text
/// A run of text: bold (the saved word, underlined) or not.
type Line = Vec<(bool, String)>;
fn closing(c: char) -> bool { "。、」』！？）,.;:!?)".contains(c) }

/// Wraps text with \x01..\x02 marks into lines: Latin breaks at spaces, CJK between characters,
/// closing punctuation kept on its line; a word wider than the line is cut. matcha's drawRich.
fn rich_wrap(st: Style, stb: Style, text: &str, width: i32, hyphenate: bool) -> Vec<Line> {
    // Tokens: (bold, text, is_space, glued to the previous one)
    let mut toks: Vec<(bool, String, bool, bool)> = Vec::new();
    let mut bold = false;
    for c in text.chars() {
        match c {
            '\u{1}' => bold = true,
            '\u{2}' => bold = false,
            '\n' | ' ' => toks.push((bold, String::from(" "), true, false)),
            _ => {
                let cjk = is_cjk(c);
                if let Some(last) = toks.last_mut() {
                    let last_cjk = last.1.chars().last().is_some_and(is_cjk);
                    if !last.2 && !cjk && !last_cjk && last.0 == bold {
                        last.1.push(c);
                        continue;
                    }
                }
                let glued = toks.last().is_some_and(|l| !l.2) && (!cjk && !toks.last().unwrap().1.chars().last().is_some_and(is_cjk) || closing(c));
                let mut s = String::new();
                s.push(c);
                toks.push((bold, s, false, glued));
            }
        }
    }
    let mut lines: Vec<Line> = Vec::new();
    let mut line: Line = Vec::new();
    let mut w = 0;
    for (b, mut s, space, glued) in toks {
        let st_ = if b { stb } else { st };
        if space && w == 0 {
            continue;
        }
        // A Latin or Cyrillic word that does not fit is hyphenated at its syllables (matcha's
        // drawRich with the reader's Liang patterns), so it runs on rather than jumping a line.
        if hyphenate && !space && !glued && !s.chars().any(is_cjk) {
            loop {
                let tw = gfx::text_width(st_, &s);
                if w + tw <= width {
                    break;
                }
                let mut cut: Option<(usize, String)> = None;
                for (off, hy) in crate::hyph::break_offsets(&s, lang()) {
                    let head = if hy { alloc::format!("{}-", &s[..off]) } else { String::from(&s[..off]) };
                    if gfx::text_width(st_, &head) > width - w {
                        break;
                    }
                    cut = Some((off, head));
                }
                let Some((off, head)) = cut else { break };
                line.push((b, head));
                lines.push(core::mem::take(&mut line));
                w = 0;
                s = String::from(&s[off..]);
            }
        }
        let tw = if space { gfx::text_width(st_, " ").max(4) } else { gfx::text_width(st_, &s) };
        if w > 0 && w + tw > width && !glued {
            while line.last().is_some_and(|l| l.1 == " ") {
                line.pop();
            }
            lines.push(core::mem::take(&mut line));
            w = 0;
            if space {
                continue;
            }
        }
        if !space && tw > width {
            // Longer than a whole line: cut where it fills it.
            let mut head = String::new();
            for ch in s.chars() {
                let mut t = head.clone();
                t.push(ch);
                if gfx::text_width(st_, &t) > width - w && !head.is_empty() {
                    line.push((b, core::mem::take(&mut head)));
                    lines.push(core::mem::take(&mut line));
                    w = 0;
                }
                head.push(ch);
            }
            w += gfx::text_width(st_, &head);
            line.push((b, head));
            continue;
        }
        line.push((b, s));
        w += tw;
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}
/// Draws at most `max` lines from the top-left; the last ends in "…" when text was left over.
fn draw_lines(st: Style, stb: Style, lines: &[Line], x: i32, y: i32, width: i32, line_h: i32, max: usize) {
    let n = lines.len().min(max);
    for (i, line) in lines[..n].iter().enumerate() {
        let mut cv = Canvas::new(width + 8, line_h + 10);
        let mut cx = 0;
        for (b, s) in line {
            let st_ = if *b { stb } else { st };
            let tw = gfx::text_width(st_, s);
            if s != " " {
                gfx::draw_text(&mut cv, cx, 0, st_, s);
                if *b {
                    for xx in cx..cx + tw {
                        cv.set(xx, gfx::ascent(st_) + 4);
                        cv.set(xx, gfx::ascent(st_) + 5);
                    }
                }
            }
            cx += if s == " " { gfx::text_width(st_, " ").max(4) } else { tw };
        }
        if i == n - 1 && lines.len() > n {
            gfx::draw_text(&mut cv, cx.min(width - 12), 0, st, "…");
        }
        gfx::blit(&cv, x, y + i as i32 * line_h, dsn::BLACK);
    }
}

// ---------------------------------------------------------------- the front
/// An opening quotation mark drawn as geometry: a round head with a tail tapering up and right.
fn draw_opening_quote(left: i32, bottom: i32, rad: i32) {
    let mut cv = Canvas::new(rad * 7, rad * 4);
    let (ox, oy) = (0, rad * 4); // canvas origin: `left`, `bottom - 4 rad`
    for m in 0..2 {
        let cx = ox + rad + m * (rad * 5 / 2);
        let cy = oy - rad - 1;
        cv.fill_disk(cx, cy, rad);
        let (x0, y0) = (cx - rad * 6 / 10, cy - rad * 4 / 10);
        let (x1, y1) = (cx - rad * 7 / 10, cy - rad * 21 / 10);
        let (x2, y2) = (cx + rad * 9 / 10, cy - rad * 25 / 10);
        const STEPS: i32 = 48;
        for t in 0..=STEPS {
            let u = STEPS - t;
            let x = (u * u * x0 + 2 * u * t * x1 + t * t * x2) / (STEPS * STEPS);
            let y = (u * u * y0 + 2 * u * t * y1 + t * t * y2) / (STEPS * STEPS);
            cv.fill_disk(x, y, ((rad * 55 * u + rad * 12 * t) / (100 * STEPS)).max(1));
        }
    }
    gfx::blit(&cv, left, bottom - rad * 4, dsn::BLACK);
}

/// The sentence it was saved from, the word marked, under a large opening quote; the book below.
fn draw_front(m: &Mined, body: &Rect) {
    let book_h = book().line_h - 6;
    let top = body.y;
    let bottom = body.y + body.h - book_h - 30;
    let quote_h = s(SERIF18B).line_h * 3 / 2;
    // The largest size at which the whole sentence fits; the smallest one ends in an ellipsis.
    let sizes = [(s(CARD), s(CARDB)), (s(SERIF12), s(SERIF12B))];
    let mut chosen = (sizes[0].0, sizes[0].1, Vec::new(), 1, 1);
    for (st, stb) in sizes {
        let line_h = st.line_h * 112 / 100;
        let max = ((bottom - top - quote_h) / line_h).max(1) as usize;
        let lines = rich_wrap(st, stb, &m.sentence, body.w, true);
        let fits = lines.len() <= max;
        chosen = (st, stb, lines, max, line_h);
        if fits {
            break;
        }
    }
    let (st, stb, lines, max, line_h) = chosen;
    let n = lines.len().min(max) as i32;
    let block_y = top + (bottom - top - quote_h - n * line_h) / 2;
    draw_opening_quote(body.x + 2, block_y + quote_h - 26, 9);
    draw_lines(st, stb, &lines, body.x, block_y + quote_h, body.w, line_h, max);
    if !m.book.is_empty() {
        dsn::fill(body.x, body.y + body.h - book_h - 14, body.w, 1, dsn::BLACK);
        gfx::text(body.x, body.y + body.h - book_h - 4, book(), &gfx::truncated(book(), &m.book, body.w), dsn::BLACK);
    }
}

/// Characters set turned in vertical text: the long vowel mark, dashes, ellipses and brackets.
fn turns_upright(c: char) -> bool { "ー―—…‥～〜「」『』（）()【】〈〉《》［］-".contains(c) }
/// Marks that may not start a column; they hang at the foot of the one before.
fn no_column_start(c: char) -> bool { "、。，．！？）」』】〉》ーぁぃぅぇぉっゃゅょァィゥェォッャュョ…".contains(c) }
fn small_kana(c: char) -> bool { "ぁぃぅぇぉっゃゅょゎァィゥェォッャュョヮ".contains(c) }

/// Tategaki: columns of `cell`-high rows, right to left, `pitch` apart, the first at `first_left`.
/// The saved word is bold with a rule beside it. Returns the columns used (more than max_cols: it
/// did not fit).
fn draw_vertical(st: Style, text: &str, first_left: i32, top: i32, cell: i32, pitch: i32, rows: i32, max_cols: i32, draw: bool) -> i32 {
    let (mut col, mut row) = (0, 0);
    let mut bold = false;
    for c in text.chars() {
        match c {
            '\u{1}' => { bold = true; continue; }
            '\u{2}' => { bold = false; continue; }
            ' ' | '\u{3000}' | '\n' => continue,
            _ => {}
        }
        if row >= rows {
            if !(row == rows && no_column_start(c)) {
                col += 1;
                row = 0;
            }
        } else if row == rows - 1 && "（「『【〈《(".contains(c) {
            col += 1;
            row = 0;
        }
        if col >= max_cols {
            return max_cols + 1;
        }
        if draw {
            let x = first_left - col * pitch;
            let y = top + row * cell;
            let mut s = String::new();
            s.push(c);
            let mut cv = Canvas::new(gfx::text_width(st, &s) + 6, st.line_h + 8);
            gfx::draw_text(&mut cv, 1, 0, st, &s);
            if bold {
                cv.embolden();
            }
            if turns_upright(c) {
                cv = cv.turned_ccw().turned_ccw().turned_ccw();
            }
            if let Some(ib) = cv.ink_box() {
                if "、。，．".contains(c) {
                    // 、 and 。 sit at the head of their cell, to the right.
                    gfx::blit(&cv, x + cell * 3 / 4 - ib.0 - ib.2 / 2, y + cell / 4 - ib.1 - ib.3 / 2, dsn::BLACK);
                } else {
                    let dx = if small_kana(c) { cell / 8 } else { 0 };
                    blit_centered(&cv, ib, x + cell / 2 + dx, y + cell / 2);
                }
            }
            if bold {
                dsn::fill(x + cell + 3, y, 2, cell, dsn::BLACK);
            }
        }
        row += 1;
    }
    col + 1
}

/// Japanese reads in columns on the front, as in a book: the sentence starts against the spine on
/// the right, and the book's title stands in its own column at the bottom left.
fn draw_front_vertical(m: &Mined, body: &Rect) {
    let em = |st: Style| gfx::text_width(st, "国").max(1);
    let book_cell = em(s(CJK12));
    let book_w = if m.book.is_empty() { 0 } else { book_cell + 16 };
    let (top, bottom) = (body.y, body.y + body.h);
    let mut pick = (s(CJK14), 1, 1, 1, 1);
    for st in [s(CJK14), s(CJK12)] {
        let cell = em(st);
        let pitch = cell * 150 / 100;
        let rows = ((bottom - top) / cell).max(1);
        let max_cols = ((body.w - book_w - cell) / pitch + 1).max(1);
        pick = (st, cell, pitch, rows, max_cols);
        if draw_vertical(st, &m.sentence, 0, top, cell, pitch, rows, max_cols, false) <= max_cols {
            break;
        }
    }
    let (st, cell, pitch, rows, max_cols) = pick;
    draw_vertical(st, &m.sentence, body.x + body.w - cell, top, cell, pitch, rows, max_cols, true);
    if !m.book.is_empty() {
        // One column only: a title longer than the card is cut.
        let book_rows = ((bottom - top) / book_cell).max(1) as usize;
        let book: String = m.book.chars().filter(|&c| c != ' ').take(book_rows).collect();
        let n = book.chars().count() as i32;
        draw_vertical(s(CJK12), &book, body.x, bottom - n * book_cell, book_cell, book_cell, n, 1, true);
    }
}

// ---------------------------------------------------------------- the back
fn numbered(row: &str) -> Option<(&str, &str)> {
    let dot = row.find(". ")?;
    (dot >= 1 && dot <= 2 && row.as_bytes()[0].is_ascii_digit()).then(|| (&row[..dot], &row[dot + 2..]))
}

/// The reading (beside the word on the spine for Japanese), the senses, and at the foot the
/// sentence and the book. The senses get the room above three sentence lines; the sentence then
/// grows into whatever they leave free.
fn draw_back(m: &Mined, body: &Rect, japanese: bool) {
    let mut y = body.y;
    if !m.reading.is_empty() && !japanese {
        // A long reading shrinks to fit the line, and wraps only past the smallest size.
        let st = if gfx::text_width(s(CARD), &m.reading) <= body.w { s(CARD) } else { s(SERIF12) };
        let lines = rich_wrap(st, st, &m.reading, body.w, false);
        let n = lines.len().min(2);
        draw_lines(st, st, &lines, body.x, y, body.w, st.line_h, 2);
        y += n as i32 * st.line_h + 8;
        dsn::fill(body.x, y, body.w, 1, dsn::BLACK);
        y += 18;
    }
    const MIN_SENT_LINES: i32 = 3;
    let sent_h = s(SERIF12).line_h * 110 / 100;
    let book_h = if m.book.is_empty() { 0 } else { book().line_h };
    let foot_bottom = body.y + body.h - book_h - 22;
    let senses_top = y;

    // One numbered row per sense, the numbers in their own bold column; the part-of-speech line
    // and the "→" grammar notes as matcha keeps them.
    let def_h = s(SERIF12).line_h * 108 / 100;
    let note_h = s(SERIF12I).line_h - 4;
    let num_w = gfx::text_width(s(SERIF12B), "00") + 8;
    let structured = m.definition.lines().any(|r| numbered(r).is_some());
    let senses = |limit: i32, draw: bool| -> i32 {
        let mut y = senses_top;
        let mut seen = false;
        for row in m.definition.lines() {
            if y + note_h > limit {
                break;
            }
            if row.trim().is_empty() {
                continue;
            }
            if let (true, Some((num, rest))) = (structured, numbered(row)) {
                if y + def_h > limit {
                    break;
                }
                seen = true;
                let lines = rich_wrap(s(SERIF12), s(SERIF12B), rest, body.w - num_w, false);
                let rows = (((limit - y) / def_h).max(1) as usize).min(lines.len());
                if draw {
                    gfx::text(body.x, y, s(SERIF12B), num, dsn::BLACK);
                    draw_lines(s(SERIF12), s(SERIF12B), &lines, body.x + num_w, y, body.w - num_w, def_h, rows);
                }
                y += rows as i32 * def_h + 4;
            } else if structured && row.starts_with("→ ") {
                if draw {
                    gfx::text(body.x + num_w, y, s(SERIF12I), &gfx::truncated(s(SERIF12I), &row[4..], body.w - num_w), dsn::BLACK);
                }
                y += note_h + 2;
            } else if structured && !seen {
                if draw {
                    gfx::text(body.x, y, s(SERIF12B), &gfx::truncated(s(SERIF12B), row, body.w), dsn::BLACK);
                }
                y += note_h + 4;
            } else if !structured {
                let rows = (limit - y) / def_h;
                if rows <= 0 {
                    break;
                }
                let lines = rich_wrap(s(SERIF12), s(SERIF12B), row, body.w, false);
                let n = lines.len().min(rows as usize);
                if draw {
                    draw_lines(s(SERIF12), s(SERIF12B), &lines, body.x, y, body.w, def_h, n);
                }
                y += n as i32 * def_h + 4;
            }
        }
        y
    };
    let senses_end = senses(foot_bottom - MIN_SENT_LINES * sent_h - 26, false);
    let room = MIN_SENT_LINES.max((foot_bottom - senses_end - 26) / sent_h) as usize;
    let sent = rich_wrap(s(SERIF12), s(SERIF12B), &m.sentence, body.w, true);
    let sent_lines = sent.len().min(room) as i32;
    let foot_top = foot_bottom - sent_lines * sent_h;
    senses(foot_top - 12, true);
    if sent_lines > 0 {
        dsn::fill(body.x, foot_top, body.w, 1, dsn::BLACK);
        draw_lines(s(SERIF12), s(SERIF12B), &sent, body.x, foot_top + 14, body.w, sent_h, room);
    }
    if book_h > 0 {
        gfx::text(body.x, body.y + body.h - book_h + 6, book(), &gfx::truncated(book(), &m.book, body.w), dsn::BLACK);
    }
}
