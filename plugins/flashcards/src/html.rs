//! A card side's HTML (as the server renders it for devices) to plain text, for this proof of
//! concept's text-only cards: block tags break lines, <hr> is the rule between question and answer,
//! <b>/<strong> stay bold, ruby readings are left out, and images are kept by name.
use alloc::{string::String, vec::Vec};

pub struct Converted {
    pub side: String,        // lines of "T" text, "R" rule, "I" image name
    pub images: Vec<String>, // image file names, in order
}

const BLOCK: [&str; 21] = ["div", "p", "li", "ul", "ol", "h1", "h2", "h3", "h4", "h5", "h6", "tr", "table", "blockquote",
                           "section", "center", "article", "header", "footer", "dl", "dd"];

fn entity(name: &str) -> Option<char> {
    Some(match name {
        "nbsp" => ' ',
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        "mdash" => '—',
        "ndash" => '–',
        "hellip" => '…',
        "middot" => '·',
        "laquo" => '«',
        "raquo" => '»',
        _ => {
            let n = name.strip_prefix('#')?;
            let v = match n.strip_prefix(['x', 'X']) {
                Some(h) => u32::from_str_radix(h, 16).ok()?,
                None => n.parse().ok()?,
            };
            return char::from_u32(v);
        }
    })
}

fn attr(tag: &str, name: &str) -> Option<String> {
    let lower = tag.to_ascii_lowercase();
    let mut from = 0;
    while let Some(i) = lower[from..].find(name) {
        let at = from + i;
        let before_ok = at == 0 || !lower.as_bytes()[at - 1].is_ascii_alphanumeric();
        let rest = tag[at + name.len()..].trim_start();
        if before_ok && rest.starts_with('=') {
            let v = rest[1..].trim_start();
            return Some(String::from(match v.chars().next() {
                Some(q @ ('"' | '\'')) => v[1..].split(q).next().unwrap_or(""),
                _ => v.split(|c: char| c.is_whitespace() || c == '>').next().unwrap_or(""),
            }));
        }
        from = at + name.len();
    }
    None
}

pub fn to_side(html: &str) -> Converted {
    let mut out = Converted { side: String::new(), images: Vec::new() };
    let mut line = String::new();
    let mut skip: Option<&str> = None; // inside rt, rp, script or style
    let flush = |line: &mut String, side: &mut String| {
        let t = line.trim();
        if !t.is_empty() && t != "\u{1}\u{2}" {
            side.push('T');
            side.push_str(t);
            side.push('\n');
        }
        line.clear();
    };
    let mut space = false;
    let mut i = 0;
    let b = html.as_bytes();
    while i < b.len() {
        if b[i] == b'<' {
            let end = html[i..].find('>').map(|e| i + e).unwrap_or(b.len() - 1);
            let tag = &html[i + 1..end];
            i = end + 1;
            if tag.starts_with('!') {
                continue; // comments and doctypes
            }
            let closing = tag.starts_with('/');
            let name: String = tag.trim_start_matches('/').chars().take_while(|c| c.is_ascii_alphanumeric())
                .collect::<String>().to_ascii_lowercase();
            if let Some(s) = skip {
                if closing && name == s {
                    skip = None;
                }
                continue;
            }
            match name.as_str() {
                "rt" | "rp" | "script" | "style" if !closing => {
                    skip = Some(match name.as_str() { "rt" => "rt", "rp" => "rp", "script" => "script", _ => "style" });
                }
                "br" => flush(&mut line, &mut out.side),
                "hr" => {
                    flush(&mut line, &mut out.side);
                    out.side.push_str("R\n");
                }
                "img" if !closing => {
                    if let Some(src) = attr(tag, "src") {
                        flush(&mut line, &mut out.side);
                        out.side.push('I');
                        out.side.push_str(&src);
                        out.side.push('\n');
                        out.images.push(src);
                    }
                }
                "b" | "strong" => line.push(if closing { '\u{2}' } else { '\u{1}' }),
                "td" | "th" if closing => line.push_str("  "),
                n if BLOCK.contains(&n) => flush(&mut line, &mut out.side),
                _ => {}
            }
            space = line.ends_with(' ');
            continue;
        }
        let c = html[i..].chars().next().unwrap();
        i += c.len_utf8();
        if skip.is_some() {
            continue;
        }
        let c = if c == '&' {
            match html[i..].find(';').filter(|&e| e <= 10).and_then(|e| entity(&html[i..i + e]).map(|ch| (ch, e))) {
                Some((ch, e)) => {
                    i += e + 1;
                    ch
                }
                None => '&',
            }
        } else {
            c
        };
        if c.is_ascii_whitespace() {
            if !space && !line.is_empty() {
                line.push(' ');
            }
            space = true;
        } else {
            line.push(c);
            space = false;
        }
    }
    flush(&mut line, &mut out.side);
    out
}
