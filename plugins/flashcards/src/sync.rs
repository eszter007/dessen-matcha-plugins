//! Sync with matcha-anki-server, in matcha's order (AnkiSync.cpp syncAll and DeckSync::run): the
//! server syncs with AnkiWeb (or Migaku), then for each chosen deck its preset, its cards and their
//! sides, Anki's reviews in, the device's reviews and schedules out, and a second remote sync.
//!
//! config.json in the plugin folder names the server and the decks:
//!   {"server": "http://192.168.1.2:8080", "key": "...", "anki": ["Kaishi 1.5k"], "migaku": ["日本語"]}
//! "ankiServer" / "migakuServer" (and "ankiKey" / "migakuKey") point a service elsewhere.
use crate::rpc::{Client, Error, Kind};
use crate::store::{self, CardRecord, DeckInfo, Pulled};
use crate::png;
use crate::i18n::{tr, trf, Arg};
use alloc::{format, string::String, vec::Vec};
use serde_json::{json, Value};

pub struct Service {
    pub name: &'static str,
    pub migaku: bool,
    pub url: String,
    pub key: String,
    pub decks: Vec<String>,
    pub takes_mined: bool, // matcha's Mined Words To: this service gets the words mined while reading
}

pub fn services() -> Result<Vec<Service>, &'static str> {
    let cfg = store::read_json("config.json").ok_or("no_server")?;
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
        let takes = (s("minedTo") == "migaku") == migaku && !crate::mined::languages().is_empty();
        if base.is_empty() || (wanted.is_empty() && !takes) {
            continue;
        }
        let takes_mined = (s("minedTo") == "migaku") == migaku;
        out.push(Service { name, migaku, url: base + path, key: if own_key.is_empty() { s("key") } else { own_key }, decks: wanted, takes_mined });
    }
    if out.is_empty() {
        return Err(if server.is_empty() {
            "no_server"
        } else {
            "no_service"
        });
    }
    Ok(out)
}

