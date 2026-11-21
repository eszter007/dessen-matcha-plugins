//! An Anki card side laid out with its note type's CSS, as matcha lays card sides out with its
//! reader engine (buildCardHtmlPages), on a one-bit panel that can also draw two greys.
//!
//! The model: block boxes (margins, padding as indent, borders drawn as rules, a background as a
//! light grey band, lists with markers, tables with real columns, images) holding lines of inline
//! runs (serif or sans at eight sizes, bold, italic, colour as black or grey, highlight,
//! underline, strike-through, sub/superscript, ruby readings, inline pictures). Lines break at
//! spaces, between CJK characters (kinsoku: closing marks stay), and inside Latin and Cyrillic
//! words at their syllables (the card's language guessed, Liang patterns); they align left,
//! centre, right or justified, and right-to-left paragraphs to the right. Pages split between
//! lines.
//!
//! CSS is a subset: rules of tag, .class and #id selectors with descendant and child combinators
//! (both read as descendant; pseudo-classes and attribute selectors are skipped), inline style=
//! attributes and the legacy <font>/align= attributes. Lengths: px, pt, em, rem, %, keywords.
//! Anki's 20 px card text is set at 14 pt, matcha's card size; other sizes scale with it.
use crate::cardfaces::SIZES;
use crate::gfx::{self, Canvas, Style};
use crate::html::{attr, entity};
use crate::{dsn, store, sync};
use alloc::{boxed::Box, format, string::String, vec, vec::Vec};

// ---------------------------------------------------------------- the tree
enum Node {
    El(Box<El>),
    Text(String),
}
struct El {
    tag: String,
    id: String,
    classes: Vec<String>,
    attrs: Vec<(String, String)>,
    kids: Vec<Node>,
    pos: usize, // its place among its parent's elements, from 0
    of: usize,  // how many elements its parent has
}
impl El {
    fn attr(&self, k: &str) -> Option<&str> { self.attrs.iter().find(|a| a.0 == k).map(|a| a.1.as_str()) }
}
const VOID: [&str; 8] = ["br", "hr", "img", "meta", "link", "input", "wbr", "col"];
/// Every attribute of a start tag, names lower-cased (a bare name has an empty value).
fn parse_attrs(tag: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let b: Vec<char> = tag.chars().collect();
    let mut i = b.iter().position(|c| c.is_whitespace()).unwrap_or(b.len());
    while i < b.len() {
        while i < b.len() && (b[i].is_whitespace() || b[i] == '/') {
            i += 1;
        }
        let start = i;
        while i < b.len() && !b[i].is_whitespace() && b[i] != '=' && b[i] != '/' {
            i += 1;
        }
        if start == i {
            break;
        }
        let name: String = b[start..i].iter().collect::<String>().to_ascii_lowercase();
        while i < b.len() && b[i].is_whitespace() {
            i += 1;
        }
        let mut value = String::new();
        if i < b.len() && b[i] == '=' {
            i += 1;
            while i < b.len() && b[i].is_whitespace() {
                i += 1;
            }
            if i < b.len() && (b[i] == '"' || b[i] == '\'') {
                let q = b[i];
                i += 1;
                while i < b.len() && b[i] != q {
                    value.push(b[i]);
                    i += 1;
                }
                i += 1;
            } else {
                while i < b.len() && !b[i].is_whitespace() {
                    value.push(b[i]);
                    i += 1;
                }
            }
        }
        out.push((name, value));
    }
    out
}
/// Each element's place among its siblings, for :first-child and its kin.
fn number(e: &mut El) {
    let of = e.kids.iter().filter(|k| matches!(k, Node::El(_))).count();
    let mut pos = 0;
    for k in e.kids.iter_mut() {
        if let Node::El(c) = k {
            c.pos = pos;
            c.of = of;
            pos += 1;
            number(c);
        }
    }
}

fn parse(html: &str) -> El {
    let root = || El { tag: String::from("body"), id: String::new(), classes: vec![String::from("card")], attrs: Vec::new(), kids: Vec::new(), pos: 0, of: 1 };
    let mut stack: Vec<El> = vec![root()];
    let b = html.as_bytes();
    let mut i = 0;
    let mut text = String::new();
    let flush = |text: &mut String, stack: &mut Vec<El>| {
        if !text.is_empty() {
            stack.last_mut().unwrap().kids.push(Node::Text(core::mem::take(text)));
        }
    };
    let close_top = |stack: &mut Vec<El>| {
        let el = stack.pop().unwrap();
        stack.last_mut().unwrap().kids.push(Node::El(Box::new(el)));
    };
    while i < b.len() {
        if b[i] == b'<' {
            let end = html[i..].find('>').map(|e| i + e).unwrap_or(b.len() - 1);
            let tag = &html[i + 1..end];
            i = end + 1;
            if tag.starts_with('!') || tag.starts_with('?') {
                continue;
            }
            flush(&mut text, &mut stack);
            let closing = tag.starts_with('/');
            let name: String = tag.trim_start_matches('/').chars().take_while(|c| c.is_ascii_alphanumeric())
                .collect::<String>().to_ascii_lowercase();
            if name.is_empty() {
                continue;
            }
            if !closing && (name == "script" || name == "style" || name == "head" || name == "title") {
                let close = format!("</{}", name);
                i = html[i..].to_ascii_lowercase().find(&close).map(|e| i + e).unwrap_or(b.len());
                i = html[i..].find('>').map(|e| i + e + 1).unwrap_or(b.len());
                continue;
            }
            if closing {
                if let Some(pos) = stack.iter().rposition(|e| e.tag == name) {
                    while pos > 0 && stack.len() > pos {
                        close_top(&mut stack);
                    }
                }
                continue;
            }
            let attrs = parse_attrs(tag);
            let el = El {
                tag: name.clone(),
                id: attr(tag, "id").unwrap_or_default(),
                classes: attr(tag, "class").unwrap_or_default().split_whitespace().map(String::from).collect(),
                attrs,
                kids: Vec::new(),
                pos: 0,
                of: 1,
            };
            if VOID.contains(&name.as_str()) || tag.ends_with('/') {
                stack.last_mut().unwrap().kids.push(Node::El(Box::new(el)));
            } else {
                // Elements whose end tag may be left out close an open one of their kind.
                let implied: &[&str] = match name.as_str() {
                    "p" => &["p"],
                    "li" => &["li"],
                    "tr" => &["tr", "td", "th"],
                    "td" | "th" => &["td", "th"],
                    "dt" | "dd" => &["dt", "dd"],
                    "rt" | "rp" => &["rt", "rp"],
                    _ => &[],
                };
                while stack.len() > 1 && implied.contains(&stack.last().unwrap().tag.as_str()) {
                    close_top(&mut stack);
                }
                stack.push(el);
            }
            continue;
        }
        let c = html[i..].chars().next().unwrap();
        i += c.len_utf8();
        if c == '&' {
            if let Some((ch, e)) = html[i..].find(';').filter(|&e| e <= 10).and_then(|e| entity(&html[i..i + e]).map(|ch| (ch, e))) {
                i += e + 1;
                text.push(ch);
                continue;
            }
        }
        text.push(c);
    }
    flush(&mut text, &mut stack);
    while stack.len() > 1 {
        close_top(&mut stack);
    }
    let mut root = stack.pop().unwrap();
    number(&mut root);
    root
}

