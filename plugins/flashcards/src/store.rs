//! Files and the review session, after matcha's FlashcardStore.cpp. Dessen apps read and write whole
//! files and cannot make folders, so each deck keeps a few flat files in the plugin folder:
//!   <key>.json   the deck: its name, service, server id and Anki preset, last pulled review
//!   <key>.ids    its cards in Anki's order: u64 card, u64 note, u8 flags
//!   <key>.cards  the cards' two sides as plain text (see Side)
//!   <key>.state  progress: u16 day, u16 new today, then 24-byte CardState records sorted by id
//!   <key>.log    ratings not yet sent to Anki, 24 bytes each
//!   <key>.steps  when cards between learning steps fall due: u64 card, i64 unix seconds
//! A deck key is "a<id>" for an Anki deck and "m<id>" for a Migaku one.
use crate::{dsn, fsrs};
use alloc::{format, string::String, vec, vec::Vec};
use serde_json::Value;

static mut DIR: String = String::new();

pub fn init() {
    let mut b = [0u8; 160];
    let n = dsn::fs_app_dir(&mut b);
    let d = if n > 0 { core::str::from_utf8(&b[..n as usize]).unwrap_or("") } else { "" };
    unsafe { *core::ptr::addr_of_mut!(DIR) = String::from(d.trim_end_matches('/')) };
}
pub fn path(name: &str) -> String { format!("{}/{}", unsafe { &*core::ptr::addr_of!(DIR) }, name) }

/// A whole file, or None when it is missing.
pub fn read(path: &str) -> Option<Vec<u8>> {
    let mut cap = 64 * 1024;
    loop {
        let mut buf = vec![0u8; cap];
        let n = dsn::fs_read(path, &mut buf);
        if n == dsn::ERR_TOO_BIG && cap < 8 << 20 {
            cap *= 4;
            continue;
        }
        if n < 0 {
            return None;
        }
        buf.truncate(n as usize);
        return Some(buf);
    }
}
pub fn write(path: &str, data: &[u8]) -> bool { dsn::fs_write(path, data) == 0 }
pub fn read_file(name: &str) -> Option<Vec<u8>> { read(&path(name)) }
pub fn write_file(name: &str, data: &[u8]) -> bool { write(&path(name), data) }
pub fn delete_file(name: &str) { dsn::fs_delete(&path(name)); }
pub fn read_json(name: &str) -> Option<Value> { serde_json::from_slice(&read_file(name)?).ok() }
pub fn write_json(name: &str, v: &Value) -> bool { write_file(name, serde_json::to_string(v).unwrap_or_default().as_bytes()) }

// ---------------------------------------------------------------- the clock
const EPOCH_2020: i64 = 18262; // 2020-01-01 as days since 1970
static mut TZ: i64 = 0;
static mut LAST_DAY: i32 = -1;

pub fn days_from_civil(y: i32, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y } as i64;
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m as i64 + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    era * 146097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719468
}
/// Unix seconds now, 0 when the clock was never set.
pub fn now() -> i64 {
    let t = dsn::time() as i64;
    if t > 1_577_836_800 { t } else { 0 }
}
/// Today as days since 2020-01-01 in local time; the last known day when the clock is unset.
pub fn today() -> u16 {
    let t = now();
    if t > 0 {
        if let Some((y, mo, d, h, mi, s)) = dsn::clock() {
            let local = days_from_civil(y as i32, mo as u32, d as u32) * 86400 + h as i64 * 3600 + mi as i64 * 60 + s as i64;
            let tz = ((local - t) + if local >= t { 450 } else { -450 }) / 900 * 900;
            unsafe { TZ = tz };
        }
        let day = (t + unsafe { TZ }).div_euclid(86400) - EPOCH_2020;
        unsafe { LAST_DAY = day as i32 };
    }
    unsafe { LAST_DAY.max(0) as u16 }
}
/// A unix time in ms as a local day since 2020-01-01.
pub fn day_of_ms(ms: i64) -> i32 { ((ms / 1000 + unsafe { TZ }).div_euclid(86400) - EPOCH_2020) as i32 }

// ---------------------------------------------------------------- decks
pub struct DeckInfo {
    pub key: String,
    pub name: String,
    pub migaku: bool,
    pub server_id: String,
    pub new_per_day: i32,
    pub reviews_per_day: i32,
    pub retention: f32,
    pub max_interval: u16,
    pub learn_steps: Vec<u16>,
    pub relearn_steps: Vec<u16>,
    pub bury_new: bool,
    pub bury_reviews: bool,
    pub easy_days: [u8; 7],
    pub leech_threshold: u8,
    pub leech_suspend: bool,
    pub review_order: String,
    pub new_mix: String,
    pub new_random: bool,
    pub last_review_id: u64,
    pub preset: bool, // its Anki preset was read; else the plugin's settings schedule it
}

fn num(v: &Value, k: &str, d: f64) -> f64 { v.get(k).and_then(|x| x.as_f64()).unwrap_or(d) }
fn text(v: &Value, k: &str) -> String { String::from(v.get(k).and_then(|x| x.as_str()).unwrap_or("")) }
fn steps(v: &Value, k: &str) -> Vec<u16> {
    v.get(k).and_then(|x| x.as_array()).map(|a| a.iter().filter_map(|m| m.as_f64()).take(4).map(|m| m as u16).collect())
        .unwrap_or_else(|| vec![10])
}

