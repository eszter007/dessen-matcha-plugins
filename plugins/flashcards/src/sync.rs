//! Sync with matcha-anki-server, in matcha's order (AnkiSync.cpp syncAll and DeckSync::run): the
//! server syncs with AnkiWeb (or Migaku), then for each chosen deck its preset, its cards and their
//! sides, Anki's reviews in, the device's reviews and schedules out, and a second remote sync.
//!
//! config.json in the plugin folder names the server and the decks:
//!   {"server": "http://192.168.1.2:8080", "key": "...", "anki": ["Kaishi 1.5k"], "migaku": ["日本語"]}
//! "ankiServer" / "migakuServer" (and "ankiKey" / "migakuKey") point a service elsewhere.
use crate::rpc::{Client, Error, Kind};
use crate::store::{self, CardRecord, DeckInfo, Pulled};
use crate::{html, png};
use alloc::{format, string::String, vec::Vec};
use serde_json::{json, Value};

pub struct Service {
    pub name: &'static str,
    pub migaku: bool,
    pub url: String,
    pub key: String,
    pub decks: Vec<String>,
}

pub fn services() -> Result<Vec<Service>, &'static str> {
    let cfg = store::read_json("config.json").ok_or("Set the server address in config.json in the Flashcards plugin folder")?;
    let s = |k: &str| String::from(cfg.get(k).and_then(|v| v.as_str()).unwrap_or("").trim_end_matches('/'));
    let decks = |k: &str| -> Vec<String> {
        cfg.get(k).and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|d| d.as_str().map(String::from)).collect())
            .unwrap_or_default()
    };
    let server = s("server");
    let mut out = Vec::new();
    for (name, migaku, path, k) in [("Anki", false, "/api/v1", "anki"), ("Migaku", true, "/migaku/api/v1", "migaku")] {
        let own = s(&format!("{}Server", k));
        let base = if own.is_empty() { server.clone() } else { own };
        let own_key = s(&format!("{}Key", k));
        let wanted = decks(k);
        if base.is_empty() || wanted.is_empty() {
            continue;
        }
        out.push(Service { name, migaku, url: base + path, key: if own_key.is_empty() { s("key") } else { own_key }, decks: wanted });
    }
    if out.is_empty() {
        return Err(if server.is_empty() {
            "Set the server address in config.json in the Flashcards plugin folder"
        } else {
            "Name your Anki or Migaku decks in config.json in the Flashcards plugin folder"
        });
    }
    Ok(out)
}

#[derive(Default)]
pub struct Totals {
    pub added: u32,
    pub pulled: u32,
    pub pushed: u32,
}

/// What the sync screen shows while it works.
pub trait Progress {
    fn show(&mut self, message: &str, detail: &str);
}

fn id_str(v: &Value) -> u64 {
    v.as_str().and_then(|s| s.parse().ok()).or_else(|| v.as_u64()).unwrap_or(0)
}
const RATING_NAMES: [&str; 4] = ["again", "hard", "good", "easy"];

