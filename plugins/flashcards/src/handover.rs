//! Mined words to Anki (or Migaku), as matcha's sync hands them over (AnkiSync.cpp handOverMined,
//! FlashcardStore.cpp handOver): each word of /sentence-mining not handed over yet becomes a
//! "Matcha sentence" note in the deck Matcha::<language>; its progress and review history move to
//! that card, and the local deck leaves it out from then on. The deck joins the synced decks.
use crate::mined::{self, Row};
use crate::rpc::{Client, Error, Kind};
use crate::store::{self, CardState, LogEntry};
use alloc::{format, string::String, vec::Vec};
use serde_json::{json, Value};

const FIELDS: [&str; 9] = ["Word", "Reading", "Sentence", "Definition", "Book", "Author", "Date", "Dictionary", "Furigana"];
const BATCH: usize = 5; // notes per request: a mined row holds a whole definition
const MINED_FRONT: &str = "<div class=\"sentence\">{{Sentence}}</div>\n<div class=\"book\">{{Book}}</div>";
const MINED_BACK: &str = "<div class=\"word\">{{#Furigana}}{{furigana:Furigana}}{{/Furigana}}{{^Furigana}}{{Word}}{{/Furigana}}</div>\n<div class=\"reading\">{{Reading}}</div>\n<hr>\n<div class=\"def\">{{Definition}}</div>\n<hr>\n<div class=\"sentence small\">{{Sentence}}</div>\n<div class=\"book\">{{Book}}</div>";
const MINED_CSS: &str = "";

/// A field without control characters other than tab and newline (matcha's minedField).
fn field(v: &str) -> String { v.chars().filter(|&c| c as u32 >= 0x20 || c == '\n' || c == '\t').collect() }
/// A value inside a quoted field search: Anki's wildcards taken literally.
fn quoted(v: &str) -> String {
    let mut o = String::new();
    for c in v.chars() {
        if matches!(c, '"' | '\\' | '*' | '_') {
            o.push('\\');
        }
        o.push(c);
    }
    o
}
fn id_of(v: &Value) -> u64 { v.as_str().and_then(|s| s.parse().ok()).or_else(|| v.as_u64()).unwrap_or(0) }

fn autonym(code: &str) -> String {
    let n = match code {
        "ja" => "日本語", "zh" => "中文", "en" => "English", "de" => "Deutsch", "fr" => "Français", "es" => "Español",
        "it" => "Italiano", "pt" => "Português", "nl" => "Nederlands", "hu" => "Magyar", "pl" => "Polski",
        "ru" => "Русский", "uk" => "Українська",
        _ => return code.to_uppercase(),
    };
    String::from(n)
}

fn ensure_note_type(client: &mut Client) -> Result<(), Error> {
    let types = client.call("noteTypes.list", json!({}))?;
    if types.as_array().into_iter().flatten().any(|t| t["name"].as_str() == Some(store::MINED_NOTE_TYPE)) {
        return Ok(());
    }
    let fields: Vec<Value> = FIELDS.iter().map(|f| json!({"name": f})).collect();
    client.call("noteTypes.create", json!({"name": store::MINED_NOTE_TYPE, "fields": fields,
        "templates": [{"name": "Card 1", "questionHtml": MINED_FRONT, "answerHtml": MINED_BACK}], "css": MINED_CSS}))?;
    Ok(())
}

fn note(row: &Row, deck: &str, allow_dupes: bool) -> Value {
    let mut fields = serde_json::Map::new();
    for (i, f) in FIELDS.iter().enumerate() {
        fields.insert(String::from(*f), Value::from(field(row.fields.get(i).map(|s| s.as_str()).unwrap_or(""))));
    }
    let mut tags = alloc::vec![Value::from("matcha")];
    tags.extend(field(row.fields.get(9).map(|s| s.as_str()).unwrap_or("")).split_whitespace().map(Value::from));
    let mut n = json!({"noteType": {"name": store::MINED_NOTE_TYPE}, "deck": {"id": deck}, "fields": fields, "tags": tags});
    if allow_dupes {
        n["duplicates"] = json!({"allow": true});
    }
    n
}

enum Adopt { Found(u64), FoundNew(u64), None }
/// A note already there with the same word and sentence (a sync cut off after adding it): its
/// first card moves to the deck. A reviewed one keeps Anki's schedule.
fn adopt(client: &mut Client, row: &Row, deck: &str) -> Result<Adopt, Error> {
    let q = format!("\"note:{}\" \"Word:{}\"", store::MINED_NOTE_TYPE, quoted(&field(&row.fields[0])));
    let page = client.call("notes.search", json!({"query": q, "limit": 20}))?;
    let ids: Vec<Value> = page["ids"].as_array().cloned().unwrap_or_default();
    if ids.is_empty() {
        return Ok(Adopt::None);
    }
    let notes = client.call("notes.getMany", json!({"ids": ids}))?;
    for item in notes["items"].as_array().into_iter().flatten() {
        let n = &item["result"];
        if n["fields"]["Sentence"].as_str() != Some(field(&row.fields[2]).as_str()) {
            continue;
        }
        let card = n["cardIds"][0].clone();
        let cid = id_of(&card);
        if cid == 0 {
            continue;
        }
        client.call("cards.move", json!({"ids": [card.clone()], "deck": {"id": deck}}))?;
        let cards = client.call("cards.getMany", json!({"ids": [card]}))?;
        let fresh = cards["items"][0]["result"]["scheduling"]["type"].as_str().unwrap_or("new") == "new";
        return Ok(if fresh { Adopt::FoundNew(cid) } else { Adopt::Found(cid) });
    }
    Ok(Adopt::None)
}

