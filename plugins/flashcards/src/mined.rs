//! Words mined in matcha, reviewed before they reach Anki (FlashcardStore's local decks).
//!
//! matcha keeps them in /sentence-mining/sentences-<language>.csv (RFC 4180 after a block of '#'
//! header lines; columns GUID, Word, Reading, Sentence, Definition, Book, Author, Date,
//! Dictionary, Furigana, Tags). Each file becomes a deck "local-<language>" here: its rows as
//! mined cards (the spine card), without the ones matcha already handed over to Anki
//! (/.system/flashcards-<language>.anki), which are reviewed in their Anki deck instead.
//! Progress is matcha's own file, /.system/flashcards-<language>.bin, so either can review.
use crate::store::{self, CardRecord, DeckInfo};
use crate::dsn;
use alloc::{format, string::String, vec::Vec};

pub const MINING_DIR: &str = "/sentence-mining";
pub const PREFIX: &str = "local-";

/// The language of a local deck's key ("local-ja" -> "ja").
pub fn language(key: &str) -> Option<&str> { key.strip_prefix(PREFIX) }

/// matcha's firmware folder (/.system, or /system on older cards); None when matcha never ran.
pub fn system_dir() -> Option<&'static str> {
    let mut b = alloc::vec![0u8; 8192];
    ["/.system", "/system"].into_iter().find(|d| {
        let n = dsn::fs_list(d, &mut b);
        n >= 0 || n == dsn::ERR_TOO_BIG
    })
}

/// "m" + 16 hex digits as a number; 0 when it is not one (matcha's guidToId).
fn guid_to_id(g: &str) -> u64 {
    let Some(h) = g.strip_prefix('m') else { return 0 };
    if h.len() != 16 {
        return 0;
    }
    u64::from_str_radix(h, 16).unwrap_or(0)
}

/// The data records of a CSV export: '#' lines and blank lines skipped, quotes undone.
fn records(text: &str) -> Vec<Vec<String>> {
    let mut out = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let mut field = String::new();
    let (mut quoted, mut at_start, mut comment, mut any) = (false, true, false, false);
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if comment {
            if c == '\n' {
                comment = false;
                at_start = true;
            }
            continue;
        }
        if at_start && c == '#' {
            comment = true;
            continue;
        }
        at_start = false;
        if quoted {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    field.push('"');
                    chars.next();
                } else {
                    quoted = false;
                }
            } else {
                field.push(c);
            }
            continue;
        }
        match c {
            '"' => {
                quoted = true;
                any = true;
            }
            ',' => {
                row.push(core::mem::take(&mut field));
                any = true;
            }
            '\r' => {}
            '\n' => {
                if any || !field.is_empty() {
                    row.push(core::mem::take(&mut field));
                    out.push(core::mem::take(&mut row));
                }
                row.clear();
                any = false;
                at_start = true;
            }
            _ => {
                field.push(c);
                any = true;
            }
        }
    }
    if any || !field.is_empty() {
        row.push(field);
        out.push(row);
    }
    out
}

fn language_name(code: &str) -> String {
    use crate::i18n::{tr, trf, Arg};
    let key = alloc::format!("lang_{}", code.to_ascii_lowercase().replace('-', "_"));
    let name = match code { "zh-TW" | "zh-Hant" => tr("lang_zh_hant"), _ => tr(&key) };
    // An unknown code comes back as its key: show the code itself then.
    let name = if name == key { String::from(code) } else { name };
    trf("mined_deck", &[Arg::S(&name)])
}

/// Brings each mining export in as a local deck, again only when the file changed.
pub fn import() {
    let mut list = alloc::vec![0u8; 16 * 1024];
    let n = dsn::fs_list(MINING_DIR, &mut list);
    if n < 0 {
        return;
    }
    let names: Vec<String> = core::str::from_utf8(&list[..n as usize]).unwrap_or("").lines()
        .filter(|l| l.starts_with("sentences-") && l.ends_with(".csv")).map(String::from).collect();
    let mut keys = store::deck_keys();
    for name in names {
        let lang = &name["sentences-".len()..name.len() - 4];
        let key = format!("{}{}", PREFIX, lang);
        let Some(csv) = store::read(&format!("{}/{}", MINING_DIR, name)) else { continue };
        // Handed-over cards: their CSV ids, 8 bytes each.
        let handed = read_handed(lang);
        // Rebuilt only when the export or the handed-over list changed.
        let stamp = format!("{}:{}", csv.len(), handed.len());
        if store::read_file(&format!("{}.src", key)).as_deref() == Some(stamp.as_bytes()) {
            continue;
        }
        let text = String::from_utf8_lossy(&csv);
        let mut ids = Vec::new();
        let mut sides = Vec::new();
        for r in records(&text) {
            let id = guid_to_id(r.first().map(|s| s.as_str()).unwrap_or(""));
            if id == 0 || handed.contains(&id) || ids.iter().any(|c: &CardRecord| c.id == id) {
                continue;
            }
            let f = |i: usize| r.get(i).cloned().unwrap_or_default();
            // Exports from before the Furigana column hold Tags there: only Anki's syntax counts.
            let furigana = { let v = f(9); if v.contains('[') { v } else { String::new() } };
            let fields = serde_json::json!({
                "Word": f(1), "Reading": f(2), "Sentence": f(3), "Definition": f(4), "Book": f(5), "Furigana": furigana,
            });
            ids.push(CardRecord { id, note: id, flags: 0 });
            sides.push((id, store::encode_mined(&fields), String::new()));
        }
        // Every word handed over: the words live in their Anki deck now, the local deck goes.
        if ids.is_empty() {
            keys.retain(|k| *k != key);
            store::write_file(&format!("{}.src", key), stamp.as_bytes());
            continue;
        }
        let mut info = DeckInfo::read(&key).unwrap_or_else(|| DeckInfo::new(&key, &language_name(lang), false, ""));
        info.lang = String::from(lang.split(['-', '_']).next().unwrap_or(lang));
        info.name = language_name(lang);
        if store::write_ids(&key, &ids) && store::Sides::open(&key).replace_all(sides) && info.write() {
            store::write_file(&format!("{}.src", key), stamp.as_bytes());
        }
        if !keys.contains(&key) {
            keys.push(key);
        }
    }
    store::write_json("decks.json", &serde_json::json!(keys));
}