/// Syncs one service; Err carries the deck it stopped at and why.
pub fn sync_service(svc: &Service, progress: &mut dyn Progress, totals: &mut Totals, keys: &mut Vec<String>)
    -> Result<(), (String, Error)> {
    let mut client = Client::new(&svc.url, &svc.key);
    let remote = |client: &mut Client| -> Result<(), Error> {
        match client.call_operation("collection.sync", json!({"includeMedia": false})) {
            Ok(_) => Ok(()),
            // Without an AnkiWeb account the server syncs locally; the Migaku bridge needs its login.
            Err(e) if !svc.migaku && e.kind == Kind::Rpc && e.kind_name != "timeout" => Ok(()),
            Err(e) => Err(e),
        }
    };
    progress.show(&format!("Syncing {}", svc.name), "");
    remote(&mut client).map_err(|e| (String::from(svc.name), e))?;
    let decks = client.call("decks.list", json!({})).map_err(|e| (String::new(), e))?;
    let list = decks.as_array().cloned().unwrap_or_default();
    for wanted in &svc.decks {
        let found = list.iter().find(|d| d.get("name").and_then(|n| n.as_str()) == Some(wanted.as_str()));
        let Some(d) = found else {
            return Err((wanted.clone(), Error { kind: Kind::Rpc, kind_name: String::from("deckMissing"),
                                               message: format!("{} has no deck named {}", svc.name, wanted) }));
        };
        let server_id = String::from(d.get("id").and_then(|v| v.as_str()).unwrap_or(""));
        let preset = String::from(d.get("presetId").and_then(|v| v.as_str()).unwrap_or(""));
        let key = format!("{}{}", if svc.migaku { "m" } else { "a" }, server_id);
        sync_deck(&mut client, svc, wanted, &key, &server_id, &preset, progress, totals).map_err(|e| (wanted.clone(), e))?;
        keys.push(key);
    }
    remote(&mut client).map_err(|e| (String::from(svc.name), e))?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn sync_deck(client: &mut Client, svc: &Service, name: &str, key: &str, server_id: &str, preset: &str,
             progress: &mut dyn Progress, totals: &mut Totals) -> Result<(), Error> {
    let mut info = DeckInfo::read(key).unwrap_or_else(|| DeckInfo::new(key, name, svc.migaku, server_id));
    info.name = String::from(name);
    let title = format!("Syncing {}", name);
    progress.show(&title, "");
    if !preset.is_empty() {
        if let Ok(p) = client.call("deckPresets.get", json!({"preset": {"id": preset}})) {
            if let Some(s) = p.get("settings") {
                info.apply_preset(s);
            }
        }
    }
    // Leeches noted on the device: tagged, and suspended as the preset says, in Anki.
    let leeches = store::read_leech_records(key);
    if !leeches.is_empty() {
        let notes: Vec<String> = leeches.iter().map(|l| format!("{}", l.1)).collect();
        client.call("tags.add", json!({"noteIds": notes, "tags": ["leech"]}))?;
        if info.leech_suspend {
            let ids: Vec<String> = leeches.iter().map(|l| format!("{}", l.0)).collect();
            client.call("cards.setSuspended", json!({"ids": ids, "suspended": true}))?;
        }
    }
    // The deck's cards, in Anki's order; suspended cards leave the device.
    let query = format!("\"deck:{}\" -is:suspended", name.replace('"', "\\\""));
    let mut ids: Vec<u64> = Vec::new();
    let mut cursor = Value::Null;
    loop {
        let mut params = json!({"query": query, "limit": 200});
        if !cursor.is_null() {
            params["cursor"] = cursor.clone();
        }
        let page = client.call("cards.search", params)?;
        ids.extend(page.get("ids").and_then(|v| v.as_array()).into_iter().flatten().map(id_str));
        cursor = page.get("nextCursor").cloned().unwrap_or(Value::Null);
        if cursor.is_null() {
            break;
        }
    }
    let known = store::read_ids(key);
    let mut sides = store::read_sides(key);
    let mut records: Vec<CardRecord> = Vec::with_capacity(ids.len());
    let missing: Vec<u64> = ids.iter().copied().filter(|id| !sides.iter().any(|s| s.0 == *id)).collect();
    // Notes for the cards not seen before (burying needs them).
    let mut notes: Vec<(u64, u64)> = known.iter().map(|r| (r.id, r.note)).collect();
    for chunk in missing.chunks(50) {
        let list: Vec<String> = chunk.iter().map(|i| format!("{}", i)).collect();
        let got = client.call("cards.getMany", json!({"ids": list}))?;
        for item in got.get("items").and_then(|v| v.as_array()).into_iter().flatten() {
            if let Some(r) = item.get("result") {
                notes.push((id_str(&r["id"]), id_str(&r["noteId"])));
            }
        }
    }
    // Each new card's two sides, as plain text, and its pictures as 1-bit images.
    for (n, id) in missing.iter().enumerate() {
        if n % 5 == 0 {
            progress.show(&title, &format!("{} / {}", n, missing.len()));
        }
        let r = client.call("cards.render", json!({"id": format!("{}", id)}))?;
        let q = html::to_side(r.get("questionHtml").and_then(|v| v.as_str()).unwrap_or(""));
        let a = html::to_side(r.get("answerHtml").and_then(|v| v.as_str()).unwrap_or(""));
        for img in q.images.iter().chain(a.images.iter()) {
            fetch_image(client, img);
        }
        sides.push((*id, q.side, a.side));
        totals.added += 1;
    }
    for id in &ids {
        let note = notes.iter().find(|n| n.0 == *id).map(|n| n.1).unwrap_or(0);
        records.push(CardRecord { id: *id, note, flags: 0 });
    }
    sides.retain(|s| ids.contains(&s.0));
    if !store::write_sides(key, &sides) || !store::write_ids(key, &records) {
        return Err(Error { kind: Kind::Reply, kind_name: String::new(), message: String::from("SD card write failed") });
    }
    progress.show(&title, "");
    // Pull before push: Anki's reviews land first, then the device's own go out on top.
    pull_reviews(client, &mut info, totals)?;
    push_reviews(client, &mut info, &ids, totals)?;
    if let Ok(latest) = client.call("reviews.getLatestId", json!({"deck": {"id": server_id}})) {
        info.last_review_id = info.last_review_id.max(id_str(&latest["id"]));
    }
    info.write();
    if !leeches.is_empty() {
        store::delete_file(&format!("{}.leeches", key));
    }
    Ok(())
}

/// image file name -> "img-<fnv>.bit" in the plugin folder.
pub fn image_file(name: &str) -> String {
    let mut h: u32 = 0x811c_9dc5;
    for b in name.bytes() {
        h = (h ^ b as u32).wrapping_mul(0x0100_0193);
    }
    format!("img-{:08x}.bit", h)
}
fn fetch_image(client: &mut Client, name: &str) {
    let file = image_file(name);
    if store::read_file(&file).is_some() {
        return;
    }
    // Only PNG is decoded here: anything else is asked for as "<file>.png", which
    // matcha-anki-server sends converted (as it does WebP for the firmware).
    let request = if name.to_ascii_lowercase().ends_with(".png") { String::from(name) } else { format!("{}.png", name) };
    let Ok(r) = client.call("media.get", json!({"filename": request})) else { return };
    let Some(data) = r.get("data").and_then(|d| d.as_str()).and_then(base64) else { return };
    if let Some(bits) = png::to_bits(&data, 380, 320) {
        store::write_file(&file, &bits);
    }
}
fn base64(s: &str) -> Option<Vec<u8>> {
    let val = |c: u8| -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        } as u32)
    };
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0);
    for c in s.bytes().filter(|&c| c != b'=' && !c.is_ascii_whitespace()) {
        acc = acc << 6 | val(c)?;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Some(out)
}

