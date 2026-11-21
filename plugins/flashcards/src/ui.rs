//! The screens, laid out as matcha's FlashcardsActivity, FlashcardReviewActivity and AnkiSyncActivity:
//! the deck list with its big count, the card with its double rule and rating boxes, the session's
//! seal, and the sync's status lines.
use crate::gfx::{self, Canvas, SMALL, UI10, UI10B, UI12, UI12B, CARD, CARDB, HERO_NUM, SERIF18B};
use crate::store::{self, DeckSummary, Interval, Session};
use crate::tabbar::{self, Bar, Tab};
use crate::{dsn, fsrs, icons, sync};
use crate::i18n::{tr, trf, count, Arg};
use alloc::{format, string::String, vec::Vec};

static mut SCREEN: (i32, i32) = (480, 800);
static mut TOUCH: bool = false;
static mut INSET_TOP: i32 = 0;
pub fn screen() -> (i32, i32) { unsafe { SCREEN } }
fn touch() -> bool { unsafe { TOUCH } }

// Lyra's header, as matcha draws it: battery in the corner strip, the title below.
const HDR_TOP: i32 = 10;
const HDR_HEIGHT: i32 = 84;
const HDR_STRIP: i32 = 20;
const HDR_SIDE: i32 = 18;
fn band_top() -> i32 { (HDR_TOP - unsafe { INSET_TOP }).max(0) }
/// The screen's title; a key of the word list, or text shown as it is.
fn header(title: &str) { header_with(title, false) }
const HDR_BACK: i32 = 44;
/// A sub page's header: on touch boards a back arrow leads the title.
fn header_with(title: &str, back: bool) {
    let title = &tr(title);
    let (w, _) = screen();
    let top = band_top();
    let b = dsn::battery();
    if b >= 0 {
        let pct = format!("{}%", b & 0xff);
        let (gw, gh) = (16, 12);
        let gx = w - HDR_SIDE - 4 - gw - 2;
        let gy = top + (HDR_STRIP - gh) / 2;
        dsn::stroke(gx, gy, gw, gh, dsn::BLACK, 1, 0);
        dsn::fill(gx + gw, gy + 3, 2, gh - 6, dsn::BLACK);
        let fill = (gw - 4) * (b & 0xff).min(100) / 100;
        if fill > 0 {
            dsn::fill(gx + 2, gy + 2, fill, gh - 4, dsn::BLACK);
        }
        let tw = gfx::text_width(SMALL, &pct);
        gfx::text(gx - 6 - tw, top + (HDR_STRIP - SMALL.line_h) / 2, SMALL, &pct, dsn::BLACK);
    }
    let row_top = top + HDR_STRIP;
    // Right to left: the title stands at the right (the settings icon then sits at the left).
    let rtl = crate::i18n::rtl();
    let back = back && touch();
    if back {
        let cy = row_top + (HDR_HEIGHT - HDR_STRIP) / 2 - 2;
        let (cx, d) = if rtl { (w - HDR_SIDE - HDR_BACK / 2 + 4, -1) } else { (HDR_SIDE + HDR_BACK / 2 - 4, 1) };
        dsn::line(cx - 10 * d, cy, cx + 10 * d, cy, 3, dsn::BLACK);
        dsn::line(cx - 10 * d, cy, cx - 2 * d, cy - 8, 3, dsn::BLACK);
        dsn::line(cx - 10 * d, cy, cx - 2 * d, cy + 8, 3, dsn::BLACK);
    }
    let shift = if back { HDR_BACK + 4 } else { 0 };
    let tx = if rtl { w - HDR_SIDE - shift - gfx::text_width(UI12B, title) } else { HDR_SIDE + shift };
    gfx::text(tx, row_top + (HDR_HEIGHT - HDR_STRIP - UI12B.line_h) / 2 - 2, UI12B, title, dsn::BLACK);
}
fn content_top() -> i32 { band_top() + HDR_HEIGHT }

/// The firmware's button hints, on boards without touch (matcha reserves room for them).
const HINTS_H: i32 = 46;
fn hints(labels: [&str; 4]) {
    if touch() {
        return;
    }
    let (w, h) = screen();
    let bw = (w - 5 * 12) / 4;
    for (i, l) in labels.iter().enumerate() {
        if l.is_empty() {
            continue;
        }
        let l = &tr(l);
        let x = 12 + i as i32 * (bw + 12) + if i >= 2 { 8 } else { 0 };
        dsn::stroke(x, h - HINTS_H + 6, bw - 8, HINTS_H, dsn::BLACK, 1, 6);
        gfx::text_centered(x + (bw - 8) / 2, h - HINTS_H + 14, SMALL, l, dsn::BLACK);
    }
}
fn safe_bottom() -> i32 { screen().1 - if touch() { 0 } else { HINTS_H } }

// ---------------------------------------------------------------- the deck list
const ROW_PITCH: i32 = 54;
const ROW_INSET: i32 = 16;
const SIDE_PAD: i32 = 20;
const NUMERAL_GAP: i32 = 14;
const ORNAMENT_ABOVE: i32 = 30;
const ORNAMENT_BELOW: i32 = 26;
const TOP_GAP_MAX: i32 = 24;

struct DeckList {
    decks: Vec<DeckSummary>,
    sync_row: bool,
    hint: Option<&'static str>,
    selected: i32,
    top_row: i32,
    bar: Bar,
    band: Option<usize>, // the cursor's slot while it is in the tab bar
    row_ys: Vec<(i32, i32)>, // (row index, y) as drawn, for taps
}

impl DeckList {
    fn load() -> DeckList {
        crate::mined::import();
        let services = sync::services();
        DeckList {
            decks: store::list_decks(),
            sync_row: services.is_ok(),
            hint: services.err(),
            selected: 0,
            top_row: 0,
            bar: Bar::new(Tab::Flashcards),
            band: None,
            row_ys: Vec::new(),
        }
    }
    fn rows(&self) -> i32 { self.decks.len() as i32 + self.sync_row as i32 }
    /// The settings icon beside the title is the row above the first (index -1).
    fn settings_row(&self) -> i32 { -1 }
    fn due_total(&self) -> u32 { self.decks.iter().map(|d| d.due + d.fresh).sum() }
    fn hero_height(&self) -> i32 {
        (if self.due_total() > 0 { HERO_NUM.line_h * 3 / 4 + NUMERAL_GAP } else { 0 }) + UI12.line_h + ORNAMENT_ABOVE + ORNAMENT_BELOW
    }

