//! The screens, laid out as matcha's FlashcardsActivity, FlashcardReviewActivity and AnkiSyncActivity:
//! the deck list with its big count, the card with its double rule and rating boxes, the session's
//! seal, and the sync's status lines.
use crate::gfx::{self, Canvas, SMALL, UI10, UI10B, UI12, UI12B, CARD, CARDB, HERO_NUM, SERIF18B};
use crate::store::{self, DeckSummary, Interval, Session};
use crate::{dsn, fsrs, icons, sync};
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
fn header(title: &str) {
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
    gfx::text(HDR_SIDE, row_top + (HDR_HEIGHT - HDR_STRIP - UI12B.line_h) / 2 - 2, UI12B, title, dsn::BLACK);
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
    row_ys: Vec<(i32, i32)>, // (row index, y) as drawn, for taps
}

impl DeckList {
    fn load() -> DeckList {
        let services = sync::services();
        DeckList {
            decks: store::list_decks(),
            sync_row: services.is_ok(),
            hint: services.err(),
            selected: 0,
            top_row: 0,
            row_ys: Vec::new(),
        }
    }
    fn rows(&self) -> i32 { self.decks.len() as i32 + self.sync_row as i32 }
    fn due_total(&self) -> u32 { self.decks.iter().map(|d| d.due + d.fresh).sum() }
    fn hero_height(&self) -> i32 {
        (if self.due_total() > 0 { HERO_NUM.line_h * 3 / 4 + NUMERAL_GAP } else { 0 }) + UI12.line_h + ORNAMENT_ABOVE + ORNAMENT_BELOW
    }