/// Where a local deck's progress lives: matcha's own file when matcha's folder exists.
pub fn progress_path(key: &str) -> Option<String> { side_path(key, "bin") }
/// matcha's file of a language with this extension (bin progress, log reviews, anki handed-over
/// ids, deck the Anki deck they went to); None for other decks or without matcha's folder.
pub fn side_path(key: &str, ext: &str) -> Option<String> {
    let lang = language(key)?;
    system_dir().map(|d| format!("{}/flashcards-{}.{}", d, lang, ext))
}
/// A language's handed-over ids (sorted u64s): matcha's file, or the plugin's own without it.
pub fn handed_path(lang: &str) -> String {
    let key = format!("{}{}", PREFIX, lang);
    side_path(&key, "anki").unwrap_or_else(|| store::path(&format!("{}.anki", key)))
}
pub fn read_handed(lang: &str) -> Vec<u64> {
    store::read(&handed_path(lang)).unwrap_or_default().chunks_exact(8).map(|c| u64::from_le_bytes(c.try_into().unwrap())).collect()
}
/// The languages with a mining export.
pub fn languages() -> Vec<String> {
    let mut list = alloc::vec![0u8; 16 * 1024];
    let n = dsn::fs_list(MINING_DIR, &mut list);
    if n < 0 {
        return Vec::new();
    }
    core::str::from_utf8(&list[..n as usize]).unwrap_or("").lines()
        .filter(|l| l.starts_with("sentences-") && l.ends_with(".csv"))
        .map(|l| String::from(&l["sentences-".len()..l.len() - 4])).collect()
}
/// One mined row: its id and its columns (Word .. Tags, the export's order after GUID).
pub struct Row {
    pub id: u64,
    pub fields: Vec<String>,
}
/// The rows of a language's export not handed over yet.
pub fn unhanded_rows(lang: &str) -> Vec<Row> {
    let handed = read_handed(lang);
    let Some(csv) = store::read(&format!("{}/sentences-{}.csv", MINING_DIR, lang)) else { return Vec::new() };
    let text = String::from_utf8_lossy(&csv);
    let mut out: Vec<Row> = Vec::new();
    for r in records(&text) {
        let id = guid_to_id(r.first().map(|s| s.as_str()).unwrap_or(""));
        if id == 0 || handed.contains(&id) || out.iter().any(|x| x.id == id) {
            continue;
        }
        let mut fields: Vec<String> = r.into_iter().skip(1).collect();
        fields.resize(10, String::new());
        // Exports from before the Furigana column hold Tags there.
        if !fields[8].contains('[') && fields[9].is_empty() {
            fields[9] = core::mem::take(&mut fields[8]);
        }
        out.push(Row { id, fields });
    }
    out
}
/// matcha's progress file: "FCS1", u16 day, u16 new today, u32 count, then 24-byte records (the
/// plugin's own layout after its 4-byte header).
pub fn from_matcha(b: &[u8]) -> Option<Vec<u8>> {
    if b.len() < 12 || &b[0..4] != b"FCS1" {
        return None;
    }
    let count = u32::from_le_bytes(b[8..12].try_into().ok()?) as usize;
    let mut out = Vec::with_capacity(4 + count * 24);
    out.extend_from_slice(&b[4..8]);
    out.extend_from_slice(b.get(12..12 + count * 24)?);
    Some(out)
}
pub fn to_matcha(plugin: &[u8]) -> Vec<u8> {
    let count = (plugin.len().saturating_sub(4) / 24) as u32;
    let mut out = Vec::with_capacity(plugin.len() + 8);
    out.extend_from_slice(b"FCS1");
    out.extend_from_slice(&plugin[..4.min(plugin.len())]);
    out.extend_from_slice(&count.to_le_bytes());
    out.extend_from_slice(plugin.get(4..).unwrap_or(&[]));
    out
}