    /// Settings: an icon at the right end of the title row (matcha's tab icon for Settings).
    fn settings_icon_rect() -> Rect {
        let (w, _) = screen();
        let y = band_top() + HDR_STRIP + (HDR_HEIGHT - HDR_STRIP - 32) / 2 - 2;
        Rect { x: if crate::i18n::rtl() { HDR_SIDE } else { w - HDR_SIDE - 32 }, y, w: 32, h: 32 }
    }
    fn draw_settings_icon(&self) {
        let r = Self::settings_icon_rect();
        if self.selected == self.settings_row() && self.band.is_none() {
            dsn::stroke(r.x - 8, r.y - 8, r.w + 16, r.h + 16, dsn::BLACK, 2, 10);
        }
        gfx::icon(r.x, r.y, 32, &crate::tabicons::SETTINGS_TAB_ICON, dsn::BLACK);
    }
    fn draw(&mut self) {
        let (w, h) = screen();
        dsn::clear();
        gfx::set_script(gfx::detect_script(self.decks.iter().map(|d| d.label.as_str())));
        header("flashcards");
        self.row_ys.clear();
        let left = SIDE_PAD + ROW_INSET;
        let right = w - SIDE_PAD - ROW_INSET;
        let _ = h;
        let h = tabbar::Bar::top(); // the tab bar takes the place of the button hints
        self.draw_settings_icon();
        if self.rows() == 0 {
            let mid = (content_top() + h) / 2;
            gfx::text_centered(w / 2, mid - 40, UI12B, &tr("no_decks"), dsn::BLACK);
            let msg = &tr(self.hint.unwrap_or("setup_hint"));
            for (i, l) in gfx::wrap(UI10, msg, w - 80).iter().enumerate().take(4) {
                gfx::text_centered(w / 2, mid - 8 + i as i32 * UI10.line_h, UI10, l, dsn::BLACK);
            }
            self.bar.draw(self.band);
            dsn::refresh();
            return;
        }
        let top = content_top();
        let avail = h - top;
        let block = self.hero_height() + self.rows() * ROW_PITCH;
        let hero_top = top + 10i32.max(TOP_GAP_MAX.min((avail - block) * 2 / 5));
        let due = self.due_total();
        let mut y = hero_top;
        let caption = if due > 0 {
            let n = format!("{}", due);
            gfx::text_centered(w / 2, y - HERO_NUM.line_h / 8, HERO_NUM, &n, dsn::BLACK);
            y += HERO_NUM.line_h * 3 / 4 + NUMERAL_GAP;
            count("to_review", due as i64)
        } else if store::Settings::read().vacation {
            tr("on_vacation")
        } else {
            tr("nothing_due")
        };
        let cw = gfx::text_width(UI12, &caption);
        gfx::text(w / 2 - cw / 2, y, UI12, &caption, dsn::BLACK);
        y += UI12.line_h + ORNAMENT_ABOVE;
        // Ornament: two short rules meeting a small diamond, as wide as the caption.
        let half = (cw / 2).max(60);
        dsn::fill(w / 2 - half, y, half - 12, 1, dsn::BLACK);
        dsn::fill(w / 2 + 12, y, half - 12, 1, dsn::BLACK);
        for i in 0..=5 {
            dsn::fill(w / 2 - i, y - 5 + i, 2 * i + 1, 1, dsn::BLACK);
        }
        for i in 0..5 {
            dsn::fill(w / 2 - i, y + 5 - i, 2 * i + 1, 1, dsn::BLACK);
        }
        let list_top = hero_top + self.hero_height();
        let bottom = h;
        let fit = ((bottom - list_top) / ROW_PITCH).max(1);
        if self.selected < self.top_row {
            self.top_row = self.selected.max(0);
        }
        if self.selected >= self.top_row + fit {
            self.top_row = self.selected - fit + 1;
        }
        for i in self.top_row..self.rows() {
            let row_y = list_top + (i - self.top_row) * ROW_PITCH;
            if row_y + ROW_PITCH > bottom {
                break;
            }
            self.row_ys.push((i, row_y));
            let focused = i == self.selected && self.band.is_none();
            if self.sync_row && i == self.decks.len() as i32 {
                // Like Insights' Refresh row: icon and small text, outlined only while focused.
                let label = &tr("sync_flashcards");
                let row_w = 24 + 8 + gfx::text_width(SMALL, label);
                let rx = (left + right - row_w) / 2;
                if focused {
                    dsn::stroke(rx - 16, row_y + 2, row_w + 32, ROW_PITCH - 4, dsn::BLACK, 2, 12);
                }
                gfx::icon(rx, row_y + (ROW_PITCH - 24) / 2, 24, &icons::REFRESH_24, dsn::BLACK);
                gfx::text(rx + 32, row_y + (ROW_PITCH - SMALL.line_h) / 2, SMALL, label, dsn::BLACK);
                continue;
            }
            let d = &self.decks[i as usize];
            let ty = row_y + (ROW_PITCH - UI12.line_h) / 2;
            let baseline = ty + 24;
            if focused {
                dsn::fill_round(left - 8, row_y + 4, right - left + 16, ROW_PITCH - 8, dsn::LIGHT_GRAY, 6);
            }
            let value = format!("{}", d.due + d.fresh);
            let vw = gfx::text_width(UI12, &value);
            let name = gfx::truncated(UI12, &d.label, right - left - vw - 24);
            let nw = gfx::text_width(UI12, &name);
            let rtl = crate::i18n::rtl();
            // Right to left: the name at the right, its count at the left.
            gfx::text(if rtl { right - nw } else { left }, ty, UI12, &name, dsn::BLACK);
            gfx::text(if rtl { left } else { right - vw }, ty, UI12, &value, dsn::BLACK);
            let (mut x, end) = if rtl { (left + vw + 12, right - nw - 12) } else { (left + nw + 12, right - vw - 12) };
            while x < end {
                dsn::fill(x, baseline - 1, 1, 1, dsn::BLACK);
                x += 5;
            }
        }
        self.bar.draw(self.band);
        dsn::refresh();
    }
}