impl DeckInfo {
    pub fn new(key: &str, name: &str, migaku: bool, server_id: &str) -> DeckInfo {
        DeckInfo {
            key: String::from(key), name: String::from(name), migaku, server_id: String::from(server_id),
            new_per_day: 20, reviews_per_day: 9999, retention: 0.9, max_interval: 36500, learn_steps: vec![10],
            relearn_steps: vec![10], bury_new: false, bury_reviews: false, easy_days: [100; 7], leech_threshold: 0,
            leech_suspend: true, review_order: String::from("day"), new_mix: String::from("mixed"), new_random: false,
            last_review_id: 0, preset: false,
        }
    }
    pub fn read(key: &str) -> Option<DeckInfo> {
        let v = read_json(&format!("{}.json", key))?;
        let mut d = DeckInfo::new(key, &text(&v, "name"), v.get("migaku").and_then(|x| x.as_bool()).unwrap_or(false),
                                  &text(&v, "serverId"));
        d.apply_preset(&v);
        d.last_review_id = text(&v, "lastReviewId").parse().unwrap_or(0);
        d.preset = v.get("preset").and_then(|x| x.as_bool()).unwrap_or(false);
        if !d.preset {
            // As matcha's schedulingParams: the user's settings where no preset was read.
            let st = Settings::read();
            d.retention = st.retention_value();
            d.max_interval = st.max_interval_days();
            d.new_per_day = st.new_per_day_value() as i32;
        }
        Some(d)
    }
    /// A preset's settings as deckPresets.get returns them (also how they are saved).
    pub fn apply_preset(&mut self, s: &Value) {
        self.new_per_day = num(s, "newPerDay", self.new_per_day as f64) as i32;
        self.reviews_per_day = num(s, "reviewsPerDay", self.reviews_per_day as f64) as i32;
        self.retention = num(s, "desiredRetention", self.retention as f64) as f32;
        self.max_interval = num(s, "maximumIntervalDays", self.max_interval as f64).clamp(1.0, 36500.0) as u16;
        if s.get("learnStepsMinutes").is_some() { self.learn_steps = steps(s, "learnStepsMinutes"); }
        if s.get("relearnStepsMinutes").is_some() { self.relearn_steps = steps(s, "relearnStepsMinutes"); }
        self.bury_new = s.get("buryNew").and_then(|x| x.as_bool()).unwrap_or(self.bury_new);
        self.bury_reviews = s.get("buryReviews").and_then(|x| x.as_bool()).unwrap_or(self.bury_reviews);
        if let Some(e) = s.get("easyDaysPercentages").and_then(|x| x.as_array()) {
            for (i, p) in e.iter().take(7).enumerate() {
                let p = p.as_f64().unwrap_or(1.0);
                self.easy_days[i] = (if p <= 1.5 { p * 100.0 } else { p }).clamp(0.0, 200.0) as u8;
            }
        }
        self.leech_threshold = num(s, "leechThreshold", self.leech_threshold as f64).clamp(0.0, 255.0) as u8;
        if let Some(a) = s.get("leechAction").and_then(|x| x.as_str()) { self.leech_suspend = a != "tagOnly"; }
        if let Some(o) = s.get("reviewOrder").and_then(|x| x.as_str()) { self.review_order = String::from(o); }
        if let Some(o) = s.get("newMix").and_then(|x| x.as_str()) { self.new_mix = String::from(o); }
        if let Some(o) = s.get("newSortOrder").and_then(|x| x.as_str()) {
            self.new_random = o == "randomCard" || o == "randomNoteThenTemplate";
        }
    }
    pub fn write(&self) -> bool {
        let v = serde_json::json!({
            "name": self.name, "migaku": self.migaku, "serverId": self.server_id,
            "newPerDay": self.new_per_day, "reviewsPerDay": self.reviews_per_day, "desiredRetention": self.retention,
            "maximumIntervalDays": self.max_interval, "learnStepsMinutes": self.learn_steps,
            "relearnStepsMinutes": self.relearn_steps, "buryNew": self.bury_new, "buryReviews": self.bury_reviews,
            "easyDaysPercentages": self.easy_days, "leechThreshold": self.leech_threshold,
            "leechAction": if self.leech_suspend { "suspend" } else { "tagOnly" }, "reviewOrder": self.review_order,
            "newMix": self.new_mix, "newSortOrder": if self.new_random { "randomCard" } else { "template" },
            "lastReviewId": format!("{}", self.last_review_id), "preset": self.preset,
        });
        write_json(&format!("{}.json", self.key), &v)
    }
    pub fn params(&self) -> fsrs::Params { fsrs::Params { retention: self.retention, max_interval: self.max_interval } }
}

// ---------------------------------------------------------------- settings
/// matcha's Settings > Flashcards, kept in config.json beside the sync's server and decks:
/// "retention" 0-3 (80/85/90/95 %), "newPerDay" 0-4 (5/10/20/30/50), "maxInterval" 0-3
/// (1 month, 6 months, 1 year, no limit), "vacation". Anki's own deck preset wins where read.
pub struct Settings {
    pub retention: u8,
    pub new_per_day: u8,
    pub max_interval: u8,
    pub vacation: bool,
}
pub const RETENTION: [f32; 4] = [0.80, 0.85, 0.90, 0.95];
pub const NEW_PER_DAY: [u16; 5] = [5, 10, 20, 30, 50];
pub const MAX_DAYS: [u16; 4] = [30, 180, 365, 36500];
impl Settings {
    pub fn read() -> Settings {
        let cfg = read_json("config.json").unwrap_or(Value::Null);
        let n = |k: &str, d: u8, max: u8| cfg.get(k).and_then(|v| v.as_u64()).map(|v| (v as u8).min(max)).unwrap_or(d);
        Settings {
            retention: n("retention", 2, 3),
            new_per_day: n("newPerDay", 1, 4),
            max_interval: n("maxInterval", 3, 3),
            vacation: cfg.get("vacation").and_then(|v| v.as_bool()).unwrap_or(false),
        }
    }
    pub fn retention_value(&self) -> f32 { RETENTION[self.retention as usize] }
    pub fn new_per_day_value(&self) -> u16 { NEW_PER_DAY[self.new_per_day as usize] }
    pub fn max_interval_days(&self) -> u16 { MAX_DAYS[self.max_interval as usize] }
}
/// Writes one field of config.json, keeping the rest.
pub fn set_config(key: &str, value: Value) -> bool {
    let mut cfg = read_json("config.json").filter(|v| v.is_object()).unwrap_or_else(|| serde_json::json!({}));
    cfg[key] = value;
    write_json("config.json", &cfg)
}