#[derive(Default)]
pub struct Totals {
    pub remote_skipped: bool, // AnkiWeb was not synced this time
    pub sent: bool,           // something went to Anki: the closing AnkiWeb sync is worth it
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
    // matcha's syncRemote: without an AnkiWeb account the sync goes on locally. An AnkiWeb sync
    // still running before the device's changes (it may wait for a full sync to be chosen in Anki)
    // stops the sync, so the changes cannot land in a collection that full sync replaces. Any other
    // AnkiWeb error is noted and the next sync tries again. For the Migaku bridge every failure stops.
    let remote = |client: &mut Client, before_changes: bool, skipped: &mut bool| -> Result<(), Error> {
        match client.call_operation("collection.sync", json!({"includeMedia": false})) {
            Ok(_) => Ok(()),
            Err(e) if svc.migaku => Err(e),
            Err(e) if e.kind_name == "authenticationRequired" => Ok(()),
            Err(e) if e.kind != Kind::Rpc => Err(e),
            Err(e) if before_changes && e.kind_name == "timeout" => {
                Err(Error { kind: Kind::Rpc, kind_name: String::from("remoteBlocked"), message: e.message })
            }
            Err(_) => {
                *skipped = true;
                Ok(())
            }
        }
    };
    progress.show(&trf("syncing", &[Arg::S(svc.name)]), "");
    remote(&mut client, true, &mut totals.remote_skipped).map_err(|e| (String::from(svc.name), e))?;
    // Words mined in matcha first: their deck then syncs with the others (and from now on).
    let mut wanted_decks = svc.decks.clone();
    if svc.takes_mined {
        let handed = crate::handover::hand_over(&mut client, svc.migaku, &mut totals.added)
            .map_err(|e| (String::from(svc.name), e))?;
        let cfg_key = if svc.migaku { "migaku" } else { "anki" };
        totals.sent |= !handed.is_empty();
        for d in handed {
            if !wanted_decks.contains(&d) {
                wanted_decks.push(d);
            }
        }
        if wanted_decks.len() > svc.decks.len() {
            store::set_config(cfg_key, json!(wanted_decks));
        }
    }
    let decks = client.call("decks.list", json!({})).map_err(|e| (String::new(), e))?;
    let list = decks.as_array().map(|a| a.as_slice()).unwrap_or(&[]);
    // (name, server id, preset, the deck as stored here)
    let mut chosen: Vec<(String, String, String, DeckInfo)> = Vec::new();
    for wanted in &wanted_decks {
        let found = list.iter().find(|d| d.get("name").and_then(|n| n.as_str()) == Some(wanted.as_str()));
        let Some(d) = found else {
            return Err((wanted.clone(), Error { kind: Kind::Rpc, kind_name: String::from("deckMissing"),
                                               message: trf("deck_missing", &[Arg::S(svc.name), Arg::S(wanted)]) }));
        };
        let server_id = String::from(d.get("id").and_then(|v| v.as_str()).unwrap_or(""));
        let preset = String::from(d.get("presetId").and_then(|v| v.as_str()).unwrap_or(""));
        let key = format!("{}{}", if svc.migaku { "m" } else { "a" }, server_id);
        let info = DeckInfo::read(&key).unwrap_or_else(|| DeckInfo::new(&key, wanted, svc.migaku, &server_id));
        chosen.push((wanted.clone(), server_id, preset, info));
    }
    drop(decks);
    // Every deck's preset, newest review and cards in one request, where the server has it.
    let mut overviews: Vec<Option<Overview>> = overviews(&mut client, &chosen).map_err(|e| (String::from(svc.name), e))?
        .into_iter().map(Some).collect();
    for (i, (name, server_id, preset, info)) in chosen.iter().enumerate() {
        let ov = overviews.get_mut(i).and_then(Option::take).unwrap_or(Overview {
            total: 0, settings: None, latest: None, cards: Vec::new(), next: Some(0) });
        sync_deck(&mut client, svc, name, &info.key, server_id, preset, ov, progress, totals).map_err(|e| (name.clone(), e))?;
        keys.push(info.key.clone());
    }
    // Nothing sent: AnkiWeb already has all of it from the first sync.
    if totals.sent {
        remote(&mut client, false, &mut totals.remote_skipped).map_err(|e| (String::from(svc.name), e))?;
    }
    Ok(())
}

/// One deck as matcha-anki-server's device.decks gives it: its preset's settings, its newest
/// review, and its cards in Anki's order with their notes and whether the note was edited.
pub struct Overview {
    total: usize,
    settings: Option<Value>,
    latest: Option<u64>,
    cards: Vec<(u64, u64, bool)>,
    next: Option<u64>,
}
fn parse_overview(d: &Value) -> Overview {
    let mut cards = Vec::new();
    for t in d.get("cards").and_then(|c| c.as_str()).unwrap_or("").split_ascii_whitespace() {
        let mut p = t.split('.').map(|x| x.parse::<u64>().unwrap_or(0));
        if let (Some(c), Some(n), Some(e)) = (p.next(), p.next(), p.next()) {
            cards.push((c, n, e == 1));
        }
    }
    Overview {
        total: d.get("total").and_then(|v| v.as_u64()).unwrap_or(0) as usize,
        settings: d.get("settings").filter(|s| !s.is_null()).cloned(),
        latest: d.get("latestReviewId").filter(|v| !v.is_null()).map(id_str),
        cards,
        next: d.get("nextOffset").and_then(|v| v.as_u64()),
    }
}
/// The deck reference device.decks takes; `since` makes it mark notes edited after the last sync.
fn deck_ref(info: &DeckInfo, name: &str, preset: &str, offset: u64) -> Value {
    json!({"id": info.server_id, "name": name, "presetId": preset,
           "since": if info.last_sync > 0 { info.last_sync_at } else { 0 }, "offset": offset})
}
const OVERVIEW_LIMIT: u32 = 2000;
/// Every chosen deck in one request (more only for decks past the card limit).
fn overviews(client: &mut Client, decks: &[(String, String, String, DeckInfo)]) -> Result<Vec<Overview>, Error> {
    let refs: Vec<Value> = decks.iter().map(|(name, _, preset, info)| deck_ref(info, name, preset, 0)).collect();
    let r = client.call("device.decks", json!({"decks": refs, "limit": OVERVIEW_LIMIT}))?;
    Ok(r.get("decks").and_then(|d| d.as_array()).into_iter().flatten().map(parse_overview).collect())
}