// ---------------------------------------------------------------- the review
const PAD: i32 = 22;
const BAND_HEIGHT: i32 = 80;

struct Rect {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
}
impl Rect {
    fn contains(&self, x: i32, y: i32) -> bool { x >= self.x && y >= self.y && x < self.x + self.w && y < self.y + self.h }
}
fn card_rect() -> Rect { let (w, _) = screen(); Rect { x: PAD, y: PAD, w: w - 2 * PAD, h: safe_bottom() - PAD - 8 - BAND_HEIGHT } }
fn ratings_rect() -> Rect { let c = card_rect(); Rect { x: c.x, y: c.y + c.h + 18, w: c.w, h: BAND_HEIGHT - 18 } }
fn body_rect() -> Rect { let c = card_rect(); Rect { x: c.x + 6 + 18, y: c.y + 6 + 16, w: c.w - 12 - 36, h: c.h - 12 - 32 - 18 } }

/// A laid-out piece of a card side.
enum Item {
    Line(String),
    Rule,
    Image(Canvas),
    Missing,
    Styled(Vec<crate::cardhtml::Item>), // a whole page of an Anki card laid out with its CSS
}
fn item_height(it: &Item) -> i32 {
    match it {
        Item::Line(_) => CARD.line_h,
        Item::Rule => 30,
        Item::Image(c) => c.h + 12,
        Item::Missing => 72,
        Item::Styled(_) => 0,
    }
}
fn load_image(name: &str) -> Item {
    let Some(b) = store::read_file(&sync::image_file(name)) else { return Item::Missing };
    if b.len() < 4 {
        return Item::Missing;
    }
    let (w, h) = (u16::from_le_bytes([b[0], b[1]]) as i32, u16::from_le_bytes([b[2], b[3]]) as i32);
    let rb = ((w + 7) / 8) as usize;
    let mut c = Canvas::new(w, h);
    for y in 0..h as usize {
        for x in 0..w as usize {
            if b.get(4 + y * rb + x / 8).is_some_and(|v| v & (0x80 >> (x % 8)) != 0) {
                c.set(x as i32, y as i32);
            }
        }
    }
    Item::Image(c)
}
/// A side's lines wrapped to the card, split into pages.
fn layout(side: &str) -> Vec<Vec<Item>> {
    let body = body_rect();
    let mut items = Vec::new();
    for line in side.lines() {
        let (kind, rest) = line.split_at(line.char_indices().nth(1).map(|(i, _)| i).unwrap_or(line.len()));
        match kind {
            "T" => items.extend(gfx::wrap(CARD, rest, body.w).into_iter().map(Item::Line)),
            "R" => items.push(Item::Rule),
            "I" => items.push(load_image(rest)),
            _ => {}
        }
    }
    let mut pages: Vec<Vec<Item>> = Vec::new();
    let mut page = Vec::new();
    let mut used = 0;
    for it in items {
        let h = item_height(&it);
        if used + h > body.h && !page.is_empty() {
            pages.push(core::mem::take(&mut page));
            used = 0;
        }
        used += h;
        page.push(it);
    }
    if !page.is_empty() || pages.is_empty() {
        pages.push(page);
    }
    pages
}
/// A line with \x01..\x02 around bold parts, centred like Anki's default card style.
fn draw_rich(cx: i32, y: i32, line: &str) {
    let parts: Vec<(bool, &str)> = {
        let mut v = Vec::new();
        let mut bold = false;
        for seg in line.split(['\u{1}', '\u{2}']) {
            v.push((bold, seg));
            bold = !bold;
        }
        // split alternates at every marker: an opening \x01 starts bold, \x02 ends it
        v
    };
    let width: i32 = parts.iter().map(|(b, s)| gfx::text_width(if *b { CARDB } else { CARD }, s)).sum();
    let mut c = Canvas::new(width + 4, CARD.line_h + 8);
    let mut x = 0;
    for (b, s) in parts {
        let st = if b { CARDB } else { CARD };
        gfx::draw_text(&mut c, x, 0, st, s);
        x += gfx::text_width(st, s);
    }
    gfx::blit(&c, cx - width / 2, y, dsn::BLACK);
}

fn format_interval(i: Interval) -> String {
    let v = i.value as u32;
    if i.minutes {
        return if v < 60 { trf("interval_m", &[Arg::N(v as i64)]) } else { trf("interval_h", &[Arg::N(((v + 30) / 60) as i64)]) };
    }
    match v {
        0 => tr("interval_lt1d"),
        1..=29 => trf("interval_d", &[Arg::N(v as i64)]),
        30..=364 => trf("interval_mo", &[Arg::N(((v + 15) / 30) as i64)]),
        _ => trf("interval_y", &[Arg::S(&format!("{}.{}", v / 365, (v % 365) * 10 / 365))]),
    }
}
const RATINGS: [&str; 4] = ["again", "hard", "good", "easy"];

struct Review {
    session: Session,
    show_back: bool,
    selected: usize,
    pages: Vec<Vec<Item>>,
    page: usize,
    intervals: [Interval; 4],
    mined: Option<store::Mined>, // a word mined in matcha: drawn on matcha's spine card
    keys: bool,                  // a key was used: the selected rating is shown, touch board or not
}