    fn draw(&mut self) {
        let (w, h) = screen();
        dsn::clear();
        header("Flashcards");
        self.row_ys.clear();
        let left = SIDE_PAD + ROW_INSET;
        let right = w - SIDE_PAD - ROW_INSET;
        if self.rows() == 0 {
            let mid = (content_top() + h) / 2;
            gfx::text_centered(w / 2, mid - 40, UI12B, "No decks yet", dsn::BLACK);
            let msg = self.hint.unwrap_or("Name your Anki or Migaku decks in config.json, then sync.");
            for (i, l) in gfx::wrap(UI10, msg, w - 80).iter().enumerate().take(4) {
                gfx::text_centered(w / 2, mid - 8 + i as i32 * UI10.line_h, UI10, l, dsn::BLACK);
            }
            dsn::refresh();
            return;
        }
        let top = content_top();
        let avail = safe_bottom() - top;
        let block = self.hero_height() + self.rows() * ROW_PITCH;
        let hero_top = top + 10i32.max(TOP_GAP_MAX.min((avail - block) * 2 / 5));
        let due = self.due_total();
        let mut y = hero_top;
        let caption = if due > 0 {
            let n = format!("{}", due);
            gfx::text_centered(w / 2, y - HERO_NUM.line_h / 8, HERO_NUM, &n, dsn::BLACK);
            y += HERO_NUM.line_h * 3 / 4 + NUMERAL_GAP;
            String::from(if due == 1 { "card to review" } else { "cards to review" })
        } else {
            String::from("Nothing due today")
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
        let bottom = safe_bottom();
        let fit = ((bottom - list_top) / ROW_PITCH).max(1);
        if self.selected < self.top_row {
            self.top_row = self.selected;
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
            if self.sync_row && i == self.decks.len() as i32 {
                // Like Insights' Refresh row: icon and small text, outlined only while focused.
                let label = "Sync Flashcards";
                let row_w = 24 + 8 + gfx::text_width(SMALL, label);
                let rx = (left + right - row_w) / 2;
                if i == self.selected {
                    dsn::stroke(rx - 16, row_y + 2, row_w + 32, ROW_PITCH - 4, dsn::BLACK, 2, 12);
                }
                gfx::icon(rx, row_y + (ROW_PITCH - 24) / 2, 24, &icons::REFRESH_24, dsn::BLACK);
                gfx::text(rx + 32, row_y + (ROW_PITCH - SMALL.line_h) / 2, SMALL, label, dsn::BLACK);
                continue;
            }
            let d = &self.decks[i as usize];
            let ty = row_y + (ROW_PITCH - UI12.line_h) / 2;
            let baseline = ty + 24;
            if i == self.selected {
                dsn::fill_round(left - 8, row_y + 4, right - left + 16, ROW_PITCH - 8, dsn::LIGHT_GRAY, 6);
            }
            let value = format!("{}", d.due + d.fresh);
            let vw = gfx::text_width(UI12, &value);
            let name = gfx::truncated(UI12, &d.label, right - left - vw - 24);
            gfx::text(left, ty, UI12, &name, dsn::BLACK);
            gfx::text(right - vw, ty, UI12, &value, dsn::BLACK);
            let mut x = left + gfx::text_width(UI12, &name) + 12;
            while x < right - vw - 12 {
                dsn::fill(x, baseline - 1, 1, 1, dsn::BLACK);
                x += 5;
            }
        }
        hints(["« Back", "Select", "", ""]);
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
}
fn item_height(it: &Item) -> i32 {
    match it {
        Item::Line(_) => CARD.line_h,
        Item::Rule => 30,
        Item::Image(c) => c.h + 12,
        Item::Missing => 72,
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
        return if v < 60 { format!("{}m", v) } else { format!("{}h", (v + 30) / 60) };
    }
    match v {
        0 => String::from("<1d"),
        1..=29 => format!("{}d", v),
        30..=364 => format!("{}mo", (v + 15) / 30),
        _ => format!("{}.{}y", v / 365, (v % 365) * 10 / 365),
    }
}
const RATINGS: [&str; 4] = ["Again", "Hard", "Good", "Easy"];

struct Review {
    session: Session,
    show_back: bool,
    selected: usize,
    pages: Vec<Vec<Item>>,
    page: usize,
    intervals: [Interval; 4],
}

impl Review {
    fn open(key: &str) -> Review {
        let mut r = Review { session: Session::open(key), show_back: false, selected: 2, pages: Vec::new(), page: 0, intervals: Default::default() };
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
        self.pages = match self.session.current() {
            Some((f, b, false)) => layout(if self.show_back { b } else { f }),
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
        if self.pages.is_empty() || self.pages.iter().all(|p| p.is_empty()) {
            let too_large = self.session.current().is_some_and(|c| c.2);
            let msg = if too_large { "This card is too large for the device" } else { "This card could not be read" };
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
            gfx::text_centered(w / 2, band.y + 34, UI10, if touch() { "Tap to turn" } else { "Turn" }, dsn::BLACK);
        }
        hints(["« Back", if self.show_back { "Rate" } else { "Turn" }, if self.show_back { "<" } else { "" }, if self.show_back { ">" } else { "" }]);
        dsn::refresh();
    }

    // Four ruled boxes, each the rating's name over when the card comes back.
    fn draw_ratings(&self, band: &Rect) {
        const GAP: i32 = 8;
        const BOX_H: i32 = 60;
        let box_w = (band.w - 3 * GAP) / 4;
        let text_top = band.y + (BOX_H - UI10B.line_h - SMALL.line_h - 2) / 2;
        for i in 0..4 {
            let bx = band.x + i as i32 * (box_w + GAP);
            let filled = !touch() && i == self.selected;
            if filled {
                dsn::fill_round(bx, band.y, box_w, BOX_H, dsn::BLACK, 10);
            } else {
                dsn::stroke(bx, band.y, box_w, BOX_H, dsn::BLACK, 2, 10);
            }
            let ink = if filled { dsn::WHITE } else { dsn::BLACK };
            gfx::text_centered(bx + box_w / 2, text_top, UI10B, RATINGS[i], ink);
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
        gfx::text_centered(cx, y, SERIF18B, "All caught up", dsn::BLACK);
        y += SERIF18B.line_h + 6;
        let n = self.session.reviewed();
        gfx::text_centered(cx, y, UI10, &format!("{} card{} reviewed", n, if n == 1 { "" } else { "s" }), dsn::BLACK);
        y += 48;
        let gap = 10;
        let box_w = (a.w - 3 * gap) / 4;
        for i in 0..4 {
            let bx = a.x + i as i32 * (box_w + gap);
            dsn::stroke(bx, y, box_w, 96, dsn::BLACK, 2, 10);
            gfx::text_centered(bx + box_w / 2, y + 10, SERIF18B, &format!("{}", self.session.rating_count(i)), dsn::BLACK);
            gfx::text_centered(bx + box_w / 2, y + 96 - 30, UI10, RATINGS[i], dsn::BLACK);
        }
        let by = a.y + a.h - 56 - 8;
        dsn::fill_round(a.x, by, a.w, 56, dsn::BLACK, 12);
        gfx::text_centered(cx, by + (56 - UI12B.line_h) / 2, UI12B, "Flashcards", dsn::WHITE);
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
                        self.rate(((x - band.x) * 4 / band.w.max(1)).clamp(0, 3) as usize);
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
            gfx::text_centered(w / 2, top - 40, UI12B, "Sync stopped", dsn::BLACK);
        }
        for (i, l) in gfx::wrap(UI10, message, w - 60).iter().enumerate().take(4) {
            gfx::text_centered(w / 2, top + i as i32 * UI10.line_h, UI10, l, dsn::BLACK);
        }
        if !detail.is_empty() {
            gfx::text_centered(w / 2, top + 110, UI10, detail, dsn::BLACK);
        }
        if finished {
            hints(["« Back", "Done", "", ""]);
        }
        let _ = h;
        dsn::refresh();
    }
}
impl sync::Progress for SyncScreen {
    fn show(&mut self, message: &str, detail: &str) { self.draw(false, message, detail, false); }
}

fn run_sync() {
    let mut screen_ = SyncScreen { title: String::from("Sync Flashcards") };
    let services = match sync::services() {
        Ok(s) => s,
        Err(msg) => {
            screen_.draw(true, msg, "", true);
            wait_dismiss();
            return;
        }
    };
    screen_.draw(false, "Connecting to Wi-Fi", "", false);
    if dsn::wifi_connect() != 0 {
        screen_.draw(true, "Could not connect to Wi-Fi", "", true);
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
                summary += &format!("{}: {} new card{}, {} review{} received, {} sent", svc.name, totals.added,
                                    if totals.added == 1 { "" } else { "s" }, totals.pulled,
                                    if totals.pulled == 1 { "" } else { "s" }, totals.pushed);
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
fn wait_dismiss() {
    loop {
        let t = dsn::ev_type(dsn::wait_event());
        if t == dsn::EV_BACK || t == dsn::EV_CONFIRM || t == dsn::EV_TAP {
            return;
        }
    }
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
    let mut list = DeckList::load();
    list.draw();
    loop {
        let e = dsn::wait_event();
        let rows = list.rows();
        let activate = |list: &mut DeckList, i: i32| {
            if list.sync_row && i == list.decks.len() as i32 {
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
            list.selected = keep.min(list.rows() - 1).max(0);
            true
        };
        match dsn::ev_type(e) {
            dsn::EV_BACK => return,
            dsn::EV_NEXT | dsn::EV_SWIPE_UP if rows > 0 => list.selected = (list.selected + 1) % rows,
            dsn::EV_PREV | dsn::EV_SWIPE_DOWN if rows > 0 => list.selected = (list.selected + rows - 1) % rows,
            dsn::EV_CONFIRM => {
                let i = list.selected;
                if !activate(&mut list, i) {
                    continue;
                }
            }
            dsn::EV_TAP => {
                let y = dsn::ev_y(e);
                let hit = list.row_ys.iter().find(|(_, ry)| y >= *ry && y < ry + ROW_PITCH).map(|r| r.0);
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