// ---------------------------------------------------------------- the CSS
/// One compound of a selector: tag, id, classes, attribute tests and the structural
/// pseudo-classes; `child` when it must be the parent of the compound after it (`>`).
struct Compound {
    tag: String,
    id: String,
    classes: Vec<String>,
    attrs: Vec<(String, char, String)>, // name, test ('*' present, '=' '~' '^' '$' '|' 'c' contains), value
    nth: Vec<(bool, i32, i32)>,         // (from the end, a, b): an+b, 1-based
    not: Vec<Compound>,
    child: bool,
}
struct Rule {
    chain: Vec<Compound>, // ancestors first, the element last
    spec: u32,
    order: u32,
    decls: Vec<(String, String)>,
}
fn decls(body: &str) -> Vec<(String, String)> {
    body.split(';').filter_map(|d| {
        let (k, v) = d.split_once(':')?;
        Some((k.trim().to_ascii_lowercase(), String::from(v.trim().trim_end_matches("!important").trim())))
    }).collect()
}
/// "odd", "even", "3", "2n+1", "-n+3" as (a, b).
fn nth_formula(f: &str) -> Option<(i32, i32)> {
    let f: String = f.chars().filter(|c| !c.is_whitespace()).collect::<String>().to_ascii_lowercase();
    match f.as_str() {
        "odd" => return Some((2, 1)),
        "even" => return Some((2, 0)),
        _ => {}
    }
    if let Some((a, b)) = f.split_once('n') {
        let a = match a { "" | "+" => 1, "-" => -1, a => a.parse().ok()? };
        let b = if b.is_empty() { 0 } else { b.parse().ok()? };
        return Some((a, b));
    }
    f.parse().ok().map(|b| (0, b))
}
/// A compound selector, or None when it holds something not supported (:hover, ::before ...).
fn compound(part: &str) -> Option<(Compound, u32)> {
    let mut c = Compound { tag: String::new(), id: String::new(), classes: Vec::new(), attrs: Vec::new(), nth: Vec::new(), not: Vec::new(), child: false };
    let mut spec = 0;
    let p: Vec<char> = part.chars().collect();
    let mut i = 0;
    let word = |i: &mut usize| -> String {
        let s = *i;
        while *i < p.len() && (p[*i].is_alphanumeric() || p[*i] == '-' || p[*i] == '_' || p[*i] == '*') {
            *i += 1;
        }
        p[s..*i].iter().collect()
    };
    let t = word(&mut i);
    if !t.is_empty() && t != "*" {
        c.tag = t.to_ascii_lowercase();
        spec += 1;
    }
    while i < p.len() {
        match p[i] {
            '.' => { i += 1; c.classes.push(word(&mut i)); spec += 10; }
            '#' => { i += 1; c.id = word(&mut i); spec += 100; }
            '[' => {
                let end = p[i..].iter().position(|&x| x == ']')? + i;
                let inner: String = p[i + 1..end].iter().collect();
                let (name, test, value) = match inner.find('=') {
                    Some(e) => {
                        let (op, name) = match inner[..e].chars().last() {
                            Some(o @ ('~' | '^' | '$' | '|' | '*')) => (if o == '*' { 'c' } else { o }, &inner[..e - 1]),
                            _ => ('=', &inner[..e]),
                        };
                        (name.trim().to_ascii_lowercase(), op, String::from(inner[e + 1..].trim().trim_matches(['"', '\''])))
                    }
                    None => (inner.trim().to_ascii_lowercase(), '*', String::new()),
                };
                c.attrs.push((name, test, value));
                spec += 10;
                i = end + 1;
            }
            ':' => {
                if p.get(i + 1) == Some(&':') {
                    return None; // pseudo-elements draw nothing here
                }
                i += 1;
                let name = word(&mut i).to_ascii_lowercase();
                let mut arg = String::new();
                if p.get(i) == Some(&'(') {
                    let mut depth = 0;
                    let s = i + 1;
                    while i < p.len() {
                        if p[i] == '(' { depth += 1; }
                        if p[i] == ')' { depth -= 1; if depth == 0 { break; } }
                        i += 1;
                    }
                    arg = p[s..i.min(p.len())].iter().collect();
                    i += 1;
                }
                spec += 10;
                match name.as_str() {
                    "first-child" => c.nth.push((false, 0, 1)),
                    "last-child" => c.nth.push((true, 0, 1)),
                    "only-child" => { c.nth.push((false, 0, 1)); c.nth.push((true, 0, 1)); }
                    "nth-child" => { let (a, b) = nth_formula(&arg)?; c.nth.push((false, a, b)); }
                    "nth-last-child" => { let (a, b) = nth_formula(&arg)?; c.nth.push((true, a, b)); }
                    "first-of-type" | "last-of-type" => return None,
                    "not" => { let (n, _) = compound(arg.trim())?; c.not.push(n); }
                    "root" => c.tag = String::from("body"),
                    _ => return None, // :hover, :focus, :checked ... never true on a card
                }
            }
            _ => return None,
        }
    }
    Some((c, spec))
}
/// The width a card's CSS sees, in CSS px (for @media).
static mut CSS_WIDTH: f32 = 300.0;
/// Whether an @media query holds on the panel: screen, portrait, light, at the card's width.
fn media_matches(q: &str) -> bool {
    let q = q.to_ascii_lowercase();
    let w = unsafe { CSS_WIDTH };
    q.split(',').any(|alt| {
        let alt = alt.trim();
        if alt.starts_with("print") || alt.starts_with("not ") || alt.contains("only print") {
            return false;
        }
        let mut ok = true;
        let mut rest = alt;
        while let Some(open) = rest.find('(') {
            let close = rest[open..].find(')').map(|c| open + c).unwrap_or(rest.len());
            let feat = &rest[open + 1..close];
            if let Some((k, v)) = feat.split_once(':') {
                let px = length(v.trim(), 16.0, w).unwrap_or(0.0);
                ok &= match k.trim() {
                    "max-width" => w <= px,
                    "min-width" => w >= px,
                    "orientation" => v.trim() == "portrait",
                    "prefers-color-scheme" => v.trim() == "light",
                    "hover" => v.trim() == "none",
                    "pointer" => v.trim() != "fine",
                    _ => true,
                };
            }
            rest = &rest[close.min(rest.len())..];
            if rest.starts_with(')') { rest = &rest[1..]; }
        }
        ok
    })
}
fn parse_css(css: &str) -> Vec<Rule> {
    let mut src = String::new();
    let mut rest = css;
    while let Some(a) = rest.find("/*") {
        src.push_str(&rest[..a]);
        rest = rest[a..].find("*/").map(|e| &rest[a + e + 2..]).unwrap_or("");
    }
    src.push_str(rest);
    let mut rules = Vec::new();
    let mut order = 0;
    parse_rules(&src, &mut rules, &mut order);
    rules.sort_by_key(|r| (r.spec, r.order));
    rules
}
fn parse_rules(src: &str, rules: &mut Vec<Rule>, order: &mut u32) {
    let mut s = src;
    while let Some(open) = s.find('{') {
        let head = s[..open].trim();
        if head.starts_with('@') {
            // the block to its matching brace
            let mut depth = 0;
            let mut end = s.len();
            for (k, ch) in s[open..].char_indices() {
                if ch == '{' { depth += 1; }
                if ch == '}' {
                    depth -= 1;
                    if depth == 0 { end = open + k + 1; break; }
                }
            }
            if let Some(q) = head.strip_prefix("@media") {
                if media_matches(q) {
                    parse_rules(&s[open + 1..end.saturating_sub(1).max(open + 1)], rules, order);
                }
            }
            s = &s[end..];
            continue;
        }
        let Some(close) = s[open..].find('}').map(|e| open + e) else { break };
        let ds = decls(&s[open + 1..close]);
        for sel in head.split(',') {
            let sel = sel.trim();
            if sel.is_empty() {
                continue;
            }
            // compounds and the combinators between them
            let mut parts: Vec<(String, bool)> = Vec::new();
            let mut cur = String::new();
            let mut child_next = false;
            let mut depth = 0;
            for ch in sel.chars() {
                match ch {
                    '(' | '[' => { depth += 1; cur.push(ch); }
                    ')' | ']' => { depth -= 1; cur.push(ch); }
                    '>' | '+' | '~' if depth == 0 => {
                        if !cur.trim().is_empty() { parts.push((core::mem::take(&mut cur), child_next)); }
                        // siblings are read as plain descendants of the same parent
                        child_next = ch == '>';
                    }
                    c if c.is_whitespace() && depth == 0 => {
                        if !cur.trim().is_empty() {
                            parts.push((core::mem::take(&mut cur), child_next));
                            child_next = false;
                        }
                    }
                    _ => cur.push(ch),
                }
            }
            if !cur.trim().is_empty() {
                parts.push((cur, child_next));
            }
            let mut chain = Vec::new();
            let mut spec = 0;
            let mut ok = true;
            for (k, (p, child)) in parts.iter().enumerate() {
                match compound(p.trim()) {
                    Some((mut c, sp)) => {
                        // `child` is stored on the compound before the `>`
                        if k > 0 && *child {
                            if let Some(prev) = chain.last_mut() { let prev: &mut Compound = prev; prev.child = true; }
                        }
                        c.child = false;
                        spec += sp;
                        chain.push(c);
                    }
                    None => { ok = false; break; }
                }
            }
            if ok && !chain.is_empty() {
                rules.push(Rule { chain, spec, order: *order, decls: ds.clone() });
                *order += 1;
            }
        }
        s = &s[close + 1..];
    }
}
fn matches(c: &Compound, e: &El) -> bool {
    (c.tag.is_empty() || c.tag == e.tag || (c.tag == "html" && e.tag == "body"))
        && (c.id.is_empty() || c.id == e.id)
        && c.classes.iter().all(|k| e.classes.contains(k))
        && c.attrs.iter().all(|(name, test, value)| {
            let Some(v) = e.attr(name) else { return false };
            match test {
                '*' => true,
                '=' => v == value,
                '~' => v.split_whitespace().any(|w| w == value),
                '^' => v.starts_with(value.as_str()),
                '$' => v.ends_with(value.as_str()),
                '|' => v == value || v.starts_with(&format!("{}-", value)),
                _ => v.contains(value.as_str()),
            }
        })
        && c.nth.iter().all(|&(from_end, a, b)| {
            let n = if from_end { (e.of - e.pos) as i32 } else { e.pos as i32 + 1 };
            if a == 0 { n == b } else { (n - b) % a == 0 && (n - b) / a >= 0 }
        })
        && c.not.iter().all(|n| !matches(n, e))
}
fn rule_applies(r: &Rule, e: &El, ancestors: &[&El]) -> bool {
    let (last, before) = r.chain.split_last().unwrap();
    if !matches(last, e) {
        return false;
    }
    // walk the ancestors right to left; a compound marked `child` must be the direct parent
    let mut at = ancestors.len();
    for k in (0..before.len()).rev() {
        let c = &before[k];
        // c.child: c must be the direct parent of what came right after it
        let must_be_parent = c.child;
        loop {
            if at == 0 {
                return false;
            }
            at -= 1;
            if matches(c, ancestors[at]) {
                break;
            }
            if must_be_parent {
                return false;
            }
        }
    }
    true
}