impl Review {
    fn open(key: &str) -> Review {
        let mut r = Review { session: Session::open(key), show_back: false, selected: 2, pages: Vec::new(), page: 0, intervals: Default::default(), mined: None, keys: false };
        r.load_card();
        r
    }
    fn load_card(&mut self) {
        self.show_back = false;
        self.selected = 2;
        self.layout_side();
        self.intervals = self.session.previews();
    }
    fn layout_side(&mut self) {
        self.page = 0;
        self.mined = self.session.current().and_then(|c| store::decode_mined(c.0));
        // An Anki card's CJK text in the cut of its language, read from both sides.
        if let Some((f, b, _)) = self.session.current() {
            gfx::set_script(gfx::detect_script([f, b].into_iter()));
        }
        if self.mined.is_some() {
            self.pages = Vec::new();
            return;
        }
        self.pages = match self.session.current() {
            Some((f, b, false)) => {
                let side = if self.show_back { b } else { f };
                match store::decode_html(side) {
                    Some((css, html)) => {
                        let body = body_rect();
                        crate::cardhtml::layout(html, &css, body.w, body.h, self.session.lang()).into_iter().map(|p| alloc::vec![Item::Styled(p)]).collect()
                    }
                    None => layout(side),
                }
            }
            _ => Vec::new(),
        };
    }
    fn rate(&mut self, i: usize) {
        self.session.rate(fsrs::Rating::from(i as u8 + 1));
        self.load_card();
    }

    fn draw(&self) {
        let (w, _) = screen();
        dsn::clear();
        if self.session.finished() {
            self.draw_done();
            dsn::refresh();
            return;
        }
        let c = card_rect();
        dsn::stroke(c.x, c.y, c.w, c.h, dsn::BLACK, 2, 0);
        dsn::stroke(c.x + 6, c.y + 6, c.w - 12, c.h - 12, dsn::BLACK, 1, 0);
        let body = body_rect();
        let cx = body.x + body.w / 2;
        if let Some(m) = &self.mined {
            crate::spine::draw(m, &crate::spine::Rect { x: c.x + 6, y: c.y + 6, w: c.w - 12, h: c.h - 12 }, self.show_back, self.session.lang());
        } else if self.pages.is_empty() || self.pages.iter().all(|p| p.is_empty()) {
            let too_large = self.session.current().is_some_and(|c| c.2);
            let msg = match (too_large, self.session.is_migaku()) {
                (true, false) => tr("too_large_anki"),
                (true, true) => tr("too_large_migaku"),
                _ => tr("card_error"),
            };
            let msg = msg.as_str();
            let lines = gfx::wrap(UI12, msg, c.w - 52);
            let top = c.y + (c.h - lines.len() as i32 * UI12.line_h) / 2;
            for (i, l) in lines.iter().enumerate() {
                gfx::text_centered(w / 2, top + i as i32 * UI12.line_h, UI12, l, dsn::BLACK);
            }
        } else {
            let mut y = body.y;
            for it in &self.pages[self.page.min(self.pages.len() - 1)] {
                match it {
                    Item::Line(l) => draw_rich(cx, y, l),
                    Item::Rule => dsn::fill(cx - body.w / 8, y + 14, body.w / 4, 2, dsn::BLACK),
                    Item::Image(img) => gfx::blit(img, cx - img.w / 2, y + 6, dsn::BLACK),
                    Item::Missing => dsn::stroke(cx - 40, y + 6, 80, 60, dsn::BLACK, 1, 0),
                    Item::Styled(items) => crate::cardhtml::draw(items, body.x, y, body.w),
                }
                y += item_height(it);
            }
            if self.pages.len() > 1 {
                let count = format!("{} / {}", self.page + 1, self.pages.len());
                let tw = gfx::text_width(SMALL, &count);
                gfx::text(c.x + 6 + c.w - 12 - 18 - tw, c.y + 6 + c.h - 12 - 26, SMALL, &count, dsn::BLACK);
            }
        }
        let band = ratings_rect();
        if self.show_back {
            self.draw_ratings(&band);
        } else {
            let counter = format!("{} / {}", self.session.reviewed() + 1, self.session.reviewed() + self.session.remaining());
            gfx::text_centered(w / 2, band.y + 10, SMALL, &counter, dsn::BLACK);
            gfx::text_centered(w / 2, band.y + 34, UI10, &tr(if touch() { "tap_to_turn" } else { "turn" }), dsn::BLACK);
        }
        hints(["back", if self.show_back { "rate" } else { "turn" }, if self.show_back { "<" } else { "" }, if self.show_back { ">" } else { "" }]);
        dsn::refresh();
    }

    // Four ruled boxes, each the rating's name over when the card comes back.
    fn draw_ratings(&self, band: &Rect) {
        const GAP: i32 = 8;
        const BOX_H: i32 = 60;
        let box_w = (band.w - 3 * GAP) / 4;
        let text_top = band.y + (BOX_H - UI10B.line_h - SMALL.line_h - 2) / 2;
        for i in 0..4 {
            // Right to left: Again at the right, Easy at the left.
            let slot = if crate::i18n::rtl() { 3 - i as i32 } else { i as i32 };
            let bx = band.x + slot * (box_w + GAP);
            let filled = (!touch() || self.keys) && i == self.selected;
            if filled {
                dsn::fill_round(bx, band.y, box_w, BOX_H, dsn::BLACK, 10);
            } else {
                dsn::stroke(bx, band.y, box_w, BOX_H, dsn::BLACK, 2, 10);
            }
            let ink = if filled { dsn::WHITE } else { dsn::BLACK };
            gfx::text_centered(bx + box_w / 2, text_top, UI10B, &tr(RATINGS[i]), ink);
            gfx::text_centered(bx + box_w / 2, text_top + UI10B.line_h + 2, SMALL, &format_interval(self.intervals[i]), ink);
        }
    }