/// Vacation mode as matcha's syncVacation: its start day is kept in vacation.bin; when it ends,
/// every card's due date (and last review) moves forward by the days away.
pub fn sync_vacation() {
    let on = Settings::read().vacation;
    let rec = read_file("vacation.bin").filter(|b| b.len() == 2);
    let now = today();
    if on {
        if rec.is_none() && now > 0 {
            write_file("vacation.bin", &now.to_le_bytes());
        }
        return;
    }
    let Some(b) = rec else { return };
    let start = u16::from_le_bytes([b[0], b[1]]);
    let shift = now as i32 - start as i32;
    if start != 0 && shift > 0 {
        let keys = deck_keys();
        // Every deck must load before any is written, so none is shifted twice or never.
        let loaded: Vec<(String, (Vec<CardState>, u16, u16))> = keys.iter().map(|k| (k.clone(), load_states(k))).collect();
        for (k, (mut states, day, new_today)) in loaded {
            if states.is_empty() {
                continue;
            }
            for st in states.iter_mut() {
                st.due = (st.due as i32 + shift).min(u16::MAX as i32) as u16;
                st.last_review = (st.last_review as i32 + shift).min(u16::MAX as i32) as u16;
            }
            write_states(&k, &states, day, new_today);
        }
    }
    delete_file("vacation.bin");
}

/// The synced decks, in the order of decks.json (written by the sync).
pub fn deck_keys() -> Vec<String> {
    read_json("decks.json").and_then(|v| v.as_array().cloned()).unwrap_or_default().iter()
        .filter_map(|k| k.as_str().map(String::from)).collect()
}