// ---------------------------------------------------------------- values
/// CSS px to device px: Anki's 20 px text is set at 14 pt (at the panel's ~150 dpi, ~29 px).
const PX: f32 = 1.46;
fn length(v: &str, em: f32, base: f32) -> Option<f32> {
    let v = v.trim();
    let num = |s: &str| s.trim().parse::<f32>().ok();
    if v == "0" {
        return Some(0.0);
    }
    if let Some(n) = v.strip_suffix("px") { return num(n); }
    if let Some(n) = v.strip_suffix("pt") { return num(n).map(|x| x * 4.0 / 3.0); }
    if let Some(n) = v.strip_suffix("rem") { return num(n).map(|x| x * 20.0); }
    if let Some(n) = v.strip_suffix("em") { return num(n).map(|x| x * em); }
    if let Some(n) = v.strip_suffix("vw") { return num(n).map(|x| x * base / 100.0); }
    if let Some(n) = v.strip_suffix('%') { return num(n).map(|x| x * base / 100.0); }
    None
}
fn font_size(v: &str, parent: f32) -> Option<f32> {
    Some(match v.trim() {
        "xx-small" => 9.0, "x-small" => 10.0, "small" => 13.0, "medium" => 16.0, "large" => 18.0,
        "x-large" => 24.0, "xx-large" => 32.0, "xxx-large" => 48.0, "smaller" => parent * 0.83, "larger" => parent * 1.2,
        v => return length(v, parent, parent),
    })
}
/// A colour's lightness, 0 black .. 1 white; None for transparent or unknown.
fn lightness(v: &str) -> Option<f32> {
    let v = v.trim().to_ascii_lowercase();
    let rgb = |r: f32, g: f32, b: f32| Some((0.2126 * r + 0.7152 * g + 0.0722 * b) / 255.0);
    if let Some(h) = v.strip_prefix('#') {
        let h: Vec<u8> = h.bytes().collect();
        let x = |a: u8| (a as char).to_digit(16).unwrap_or(0) as f32;
        return match h.len() {
            3 | 4 => rgb(x(h[0]) * 17.0, x(h[1]) * 17.0, x(h[2]) * 17.0),
            6 | 8 => rgb(x(h[0]) * 16.0 + x(h[1]), x(h[2]) * 16.0 + x(h[3]), x(h[4]) * 16.0 + x(h[5])),
            _ => None,
        };
    }
    if let Some(inner) = v.strip_prefix("rgba(").or_else(|| v.strip_prefix("rgb(")) {
        let p: Vec<f32> = inner.trim_end_matches(')').split([',', ' ', '/']).filter_map(|x| x.trim().parse().ok()).collect();
        if p.len() >= 4 && p[3] < 0.3 {
            return None;
        }
        return if p.len() >= 3 { rgb(p[0], p[1], p[2]) } else { None };
    }
    Some(match v.as_str() {
        "black" => 0.0, "white" => 1.0, "gray" | "grey" => 0.5, "darkgray" | "darkgrey" => 0.66, "dimgray" | "dimgrey" => 0.41,
        "lightgray" | "lightgrey" => 0.83, "silver" => 0.75, "gainsboro" => 0.86, "whitesmoke" => 0.96,
        "red" => 0.21, "darkred" | "maroon" => 0.1, "green" => 0.36, "darkgreen" => 0.27, "blue" => 0.07, "navy" | "darkblue" => 0.04,
        "orange" => 0.62, "yellow" => 0.93, "lightyellow" => 0.99, "lightblue" => 0.8, "lightgreen" => 0.8, "pink" => 0.8,
        "purple" => 0.18, "brown" => 0.24, "teal" => 0.43, "olive" => 0.48, "crimson" => 0.2, "orangered" => 0.35,
        _ => return None,
    })
}
/// Text ink for a colour: black, or the panel's dark grey for light text (lighter would not read).
fn ink(l: f32) -> u32 { if l > 0.42 && l < 0.92 { dsn::DARK_GRAY } else { dsn::BLACK } }

#[derive(Clone, Copy, PartialEq)]
enum Align { Left, Center, Right, Justify }
#[derive(Clone, Copy, PartialEq)]
enum Pos { Normal, Sub, Sup }