    // The session's seal, its count, how often each rating was given, and the way out.
    fn draw_done(&self) {
        let (w, h) = screen();
        let a = Rect { x: 40, y: 30, w: w - 80, h: safe_bottom() - 60 };
        let cx = a.x + a.w / 2;
        let mut y = a.y + a.h / 2 - 190;
        const SEAL: i32 = 84;
        dsn::stroke(cx - SEAL / 2, y, SEAL, SEAL, dsn::BLACK, 2, (SEAL / 2) as u32);
        dsn::line(cx - 20, y + 44, cx - 6, y + 58, 3, dsn::BLACK);
        dsn::line(cx - 6, y + 58, cx + 22, y + 28, 3, dsn::BLACK);
        y += SEAL + 30;
        gfx::text_centered(cx, y, SERIF18B, &tr("all_caught_up"), dsn::BLACK);
        y += SERIF18B.line_h + 6;
        let n = self.session.reviewed();
        gfx::text_centered(cx, y, UI10, &count("cards_reviewed", n as i64), dsn::BLACK);
        y += 48;
        let gap = 10;
        let box_w = (a.w - 3 * gap) / 4;
        for i in 0..4 {
            let bx = a.x + i as i32 * (box_w + gap);
            dsn::stroke(bx, y, box_w, 96, dsn::BLACK, 2, 10);
            gfx::text_centered(bx + box_w / 2, y + 10, SERIF18B, &format!("{}", self.session.rating_count(i)), dsn::BLACK);
            gfx::text_centered(bx + box_w / 2, y + 96 - 30, UI10, &tr(RATINGS[i]), dsn::BLACK);
        }
        let by = a.y + a.h - 56 - 8;
        dsn::fill_round(a.x, by, a.w, 56, dsn::BLACK, 12);
        gfx::text_centered(cx, by + (56 - UI12B.line_h) / 2, UI12B, &tr("flashcards"), dsn::WHITE);
        let _ = h;
    }

    /// Runs until the user leaves; the session is saved on the way out.
    fn run(&mut self) {
        if self.session.total() == 0 {
            return;
        }
        self.draw();
        loop {
            let e = dsn::wait_event();
            let t = dsn::ev_type(e);
            let done = self.session.finished();
            let multi = self.pages.len() > 1;
            if matches!(t, dsn::EV_CONFIRM | dsn::EV_NEXT | dsn::EV_PREV) {
                self.keys = true;
            }
            match t {
                dsn::EV_BACK => break,
                _ if done => {
                    if t == dsn::EV_CONFIRM || t == dsn::EV_TAP {
                        break;
                    }
                    continue;
                }
                dsn::EV_TAP => {
                    let (x, y) = (dsn::ev_x(e), dsn::ev_y(e));
                    let band = ratings_rect();
                    if self.show_back && band.contains(x, y) {
                        let slot = ((x - band.x) * 4 / band.w.max(1)).clamp(0, 3) as usize;
                        self.rate(if crate::i18n::rtl() { 3 - slot } else { slot });
                    } else if card_rect().contains(x, y) {
                        self.show_back = !self.show_back;
                        self.layout_side();
                    } else {
                        continue;
                    }
                }
                // A long side pages first, as the side buttons do in matcha.
                dsn::EV_NEXT | dsn::EV_SWIPE_UP if multi && self.page + 1 < self.pages.len() => self.page += 1,
                dsn::EV_PREV | dsn::EV_SWIPE_DOWN if multi && self.page > 0 => self.page -= 1,
                dsn::EV_CONFIRM | dsn::EV_NEXT | dsn::EV_PREV if !self.show_back => {
                    self.show_back = true;
                    self.layout_side();
                }
                dsn::EV_NEXT | dsn::EV_SWIPE_LEFT => self.selected = (self.selected + 1) % 4,
                dsn::EV_PREV | dsn::EV_SWIPE_RIGHT => self.selected = (self.selected + 3) % 4,
                dsn::EV_CONFIRM => self.rate(self.selected),
                _ => continue,
            }
            self.draw();
        }
        self.session.save();
    }
}

// ---------------------------------------------------------------- the sync
struct SyncScreen {
    title: String,
}
impl SyncScreen {
    fn draw(&self, failed: bool, message: &str, detail: &str, finished: bool) {
        let (w, h) = screen();
        dsn::clear();
        header(&self.title);
        let top = (content_top() + safe_bottom()) / 2 - 40;
        if failed {
            gfx::text_centered(w / 2, top - 40, UI12B, &tr("sync_stopped"), dsn::BLACK);
        }
        for (i, l) in gfx::wrap(UI10, message, w - 60).iter().enumerate().take(4) {
            gfx::text_centered(w / 2, top + i as i32 * UI10.line_h, UI10, l, dsn::BLACK);
        }
        if !detail.is_empty() {
            gfx::text_centered(w / 2, top + 110, UI10, detail, dsn::BLACK);
        }
        if finished {
            if touch() {
                let by = safe_bottom() - 56 - 16;
                dsn::fill_round(30, by, w - 60, 56, dsn::BLACK, 12);
                gfx::text_centered(w / 2, by + (56 - UI12B.line_h) / 2, UI12B, &tr("done"), dsn::WHITE);
            }
            hints(["back", "done", "", ""]);
        }
        let _ = h;
        dsn::refresh();
    }
}
impl sync::Progress for SyncScreen {
    fn show(&mut self, message: &str, detail: &str) { self.draw(false, message, detail, false); }
}