fn pull_reviews(client: &mut Client, info: &mut DeckInfo, totals: &mut Totals) -> Result<(), Error> {
    // Ratings made here and not yet sent: an Anki review of the same card from before one of them
    // is already outdated on this device.
    let unsent = store::read_log(&info.key);
    let mut cursor = Value::Null;
    loop {
        let mut params = json!({"deck": {"id": info.server_id}, "includeManual": false, "limit": 200});
        if !cursor.is_null() {
            params["cursor"] = cursor.clone();
        } else if info.last_review_id != 0 {
            params["afterId"] = json!(format!("{}", info.last_review_id));
        }
        let page = client.call("reviews.list", params)?;
        let mut pulled = Vec::new();
        let mut newest = info.last_review_id;
        for r in page.get("items").and_then(|v| v.as_array()).into_iter().flatten() {
            let id = id_str(&r["id"]);
            newest = newest.max(id);
            let rating = RATING_NAMES.iter().position(|n| r["rating"].as_str() == Some(n)).map(|i| i as u8 + 1).unwrap_or(0);
            let card = id_str(&r["cardId"]);
            if rating == 0 || unsent.iter().any(|e| e.id == card && e.time_ms >= id as i64) {
                continue;
            }
            let interval = if r.pointer("/interval/unit").and_then(|u| u.as_str()) == Some("days") {
                r.pointer("/interval/value").and_then(|v| v.as_u64()).unwrap_or(0) as u16
            } else {
                0
            };
            pulled.push(Pulled { id: card, time_ms: id as i64, rating, interval });
        }
        store::apply_pulled(&info.key, &pulled);
        totals.pulled += pulled.len() as u32;
        info.last_review_id = newest;
        info.write();
        cursor = page.get("nextCursor").cloned().unwrap_or(Value::Null);
        if cursor.is_null() {
            return Ok(());
        }
    }
}