/// The inherited part of a computed style.
#[derive(Clone, Copy)]
struct Look {
    px: f32,
    sans: bool,
    bold: bool,
    italic: bool,
    underline: bool,
    strike: bool,
    color: u32,
    mark: bool, // highlighted (a light background behind the text)
    align: Align,
    line: f32, // line-height, a factor of the font's own (1 = its natural spacing)
    pos: Pos,
    upper: i8, // text-transform: 1 upper, -1 lower
    pre: bool, // white-space: pre
}
/// The rest: what one element alone carries.
#[derive(Default)]
struct BoxLook {
    hidden: bool,
    block: Option<bool>,
    margin_top: f32,
    margin_bottom: f32,
    indent_left: f32,
    indent_right: f32,
    border_top: bool,
    border_bottom: bool,
    border_left: bool,
    band: bool, // a background behind the whole block
    width: Option<f32>,
    height: Option<f32>,
    max_width: Option<f32>,
    text_indent: f32,
    flex: bool,           // display: flex — children side by side
    column: bool,         // flex-direction: column — they stack after all
    float: Option<bool>,  // float: Some(false) left, Some(true) right
}
fn family_sans(v: &str) -> Option<bool> {
    let v = v.to_ascii_lowercase();
    for f in v.split(',') {
        let f = f.trim().trim_matches(['"', '\'']);
        if f.contains("mincho") || f.contains("ming") || f.contains("song") || (f.contains("serif") && !f.contains("sans"))
            || matches!(f, "georgia" | "times" | "times new roman" | "garamond" | "baskerville" | "palatino" | "cambria" | "book antiqua")
        {
            return Some(false);
        }
        if f.contains("sans") || f.contains("gothic") || f.contains("hei") || f.contains("meiryo") || f.contains("arial")
            || matches!(f, "helvetica" | "verdana" | "tahoma" | "segoe ui" | "system-ui" | "roboto" | "calibri" | "ubuntu"
                         | "-apple-system" | "inter" | "open sans" | "lato" | "trebuchet ms")
        {
            return Some(true);
        }
    }
    None
}
fn apply(look: &mut Look, bx: &mut BoxLook, decls: &[(String, String)], parent: &Look, width: f32) {
    for (k, v) in decls {
        let v = v.as_str();
        let vl = v.to_ascii_lowercase();
        let em = look.px;
        match k.as_str() {
            "font-size" => if let Some(p) = font_size(v, parent.px) { look.px = p },
            "font-family" => if let Some(s) = family_sans(v) { look.sans = s },
            "font-weight" => look.bold = vl == "bold" || vl == "bolder" || vl.parse::<u32>().is_ok_and(|w| w >= 600),
            "font-style" => look.italic = vl == "italic" || vl == "oblique",
            "font" => {
                for w in vl.split_whitespace() {
                    match w {
                        "bold" | "bolder" | "600" | "700" | "800" | "900" => look.bold = true,
                        "italic" | "oblique" => look.italic = true,
                        _ => {
                            if w.contains(|c: char| c.is_ascii_digit()) {
                                if let Some(p) = font_size(w.split('/').next().unwrap_or(""), parent.px) {
                                    look.px = p;
                                }
                            }
                        }
                    }
                }
                if let Some(s) = family_sans(v) {
                    look.sans = s;
                }
            }
            "text-decoration" | "text-decoration-line" => {
                look.underline = vl.contains("underline");
                look.strike = vl.contains("line-through");
            }
            "text-align" => look.align = match vl.as_str() {
                "center" => Align::Center, "right" | "end" => Align::Right, "justify" => Align::Justify, _ => Align::Left,
            },
            "color" => if let Some(l) = lightness(v) { look.color = ink(l) },
            "background" | "background-color" => {
                if let Some(l) = v.split_whitespace().find_map(lightness) {
                    look.mark = l < 0.94;
                    bx.band = l < 0.94;
                }
            }
            "line-height" => {
                look.line = match length(v, em, em) {
                    Some(px) => (px / (em * 1.36)).clamp(0.8, 2.4),
                    None => vl.parse::<f32>().map(|n| (n / 1.36).clamp(0.8, 2.4)).unwrap_or(look.line),
                }
            }
            "vertical-align" => look.pos = match vl.as_str() { "sub" => Pos::Sub, "super" => Pos::Sup, _ => look.pos },
            "text-transform" => look.upper = match vl.as_str() { "uppercase" => 1, "lowercase" => -1, _ => 0 },
            "white-space" => look.pre = vl.starts_with("pre"),
            "display" => match vl.as_str() {
                "none" => bx.hidden = true,
                "flex" | "inline-flex" => {
                    bx.block = Some(true);
                    bx.flex = true;
                }
                "block" | "grid" | "list-item" | "table" => bx.block = Some(true),
                "inline" | "inline-block" => bx.block = Some(false),
                _ => {}
            },
            "visibility" => bx.hidden = vl == "hidden",
            "flex-direction" | "flex-flow" => bx.column = vl.starts_with("column"),
            "float" => bx.float = match vl.as_str() { "left" => Some(false), "right" => Some(true), _ => None },
            "margin-top" => bx.margin_top = length(v, em, width).unwrap_or(0.0),
            "margin-bottom" => bx.margin_bottom = length(v, em, width).unwrap_or(0.0),
            "padding-top" => bx.margin_top += length(v, em, width).unwrap_or(0.0),
            "padding-bottom" => bx.margin_bottom += length(v, em, width).unwrap_or(0.0),
            "margin-left" | "padding-left" | "padding-inline-start" | "margin-inline-start" => bx.indent_left += length(v, em, width).unwrap_or(0.0).max(0.0),
            "margin-right" | "padding-right" | "padding-inline-end" | "margin-inline-end" => bx.indent_right += length(v, em, width).unwrap_or(0.0).max(0.0),
            "margin" | "padding" => {
                let p: Vec<f32> = v.split_whitespace().map(|x| if x == "auto" { 0.0 } else { length(x, em, width).unwrap_or(0.0) }).collect();
                let (t, r, b, l) = match p.len() {
                    1 => (p[0], p[0], p[0], p[0]),
                    2 => (p[0], p[1], p[0], p[1]),
                    3 => (p[0], p[1], p[2], p[1]),
                    4 => (p[0], p[1], p[2], p[3]),
                    _ => (0.0, 0.0, 0.0, 0.0),
                };
                bx.margin_top += t;
                bx.margin_bottom += b;
                bx.indent_left += l.max(0.0);
                bx.indent_right += r.max(0.0);
            }
            "border-left" | "border-inline-start" => {
                bx.border_left = !vl.starts_with("none") && !vl.starts_with('0') && !vl.is_empty();
            }
            "border" | "border-top" | "border-bottom" => {
                let on = !vl.starts_with("none") && !vl.starts_with('0') && !vl.contains("hidden") && !vl.is_empty();
                match k.as_str() {
                    "border" => {
                        bx.border_top = on;
                        bx.border_bottom = on;
                    }
                    "border-top" => bx.border_top = on,
                    _ => bx.border_bottom = on,
                }
            }
            "width" => bx.width = length(v, em, width),
            "max-width" => bx.max_width = length(v, em, width),
            "height" => bx.height = length(v, em, width),
            "text-indent" => bx.text_indent = length(v, em, width).unwrap_or(0.0),
            _ => {}
        }
    }
}
/// The face closest to the run's CSS size (20 px -> 14 pt), its family, weight and slant.
fn style_of(l: &Look) -> Style {
    let pt = l.px * 0.7;
    let pt = if l.pos != Pos::Normal { pt * 0.72 } else { pt };
    let i = SIZES.iter().enumerate().min_by_key(|(_, &s)| ((s as f32 - pt).abs() * 10.0) as i32).map(|(i, _)| i).unwrap_or(1);
    gfx::card_style(l.sans as usize, i, l.bold, l.italic)
}
fn smaller(st: Style, l: &Look) -> Option<Style> {
    let i = (0..SIZES.len()).find(|&i| gfx::same_face(gfx::card_style(l.sans as usize, i, l.bold, l.italic), st))?;
    (i > 0).then(|| gfx::card_style(l.sans as usize, i - 1, l.bold, l.italic))
}

// ---------------------------------------------------------------- items
/// A piece of a laid-out line: a canvas at x (from the line's start) and y (from its top).
pub struct Piece {
    x: i32,
    y: i32,
    canvas: Canvas,
    color: u32,
}
pub enum Item {
    Line {
        pieces: Vec<Piece>,
        marks: Vec<(i32, i32, i32, i32)>,
        x: i32,
        width: i32,
        avail: i32,
        height: i32,
        align_right: bool,
        centre: bool,
        band: Option<(i32, i32)>,
        bars: Vec<i32>,
    },
    Rule { x: i32, w: i32 },
    Image { canvas: Canvas, x: i32, avail: i32, centre: bool, right: bool },
    Gap { h: i32, band: Option<(i32, i32)>, bars: Vec<i32> },
    Row { cells: Vec<(i32, i32, Vec<Item>)>, height: i32 },
}
impl Item {
    pub fn height(&self) -> i32 {
        match self {
            Item::Line { height, .. } => *height,
            Item::Rule { .. } => 21,
            Item::Image { canvas, .. } => canvas.h + 12,
            Item::Gap { h, .. } => *h,
            Item::Row { height, .. } => *height,
        }
    }
}

// ---------------------------------------------------------------- inline runs
struct Run {
    text: String,
    look: Look,
    style: Style,
    space: bool,
    ruby: Option<String>, // the reading over this run (its whole text)
    image: Option<Canvas>,
    marker: bool, // a list marker hung before the line
}
fn run_width(r: &Run) -> i32 {
    if let Some(c) = &r.image {
        return c.w + 4;
    }
    if r.space {
        return gfx::text_width(r.style, " ").max(r.style.line_h / 5);
    }
    let w = gfx::text_width(r.style, &r.text);
    match &r.ruby {
        Some(rd) => w.max(gfx::text_width(ruby_style(r.style), rd)),
        None => w,
    }
}
/// Readings: kana and pinyin small over the base, at about half its size.
fn ruby_style(base: Style) -> Style {
    if base.line_h >= 60 { gfx::UI12 } else if base.line_h >= 44 { gfx::UI10 } else { gfx::SMALL }
}
fn is_closing(c: char) -> bool { "。、，．！？）」』】〉》,.!?;:)]}…ー»،؛؟\"'".contains(c) }

