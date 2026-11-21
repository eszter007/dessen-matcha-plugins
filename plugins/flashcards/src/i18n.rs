//! The plugin's words in the firmware's language: CrossPoint keeps it in
//! /.crosspoint/settings.json ("language": "DE", matcha's language codes), and the plugin folder
//! holds i18n/<code>.json for each. A missing word falls back to English.
use crate::store;
use alloc::{string::String, vec::Vec};
use serde_json::Value;

struct Table {
    code: String,
    lang: Value,
    en: Value,
}
static mut TABLE: Option<Table> = None;

fn table() -> &'static Table {
    let t = unsafe { &mut *core::ptr::addr_of_mut!(TABLE) };
    if t.is_none() {
        let code = store::read("/.crosspoint/settings.json")
            .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
            .and_then(|v| v.get("language").and_then(|l| l.as_str()).map(String::from))
            .unwrap_or_else(|| String::from("EN"));
        let load = |c: &str| store::read_json(&alloc::format!("i18n/{}.json", c)).unwrap_or(Value::Null);
        *t = Some(Table { lang: if code == "EN" { Value::Null } else { load(&code) }, en: load("EN"), code });
    }
    t.as_ref().unwrap()
}

/// The word for `key`, or its English, or the key itself.
pub fn tr(key: &str) -> String {
    let t = table();
    t.lang.get(key).and_then(|v| v.as_str())
        .or_else(|| t.en.get(key).and_then(|v| v.as_str()))
        .map(String::from)
        .unwrap_or_else(|| String::from(key))
}

/// Which plural form `n` takes, by matcha's rules (PluralRules.h) for the language.
fn plural_form(code: &str, n: i64) -> usize {
    let n = n.abs();
    let (m10, m100) = (n % 10, n % 100);
    match code {
        "JA" | "ZHS" | "ZHT" | "VI" | "ID" => 0,
        "FR" | "PT" => (n > 1) as usize,
        "RU" | "UK" | "BE" | "BS" => {
            if m10 == 1 && m100 != 11 { 0 } else if (2..=4).contains(&m10) && !(12..=14).contains(&m100) { 1 } else { 2 }
        }
        "PL" => if n == 1 { 0 } else if (2..=4).contains(&m10) && !(12..=14).contains(&m100) { 1 } else { 2 },
        "CS" | "SK" => if n == 1 { 0 } else if (2..=4).contains(&n) { 1 } else { 2 },
        "LT" => if (11..=19).contains(&m100) { 2 } else if m10 == 1 { 0 } else if m10 >= 2 { 1 } else { 2 },
        "SI" => match m100 { 1 => 0, 2 => 1, 3 | 4 => 2, _ => 3 },
        "RO" => if n == 1 { 0 } else if n == 0 || (1..=19).contains(&m100) { 1 } else { 2 },
        "AR" => if n <= 2 { n as usize } else if (3..=10).contains(&m100) { 3 } else if m100 >= 11 { 4 } else { 5 },
        "HE" => if n == 1 || n == 2 { (n - 1) as usize } else { 2 },
        "FA" => (n > 1) as usize,
        _ => (n != 1) as usize,
    }
}

/// Whether the language reads right to left (Arabic, Hebrew, Persian).
pub fn rtl() -> bool { matches!(table().code.as_str(), "AR" | "HE" | "FA") }

/// A count message ("one|other" forms) for `n`, its %d filled in.
pub fn count(key: &str, n: i64) -> String {
    let t = table();
    let (text, code) = match t.lang.get(key).and_then(|v| v.as_str()) {
        Some(s) => (String::from(s), t.code.as_str()),
        None => (tr(key), "EN"),
    };
    let forms: Vec<&str> = text.split('|').collect();
    let form = forms[plural_form(code, n).min(forms.len() - 1)];
    fill(form, &[Arg::N(n)])
}

pub enum Arg<'a> {
    N(i64),
    S(&'a str),
}
/// printf-style %d and %s, in order.
pub fn fill(form: &str, args: &[Arg]) -> String {
    let mut out = String::new();
    let mut it = args.iter();
    let mut chars = form.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '%' && matches!(chars.peek(), Some('d') | Some('s')) {
            chars.next();
            match it.next() {
                Some(Arg::N(n)) => out.push_str(&alloc::format!("{}", n)),
                Some(Arg::S(s)) => out.push_str(s),
                None => {}
            }
        } else {
            out.push(c);
        }
    }
    out
}
/// `key` with its %s and %d filled in.
pub fn trf(key: &str, args: &[Arg]) -> String { fill(&tr(key), args) }