/// Moves handed-over words' progress and history to their Anki cards, then marks them handed.
/// `cards`: (row id, Anki card id, adopted with Anki's schedule kept).
fn move_progress(lang: &str, deck_key: &str, cards: &[(u64, u64, bool)]) -> bool {
    let local = format!("{}{}", mined::PREFIX, lang);
    let (from, _, _) = store::load_states(&local);
    let (mut to, day, new_today) = store::load_states(deck_key);
    let mut with_progress = Vec::new();
    for &(row, card, adopted) in cards {
        if adopted {
            continue;
        }
        let Ok(i) = from.binary_search_by_key(&row, |s| s.id) else { continue };
        let moved = CardState { id: card, ..from[i] };
        with_progress.push(card);
        match to.binary_search_by_key(&card, |s| s.id) {
            Ok(j) => to[j] = moved,
            Err(j) => to.insert(j, moved),
        }
    }
    if !store::write_states(deck_key, &to, if day == 0 { store::today() } else { day }, new_today) {
        return false;
    }
    let mut log = store::read_log(deck_key);
    for e in store::read_log(&local) {
        if let Some(c) = cards.iter().find(|c| c.0 == e.id) {
            log.push(LogEntry { id: c.1, ..e });
        }
    }
    // Progress without history still sends its schedule: an entry without a rating.
    let now = store::now() * 1000;
    for id in with_progress {
        log.push(LogEntry { id, time_ms: now, interval: 0, previous: 0, rating: 0, kind: 0 });
    }
    if !store::write_log(deck_key, &log) {
        return false;
    }
    let mut handed = mined::read_handed(lang);
    handed.extend(cards.iter().map(|c| c.0));
    handed.sort_unstable();
    handed.dedup();
    let b: Vec<u8> = handed.iter().flat_map(|h| h.to_le_bytes()).collect();
    store::write(&mined::handed_path(lang), &b)
}

/// Hands every language's new mined words to the service. Returns the decks they went to (by
/// name), for the sync to take in; a failure leaves the rest for the next sync.
pub fn hand_over(client: &mut Client, migaku: bool, added: &mut u32) -> Result<Vec<String>, Error> {
    let mut decks_out = Vec::new();
    let mut type_ready = false;
    for lang in mined::languages() {
        let mut rows = mined::unhanded_rows(&lang);
        if rows.is_empty() {
            continue;
        }
        if !type_ready {
            ensure_note_type(client)?;
            type_ready = true;
        }
        let name = format!("Matcha::{}", autonym(&lang));
        let decks = client.call("decks.list", json!({}))?;
        let found = decks.as_array().into_iter().flatten().find(|d| d["name"].as_str() == Some(name.as_str()))
            .and_then(|d| d["id"].as_str().map(String::from));
        let deck = match found {
            Some(id) => id,
            None => String::from(client.call("decks.create", json!({"name": name}))?["id"].as_str().unwrap_or("")),
        };
        if deck.is_empty() {
            return Err(Error { kind: Kind::Reply, kind_name: String::new(), message: String::new() });
        }
        // matcha's record of where the language's words went (its local deck id).
        if let Some(p) = mined::side_path(&format!("{}{}", mined::PREFIX, lang), "deck") {
            store::write(&p, format!("{}{}", if migaku { "m" } else { "" }, deck).as_bytes());
        }
        let deck_key = format!("{}{}", if migaku { "m" } else { "a" }, deck);
        decks_out.push(name);
        while !rows.is_empty() {
            let batch: Vec<Row> = rows.drain(..rows.len().min(BATCH)).collect();
            let notes: Vec<Value> = batch.iter().map(|r| note(r, &deck, false)).collect();
            let created = client.call("notes.createMany", json!({"notes": notes}))?;
            let mut cards: Vec<(u64, u64, bool)> = Vec::new();
            for (i, row) in batch.iter().enumerate() {
                let item = created["items"].as_array().into_iter().flatten()
                    .find(|it| it["index"].as_u64() == Some(i as u64));
                let mut card = item.map(|it| id_of(&it["result"]["cardIds"][0])).unwrap_or(0);
                let mut adopted = false;
                if card == 0 {
                    // Refused (a duplicate word): the same note may be there already; else added anyway.
                    match adopt(client, row, &deck)? {
                        Adopt::Found(c) => { card = c; adopted = true; }
                        Adopt::FoundNew(c) => card = c,
                        Adopt::None => {
                            let made = client.call("notes.createMany", json!({"notes": [note(row, &deck, true)]}))?;
                            card = id_of(&made["items"][0]["result"]["cardIds"][0]);
                        }
                    }
                }
                if card != 0 {
                    cards.push((row.id, card, adopted));
                }
            }
            if !move_progress(&lang, &deck_key, &cards) {
                break;
            }
            let _ = &added; // counted when their deck syncs, as new cards there
        }
    }
    Ok(decks_out)
}