// ---------------------------------------------------------------- the builder
struct Builder<'a> {
    rules: &'a [Rule],
    items: Vec<Item>,
    runs: Vec<Run>,
    // the current block context
    left: i32,
    right: i32,
    width: i32,
    align: Align,
    band: bool,
    first_indent: i32,
    lang: &'static str,
    pending_gap: i32,
    bars: Vec<i32>, // x of each left border running beside the current block (a blockquote's bar)
    paging: Option<Paging>, // the whole side: items paged as they come, only one page kept
}
/// Pages a side while it is laid out: every item's height counts, but only the wanted page's
/// items are kept, so a long side never holds the drawn lines of all its pages at once.
struct Paging {
    height: i32,
    want: usize,
    page: usize,
    used: i32,
    started: bool, // the current page has an item
}
impl Builder<'_> {
    fn avail(&self) -> i32 { (self.width - self.left - self.right).max(40) }
    fn gap(&mut self, h: i32) { self.pending_gap = self.pending_gap.max(h); }
    fn push_item(&mut self, it: Item) {
        if self.pending_gap > 0 {
            let band = self.band.then_some((self.left, self.avail()));
            let gap = Item::Gap { h: self.pending_gap, band, bars: self.bars.clone() };
            self.pending_gap = 0;
            self.page_item(gap);
        }
        self.page_item(it);
    }
    /// An item onto the current page: on the side's page being laid out, or counted and dropped.
    fn page_item(&mut self, it: Item) {
        let Some(p) = self.paging.as_mut() else {
            self.items.push(it);
            return;
        };
        let h = it.height();
        if p.used + h > p.height && p.started {
            if p.page == p.want {
                while self.items.last().is_some_and(|i| matches!(i, Item::Gap { .. })) {
                    self.items.pop();
                }
            }
            p.page += 1;
            p.used = 0;
            p.started = false;
        }
        if p.used == 0 && matches!(it, Item::Gap { .. }) {
            return;
        }
        p.used += h;
        p.started = true;
        if p.page == p.want {
            self.items.push(it);
        }
    }
    /// Breaks the paragraph into lines and lays them out.
    /// The wanted page is complete: nothing after it needs laying out.
    fn past_want(&self) -> bool { self.paging.as_ref().is_some_and(|p| p.page > p.want) }
    fn flush(&mut self) {
        if self.past_want() {
            self.runs.clear();
            return;
        }
        let runs = core::mem::take(&mut self.runs);
        if runs.iter().all(|r| (r.space || r.text.trim().is_empty()) && r.image.is_none()) {
            return;
        }
        let avail = self.avail();
        let text: String = runs.iter().filter(|r| !r.marker).map(|r| r.text.as_str()).collect();
        let rtl = crate::rtl::paragraph_rtl(&text);
        let mut lines: Vec<Vec<Run>> = Vec::new();
        let mut line: Vec<Run> = Vec::new();
        let mut w = self.first_indent.max(0);
        let mut queue: alloc::collections::VecDeque<Run> = runs.into();
        while let Some(mut r) = queue.pop_front() {
            if r.marker {
                line.push(r);
                continue;
            }
            if !r.space && r.image.is_none() && r.ruby.is_none() {
                // A word wider than a whole line steps down a size, as matcha scales a heading.
                while run_width(&r) > avail {
                    let Some(s) = smaller(r.style, &r.look) else { break };
                    r.style = s;
                }
            }
            let tw = run_width(&r);
            let at_start = line.iter().all(|x| x.marker);
            if r.space && at_start && !r.look.pre {
                continue;
            }
            let glued = r.text.starts_with(is_closing);
            if w + tw > avail && !at_start && !glued {
                if r.space {
                    trim(&mut line);
                    lines.push(core::mem::take(&mut line));
                    w = 0;
                    continue;
                }
                // Hyphenate a Latin or Cyrillic word into what is left of the line: in left-aligned
                // or justified text, and only when breaking before the word would leave a quarter
                // of the line empty (centred text and short gaps break between words).
                let worth = matches!(self.align, Align::Left | Align::Justify) && (avail - w) * 4 > avail;
                if worth && r.ruby.is_none() && r.image.is_none() && !r.text.chars().any(|c| c as u32 >= 0x2E80) {
                    let room = avail - w;
                    let mut cut: Option<(usize, String)> = None;
                    for (off, hy) in crate::hyph::break_offsets(&r.text, self.lang) {
                        let head = if hy { format!("{}-", &r.text[..off]) } else { String::from(&r.text[..off]) };
                        if gfx::text_width(r.style, &head) > room {
                            break;
                        }
                        cut = Some((off, head));
                    }
                    if let Some((off, head)) = cut {
                        let rest = String::from(&r.text[off..]);
                        line.push(Run { text: head, look: r.look, style: r.style, space: false, ruby: None, image: None, marker: false });
                        lines.push(core::mem::take(&mut line));
                        w = 0;
                        r.text = rest;
                        queue.push_front(r);
                        continue;
                    }
                }
                trim(&mut line);
                lines.push(core::mem::take(&mut line));
                w = 0;
            }
            w += tw;
            line.push(r);
        }
        trim(&mut line);
        if !line.is_empty() {
            lines.push(line);
        }
        let n = lines.len();
        for (k, l) in lines.into_iter().enumerate() {
            let justify = self.align == Align::Justify && k + 1 < n;
            let indent = if k == 0 { self.first_indent } else { 0 };
            let it = self.make_line(l, avail, indent, justify, rtl);
            self.push_item(it);
        }
        self.first_indent = 0;
    }
    /// One line: runs drawn into canvases on a common baseline, readings above.
    fn make_line(&self, mut runs: Vec<Run>, avail: i32, indent: i32, justify: bool, rtl: bool) -> Item {
        let marker: Option<Run> = runs.iter().position(|r| r.marker).map(|i| runs.remove(i));
        let ascent = runs.iter().map(|r| match &r.image { Some(c) => c.h - 4, None => gfx::ascent(r.style) }).max().unwrap_or(0);
        let ruby_h = runs.iter().filter(|r| r.ruby.is_some()).map(|r| ruby_style(r.style).line_h - 6).max().unwrap_or(0);
        let below = runs.iter().map(|r| if r.image.is_some() { 4 } else { r.style.line_h - gfx::ascent(r.style) }).max().unwrap_or(4);
        let natural = ascent + below;
        let factor = runs.iter().map(|r| r.look.line).fold(0.0f32, f32::max).max(0.8);
        let height = ((natural as f32) * factor) as i32 + ruby_h + 2;
        let lead = ((natural as f32) * (factor - 1.0) / 2.0).max(0.0) as i32;
        let base = ruby_h + lead + ascent; // the baseline, from the line's top
        let widths: Vec<i32> = runs.iter().map(run_width).collect();
        let total: i32 = widths.iter().sum::<i32>() + indent;
        let spaces = runs.iter().filter(|r| r.space).count() as i32;
        let extra = if justify && spaces > 0 { (avail - total).max(0) } else { 0 };
        let mut pieces = Vec::new();
        let mut marks = Vec::new();
        let mut x = indent;
        let mut seen_space = 0;
        let order = visual_order(&runs, rtl);
        for &k in &order {
            let r = &runs[k];
            let tw = widths[k];
            if r.space {
                seen_space += 1;
                let add = if extra > 0 { extra * seen_space / spaces - extra * (seen_space - 1) / spaces } else { 0 };
                if r.look.mark {
                    marks.push((x, ruby_h + lead, tw + add, natural));
                }
                // underline and strike-through run on across the space between two words
                let between = (r.look.underline || r.look.strike) && k > 0 && k + 1 < runs.len()
                    && runs[k - 1].look.underline == r.look.underline && runs[k + 1].look.underline == r.look.underline
                    && runs[k - 1].look.strike == r.look.strike && runs[k + 1].look.strike == r.look.strike;
                if between {
                    let asc = gfx::ascent(r.style);
                    let mut cv = Canvas::new(tw + add + 4, r.style.line_h + 8);
                    for xx in 0..tw + add {
                        if r.look.underline {
                            cv.set(xx, 4 + asc + 3);
                            if r.style.line_h > 40 {
                                cv.set(xx, 4 + asc + 4);
                            }
                        }
                        if r.look.strike {
                            cv.set(xx, 4 + asc * 2 / 3);
                        }
                    }
                    pieces.push(Piece { x, y: base - asc - 4, canvas: cv, color: r.look.color });
                }
                x += tw + add;
                continue;
            }
            if let Some(img) = &r.image {
                pieces.push(Piece { x, y: base - img.h + 4, canvas: clone_canvas(img), color: dsn::BLACK });
                x += tw;
                continue;
            }
            let asc = gfx::ascent(r.style);
            let shift = match r.look.pos { Pos::Sup => -(asc / 2), Pos::Sub => asc / 3, Pos::Normal => 0 };
            let text = crate::rtl::visual(&r.text);
            let text_w = gfx::text_width(r.style, &text);
            let tx = x + (tw - text_w) / 2;
            let mut cv = Canvas::new(text_w + 4, r.style.line_h + 8);
            gfx::draw_text(&mut cv, 0, 4, r.style, &text);
            if r.look.underline {
                for xx in 0..text_w {
                    cv.set(xx, 4 + asc + 3);
                    if r.style.line_h > 40 {
                        cv.set(xx, 4 + asc + 4);
                    }
                }
            }
            if r.look.strike {
                for xx in 0..text_w {
                    cv.set(xx, 4 + asc * 2 / 3);
                }
            }
            if r.look.mark {
                marks.push((x, ruby_h + lead, tw, natural));
            }
            pieces.push(Piece { x: tx, y: base - asc - 4 + shift, canvas: cv, color: r.look.color });
            if let Some(reading) = &r.ruby {
                let rs = ruby_style(r.style);
                let rw = gfx::text_width(rs, reading);
                let mut rc = Canvas::new(rw + 4, rs.line_h + 4);
                gfx::draw_text(&mut rc, 0, 0, rs, reading);
                // the reading's baseline a little above the base's ink
                let ink_top = base - asc * 9 / 10;
                pieces.push(Piece { x: x + (tw - rw) / 2, y: (ink_top - gfx::ascent(rs) - 2).max(0), canvas: rc, color: r.look.color });
            }
            x += tw;
        }
        if let Some(m) = marker {
            let mw = gfx::text_width(m.style, &m.text);
            let mut cv = Canvas::new(mw + 4, m.style.line_h + 8);
            gfx::draw_text(&mut cv, 0, 4, m.style, &m.text);
            let mx = if rtl { x + 10 } else { -mw - 10 };
            pieces.push(Piece { x: mx, y: base - gfx::ascent(m.style) - 4, canvas: cv, color: m.look.color });
        }
        let align = if rtl && self.align == Align::Left { Align::Right } else { self.align };
        Item::Line {
            pieces,
            marks,
            x: self.left,
            width: x,
            avail,
            height,
            align_right: align == Align::Right,
            centre: align == Align::Center,
            band: self.band.then_some((self.left, avail)),
            bars: self.bars.clone(),
        }
    }
    fn text(&mut self, t: &str, look: &Look) {
        let st = style_of(look);
        let t: String = match look.upper {
            1 => t.to_uppercase(),
            -1 => t.to_lowercase(),
            _ => String::from(t),
        };
        let mut word = String::new();
        let mk = |text: String, space: bool| Run { text, look: *look, style: st, space, ruby: None, image: None, marker: false };
        // A right-to-left word's trailing punctuation is a run of its own: neutral, it then takes
        // the direction around it (a full stop after Arabic in an English line goes to its right).
        // It starts with a closing mark, so it never begins a line.
        let push_word = |runs: &mut Vec<Run>, w: String| {
            if crate::rtl::has_rtl(&w) {
                let cut = w.trim_end_matches(|c: char| ".,;:!?)]}»\"'،؛؟".contains(c)).len();
                if cut > 0 && cut < w.len() {
                    runs.push(mk(String::from(&w[..cut]), false));
                    runs.push(mk(String::from(&w[cut..]), false));
                    return;
                }
            }
            runs.push(mk(w, false));
        };
        for c in t.chars() {
            if c == '\n' && look.pre {
                if !word.is_empty() {
                    push_word(&mut self.runs, core::mem::take(&mut word));
                }
                self.flush();
            } else if c.is_whitespace() && c != '\u{a0}' {
                if !word.is_empty() {
                    push_word(&mut self.runs, core::mem::take(&mut word));
                }
                if look.pre || !self.runs.last().is_some_and(|r| r.space) {
                    self.runs.push(mk(String::from(" "), true));
                }
            } else if c as u32 >= 0x2E80 {
                if !word.is_empty() {
                    push_word(&mut self.runs, core::mem::take(&mut word));
                }
                self.runs.push(mk(c.into(), false));
            } else {
                word.push(if c == '\u{a0}' { ' ' } else { c });
            }
        }
        if !word.is_empty() {
            push_word(&mut self.runs, word);
        }
    }
    fn walk(&mut self, e: &El, parent: Look, ancestors: &mut Vec<*const El>, list: &mut (bool, u32, u32)) {
        if self.past_want() {
            return;
        }
        let mut look = Look { mark: false, pos: Pos::Normal, ..parent };
        let mut bx = BoxLook::default();
        let width = self.avail() as f32 / PX;
        let tag = e.tag.as_str();
        // the user agent's own styles
        match tag {
            "b" | "strong" => look.bold = true,
            "i" | "em" | "cite" | "var" | "dfn" => look.italic = true,
            "u" | "ins" => look.underline = true,
            "s" | "strike" | "del" => look.strike = true,
            "mark" => look.mark = true,
            "sub" => look.pos = Pos::Sub,
            "sup" => look.pos = Pos::Sup,
            "small" => look.px = parent.px * 0.83,
            "big" => look.px = parent.px * 1.2,
            "code" | "kbd" | "samp" | "tt" => look.sans = true,
            "pre" => {
                look.pre = true;
                look.sans = true;
                look.align = Align::Left;
            }
            "h1" | "h2" | "h3" => {
                look.px = parent.px * match tag { "h1" => 2.0, "h2" => 1.5, _ => 1.17 };
                look.bold = true;
                bx.margin_top = 0.6 * look.px;
                bx.margin_bottom = 0.6 * look.px;
            }
            "h4" | "h5" | "h6" | "th" | "dt" => look.bold = true,
            "center" => look.align = Align::Center,
            "p" => {
                bx.margin_top = parent.px * 0.8;
                bx.margin_bottom = parent.px * 0.8;
            }
            "blockquote" => {
                bx.margin_top = parent.px * 0.8;
                bx.margin_bottom = parent.px * 0.8;
                bx.indent_left = 12.0;
                bx.indent_right = 12.0;
            }
            "ul" | "ol" => {
                bx.indent_left = 26.0;
                bx.margin_top = if list.2 > 0 { 0.0 } else { parent.px * 0.5 };
                bx.margin_bottom = bx.margin_top;
                look.align = if look.align == Align::Center { Align::Left } else { look.align };
            }
            "dd" => bx.indent_left = 26.0,
            "a" => look.underline = true,
            _ => {}
        }
        // legacy attributes Anki decks still carry
        if let Some(a) = e.attr("align") {
            look.align = match a.to_ascii_lowercase().as_str() {
                "center" | "middle" => Align::Center, "right" => Align::Right, "justify" => Align::Justify, _ => Align::Left,
            };
        }
        if tag == "font" {
            if let Some(c) = e.attr("color").and_then(lightness) {
                look.color = ink(c);
            }
            if let Some(n) = e.attr("size").and_then(|s| s.trim().parse::<i32>().ok()) {
                look.px = [10.0, 13.0, 16.0, 18.0, 24.0, 32.0, 48.0][(n - 1).clamp(0, 6) as usize];
            }
            if let Some(f) = e.attr("face").and_then(family_sans) {
                look.sans = f;
            }
        }
        let anc: Vec<&El> = ancestors.iter().map(|p| unsafe { &**p }).collect();
        for r in self.rules.iter().filter(|r| rule_applies(r, e, &anc)) {
            apply(&mut look, &mut bx, &r.decls, &parent, width);
        }
        if let Some(st) = e.attr("style") {
            apply(&mut look, &mut bx, &decls(st), &parent, width);
        }
        if bx.hidden {
            return;
        }
        let block = bx.block.unwrap_or(BLOCKS.contains(&tag));
        // An inline element's background highlights its text; a block's fills a band.
        if block {
            look.mark = false;
        } else {
            bx.band = false;
        }
        match tag {
            "br" => {
                if self.runs.is_empty() {
                    // an empty line still takes its height
                    let h = style_of(&look).line_h;
                    let band = self.band.then_some((self.left, self.avail()));
                    let bars = self.bars.clone();
                    self.push_item(Item::Gap { h, band, bars });
                } else {
                    self.flush();
                }
                return;
            }
            "wbr" => return,
            "hr" => {
                self.flush();
                self.gap((look.px * 0.3 * PX) as i32);
                let (x, w) = (self.left, self.avail());
                self.push_item(Item::Rule { x, w });
                self.gap((look.px * 0.3 * PX) as i32);
                return;
            }
            "img" => {
                let Some(mut c) = image(e.attr("src").unwrap_or("")) else { return };
                // width/height attributes or CSS, in CSS px; the stored picture only shrinks
                let attr_px = |k: &str| e.attr(k).and_then(|v| v.trim_end_matches("px").parse::<f32>().ok());
                let want_w = bx.width.or_else(|| attr_px("width")).map(|w| (w * PX) as i32);
                let want_h = bx.height.or_else(|| attr_px("height")).map(|h| (h * PX) as i32);
                let max_w = bx.max_width.map(|w| (w * PX) as i32).unwrap_or(i32::MAX).min(self.avail());
                let mut target_w = want_w.unwrap_or(c.w).min(max_w);
                if let (None, Some(h)) = (want_w, want_h) {
                    target_w = (c.w * h / c.h.max(1)).min(max_w);
                }
                if target_w < c.w {
                    c = scale_down(&c, target_w);
                }
                let line_h = style_of(&look).line_h;
                if c.h <= line_h * 3 / 2 && !bx.block.unwrap_or(false) {
                    // small pictures (icons, stars) flow with the text
                    self.runs.push(Run { text: String::new(), look, style: style_of(&look), space: false, ruby: None, image: Some(c), marker: false });
                } else {
                    self.flush();
                    // floated: to its side (text does not flow around it on this narrow card)
                    let (x, avail) = (self.left, self.avail());
                    let right = bx.float == Some(true) || (bx.float.is_none() && look.align == Align::Right);
                    let centre = bx.float.is_none() && look.align == Align::Center;
                    self.push_item(Item::Image { canvas: c, x, avail, centre, right });
                }
                return;
            }
            "ruby" => {
                let mut base = String::new();
                let mut reading = String::new();
                collect_ruby(e, &mut base, &mut reading, false);
                let st = style_of(&look);
                self.runs.push(Run { text: base, look, style: st, space: false, ruby: (!reading.is_empty()).then_some(reading), image: None, marker: false });
                return;
            }
            "table" => {
                self.flush();
                self.gap((look.px * 0.5 * PX) as i32);
                self.table(e, look, ancestors);
                self.gap((look.px * 0.5 * PX) as i32);
                return;
            }
            _ => {}
        }
        if block && bx.flex && !bx.column {
            self.flush();
            self.gap((bx.margin_top * PX) as i32);
            let saved = (self.align, self.left, self.right);
            self.left += (bx.indent_left * PX) as i32;
            self.right += (bx.indent_right * PX) as i32;
            self.align = look.align;
            self.flex_row(e, look, ancestors);
            (self.align, self.left, self.right) = saved;
            self.gap((bx.margin_bottom * PX) as i32);
            return;
        }
        let saved = (self.left, self.right, self.align, self.band, self.bars.len());
        if block {
            self.flush();
            self.gap((bx.margin_top * PX) as i32);
            if bx.border_top {
                let (x, w) = (self.left, self.avail());
                self.push_item(Item::Rule { x, w });
            }
            if bx.border_left {
                self.bars.push(self.left);
                self.left += 14;
            }
            self.left += (bx.indent_left * PX) as i32;
            self.right += (bx.indent_right * PX) as i32;
            if let Some(w) = bx.width.or(bx.max_width) {
                // a narrower block: centred when its text is
                let spare = self.avail() - (w * PX) as i32;
                if spare > 0 {
                    if look.align == Align::Center {
                        self.left += spare / 2;
                        self.right += spare - spare / 2;
                    } else {
                        self.right += spare;
                    }
                }
            }
            self.align = look.align;
            self.band = self.band || bx.band;
            self.first_indent = (bx.text_indent * PX) as i32;
        }
        let mut inner = *list;
        if tag == "ul" || tag == "ol" {
            inner = (tag == "ol", e.attr("start").and_then(|s| s.parse().ok()).unwrap_or(1), list.2 + 1);
        }
        if tag == "li" {
            let marker = if list.0 {
                let m = format!("{}.", list.1);
                list.1 += 1;
                m
            } else {
                String::from(match list.2 { 0 | 1 => "•", 2 => "–", _ => "·" })
            };
            let ml = Look { bold: false, italic: false, underline: false, strike: false, mark: false, pos: Pos::Normal, ..look };
            self.runs.push(Run { text: marker, look: ml, style: style_of(&ml), space: false, ruby: None, image: None, marker: true });
        }
        ancestors.push(e as *const El);
        for k in &e.kids {
            if self.past_want() {
                break;
            }
            match k {
                Node::El(c) => self.walk(c, look, ancestors, &mut inner),
                Node::Text(t) => self.text(t, &look),
            }
        }
        ancestors.pop();
        if block {
            self.flush();
            if bx.border_bottom {
                let (x, w) = (self.left, self.avail());
                self.push_item(Item::Rule { x, w });
            }
            (self.left, self.right, self.align, self.band) = (saved.0, saved.1, saved.2, saved.3);
            self.bars.truncate(saved.4);
            self.gap((bx.margin_bottom * PX) as i32);
        }
    }
    /// A table: columns sized to their content, each cell laid out on its own.
    fn table(&mut self, t: &El, look: Look, ancestors: &mut Vec<*const El>) {
        let mut rows: Vec<&El> = Vec::new();
        collect_rows(t, &mut rows);
        let ruled = t.attr("border").is_some_and(|b| b != "0")
            || self.rules.iter().any(|r| r.chain.last().is_some_and(|c| matches!(c.tag.as_str(), "td" | "th" | "table"))
                && r.decls.iter().any(|d| d.0.starts_with("border") && !d.1.starts_with("none")));
        let grid: Vec<Vec<&El>> = rows.iter().map(|r| cells(r)).collect();
        self.grid(grid, look, ancestors, ruled);
    }
    /// A flex row: its children side by side, as one row of a table without rules.
    fn flex_row(&mut self, e: &El, look: Look, ancestors: &mut Vec<*const El>) {
        let kids: Vec<&El> = e.kids.iter().filter_map(|k| match k { Node::El(c) => Some(&**c), _ => None }).collect();
        if kids.is_empty() {
            return;
        }
        ancestors.push(e as *const El);
        self.grid(vec![kids], look, ancestors, false);
        ancestors.pop();
    }
    /// Rows of cells: columns sized to their content, each cell laid out on its own.
    fn grid(&mut self, rows: Vec<Vec<&El>>, look: Look, ancestors: &mut Vec<*const El>, ruled: bool) {
        let cols = rows.iter().map(|r| r.len()).max().unwrap_or(0);
        if cols == 0 {
            return;
        }
        let avail = self.avail();
        let st = style_of(&look);
        let mut min_w = vec![0i32; cols];
        let mut max_w = vec![0i32; cols];
        for r in &rows {
            for (c, cell) in r.iter().enumerate() {
                let text = plain_text(cell);
                let longest = text.split_whitespace().map(|w| gfx::text_width(st, w)).max().unwrap_or(0);
                min_w[c] = min_w[c].max(longest.min(avail / cols as i32));
                max_w[c] = max_w[c].max(gfx::text_width(st, text.trim()) + 4);
            }
        }
        let gap = 18;
        let room = avail - gap * (cols as i32 - 1);
        let total_max: i32 = max_w.iter().sum();
        let widths: Vec<i32> = if total_max <= room {
            max_w.clone()
        } else {
            let total_min: i32 = min_w.iter().sum();
            let spare = (room - total_min).max(0);
            let flex: i32 = max_w.iter().zip(&min_w).map(|(a, b)| a - b).sum::<i32>().max(1);
            min_w.iter().zip(&max_w).map(|(mn, mx)| mn + spare * (mx - mn) / flex).collect()
        };
        let table_w: i32 = widths.iter().sum::<i32>() + gap * (cols as i32 - 1);
        let x0 = self.left + if self.align == Align::Center { (avail - table_w).max(0) / 2 } else { 0 };
        if ruled {
            self.push_item(Item::Rule { x: x0, w: table_w });
        }
        for r in rows {
            let mut x = x0;
            let mut out = Vec::new();
            let mut height = 0;
            for (c, cell) in r.iter().enumerate().take(cols) {
                let w = widths[c];
                let mut sub = Builder { paging: None, rules: self.rules, items: Vec::new(), runs: Vec::new(), left: 0, right: 0, width: w,
                                        align: if cell.tag == "th" { Align::Center } else { Align::Left }, band: false,
                                        first_indent: 0, lang: self.lang, pending_gap: 0, bars: Vec::new() };
                let mut l2 = (false, 1, 0);
                let cl = Look { bold: look.bold || cell.tag == "th", align: sub.align, ..look };
                sub.walk(cell, cl, ancestors, &mut l2);
                sub.flush();
                let h: i32 = sub.items.iter().map(|i| i.height()).sum();
                height = height.max(h);
                out.push((x - self.left, w, sub.items));
                x += w + gap;
            }
            let _ = x;
            self.push_item(Item::Row { cells: out.into_iter().map(|(cx, w, i)| (cx + self.left, w, i)).collect(), height: height + 6 });
            if ruled {
                self.push_item(Item::Rule { x: x0, w: table_w });
            }
        }
    }
}
const BLOCKS: [&str; 26] = ["div", "p", "li", "ul", "ol", "h1", "h2", "h3", "h4", "h5", "h6", "tr", "blockquote", "section",
                            "center", "article", "header", "footer", "dl", "dd", "dt", "body", "pre", "figure", "figcaption", "main"];