fn run_sync() {
    let mut screen_ = SyncScreen { title: tr("sync_flashcards") };
    let services = match sync::services() {
        Ok(s) => s,
        Err(msg) => {
            screen_.draw(true, &tr(msg), "", true);
            wait_dismiss();
            return;
        }
    };
    screen_.draw(false, &tr("connecting_wifi"), "", false);
    if dsn::wifi_connect() != 0 {
        screen_.draw(true, &tr("wifi_failed"), "", true);
        wait_dismiss();
        return;
    }
    let mut summary = String::new();
    let mut keys: Vec<String> = Vec::new();
    let mut failure: Option<String> = None;
    for svc in &services {
        let mut totals = sync::Totals::default();
        match sync::sync_service(svc, &mut screen_, &mut totals, &mut keys) {
            Ok(()) => {
                if !summary.is_empty() {
                    summary += "; ";
                }
                summary += &trf("service_done", &[Arg::S(svc.name), Arg::S(&count("new_cards", totals.added as i64)),
                                                  Arg::S(&count("reviews_received", totals.pulled as i64)), Arg::N(totals.pushed as i64)]);
                if totals.remote_skipped {
                    summary += ". ";
                    summary += &tr("ankiweb_not_synced");
                }
            }
            Err((deck, e)) => {
                failure = Some(if deck.is_empty() { sync::error_text(svc.name, &e) } else {
                    format!("{}\n{}", sync::error_text(svc.name, &e), deck)
                });
                break;
            }
        }
    }
    // The decks the list shows: those synced now, and earlier ones of a service that stopped.
    let mut all = store::deck_keys();
    for k in keys {
        if !all.contains(&k) {
            all.push(k);
        }
    }
    store::write_json("decks.json", &serde_json::json!(all));
    match failure {
        Some(msg) => screen_.draw(true, &msg, "", true),
        None => screen_.draw(false, &summary, "", true),
    }
    wait_dismiss();
}
/// matcha's Test Connection: each service's server asked for its decks, without syncing.
fn test_connection() {
    let screen_ = SyncScreen { title: tr("test_connection") };
    let services = match sync::services() {
        Ok(s) => s,
        Err(msg) => {
            screen_.draw(true, &tr(msg), "", true);
            wait_dismiss();
            return;
        }
    };
    screen_.draw(false, &tr("connecting_wifi"), "", false);
    if dsn::wifi_connect() != 0 {
        screen_.draw(true, &tr("wifi_failed"), "", true);
        wait_dismiss();
        return;
    }
    let mut lines = Vec::new();
    let mut failed = false;
    for svc in &services {
        let mut client = crate::rpc::Client::new(&svc.url, &svc.key);
        match client.call("decks.list", serde_json::json!({})) {
            Ok(_) => lines.push(trf("connected", &[Arg::S(svc.name)])),
            Err(e) => {
                failed = true;
                lines.push(sync::error_text(svc.name, &e));
            }
        }
    }
    screen_.draw(failed, &lines.join("\n"), "", true);
    wait_dismiss();
}
fn wait_dismiss() {
    loop {
        let t = dsn::ev_type(dsn::wait_event());
        if matches!(t, dsn::EV_BACK | dsn::EV_CONFIRM | dsn::EV_TAP | dsn::EV_NEXT | dsn::EV_PREV) {
            return;
        }
    }
}