/// What device.pack asks for at once, and how much a reply may carry.
const PACK_IDS: usize = 1000;
const PACK_BYTES: usize = 192 * 1024;
/// Index written after this many new cards (and at the end): a sync cut off loses little.
const INDEX_EVERY: usize = 2000;

#[allow(clippy::too_many_arguments)]
fn sync_deck(client: &mut Client, svc: &Service, name: &str, key: &str, server_id: &str, preset: &str,
             mut ov: Overview, progress: &mut dyn Progress, totals: &mut Totals) -> Result<(), Error> {
    let _ = preset;
    let mut info = DeckInfo::read(key).unwrap_or_else(|| DeckInfo::new(key, name, svc.migaku, server_id));
    info.name = String::from(name);
    let title = trf("syncing", &[Arg::S(name)]);
    progress.show(&title, "");
    if let Some(s) = &ov.settings {
        info.apply_preset(s);
        info.preset = true;
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
    let today = store::today();
    let mut sides = store::Sides::open(key);
    // The deck's cards in Anki's order (suspended cards leave the device), each card's note, and
    // the cards whose note was edited in Anki since the last sync (fetched again).
    // (card, note) in Anki's order, sized once from the deck's total.
    let mut cards: Vec<(u64, u64)> = Vec::with_capacity(ov.total);
    let mut changed: Vec<u64> = Vec::new();
    loop {
        for &(c, n, edited) in &ov.cards {
            cards.push((c, n));
            if edited {
                changed.push(c);
            }
        }
        let Some(offset) = ov.next else { break };
        let r = client.call("device.decks", json!({"decks": [deck_ref(&info, name, preset, offset)], "limit": OVERVIEW_LIMIT}))?;
        let Some(d) = r.get("decks").and_then(|d| d.get(0)) else { break };
        let latest = ov.latest;
        ov = parse_overview(d);
        ov.latest = latest;
    }
    ov.cards = Vec::new();
    // Cards gone from the deck and edited ones leave; only their chunks are rewritten.
    let mut in_deck: Vec<u64> = cards.iter().map(|c| c.0).collect();
    in_deck.sort_unstable();
    changed.sort_unstable();
    let sd_failed = || Error { kind: Kind::Reply, kind_name: String::new(), message: tr("sd_failed") };
    if !sides.retain(|c| in_deck.binary_search(&c).is_ok() && changed.binary_search(&c).is_err()) {
        return Err(sd_failed());
    }
    let missing: Vec<u64> = cards.iter().map(|c| c.0).filter(|id| !sides.has(*id)).collect();
    sides.reserve(missing.len());
    // Cards new to the device that were already studied in Anki, with Anki's FSRS memory.
    let mut studied: Vec<(u64, Value)> = Vec::new();
    // The new cards, packed by the server in the device's own chunk format: each chunk is
    // written as it arrives; the index only now and then.
    sides.defer_index();
    client.keep_buffer = true;
    let (mut done, mut since_index) = (0, 0);
    while done < missing.len() {
        progress.show(&title, &format!("{} / {}", done, missing.len()));
        let part = &missing[done..(done + PACK_IDS).min(missing.len())];
        let list: Vec<String> = part.iter().map(|i| format!("{}", i)).collect();
        let mut header = Value::Null;
        let mut ok = true;
        let covered = client.call_raw("device.pack", json!({"ids": list, "maxBytes": PACK_BYTES}), |body| {
            let mut p = 0;
            while p + 5 <= body.len() {
                let kind = body[p];
                let n = u32::from_le_bytes(body[p + 1..p + 5].try_into().unwrap()) as usize;
                let Some(data) = body.get(p + 5..p + 5 + n) else { break };
                p += 5 + n;
                if kind == 1 {
                    header = serde_json::from_slice(data).unwrap_or(Value::Null);
                } else if kind == 2 {
                    let ids = chunk_ids(data);
                    ok &= sides.add_chunk(&ids, data);
                    totals.added += ids.len() as u32;
                }
            }
        });
        client.keep_buffer = true;
        let covered = covered.map(|_| header["done"].as_u64().unwrap_or(0) as usize);
        let covered = match covered {
            Ok(c) => c,
            Err(e) => {
                client.keep_buffer = false;
                sides.flush_index();
                return Err(e);
            }
        };
        if !ok {
            client.keep_buffer = false;
            sides.flush_index();
            return Err(sd_failed());
        }
        for s in header["studied"].as_array().into_iter().flatten() {
            studied.push((id_str(&s["id"]), s["scheduling"].clone()));
        }
        let mined: Vec<(u64, String, String)> = header["fields"].as_array().into_iter().flatten()
            .map(|f| (id_str(&f["id"]), store::encode_mined(&f["fields"]), String::new())).collect();
        totals.added += mined.len() as u32;
        if !sides.add(mined) {
            return Err(sd_failed());
        }
        if let Some(styles) = header["css"].as_object() {
            for (file, css) in styles {
                let name = format!("{}.css", file);
                if store::read_file(&name).is_none() {
                    store::write_file(&name, css.as_str().unwrap_or("").as_bytes());
                }
            }
        }
        for img in header["images"].as_array().into_iter().flatten().filter_map(|v| v.as_str()) {
            fetch_image(client, img);
        }
        done += covered.max(1);
        since_index += covered;
        if since_index >= INDEX_EVERY {
            sides.flush_index();
            sides.defer_index();
            since_index = 0;
        }
    }
    client.release();
    if !sides.flush_index() {
        return Err(sd_failed());
    }
    let records: Vec<CardRecord> = cards.iter().map(|&(id, note)| CardRecord { id, note, flags: 0 }).collect();
    let total = cards.len();
    drop(cards);
    // Guessed on the first sync, and again when most of the deck is new.
    if info.lang.is_empty() || missing.len() * 2 > total {
        info.lang = deck_language(name, &sides.sample(80));
    }
    drop(sides);
    info.last_sync = today;
    info.last_sync_at = store::now();
    // Rewritten only when the deck's cards or their order changed.
    let same = {
        let old = store::read_file(&format!("{}.ids", key)).unwrap_or_default();
        old.len() == records.len() * 17 && old.chunks_exact(17).zip(&records).all(|(a, b)| {
            u64::from_le_bytes(a[0..8].try_into().unwrap()) == b.id && u64::from_le_bytes(a[8..16].try_into().unwrap()) == b.note
        })
    };
    if !same && !store::write_ids(key, &records) {
        return Err(sd_failed());
    }
    drop(records);
    progress.show(&title, "");
    // Pull before push: Anki's reviews land first, then the device's own go out on top.
    let latest = Some(ov.latest);
    pull_reviews(client, &mut info, totals, latest)?;
    // Anki's newest review is now in, or was one pulls leave out (made by hand): either way the
    // next sync needs to ask only for newer ones.
    if let Some(l) = ov.latest {
        info.last_review_id = info.last_review_id.max(l);
    }
    seed_studied(client, &info.key, &studied)?;
    let sending = !store::read_log(key).is_empty();
    drop(missing);
    push_reviews(client, &mut info, &in_deck, totals)?;
    // The newest review id after the device's own went in.
    if sending {
        if let Ok(l) = client.call("reviews.getLatestId", json!({"deck": {"id": server_id}})) {
            info.last_review_id = info.last_review_id.max(id_str(&l["id"]));
        }
    }
    info.write();
    if !leeches.is_empty() {
        store::delete_file(&format!("{}.leeches", key));
    }
    Ok(())
}
/// The card ids of a packed chunk (decode_sides' format), without decoding its sides.
fn chunk_ids(b: &[u8]) -> Vec<u64> {
    let mut out = Vec::new();
    let Some(n) = b.get(0..4).map(|x| u32::from_le_bytes(x.try_into().unwrap()) as usize) else { return out };
    let mut p = 4;
    for _ in 0..n {
        let Some(id) = b.get(p..p + 8).map(|x| u64::from_le_bytes(x.try_into().unwrap())) else { break };
        out.push(id);
        p += 8;
        for _ in 0..2 {
            let Some(len) = b.get(p..p + 4).map(|x| u32::from_le_bytes(x.try_into().unwrap()) as usize) else { return out };
            p += 4 + len;
        }
    }
    out
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
    // Ready for the panel from the server (device.picture: any format, scaled and dithered there);
    // a server without it sends the PNG, decoded here.
    if let Ok(p) = client.call("device.picture", json!({"filename": name, "maxWidth": 380, "maxHeight": 320})) {
        let (w, h) = (p["width"].as_u64().unwrap_or(0) as u16, p["height"].as_u64().unwrap_or(0) as u16);
        if let Some(bits) = p.get("bits").and_then(|b| b.as_str()).and_then(base64) {
            if w > 0 && h > 0 && bits.len() == (w as usize).div_ceil(8) * h as usize {
                let mut out = Vec::with_capacity(4 + bits.len());
                out.extend_from_slice(&w.to_le_bytes());
                out.extend_from_slice(&h.to_le_bytes());
                out.extend_from_slice(&bits);
                store::write_file(&file, &out);
                return;
            }
        }
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


/// A deck's language, for hyphenating cards too short to tell: from the name of a deck of mined
/// words (Matcha::Deutsch), else from the text of up to 80 of its cards.
fn deck_language(name: &str, sides: &[(u64, String, String)]) -> String {
    const AUTONYMS: [(&str, &str); 13] = [("日本語", "ja"), ("中文", "zh"), ("English", "en"), ("Deutsch", "de"), ("Français", "fr"),
        ("Español", "es"), ("Italiano", "it"), ("Português", "pt"), ("Nederlands", "nl"), ("Magyar", "hu"), ("Polski", "pl"),
        ("Русский", "ru"), ("Українська", "uk")];
    if let Some(l) = name.strip_prefix("Matcha::").and_then(|n| AUTONYMS.iter().find(|a| a.0 == n)) {
        return String::from(l.1);
    }
    let mut text = String::new();
    for (_, f, b) in sides.iter().take(80) {
        for side in [f, b] {
            let mut in_tag = false;
            for c in side.chars() {
                match c {
                    '<' => in_tag = true,
                    '>' => { in_tag = false; text.push(' '); }
                    '\u{1e}' | '\u{1f}' => text.push(' '),
                    _ if !in_tag => text.push(c),
                    _ => {}
                }
            }
            text.push(' ');
        }
    }
    String::from(crate::hyph::guess_lang_sure(&text).unwrap_or(""))
}

/// Cards that arrive already studied in Anki (a deck synced for the first time, or a mined word a
/// cut-off hand-over found in Anki) and have no progress here: their own review history is
/// replayed, as pulled reviews are, and Anki's FSRS memory is taken where it has one.
fn seed_studied(client: &mut Client, key: &str, studied: &[(u64, Value)]) -> Result<(), Error> {
    if studied.is_empty() {
        return Ok(());
    }
    let (states, _, _) = store::load_states(key);
    let todo: Vec<&(u64, Value)> = studied.iter().filter(|s| states.binary_search_by_key(&s.0, |c| c.id).is_err()).collect();
    if todo.is_empty() {
        return Ok(());
    }
    for chunk in todo.chunks(50) {
        let ids: Vec<String> = chunk.iter().map(|s| format!("{}", s.0)).collect();
        let mut pulled: Vec<Pulled> = Vec::new();
        let mut cursor = Value::Null;
        loop {
            let mut params = json!({"cardIds": ids, "includeManual": false, "limit": 200});
            if !cursor.is_null() {
                params["cursor"] = cursor.clone();
            }
            let page = client.call("reviews.list", params)?;
            for r in page.get("items").and_then(|v| v.as_array()).into_iter().flatten() {
                let rating = RATING_NAMES.iter().position(|n| r["rating"].as_str() == Some(n)).map(|i| i as u8 + 1).unwrap_or(0);
                if rating == 0 {
                    continue;
                }
                let interval = if r.pointer("/interval/unit").and_then(|u| u.as_str()) == Some("days") {
                    r.pointer("/interval/value").and_then(|v| v.as_u64()).unwrap_or(0) as u16
                } else {
                    0
                };
                pulled.push(Pulled { id: id_str(&r["cardId"]), time_ms: id_str(&r["id"]) as i64, rating, interval });
            }
            cursor = page.get("nextCursor").cloned().unwrap_or(Value::Null);
            if cursor.is_null() {
                break;
            }
        }
        pulled.sort_by_key(|p| p.time_ms);
        store::apply_pulled(key, &pulled);
    }
    // Anki's own memory is more exact than a replay from a blank card; a card with nothing to
    // replay (rescheduled by hand, or its history cut off) takes Anki's schedule as it is.
    // Anki gives its due day in the collection's own count: the server's device.today turns it
    // into days from today (an older server: due today).
    let anki_today = client.call("device.today", json!({})).ok().and_then(|v| v["day"].as_i64());
    let (mut states, day, new_today) = store::load_states(key);
    let today = store::today() as i64;
    for (id, sch) in todo {
        let mem = sch.get("fsrsMemory").filter(|m| !m.is_null())
            .and_then(|m| Some((m["stabilityDays"].as_f64()? as f32, m["difficulty"].as_f64()? as f32)));
        let ivl = sch["intervalDays"].as_i64().unwrap_or(0).max(0);
        let i = match states.binary_search_by_key(id, |c| c.id) {
            Ok(i) => i,
            Err(i) => {
                let due = match (sch.pointer("/due/kind").and_then(|k| k.as_str()), sch.pointer("/due/day").and_then(|d| d.as_i64()), anki_today) {
                    (Some("day"), Some(d), Some(t)) => today + (d - t),
                    _ => today,
                }
                .clamp(0, u16::MAX as i64);
                states.insert(i, store::CardState {
                    id: *id,
                    stability: ivl.max(1) as f32,
                    difficulty: 5.0,
                    due: due as u16,
                    last_review: (due - ivl).clamp(0, today) as u16,
                    reps: sch["repetitions"].as_u64().unwrap_or(1).max(1) as u16,
                    lapses: sch["lapses"].as_u64().unwrap_or(0).min(255) as u8,
                    flags: 0,
                });
                i
            }
        };
        if let Some((stab, diff)) = mem {
            states[i].stability = stab;
            states[i].difficulty = diff.clamp(1.0, 10.0);
        }
    }
    store::write_states(key, &states, if day == 0 { store::today() } else { day }, new_today);
    Ok(())
}

/// `latest`: the deck's newest review as device.decks gave it (Some(None): it has none), so no
/// reviews.getLatestId is needed.
fn pull_reviews(client: &mut Client, info: &mut DeckInfo, totals: &mut Totals, latest: Option<Option<u64>>) -> Result<(), Error> {
    // Ratings made here and not yet sent: an Anki review of the same card from before one of them
    // is already outdated on this device.
    let unsent = store::read_log(&info.key);
    // Nothing newer in Anki than the last review already pulled: no list to ask for.
    if let Some(l) = latest {
        if l.unwrap_or(0) <= info.last_review_id && (info.last_review_id != 0 || l.is_none()) {
            return Ok(());
        }
    } else if info.last_review_id != 0 {
        if let Ok(latest) = client.call("reviews.getLatestId", json!({"deck": {"id": info.server_id}})) {
            if id_str(&latest["id"]) <= info.last_review_id {
                return Ok(());
            }
        }
    }
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

/// `in_anki`: the deck's cards in Anki, sorted.
fn push_reviews(client: &mut Client, info: &mut DeckInfo, in_anki: &[u64], totals: &mut Totals) -> Result<(), Error> {
    let mut log = store::read_log(&info.key);
    if log.is_empty() {
        return Ok(());
    }
    totals.sent = true;
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
                        Err(e) if e.kind_name == "notFound" && in_anki.binary_search(&entry.id).is_err() => gone.push(entry.id),
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
    // Due dates grouped by day (one setDueDate per day), then every interval and FSRS memory in
    // one device.cardsUpdateMany (card by card from a server without it).
    let mut by_days: Vec<(u16, Vec<String>)> = Vec::new();
    let mut updates: Vec<Value> = Vec::new();
    for id in ids.iter().filter(|id| !gone.contains(id)) {
        let Ok(i) = states.binary_search_by_key(id, |s| s.id) else { continue };
        let s = states[i];
        let days = s.due.saturating_sub(today);
        match by_days.iter_mut().find(|d| d.0 == days) {
            Some(d) => d.1.push(format!("{}", id)),
            None => by_days.push((days, alloc::vec![format!("{}", id)])),
        }
        let interval = if s.due > s.last_review { s.due - s.last_review } else { 1 };
        updates.push(json!({"cardId": format!("{}", id), "patch": {"intervalDays": interval}}));
        updates.push(json!({"cardId": format!("{}", id), "patch": {"fsrsMemory": {
            "difficulty": s.difficulty.clamp(1.0, 10.0), "stabilityDays": s.stability}}}));
    }
    for (days, cards) in by_days {
        client.call("scheduling.setDueDate", json!({"cardIds": cards, "days": format!("{}", days)}))?;
    }
    if !updates.is_empty() {
        // An SM-2 collection refuses FSRS memory: that item's error is left alone.
        match client.call("device.cardsUpdateMany", json!({"items": updates})) {
            Ok(_) => {}
            Err(e) if e.kind == Kind::Rpc && e.kind_name == "methodNotFound" => {
                let mut fsrs_refused = false;
                for u in &updates {
                    if fsrs_refused && u.pointer("/patch/fsrsMemory").is_some() {
                        continue;
                    }
                    if let Err(e) = client.call("advanced.cards.update", u.clone()) {
                        if e.kind != Kind::Rpc {
                            return Err(e);
                        }
                        fsrs_refused |= e.kind_name == "unsupportedCapability";
                    }
                }
            }
            Err(e) => return Err(e),
        }
    }
    totals.pushed += log.iter().filter(|e| e.rating > 0 && !gone.contains(&e.id)).count() as u32;
    store::write_log(&info.key, &[]);
    Ok(())
}

/// What a failed sync says, by what went wrong (matcha's errorText).
pub fn error_text(svc: &str, e: &Error) -> String {
    match &e.kind {
        Kind::Network | Kind::Http(_) => trf("unreachable", &[Arg::S(svc)]),
        Kind::Unauthorized => trf("key_refused", &[Arg::S(svc)]),
        Kind::Reply => trf("reply_error", &[Arg::S(svc)]),
        Kind::Rpc if e.kind_name == "deckMissing" => e.message.clone(),
        Kind::Rpc if e.kind_name == "remoteBlocked" => tr("ankiweb_needs_anki"),
        Kind::Rpc if svc == "Migaku" && e.kind_name == "authenticationRequired" => tr("migaku_not_set_up"),
        Kind::Rpc => format!("{}: {}", svc, if e.message.is_empty() { &e.kind_name } else { &e.message }),
    }
}