/// The order runs are drawn in, left to right (UAX#9 at the level of runs: each run's own text is
/// put in visual order by rtl::visual). A run is right to left with Hebrew or Arabic in it, left to
/// right with other letters; spaces, digits and marks take the direction around them.
fn visual_order(runs: &[Run], para_rtl: bool) -> Vec<usize> {
    let strong: Vec<Option<bool>> = runs.iter().map(|r| {
        if r.image.is_some() {
            Some(para_rtl)
        } else if crate::rtl::has_rtl(&r.text) {
            Some(true)
        } else if r.text.chars().any(|c| c.is_alphabetic()) {
            Some(false)
        } else {
            None
        }
    }).collect();
    let dir: Vec<bool> = (0..runs.len()).map(|i| {
        strong[i].unwrap_or_else(|| {
            let before = strong[..i].iter().rev().find_map(|d| *d).unwrap_or(para_rtl);
            let after = strong[i + 1..].iter().find_map(|d| *d).unwrap_or(para_rtl);
            if before == after { before } else { para_rtl }
        })
    }).collect();
    let mut order: Vec<usize> = (0..runs.len()).collect();
    if para_rtl {
        order.reverse();
    }
    // reverse each stretch running against the paragraph
    let mut i = 0;
    while i < order.len() {
        if dir[order[i]] != para_rtl {
            let mut j = i;
            while j < order.len() && dir[order[j]] != para_rtl {
                j += 1;
            }
            order[i..j].reverse();
            i = j;
        } else {
            i += 1;
        }
    }
    order
}
fn trim(line: &mut Vec<Run>) {
    while line.last().is_some_and(|r| r.space && !r.look.pre) {
        line.pop();
    }
}
fn collect_rows<'a>(e: &'a El, out: &mut Vec<&'a El>) {
    for k in &e.kids {
        if let Node::El(c) = k {
            if c.tag == "tr" {
                out.push(c);
            } else if matches!(c.tag.as_str(), "tbody" | "thead" | "tfoot") {
                collect_rows(c, out);
            }
        }
    }
}
fn cells(r: &El) -> Vec<&El> {
    r.kids.iter().filter_map(|k| match k { Node::El(c) if c.tag == "td" || c.tag == "th" => Some(&**c), _ => None }).collect()
}
fn plain_text(e: &El) -> String {
    let mut s = String::new();
    for k in &e.kids {
        match k {
            Node::Text(t) => s.push_str(t),
            Node::El(c) if c.tag == "rt" || c.tag == "rp" => {}
            Node::El(c) => s.push_str(&plain_text(c)),
        }
    }
    s
}
fn collect_ruby(e: &El, base: &mut String, reading: &mut String, in_rt: bool) {
    for k in &e.kids {
        match k {
            Node::Text(t) => {
                if in_rt {
                    reading.push_str(t.trim())
                } else {
                    base.push_str(t.trim())
                }
            }
            Node::El(c) if c.tag == "rp" => {}
            Node::El(c) => collect_ruby(c, base, reading, in_rt || c.tag == "rt"),
        }
    }
}
fn clone_canvas(c: &Canvas) -> Canvas { Canvas { w: c.w, h: c.h, stride: c.stride, b: c.b.clone() } }
/// A picture as stored by the sync (1-bit, at most the card's width), or None when missing.
fn image(src: &str) -> Option<Canvas> {
    let b = store::read_file(&sync::image_file(src))?;
    if b.len() < 4 {
        return None;
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
    Some(c)
}
/// Shrinks a dithered picture by averaging, then dithers it again so its tones keep.
fn scale_down(c: &Canvas, w: i32) -> Canvas {
    let w = w.max(1);
    let h = (c.h * w / c.w.max(1)).max(1);
    let mut out = Canvas::new(w, h);
    const BAYER: [[i32; 4]; 4] = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]];
    for y in 0..h {
        for x in 0..w {
            let (x0, x1) = (x * c.w / w, ((x + 1) * c.w / w).max(x * c.w / w + 1));
            let (y0, y1) = (y * c.h / h, ((y + 1) * c.h / h).max(y * c.h / h + 1));
            let mut ink = 0;
            for yy in y0..y1 {
                for xx in x0..x1 {
                    ink += c.get(xx, yy) as i32;
                }
            }
            let frac = ink * 16 / ((x1 - x0) * (y1 - y0)).max(1);
            if frac > BAYER[(y & 3) as usize][(x & 3) as usize] {
                out.set(x, y);
            }
        }
    }
    out
}