fn push_reviews(client: &mut Client, info: &mut DeckInfo, in_anki: &[u64], totals: &mut Totals) -> Result<(), Error> {
    let mut log = store::read_log(&info.key);
    if log.is_empty() {
        return Ok(());
    }
    // A review's id is its time in ms: kept unique within the log, so a retry is a duplicate in Anki.
    log.sort_by_key(|e| e.time_ms);
    for i in 1..log.len() {
        if log[i].time_ms <= log[i - 1].time_ms {
            log[i].time_ms = log[i - 1].time_ms + 1;
        }
    }
    store::write_log(&info.key, &log);
    let review = |e: &store::LogEntry| {
        let minutes = e.kind & store::MINUTES != 0;
        let kind = e.kind & !store::MINUTES;
        json!({
            "id": format!("{}", e.time_ms), "cardId": format!("{}", e.id),
            "rating": RATING_NAMES[(e.rating.clamp(1, 4) - 1) as usize],
            "interval": {"value": if minutes { e.interval as u32 * 60 } else { e.interval as u32 },
                         "unit": if minutes { "seconds" } else { "days" }},
            "previousInterval": {"value": e.previous, "unit": "days"},
            "factor": {"encoding": "unknown", "value": 0}, "elapsedMs": 0,
            "kind": match kind { 0 => "learning", 2 => "relearning", _ => "review" },
        })
    };
    // History first, in batches; a batch refused for a duplicate or a deleted card goes one by one.
    let mut gone: Vec<u64> = Vec::new();
    for batch in log.chunks(50) {
        let reviews: Vec<Value> = batch.iter().filter(|e| e.rating > 0).map(review).collect();
        if reviews.is_empty() {
            continue;
        }
        match client.call_operation("advanced.reviews.import", json!({"reviews": reviews})) {
            Ok(_) => {}
            Err(e) if e.kind_name == "conflict" || e.kind_name == "notFound" => {
                for entry in batch.iter().filter(|e| e.rating > 0) {
                    match client.call_operation("advanced.reviews.import", json!({"reviews": [review(entry)]})) {
                        Ok(_) => {}
                        Err(e) if e.kind_name == "conflict" => {}
                        Err(e) if e.kind_name == "notFound" && !in_anki.contains(&entry.id) => gone.push(entry.id),
                        Err(e) => return Err(e),
                    }
                }
            }
            Err(e) => return Err(e),
        }
    }
    for e in &log {
        info.last_review_id = info.last_review_id.max(e.time_ms as u64);
    }
    info.write();
    // Then each reviewed card's resulting schedule.
    let (states, _, _) = store::load_states(&info.key);
    let today = store::today();
    let mut ids: Vec<u64> = log.iter().map(|e| e.id).collect();
    ids.sort_unstable();
    ids.dedup();
    let mut fsrs_refused = false;
    for id in ids.iter().filter(|id| !gone.contains(id)) {
        let Ok(i) = states.binary_search_by_key(id, |s| s.id) else { continue };
        let s = states[i];
        let days = s.due.saturating_sub(today);
        client.call("scheduling.setDueDate", json!({"cardIds": [format!("{}", id)], "days": format!("{}", days)}))?;
        let interval = if s.due > s.last_review { s.due - s.last_review } else { 1 };
        if let Err(e) = client.call("advanced.cards.update", json!({"cardId": format!("{}", id), "patch": {"intervalDays": interval}})) {
            if e.kind != Kind::Rpc {
                return Err(e);
            }
        }
        if fsrs_refused {
            continue;
        }
        let memory = json!({"cardId": format!("{}", id), "patch": {"fsrsMemory": {
            "difficulty": s.difficulty.clamp(1.0, 10.0), "stabilityDays": s.stability}}});
        if let Err(e) = client.call("advanced.cards.update", memory) {
            if e.kind != Kind::Rpc {
                return Err(e);
            }
            fsrs_refused = e.kind_name == "unsupportedCapability";
        }
    }
    totals.pushed += log.iter().filter(|e| e.rating > 0 && !gone.contains(&e.id)).count() as u32;
    store::write_log(&info.key, &[]);
    Ok(())
}

/// What a failed sync says, by what went wrong (matcha's errorText).
pub fn error_text(svc: &str, e: &Error) -> String {
    match &e.kind {
        Kind::Network | Kind::Http(_) => format!("Could not reach the {} server at this address", svc),
        Kind::Unauthorized => format!("The {} server refused the key", svc),
        Kind::Reply => format!("The {} server sent an unexpected reply", svc),
        Kind::Rpc if e.kind_name == "deckMissing" => e.message.clone(),
        Kind::Rpc if svc == "Migaku" && e.kind_name == "authenticationRequired" => String::from(
            "Migaku did not accept the server's login, or the server has none. Check the Migaku login on the server"),
        Kind::Rpc => format!("{}: {}", svc, if e.message.is_empty() { &e.kind_name } else { &e.message }),
    }
}