// ---------------------------------------------------------------- the settings
/// matcha's Settings > Flashcards and Flashcards Sync pages, as one list: the scheduling
/// settings, vacation mode, and the sync's server, key and decks (all kept in config.json).
enum Row {
    Section(&'static str),
    Page(&'static str, &'static str, &'static [Row]), // label, page title, its rows
    Choice(&'static str, &'static str, &'static [&'static str]), // label, config key, choices
    Toggle(&'static str, &'static str),
    Text(&'static str, &'static str, bool),  // label, config key, secret
    List(&'static str, &'static str),        // decks, comma separated
    Pick(&'static str, &'static str, &'static [(&'static str, &'static str)]), // label, key, (value, shown)
    Text2(&'static str, &'static str, bool), // an own server or key: shows the shared one when unset
    Action(&'static str),                    // Test Connection
}
const SCHEDULING_ROWS: [Row; 5] = [
    Row::Choice("retention", "retention", &["80%", "85%", "90%", "95%"]),
    Row::Choice("new_per_day", "newPerDay", &["5", "10", "20", "30", "50"]),
    Row::Choice("max_interval", "maxInterval", &["one_month", "six_months", "one_year", "no_limit"]),
    Row::Toggle("vacation_mode", "vacation"),
    Row::Section("scheduling_note"),
];
const SYNC_ROWS: [Row; 11] = [
    Row::Text("server_address", "server", false),
    Row::Text("server_key", "key", true),
    Row::List("anki_decks", "anki"),
    Row::List("migaku_decks", "migaku"),
    Row::Pick("mined_to", "minedTo", &[("anki", "Anki"), ("migaku", "Migaku")]),
    Row::Text2("anki_server", "ankiServer", false),
    Row::Text2("anki_key", "ankiKey", true),
    Row::Text2("migaku_server", "migakuServer", false),
    Row::Text2("migaku_key", "migakuKey", true),
    Row::Action("test_connection"),
    Row::Section("sync_note"),
];
const SETTINGS_ROWS: [Row; 2] = [
    Row::Page("scheduling", "scheduling", &SCHEDULING_ROWS),
    Row::Page("flashcards_sync", "flashcards_sync", &SYNC_ROWS),
];
const CHOICE_DEFAULTS: [(&str, u64); 3] = [("retention", 2), ("newPerDay", 1), ("maxInterval", 3)];

struct SettingsScreen {
    title: &'static str,
    rows: &'static [Row],
    selected: usize,
    row_ys: Vec<(usize, i32)>,
}
impl SettingsScreen {
    fn new() -> SettingsScreen { SettingsScreen::page("settings", &SETTINGS_ROWS) }
    fn page(title: &'static str, rows: &'static [Row]) -> SettingsScreen {
        let first = rows.iter().position(|r| !matches!(r, Row::Section(_))).unwrap_or(0);
        SettingsScreen { title, rows, selected: first, row_ys: Vec::new() }
    }
    fn value(row: &Row) -> String {
        let cfg = store::read_json("config.json").unwrap_or(serde_json::Value::Null);
        match row {
            Row::Choice(_, k, choices) => {
                let d = CHOICE_DEFAULTS.iter().find(|c| c.0 == *k).map(|c| c.1).unwrap_or(0);
                let i = cfg.get(*k).and_then(|v| v.as_u64()).unwrap_or(d) as usize;
                tr(choices[i.min(choices.len() - 1)])
            }
            Row::Toggle(_, k) => tr(if cfg.get(*k).and_then(|v| v.as_bool()).unwrap_or(false) { "on" } else { "off" }),
            Row::Text(_, k, secret) => {
                let v = cfg.get(*k).and_then(|v| v.as_str()).unwrap_or("");
                if v.is_empty() { tr("not_set") } else if *secret { "•".repeat(v.chars().count().min(8)) } else { String::from(v) }
            }
            Row::List(_, k) => {
                let names: Vec<String> = cfg.get(*k).and_then(|v| v.as_array()).map(|a| {
                    a.iter().filter_map(|d| d.as_str().map(String::from)).collect()
                }).unwrap_or_default();
                if names.is_empty() { tr("none") } else { names.join(", ") }
            }
            Row::Page(_, _, rows) => {
                // What the page holds, in short: its choices, or the server's host.
                let parts: Vec<String> = rows.iter().filter_map(|r| match r {
                    Row::Choice(..) => Some(Self::value(r)),
                    Row::Toggle(l, k) if store::read_json("config.json").and_then(|c| c.get(*k).and_then(|v| v.as_bool())).unwrap_or(false) => Some(tr(l)),
                    Row::Text(_, "server", _) => Some(Self::value(r).split("//").last().unwrap_or("").into()),
                    _ => None,
                }).collect();
                parts.join(" · ")
            }
            Row::Pick(_, k, opts) => {
                let v = cfg.get(*k).and_then(|v| v.as_str()).unwrap_or(opts[0].0);
                String::from(opts.iter().find(|o| o.0 == v).unwrap_or(&opts[0]).1)
            }
            Row::Text2(_, k, secret) => {
                let v = cfg.get(*k).and_then(|v| v.as_str()).unwrap_or("");
                if v.is_empty() { tr("same_server") } else if *secret { "•".repeat(v.chars().count().min(8)) } else { String::from(v) }
            }
            Row::Section(_) | Row::Action(_) => String::new(),
        }
    }
    fn draw(&mut self) {
        let (w, _) = screen();
        dsn::clear();
        header_with(self.title, true);
        self.row_ys.clear();
        let (left, right) = (SIDE_PAD + ROW_INSET, w - SIDE_PAD - ROW_INSET);
        let mut y = content_top() + 4;
        for (i, row) in self.rows.iter().enumerate() {
            match row {
                Row::Section(t) => {
                    if !t.is_empty() {
                        let lines = gfx::wrap(SMALL, &tr(t), right - left);
                        for l in &lines {
                            let lx = if crate::i18n::rtl() { right - gfx::text_width(SMALL, l) } else { left };
                            gfx::text(lx, y + 10, SMALL, l, dsn::BLACK);
                            y += SMALL.line_h;
                        }
                        y += 16;
                    }
                }
                Row::Page(label, ..) | Row::Choice(label, ..) | Row::Toggle(label, _) | Row::Text(label, ..) | Row::List(label, _)
                | Row::Pick(label, ..) | Row::Text2(label, ..) | Row::Action(label) => {
                    self.row_ys.push((i, y));
                    let page = matches!(row, Row::Page(..));
                    let rh = ROW_PITCH + if page { SMALL.line_h + 4 } else { 0 };
                    if i == self.selected {
                        dsn::fill_round(left - 8, y + 4, right - left + 16, rh - 8, dsn::LIGHT_GRAY, 6);
                    }
                    let ty = y + (ROW_PITCH - UI12.line_h) / 2 - if page { 2 } else { 0 };
                    // The value keeps its room (up to half the row); a long label is shortened.
                    let value_full = if matches!(row, Row::Page(..)) { String::new() } else { Self::value(row) };
                    let value_w = gfx::text_width(UI12, &value_full).min((right - left) / 2);
                    let label = &gfx::truncated(UI12, &tr(label), right - left - value_w - 24);
                    let lw = gfx::text_width(UI12, label);
                    let rtl = crate::i18n::rtl();
                    gfx::text(if rtl { right - lw } else { left }, ty, UI12, label, dsn::BLACK);
                    if matches!(row, Row::Page(..)) {
                        // A page row: its summary below the label, and a chevron.
                        gfx::text(if rtl { left } else { right - 12 }, y + (rh - UI12.line_h) / 2, UI12, if rtl { "‹" } else { "›" }, dsn::BLACK);
                        let sub = gfx::truncated(SMALL, &Self::value(row), right - left - 24);
                        let sw = gfx::text_width(SMALL, &sub);
                        gfx::text(if rtl { right - sw } else { left }, ty + UI12.line_h, SMALL, &sub, dsn::BLACK);
                        y += rh;
                        continue;
                    }
                    let value = gfx::truncated(UI12, &value_full, right - left - lw - 24);
                    let vw = gfx::text_width(UI12, &value);
                    gfx::text(if rtl { left } else { right - vw }, ty, UI12, &value, dsn::BLACK);
                    y += ROW_PITCH;
                }
            }
        }
        hints(["back", if matches!(self.rows[self.selected], Row::Page(..)) { "open" } else { "change" }, "up", "down"]);
        dsn::refresh();
    }
    fn change(&self, i: usize) {
        match &self.rows[i] {
            Row::Page(_, title, rows) => SettingsScreen::page(title, rows).run(),
            Row::Choice(_, k, choices) => {
                let cur = store::read_json("config.json").and_then(|c| c.get(*k).and_then(|v| v.as_u64()))
                    .unwrap_or_else(|| CHOICE_DEFAULTS.iter().find(|c| c.0 == *k).map(|c| c.1).unwrap_or(0));
                store::set_config(k, serde_json::Value::from((cur + 1) % choices.len() as u64));
            }
            Row::Toggle(_, k) => {
                let on = store::read_json("config.json").and_then(|c| c.get(*k).and_then(|v| v.as_bool())).unwrap_or(false);
                store::set_config(k, serde_json::Value::from(!on));
            }
            Row::Action(_) => test_connection(),
            Row::Pick(_, k, opts) => {
                let cur = store::read_json("config.json").and_then(|c| c.get(*k).and_then(|v| v.as_str()).map(String::from))
                    .unwrap_or_default();
                let i = opts.iter().position(|o| o.0 == cur).unwrap_or(0);
                store::set_config(k, serde_json::Value::from(opts[(i + 1) % opts.len()].0));
            }
            Row::Text(label, k, secret) | Row::Text2(label, k, secret) => {
                if let Some(t) = keyboard(&tr(label), *secret) {
                    store::set_config(k, serde_json::Value::from(t.trim()));
                }
            }
            Row::List(label, k) => {
                if let Some(t) = keyboard(&trf("comma_separated", &[Arg::S(&tr(label))]), false) {
                    let names: Vec<serde_json::Value> = t.split(',').map(|n| n.trim()).filter(|n| !n.is_empty())
                        .map(serde_json::Value::from).collect();
                    store::set_config(k, serde_json::Value::Array(names));
                }
            }
            Row::Section(_) => {}
        }
    }
    fn run(&mut self) {
        let rows: Vec<usize> = self.rows.iter().enumerate().filter(|(_, r)| !matches!(r, Row::Section(_))).map(|(i, _)| i).collect();
        self.draw();
        loop {
            let e = dsn::wait_event();
            let at = rows.iter().position(|&r| r == self.selected).unwrap_or(0);
            match dsn::ev_type(e) {
                dsn::EV_BACK => return,
                dsn::EV_NEXT | dsn::EV_SWIPE_UP => self.selected = rows[(at + 1) % rows.len()],
                dsn::EV_PREV | dsn::EV_SWIPE_DOWN => self.selected = rows[(at + rows.len() - 1) % rows.len()],
                dsn::EV_CONFIRM => self.change(self.selected),
                dsn::EV_TAP => {
                    let y = dsn::ev_y(e);
                    let x = dsn::ev_x(e);
                    let (w, _) = screen();
                    let in_slot = if crate::i18n::rtl() { x > w - HDR_SIDE - HDR_BACK - 8 } else { x < HDR_SIDE + HDR_BACK + 8 };
                    if touch() && y >= band_top() + HDR_STRIP && y < content_top() && in_slot {
                        return;
                    }
                    let Some(&(i, _)) = self.row_ys.iter().rev().find(|(_, ry)| y >= *ry) else { continue };
                    self.selected = i;
                    self.change(i);
                }
                _ => continue,
            }
            self.draw();
        }
    }
}
/// The firmware's on-screen keyboard; None when cancelled.
fn keyboard(prompt: &str, secret: bool) -> Option<String> {
    let mut buf = alloc::vec![0u8; 512];
    let n = dsn::dsn_text_input(prompt.as_bytes(), &mut buf, if secret { 1 } else { 0 });
    if n < 0 {
        return None;
    }
    let n = (n as usize).min(buf.len());
    let end = buf[..n].iter().position(|&b| b == 0).unwrap_or(n);
    core::str::from_utf8(&buf[..end]).ok().map(String::from)
}

// ---------------------------------------------------------------- the app
pub fn run() {
    let (w, h) = dsn::screen();
    let dev = dsn::device();
    unsafe {
        SCREEN = (w, h);
        TOUCH = dev.has_touch;
        INSET_TOP = dev.inset_top as i32;
    }
    store::today();
    store::sync_vacation();
    let mut list = DeckList::load();
    list.draw();
    loop {
        let e = dsn::wait_event();
        let rows = list.rows();
        let activate = |list: &mut DeckList, i: i32| {
            if i == list.settings_row() {
                SettingsScreen::new().run();
                store::sync_vacation();
            } else if list.sync_row && i == list.decks.len() as i32 {
                run_sync();
            } else if i >= 0 && (i as usize) < list.decks.len() {
                let d = &list.decks[i as usize];
                if d.due + d.fresh == 0 {
                    return false; // a deck with nothing due has no session to open
                }
                let key = d.key.clone();
                Review::open(&key).run();
            } else {
                return false;
            }
            let keep = list.selected;
            *list = DeckList::load();
            list.selected = keep.min(list.rows() - 1).max(-1);
            true
        };
        let slots = list.bar.count();
        match dsn::ev_type(e) {
            // In the tab bar: Up and Down walk it, Back and Up past the first slot leave it,
            // Confirm switches (as matcha's Left/Right and Confirm do).
            dsn::EV_BACK if list.band.is_some() => list.band = None,
            dsn::EV_NEXT if list.band.is_some() => list.band = list.band.map(|f| (f + 1).min(slots - 1)),
            dsn::EV_PREV if list.band.is_some() => list.band = list.band.and_then(|f| f.checked_sub(1)),
            dsn::EV_CONFIRM if list.band.is_some() => {
                if !list.bar.activate(list.band.unwrap()) {
                    list.band = None; // Confirm on this screen's own tab hands the cursor back
                }
            }
            dsn::EV_BACK => return,
            // Down past the last row moves into the tab bar, on this screen's tab.
            dsn::EV_NEXT if list.selected == rows - 1 => list.band = Some(list.bar.slot_of_current()),
            dsn::EV_NEXT | dsn::EV_SWIPE_UP => list.selected = (list.selected + 1).min(rows - 1),
            // Up from the first row reaches the settings icon beside the title.
            dsn::EV_PREV | dsn::EV_SWIPE_DOWN => list.selected = (list.selected - 1).max(-1),
            dsn::EV_CONFIRM => {
                let i = list.selected;
                if !activate(&mut list, i) {
                    continue;
                }
            }
            dsn::EV_TAP => {
                let (x, y) = (dsn::ev_x(e), dsn::ev_y(e));
                if let Some(slot) = list.bar.hit(x, y) {
                    list.bar.activate(slot);
                    continue;
                }
                let icon = DeckList::settings_icon_rect();
                let on_icon = x >= icon.x - 12 && x < icon.x + icon.w + 12 && y >= icon.y - 12 && y < icon.y + icon.h + 12;
                let hit = if on_icon { Some(-1) } else { list.row_ys.iter().find(|(_, ry)| y >= *ry && y < ry + ROW_PITCH).map(|r| r.0) };
                let Some(i) = hit else { continue };
                list.selected = i;
                if !activate(&mut list, i) {
                    continue;
                }
            }
            _ => continue,
        }
        list.draw();
    }
}