/// A language code as the hyphenation patterns name it (they are static); English when unknown.
pub fn lang_static(code: &str) -> &'static str {
    const KNOWN: [&str; 11] = ["de", "en", "es", "fi", "fr", "it", "pl", "pt", "ru", "sv", "uk"];
    KNOWN.iter().find(|k| **k == code).copied().unwrap_or("en")
}

// ---------------------------------------------------------------- pages
/// One page of a card side for a body `width` x `height`, and how many pages the side has so
/// far: the layout stops once page `want` is complete, so `more` says whether pages follow.
/// `css` is the note type's; `deck_lang` is the deck's language, for a card too short to tell
/// its own. Only page `want` is drawn and kept.
pub fn layout(html: &str, css: &str, width: i32, height: i32, deck_lang: &str, want: usize) -> (Vec<Item>, usize, bool) {
    let root = parse(html);
    unsafe { CSS_WIDTH = width as f32 / PX };
    let rules = parse_css(css);
    let lang = crate::hyph::guess_lang_sure(&plain_text(&root)).unwrap_or_else(|| lang_static(deck_lang));
    let paging = Paging { height, want, page: 0, used: 0, started: false };
    let mut b = Builder { rules: &rules, items: Vec::new(), runs: Vec::new(), left: 0, right: 0, width,
                          align: Align::Center, band: false, first_indent: 0, lang, pending_gap: 0, bars: Vec::new(),
                          paging: Some(paging) };
    // Anki's default card: 20 px, centred, black.
    let look = Look { px: 20.0, sans: false, bold: false, italic: false, underline: false, strike: false, color: dsn::BLACK,
                      mark: false, align: Align::Center, line: 1.0, pos: Pos::Normal, upper: 0, pre: false };
    let mut anc = Vec::new();
    let mut list = (false, 1, 0);
    b.walk(&root, look, &mut anc, &mut list);
    b.flush();
    gfx::forget_spine();
    let p = b.paging.take().unwrap_or(Paging { height, want, page: 0, used: 0, started: false });
    let mut items = b.items;
    while items.last().is_some_and(|i| matches!(i, Item::Gap { .. })) {
        items.pop();
    }
    let more = p.page > want;
    (items, p.page.min(want) + 1, more)
}