// ---------------------------------------------------------------- mined words
/// matcha's note type for words mined while reading (AnkiSync.cpp, docs/anki).
pub const MINED_NOTE_TYPE: &str = "Matcha sentence";
/// An Anki card's side kept as HTML: this, its CSS file's name, \x1f, the HTML.
pub const HTML_MARK: &str = "\u{1e}H";
/// (CSS, HTML) of a side kept as HTML.
pub fn decode_html(side: &str) -> Option<(String, &str)> {
    let rest = side.strip_prefix(HTML_MARK)?;
    let (css_file, html) = rest.split_once('\u{1f}')?;
    let css = read_file(&format!("{}.css", css_file)).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default();
    Some((css, html))
}
/// A mined card's front side starts with this; its fields follow, split by \x1f.
pub const MINED_MARK: &str = "\u{1e}M";
pub struct Mined {
    pub word: String,
    pub reading: String,
    pub sentence: String,   // the word between \x01 and \x02
    pub definition: String, // one row per line
    pub book: String,
    pub furigana: String,
}
/// matcha's stripExportHtml: <b> to the given marks, <br> to the line break, the five entities.
pub fn strip_export_html(html: &str, open: &str, close: &str, br: &str) -> String {
    const SWAPS: [(&str, usize); 9] = [("<b>", 0), ("</b>", 1), ("<br>", 2), ("<br/>", 2), ("&amp;", 3), ("&lt;", 4), ("&gt;", 5), ("&quot;", 6), ("&#39;", 7)];
    let to = [open, close, br, "&", "<", ">", "\"", "'"];
    let mut out = String::with_capacity(html.len());
    let mut i = 0;
    let b = html.as_bytes();
    'outer: while i < b.len() {
        if b[i] == b'<' || b[i] == b'&' {
            for (from, k) in SWAPS {
                if html[i..].starts_with(from) {
                    out.push_str(to[k]);
                    i += from.len();
                    continue 'outer;
                }
            }
        }
        let ch = html[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}
pub fn encode_mined(fields: &serde_json::Value) -> String {
    let f = |k: &str| String::from(fields.get(k).and_then(|v| v.as_str()).unwrap_or("")).replace('\u{1f}', " ");
    format!(
        "{}\u{1f}{}\u{1f}{}\u{1f}{}\u{1f}{}\u{1f}{}\u{1f}{}",
        MINED_MARK,
        strip_export_html(&f("Word"), "", "", " "),
        strip_export_html(&f("Reading"), "", "", " "),
        strip_export_html(&f("Sentence"), "\u{1}", "\u{2}", " "),
        strip_export_html(&f("Definition"), "", "", "\n"),
        strip_export_html(&f("Book"), "", "", " "),
        f("Furigana"),
    )
}
pub fn decode_mined(side: &str) -> Option<Mined> {
    let rest = side.strip_prefix(MINED_MARK)?;
    let mut p = rest.split('\u{1f}').skip(1).map(String::from);
    let mut n = || p.next().unwrap_or_default();
    Some(Mined { word: n(), reading: n(), sentence: n(), definition: n(), book: n(), furigana: n() })
}

// ---------------------------------------------------------------- card index and sides
pub const TOO_LARGE: u8 = 1;
#[derive(Clone, Copy)]
pub struct CardRecord {
    pub id: u64,
    pub note: u64,
    pub flags: u8,
}
pub fn read_ids(key: &str) -> Vec<CardRecord> {
    let b = read_file(&format!("{}.ids", key)).unwrap_or_default();
    b.chunks_exact(17).map(|c| CardRecord {
        id: u64::from_le_bytes(c[0..8].try_into().unwrap()),
        note: u64::from_le_bytes(c[8..16].try_into().unwrap()),
        flags: c[16],
    }).collect()
}
pub fn write_ids(key: &str, ids: &[CardRecord]) -> bool {
    let mut b = Vec::with_capacity(ids.len() * 17);
    for r in ids {
        b.extend_from_slice(&r.id.to_le_bytes());
        b.extend_from_slice(&r.note.to_le_bytes());
        b.push(r.flags);
    }
    write_file(&format!("{}.ids", key), &b)
}

// A card side is plain text: one entry per line, "T" text (\x01..\x02 around bold), "R" a rule
// (Anki's <hr id=answer>), "I" an image's name.
/// Every card's sides: u32 count, then per card u64 id, u32 + front, u32 + back.
pub fn read_sides(key: &str) -> Vec<(u64, String, String)> {
    let b = read_file(&format!("{}.cards", key)).unwrap_or_default();
    let mut out = Vec::new();
    if b.len() < 4 {
        return out;
    }
    let n = u32::from_le_bytes(b[0..4].try_into().unwrap()) as usize;
    let mut p = 4;
    let take = |p: &mut usize| -> Option<String> {
        let len = u32::from_le_bytes(b.get(*p..*p + 4)?.try_into().ok()?) as usize;
        let s = String::from_utf8_lossy(b.get(*p + 4..*p + 4 + len)?).into_owned();
        *p += 4 + len;
        Some(s)
    };
    for _ in 0..n {
        let Some(id) = b.get(p..p + 8).map(|x| u64::from_le_bytes(x.try_into().unwrap())) else { break };
        p += 8;
        let (Some(f), Some(bk)) = (take(&mut p), take(&mut p)) else { break };
        out.push((id, f, bk));
    }
    out
}
pub fn write_sides(key: &str, sides: &[(u64, String, String)]) -> bool {
    let mut b = Vec::new();
    b.extend_from_slice(&(sides.len() as u32).to_le_bytes());
    for (id, f, bk) in sides {
        b.extend_from_slice(&id.to_le_bytes());
        for s in [f, bk] {
            b.extend_from_slice(&(s.len() as u32).to_le_bytes());
            b.extend_from_slice(s.as_bytes());
        }
    }
    write_file(&format!("{}.cards", key), &b)
}

// ---------------------------------------------------------------- progress
#[derive(Clone, Copy, Default)]
pub struct CardState {
    pub id: u64,
    pub stability: f32,
    pub difficulty: f32,
    pub due: u16,
    pub last_review: u16,
    pub reps: u16,
    pub lapses: u8,
    pub flags: u8, // LEARN_STEP and LEARN_RELEARN while between learning steps
}
pub const LEARN_STEP: u8 = 0x07;
pub const LEARN_RELEARN: u8 = 0x08;

pub fn load_states(key: &str) -> (Vec<CardState>, u16, u16) {
    // A deck of words mined in matcha keeps its progress in matcha's own file.
    let b = match crate::mined::progress_path(key) {
        Some(p) => read(&p).and_then(|m| crate::mined::from_matcha(&m)).unwrap_or_default(),
        None => read_file(&format!("{}.state", key)).unwrap_or_default(),
    };
    if b.len() < 4 {
        return (Vec::new(), today(), 0);
    }
    let day = u16::from_le_bytes([b[0], b[1]]);
    let new_today = u16::from_le_bytes([b[2], b[3]]);
    let states = b[4..].chunks_exact(24).map(|c| CardState {
        id: u64::from_le_bytes(c[0..8].try_into().unwrap()),
        stability: f32::from_le_bytes(c[8..12].try_into().unwrap()),
        difficulty: f32::from_le_bytes(c[12..16].try_into().unwrap()),
        due: u16::from_le_bytes([c[16], c[17]]),
        last_review: u16::from_le_bytes([c[18], c[19]]),
        reps: u16::from_le_bytes([c[20], c[21]]),
        lapses: c[22],
        flags: c[23],
    }).collect::<Vec<_>>();
    let mut states = states;
    states.sort_by_key(|s| s.id);
    (states, day, new_today)
}
pub fn write_states(key: &str, states: &[CardState], day: u16, new_today: u16) -> bool {
    let mut b = Vec::with_capacity(4 + states.len() * 24);
    b.extend_from_slice(&day.to_le_bytes());
    b.extend_from_slice(&new_today.to_le_bytes());
    for s in states {
        b.extend_from_slice(&s.id.to_le_bytes());
        b.extend_from_slice(&s.stability.to_le_bytes());
        b.extend_from_slice(&s.difficulty.to_le_bytes());
        b.extend_from_slice(&s.due.to_le_bytes());
        b.extend_from_slice(&s.last_review.to_le_bytes());
        b.extend_from_slice(&s.reps.to_le_bytes());
        b.push(s.lapses);
        b.push(s.flags);
    }
    match crate::mined::progress_path(key) {
        Some(p) => write(&p, &crate::mined::to_matcha(&b)),
        None => write_file(&format!("{}.state", key), &b),
    }
}
fn find(states: &[CardState], id: u64) -> Option<usize> { states.binary_search_by_key(&id, |s| s.id).ok() }

/// One rating, kept until a sync hands it to Anki.
#[derive(Clone, Copy, Default)]
pub struct LogEntry {
    pub id: u64,
    pub time_ms: i64,
    pub interval: u16, // days; minutes for a learning step with MINUTES set
    pub previous: u16,
    pub rating: u8,
    pub kind: u8, // 0 learning, 1 review, 2 relearning (Anki's revlog types), | MINUTES
}
pub const MINUTES: u8 = 0x80;
/// A deck of mined words keeps its review log in matcha's own file too (same 24-byte records).
fn log_file(key: &str) -> Option<String> { crate::mined::side_path(key, "log") }
pub fn read_log(key: &str) -> Vec<LogEntry> {
    let b = match log_file(key) { Some(p) => read(&p), None => read_file(&format!("{}.log", key)) };
    b.unwrap_or_default().chunks_exact(24).map(|c| LogEntry {
        id: u64::from_le_bytes(c[0..8].try_into().unwrap()),
        time_ms: i64::from_le_bytes(c[8..16].try_into().unwrap()),
        interval: u16::from_le_bytes([c[16], c[17]]),
        previous: u16::from_le_bytes([c[18], c[19]]),
        rating: c[20],
        kind: c[21],
    }).collect()
}
pub fn write_log(key: &str, log: &[LogEntry]) -> bool {
    if log.is_empty() {
        match log_file(key) { Some(p) => { dsn::fs_delete(&p); } None => delete_file(&format!("{}.log", key)) }
        return true;
    }
    let mut b = Vec::with_capacity(log.len() * 24);
    for e in log {
        b.extend_from_slice(&e.id.to_le_bytes());
        b.extend_from_slice(&e.time_ms.to_le_bytes());
        b.extend_from_slice(&e.interval.to_le_bytes());
        b.extend_from_slice(&e.previous.to_le_bytes());
        b.push(e.rating);
        b.push(e.kind);
        b.extend_from_slice(&[0, 0]);
    }
    match log_file(key) { Some(p) => write(&p, &b), None => write_file(&format!("{}.log", key), &b) }
}

/// A review done in Anki, folded into the device's progress: the device's FSRS gives the memory
/// state and Anki's interval the due date (matcha's applyPulledReviews).
pub struct Pulled {
    pub id: u64,
    pub time_ms: i64,
    pub rating: u8,
    pub interval: u16,
}
pub fn apply_pulled(key: &str, reviews: &[Pulled]) -> bool {
    if reviews.is_empty() {
        return true;
    }
    let (mut states, day, new_today) = load_states(key);
    for r in reviews {
        if !(1..=4).contains(&r.rating) || r.time_ms <= 0 {
            continue;
        }
        let review_day = day_of_ms(r.time_ms);
        if review_day < 0 {
            continue;
        }
        let i = match states.binary_search_by_key(&r.id, |s| s.id) {
            Ok(i) => i,
            Err(i) => {
                states.insert(i, CardState { id: r.id, ..Default::default() });
                i
            }
        };
        let s = &mut states[i];
        if s.reps > 0 && review_day < s.last_review as i32 {
            continue; // older than what the device already counted
        }
        let rating = fsrs::Rating::from(r.rating);
        let after = if s.reps == 0 {
            fsrs::initial(rating)
        } else {
            fsrs::next(fsrs::Memory { stability: s.stability, difficulty: s.difficulty },
                       (review_day - s.last_review as i32) as f32, rating)
        };
        s.stability = after.stability;
        s.difficulty = after.difficulty;
        s.last_review = review_day as u16;
        s.due = (review_day + r.interval as i32) as u16;
        s.flags &= !(LEARN_STEP | LEARN_RELEARN);
        if rating == fsrs::Rating::Again && s.reps > 0 {
            s.lapses = s.lapses.saturating_add(1);
        }
        s.reps += 1;
    }
    write_states(key, &states, day, new_today)
}

// ---------------------------------------------------------------- the deck list
pub struct DeckSummary {
    pub key: String,
    pub label: String,
    pub migaku: bool,
    pub due: u32,
    pub fresh: u32,
}

/// One summary per synced deck, by the name shown (an Anki subdeck by its last two levels).
pub fn list_decks() -> Vec<DeckSummary> {
    let day = today();
    let vacation = Settings::read().vacation;
    let mut out = Vec::new();
    for key in deck_keys() {
        let Some(info) = DeckInfo::read(&key) else { continue };
        let ids = read_ids(&key);
        let (states, sday, new_today) = load_states(&key);
        let new_today = if sday == day { new_today } else { 0 };
        let leeches = read_leeches(&key);
        let mut due = 0u32;
        let mut unseen = 0u32;
        for r in &ids {
            if leeches.binary_search(&r.id).is_ok() {
                continue;
            }
            match find(&states, r.id) {
                Some(i) => due += (states[i].due <= day) as u32,
                None => unseen += 1,
            }
        }
        let per_day = info.new_per_day.max(0) as u32;
        let fresh = unseen.min(per_day.saturating_sub(new_today as u32));
        let due = due.min(info.reviews_per_day.max(0) as u32);
        let (due, fresh) = if vacation { (0, 0) } else { (due, fresh) };
        out.push(DeckSummary { key, label: short_name(&info.name), migaku: info.migaku, due, fresh });
    }
    out.sort_by(|a, b| a.label.cmp(&b.label));
    // A Migaku deck named like another deck says which it is.
    let names: Vec<String> = out.iter().map(|d| d.label.clone()).collect();
    for (i, d) in out.iter_mut().enumerate() {
        if d.migaku && names.iter().enumerate().any(|(j, n)| j != i && *n == d.label) {
            d.label += " (Migaku)";
        }
    }
    out
}
/// "HSK 1::Meaning" stays; "A::B::C" shows as "B › C".
fn short_name(name: &str) -> String {
    let parts: Vec<&str> = name.split("::").collect();
    match parts.len() {
        0 | 1 => String::from(name),
        n => format!("{} \u{203A} {}", parts[n - 2], parts[n - 1]),
    }
}

// ---------------------------------------------------------------- leeches
/// Card ids noted as leeches and not yet sent to Anki, sorted (with their notes, for tags.add).
pub fn read_leech_records(key: &str) -> Vec<(u64, u64)> {
    let mut v: Vec<(u64, u64)> = read_file(&format!("{}.leeches", key)).unwrap_or_default().chunks_exact(16)
        .map(|c| (u64::from_le_bytes(c[0..8].try_into().unwrap()), u64::from_le_bytes(c[8..16].try_into().unwrap())))
        .collect();
    v.sort();
    v
}
fn read_leeches(key: &str) -> Vec<u64> { read_leech_records(key).iter().map(|l| l.0).collect() }

// ---------------------------------------------------------------- the session
pub const MAX_QUEUE: usize = 300;

#[derive(Clone, Copy)]
struct QueueItem {
    id: u64,
    flags: u8,
    note: u64,
}
#[derive(Clone, Copy)]
struct Learning {
    item: QueueItem,
    due_ms: i64,
    step: u8,
    relearn: bool,
}

#[derive(Clone, Copy, Default)]
pub struct Interval {
    pub value: u16,
    pub minutes: bool,
}

pub struct Session {
    pub key: String,
    info: Option<DeckInfo>,
    states: Vec<CardState>,
    queue: Vec<QueueItem>,
    position: usize,
    reviewed: usize,
    rating_counts: [u16; 4],
    day: u16,
    new_today: u16,
    params: fsrs::Params,
    learn: Vec<u16>,
    relearn: Vec<u16>,
    learning: Vec<Learning>,
    active_learn: Option<usize>,
    bury_new: bool,
    bury_reviews: bool,
    easy_days: [u8; 7],
    leech_threshold: u8,
    leech_suspend: bool,
    notes_done: Vec<u64>,
    leeches: Vec<u64>,
    unsaved: u8,
    pending_log: Vec<LogEntry>,
    sides: Vec<(u64, String, String)>,
}

fn ms_now() -> i64 {
    let t = now();
    if t > 0 { t * 1000 } else { dsn::time() as i64 * 1000 }
}

impl Session {
    /// Loads a deck's progress and queues today's cards: due reviews first, then new ones.
    pub fn open(key: &str) -> Session {
        let info = DeckInfo::read(key);
        let params = info.as_ref().map(|i| i.params()).unwrap_or_default();
        let (states, sday, mut new_today) = load_states(key);
        let day = today();
        if sday != day {
            new_today = 0;
        }
        let mut s = Session {
            key: String::from(key),
            params,
            learn: info.as_ref().map(|i| i.learn_steps.clone()).unwrap_or_else(|| vec![10]),
            relearn: info.as_ref().map(|i| i.relearn_steps.clone()).unwrap_or_else(|| vec![10]),
            bury_new: info.as_ref().is_some_and(|i| i.bury_new),
            bury_reviews: info.as_ref().is_some_and(|i| i.bury_reviews),
            easy_days: info.as_ref().map(|i| i.easy_days).unwrap_or([100; 7]),
            leech_threshold: info.as_ref().map(|i| i.leech_threshold).unwrap_or(0),
            leech_suspend: info.as_ref().is_none_or(|i| i.leech_suspend),
            info,
            states,
            queue: Vec::new(),
            position: 0,
            reviewed: 0,
            rating_counts: [0; 4],
            day,
            new_today,
            learning: Vec::new(),
            active_learn: None,
            notes_done: Vec::new(),
            leeches: Vec::new(),
            unsaved: 0,
            pending_log: Vec::new(),
            sides: read_sides(key),
        };
        if s.leech_threshold > 0 && s.leech_suspend {
            s.leeches = read_leeches(key);
        }
        if !Settings::read().vacation {
            s.queue_cards(); // on vacation: progress loaded, nothing queued
        }
        s
    }

    fn queue_cards(&mut self) {
        let info = self.info.as_ref();
        let per_day = info.map(|i| i.new_per_day.max(0) as u16).unwrap_or(20);
        let new_limit = per_day.saturating_sub(self.new_today) as usize;
        let mut fresh: Vec<QueueItem> = Vec::new();
        // When saved learning steps fall due, by the wall clock.
        let step_due: Vec<(u64, i64)> = read_file(&format!("{}.steps", self.key)).unwrap_or_default().chunks_exact(16)
            .map(|c| (u64::from_le_bytes(c[0..8].try_into().unwrap()), i64::from_le_bytes(c[8..16].try_into().unwrap())))
            .collect();
        let wall = now();
        let ms = ms_now();
        for c in read_ids(&self.key) {
            if self.leeches.binary_search(&c.id).is_ok() {
                continue;
            }
            let item = QueueItem { id: c.id, flags: c.flags, note: c.note };
            if let Some(i) = find(&self.states, c.id) {
                let st = self.states[i];
                if (self.bury_new || self.bury_reviews) && st.reps > 0 && st.last_review == self.day {
                    self.notes_done.push(c.note);
                }
                let saved = st.flags & LEARN_STEP;
                let relearn = st.flags & LEARN_RELEARN != 0;
                let count = if relearn { self.relearn.len() } else { self.learn.len() };
                if saved > 0 && count > 0 && st.due <= self.day && self.learning.len() < 64 {
                    let step = ((saved - 1) as usize).min(count - 1) as u8;
                    let due_ms = step_due.iter().find(|d| d.0 == c.id).map(|d| if wall > 0 { d.1 * 1000 } else { ms })
                        .unwrap_or(ms);
                    self.learning.push(Learning { item, due_ms, step, relearn });
                    continue;
                }
                if st.due <= self.day && self.queue.len() < MAX_QUEUE {
                    self.queue.push(item);
                }
            } else if fresh.len() < new_limit && fresh.len() < MAX_QUEUE {
                fresh.push(item);
            }
        }
        self.notes_done.sort_unstable();
        self.notes_done.dedup();
        // Most overdue first; the daily review limit keeps the most overdue.
        let states = &self.states;
        let due_of = |id: u64| find(states, id).map(|i| states[i].due).unwrap_or(0);
        self.queue.sort_by_key(|q| due_of(q.id));
        let reviews_today = states.iter().filter(|s| s.reps > 0 && s.last_review == self.day && s.due > self.day).count();
        let allowed = info.map(|i| i.reviews_per_day.max(0) as usize).unwrap_or(9999).saturating_sub(reviews_today);
        self.queue.truncate(allowed);
        let order = info.map(|i| i.review_order.as_str()).unwrap_or("day");
        let mut seed = (dsn::time() as u64) ^ 0x9E37_79B9_7F4A_7C15;
        let mut rnd = move |n: usize| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed % n.max(1) as u64) as usize
        };
        if order == "random" {
            for i in (1..self.queue.len()).rev() {
                self.queue.swap(i, rnd(i + 1));
            }
        } else if order == "intervalAscending" || order == "intervalDescending" {
            let asc = order == "intervalAscending";
            let ivl = |id: u64| find(states, id).map(|i| states[i].due as i32 - states[i].last_review as i32).unwrap_or(0);
            self.queue.sort_by(|a, b| if asc { ivl(a.id).cmp(&ivl(b.id)) } else { ivl(b.id).cmp(&ivl(a.id)) });
        }
        if info.is_some_and(|i| i.new_random) {
            for i in (1..fresh.len()).rev() {
                fresh.swap(i, rnd(i + 1));
            }
        }
        let mix = info.map(|i| i.new_mix.as_str()).unwrap_or("afterReviews");
        if mix == "beforeReviews" {
            fresh.truncate(MAX_QUEUE);
            self.queue.truncate(MAX_QUEUE - fresh.len());
            let mut q = fresh;
            q.extend_from_slice(&self.queue);
            self.queue = q;
        } else if mix == "mixed" && !self.queue.is_empty() && !fresh.is_empty() {
            fresh.truncate(MAX_QUEUE.saturating_sub(self.queue.len()));
            let every = (self.queue.len() / fresh.len().max(1)).max(1);
            let mut merged = Vec::with_capacity(self.queue.len() + fresh.len());
            let mut r = 0;
            for item in fresh {
                for _ in 0..every {
                    if r < self.queue.len() {
                        merged.push(self.queue[r]);
                        r += 1;
                    }
                }
                merged.push(item);
            }
            merged.extend_from_slice(&self.queue[r..]);
            self.queue = merged;
        } else {
            fresh.truncate(MAX_QUEUE.saturating_sub(self.queue.len()));
            self.queue.extend(fresh);
        }
        self.pick_next();
    }

    pub fn finished(&self) -> bool { self.position >= self.queue.len() && self.learning.is_empty() }
    pub fn remaining(&self) -> usize { self.queue.len().saturating_sub(self.position) + self.learning.len() }
    pub fn reviewed(&self) -> usize { self.reviewed }
    /// Whether the deck comes from the Migaku bridge rather than Anki.
    pub fn is_migaku(&self) -> bool { self.info.as_ref().is_some_and(|i| i.migaku) }
    pub fn total(&self) -> usize { self.queue.len() + self.learning.len() }
    pub fn rating_count(&self, i: usize) -> u16 { self.rating_counts[i] }

    fn active(&self) -> QueueItem {
        match self.active_learn {
            Some(i) => self.learning[i].item,
            None => self.queue[self.position],
        }
    }
    /// The current card's two sides, and whether it was too large to fetch.
    pub fn current(&self) -> Option<(&str, &str, bool)> {
        if self.finished() {
            return None;
        }
        let item = self.active();
        let too_large = item.flags & TOO_LARGE != 0;
        self.sides.iter().find(|s| s.0 == item.id).map(|s| (s.1.as_str(), s.2.as_str(), too_large))
            .or(Some(("", "", too_large)))
    }

    // A learning card whose step has passed comes first; with nothing else left, the next one is
    // shown early rather than leaving the session waiting.
    fn pick_next(&mut self) {
        self.active_learn = None;
        let leeches = &self.leeches;
        self.learning.retain(|l| leeches.binary_search(&l.item.id).is_err());
        while self.position < self.queue.len()
            && (self.buried(self.queue[self.position]) || self.leeches.binary_search(&self.queue[self.position].id).is_ok())
        {
            self.position += 1;
        }
        if self.learning.is_empty() {
            return;
        }
        let earliest = (0..self.learning.len()).min_by_key(|&i| self.learning[i].due_ms).unwrap();
        if ms_now() >= self.learning[earliest].due_ms || self.position >= self.queue.len() {
            self.active_learn = Some(earliest);
        }
    }
    fn buried(&self, item: QueueItem) -> bool {
        if item.note == 0 || self.notes_done.binary_search(&item.note).is_err() {
            return false;
        }
        match find(&self.states, item.id) {
            Some(i) if self.states[i].reps > 0 && self.states[i].last_review == self.day => false,
            Some(_) => self.bury_reviews,
            None => self.bury_new,
        }
    }
    // Easy Days: within a few percent of the interval, the weekday with the most room.
    fn easy_day(&self, interval: u16) -> u16 {
        if interval < 3 || self.easy_days.iter().all(|&p| p == 100) {
            return interval;
        }
        let spread = (interval as i32 * 5 / 100).max(1);
        let mut best = interval;
        let mut best_room = -1;
        for d in -spread..=spread {
            let c = interval as i32 + d;
            if c < 1 || c > self.params.max_interval as i32 {
                continue;
            }
            // Day 0 is 2020-01-01, a Wednesday; easy_days starts on Monday.
            let room = self.easy_days[((self.day as i32 + c + 2) % 7) as usize] as i32;
            if room > best_room || (room == best_room && d.abs() < (best as i32 - interval as i32).abs()) {
                best_room = room;
                best = c as u16;
            }
        }
        best
    }
    fn next_step(rating: fsrs::Rating, current: i32, count: usize) -> i32 {
        let next = match rating {
            fsrs::Rating::Again => 0,
            fsrs::Rating::Hard => current,
            fsrs::Rating::Good => current + 1,
            fsrs::Rating::Easy => return -1,
        };
        if (next as usize) < count { next } else { -1 }
    }
    fn step_minutes(steps: &[u16], rating: fsrs::Rating, current: i32, next: i32) -> u16 {
        if rating == fsrs::Rating::Hard && current == 0 {
            if steps.len() >= 2 {
                return (steps[0] + steps[1]) / 2;
            }
            return (steps[0] as u32 * 3 / 2).min(steps[0] as u32 + 1440) as u16;
        }
        steps[next as usize]
    }

    /// What each rating would give: a learning step in minutes, or an interval in days.
    pub fn previews(&self) -> [Interval; 4] {
        let mut out = [Interval::default(); 4];
        if self.finished() {
            return out;
        }
        let st = find(&self.states, self.active().id).map(|i| self.states[i]);
        let memory = st.map(|s| fsrs::Memory { stability: s.stability, difficulty: s.difficulty }).unwrap_or_default();
        let elapsed = st.map(|s| (self.day as i32 - s.last_review as i32) as f32).unwrap_or(0.0);
        let days = fsrs::preview_intervals(memory, st.is_none(), elapsed, &self.params);
        let in_learning = self.active_learn.is_some();
        let relearn = self.active_learn.is_some_and(|i| self.learning[i].relearn);
        let current = self.active_learn.map(|i| self.learning[i].step as i32).unwrap_or(0);
        for (i, o) in out.iter_mut().enumerate() {
            let rating = fsrs::Rating::from(i as u8 + 1);
            o.value = self.easy_day(days[i]);
            let steps = st.is_none() || in_learning || rating == fsrs::Rating::Again;
            if !steps {
                continue;
            }
            let re = if in_learning { relearn } else { st.is_some() };
            let list = if re { &self.relearn } else { &self.learn };
            let next = Self::next_step(rating, current, list.len());
            if next < 0 {
                if rating == fsrs::Rating::Again {
                    let m = if st.is_some() { fsrs::next(memory, elapsed, rating) } else { fsrs::initial(rating) };
                    o.value = self.easy_day(fsrs::interval_days(m.stability, &self.params));
                }
                continue;
            }
            let minutes = Self::step_minutes(list, rating, current, next);
            *o = if minutes >= 1440 { Interval { value: minutes / 1440, minutes: false } } else { Interval { value: minutes, minutes: true } };
        }
        out
    }

    pub fn rate(&mut self, rating: fsrs::Rating) {
        if self.finished() {
            return;
        }
        let learnt = self.active_learn.map(|i| self.learning.remove(i));
        self.active_learn = None;
        let item = match learnt {
            Some(l) => l.item,
            None => {
                self.position += 1;
                self.queue[self.position - 1]
            }
        };
        let idx = match self.states.binary_search_by_key(&item.id, |s| s.id) {
            Ok(i) => i,
            Err(i) => {
                self.states.insert(i, CardState { id: item.id, ..Default::default() });
                self.new_today += 1;
                i
            }
        };
        let day = self.day;
        let s = self.states[idx];
        let is_new = s.reps == 0;
        let previous = if is_new { 0 } else { (s.due as i32 - s.last_review as i32).max(0) as u16 };
        let before = fsrs::Memory { stability: s.stability, difficulty: s.difficulty };
        let after = if is_new { fsrs::initial(rating) } else { fsrs::next(before, (day as i32 - s.last_review as i32) as f32, rating) };
        let from_learning = learnt.is_some();
        let relearn = learnt.map(|l| l.relearn).unwrap_or(!is_new && rating == fsrs::Rating::Again);
        let stepping = from_learning || is_new || rating == fsrs::Rating::Again;
        let mut st = s;
        st.stability = after.stability;
        st.difficulty = after.difficulty;
        st.last_review = day;
        st.reps += 1;
        if rating == fsrs::Rating::Again && !is_new && !from_learning {
            st.lapses = st.lapses.saturating_add(1);
            // Anki's rule: a leech at the threshold, and again every half threshold after it.
            let t = self.leech_threshold;
            if t > 0 && st.lapses >= t && (st.lapses - t) as i32 % (t as i32 / 2).max(1) == 0 {
                self.mark_leech(item);
            }
        }
        if item.note != 0 && (self.bury_new || self.bury_reviews) {
            if let Err(i) = self.notes_done.binary_search(&item.note) {
                self.notes_done.insert(i, item.note);
            }
        }
        let current = learnt.map(|l| l.step as i32).unwrap_or(0);
        let list = if relearn { self.relearn.clone() } else { self.learn.clone() };
        let next = if stepping { Self::next_step(rating, current, list.len()) } else { -1 };
        let mut step_minutes_logged = 0u16;
        st.flags &= !(LEARN_STEP | LEARN_RELEARN);
        if next >= 0 {
            let minutes = Self::step_minutes(&list, rating, current, next);
            if minutes < 1440 {
                st.flags |= ((next + 1) as u8) & LEARN_STEP;
                if relearn {
                    st.flags |= LEARN_RELEARN;
                }
                st.due = day;
                step_minutes_logged = minutes;
                if self.learning.len() < 64 {
                    self.learning.push(Learning { item, due_ms: ms_now() + minutes as i64 * 60_000, step: next as u8, relearn });
                } else if self.queue.len() < MAX_QUEUE + 50 {
                    let at = (self.position + 4).min(self.queue.len());
                    self.queue.insert(at, item);
                }
            } else {
                st.due = day + minutes / 1440; // a step of days ends this session for the card
            }
        } else {
            st.due = day + self.easy_day(fsrs::interval_days(after.stability, &self.params));
        }
        self.states[idx] = st;
        // Logged for Anki only with a real clock: a review's time is its identity there.
        if now() > 0 {
            let mut kind = if relearn { 2 } else if stepping { 0 } else { 1 };
            if step_minutes_logged > 0 {
                kind |= MINUTES;
            }
            self.pending_log.push(LogEntry {
                id: item.id,
                time_ms: ms_now(),
                interval: if step_minutes_logged > 0 { step_minutes_logged } else { st.due - day },
                previous,
                rating: rating as u8,
                kind,
            });
        }
        self.rating_counts[rating as usize - 1] += 1;
        self.reviewed += 1;
        // Saved after every rating: sleep ends a dessen app without warning, so nothing waits in RAM.
        self.unsaved += 1;
        self.save();
        self.pick_next();
    }

    fn mark_leech(&mut self, item: QueueItem) {
        let mut b = read_file(&format!("{}.leeches", self.key)).unwrap_or_default();
        b.extend_from_slice(&item.id.to_le_bytes());
        b.extend_from_slice(&item.note.to_le_bytes());
        write_file(&format!("{}.leeches", self.key), &b);
        if self.leech_suspend {
            if let Err(i) = self.leeches.binary_search(&item.id) {
                self.leeches.insert(i, item.id);
            }
        }
    }

    /// Writes progress if anything changed since the last save, then the log and step times.
    pub fn save(&mut self) {
        if self.unsaved == 0 {
            return;
        }
        if !write_states(&self.key, &self.states, self.day, self.new_today) {
            return;
        }
        // Step times, so cards between steps come back on time in the next session.
        let wall = now();
        if wall > 0 {
            if self.learning.is_empty() {
                delete_file(&format!("{}.steps", self.key));
            } else {
                let ms = ms_now();
                let mut b = Vec::new();
                for l in &self.learning {
                    b.extend_from_slice(&l.item.id.to_le_bytes());
                    b.extend_from_slice(&(wall + (l.due_ms - ms).max(0) / 1000).to_le_bytes());
                }
                write_file(&format!("{}.steps", self.key), &b);
            }
        }
        // After the states: a rating logged without its new state would be sent twice.
        let mut log = read_log(&self.key);
        log.extend_from_slice(&self.pending_log);
        if write_log(&self.key, &log) {
            self.pending_log.clear();
        }
        self.unsaved = 0;
    }
}