/// Draws a page with its top-left at (x, y).
pub fn draw(page: &[Item], x: i32, y: i32, _width: i32) {
    let mut y = y;
    for it in page {
        draw_item(it, x, y);
        y += it.height();
    }
}
fn draw_item(it: &Item, x: i32, y: i32) {
    match it {
        Item::Line { pieces, marks, x: lx, width: lw, avail, height, align_right, centre, band, bars } => {
            for b in bars {
                dsn::fill(x + b, y, 3, *height, dsn::DARK_GRAY);
            }
            if let Some((bx, bw)) = band {
                dsn::fill(x + bx - 6, y, bw + 12, *height, dsn::LIGHT_GRAY);
            }
            let ox = x + lx + if *centre { (avail - lw) / 2 } else if *align_right { avail - lw } else { 0 };
            for &(mx, my, mw, mh) in marks {
                dsn::fill(ox + mx - 2, y + my, mw + 4, mh, dsn::LIGHT_GRAY);
            }
            for p in pieces {
                gfx::blit(&p.canvas, ox + p.x, y + p.y, p.color);
            }
        }
        Item::Rule { x: rx, w } => dsn::fill(x + rx, y + 10, *w, 1, dsn::BLACK),
        Item::Image { canvas, x: ix, avail, centre, right } => {
            let ox = x + ix + if *centre { (avail - canvas.w) / 2 } else if *right { avail - canvas.w } else { 0 };
            gfx::blit(canvas, ox, y + 6, dsn::BLACK);
        }
        Item::Gap { h, band, bars } => {
            for b in bars {
                dsn::fill(x + b, y, 3, *h, dsn::DARK_GRAY);
            }
            if let Some((bx, bw)) = band {
                dsn::fill(x + bx - 6, y, bw + 12, *h, dsn::LIGHT_GRAY);
            }
        }
        Item::Row { cells, .. } => {
            for (cx, _cw, items) in cells {
                let mut cy = y + 3;
                for i in items {
                    draw_item(i, x + cx, cy);
                    cy += i.height();
                }
            }
        }
    }
}
