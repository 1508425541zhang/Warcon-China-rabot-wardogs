//! Minimal edits to Unreal INI documents; credentials never leave through document actions.
use crate::error::{ApiError, Result};
use serde_json::Value;
pub const HIDDEN: &str = "(hidden)";
pub const SESSION: &str = "/Script/WDGame.WDGameSession";
pub const RESERVED: &str = "DefaultReservedPlayerIds";
fn split(text: &str) -> (Vec<String>, &str) {
    let eol = if text.contains("\r\n") { "\r\n" } else { "\n" };
    (
        text.replace("\r\n", "\n")
            .replace('\r', "\n")
            .split('\n')
            .map(str::to_owned)
            .collect(),
        eol,
    )
}
fn header(line: &str) -> Option<&str> {
    let line = line.trim_start_matches('\u{feff}').trim();
    if line.starts_with('[') && line.ends_with(']') {
        Some(line[1..line.len() - 1].trim())
    } else {
        None
    }
}
fn comment(line: &str) -> bool {
    line.trim().starts_with([';', '#'])
}
fn command(key: &str) -> (char, &str) {
    let key = key.trim();
    match key.chars().next() {
        Some(c) if ['+', '.', '-', '!'].contains(&c) => (c, key[c.len_utf8()..].trim()),
        _ => ('=', key),
    }
}
fn unquote(s: &str) -> &str {
    if s.len() >= 2 && s.starts_with('"') && s.ends_with('"') {
        &s[1..s.len() - 1]
    } else {
        s
    }
}
pub fn array(text: &str, section: &str, key: &str) -> Vec<String> {
    let (lines, _) = split(text);
    let mut inside = false;
    let mut values = vec![];
    for raw in lines {
        let line = raw.trim();
        if let Some(h) = header(line) {
            inside = h.eq_ignore_ascii_case(section);
            continue;
        }
        if !inside || comment(line) {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        let (op, k) = command(k);
        if !k.eq_ignore_ascii_case(key) {
            continue;
        }
        let v = v.trim().to_owned();
        match op {
            '!' => values.clear(),
            '+' => {
                if !values.contains(&v) {
                    values.push(v)
                }
            }
            '.' => values.push(v),
            '-' => values.retain(|x| x != &v),
            _ => values = vec![v],
        }
    }
    values.into_iter().map(|v| unquote(&v).to_owned()).collect()
}
fn quote(value: &str) -> String {
    if value.is_empty() || (value.starts_with('"') && value.ends_with('"')) {
        return value.into();
    }
    let mut quoted = false;
    let bytes = value.as_bytes();
    let mut slash = false;
    for i in 0..bytes.len().saturating_sub(1) {
        if bytes[i] == b'"' {
            quoted = !quoted
        } else if !quoted && bytes[i] == b'/' && bytes[i + 1] == b'/' {
            slash = true
        }
    }
    if slash || value.starts_with(' ') || value.ends_with([' ', '\\']) {
        format!("\"{value}\"")
    } else {
        value.into()
    }
}
pub fn set_array(text: &str, section: &str, key: &str, values: &[String]) -> String {
    let (mut lines, eol) = split(text);
    if text.is_empty() {
        lines.clear()
    }
    let mut block = vec![format!("!{key}=ClearArray")];
    block.extend(values.iter().map(|v| format!(".{key}={}", quote(v))));
    let start = lines
        .iter()
        .position(|l| header(l).is_some_and(|h| h.eq_ignore_ascii_case(section)));
    let Some(start) = start else {
        while lines.last().is_some_and(|l| l.trim().is_empty()) {
            lines.pop();
        }
        if !lines.is_empty() {
            lines.push(String::new())
        }
        lines.push(format!("[{section}]"));
        lines.extend(block);
        lines.push(String::new());
        return lines.join(eol);
    };
    let end = lines
        .iter()
        .enumerate()
        .skip(start + 1)
        .find(|(_, l)| header(l).is_some())
        .map(|(i, _)| i)
        .unwrap_or(lines.len());
    let mut at = None;
    let mut kept = vec![];
    for (i, line) in lines.into_iter().enumerate() {
        let ours = i > start
            && i < end
            && !comment(&line)
            && line
                .trim()
                .split_once('=')
                .is_some_and(|(k, _)| command(k).1.eq_ignore_ascii_case(key));
        if ours {
            if at.is_none() {
                at = Some(kept.len())
            }
        } else {
            kept.push(line)
        }
    }
    let at = at.unwrap_or_else(|| {
        let mut at = end;
        while at > start + 1 && kept[at - 1].trim().is_empty() {
            at -= 1
        }
        at
    });
    kept.splice(at..at, block);
    kept.join(eol)
}
struct Secret {
    index: usize,
    slot: (String, String, bool),
    head: String,
    value: String,
}
fn secret_lines(lines: &[String]) -> Vec<Secret> {
    let mut section = String::new();
    let mut result = vec![];
    for (index, raw) in lines.iter().enumerate() {
        if let Some(h) = header(raw) {
            section = h.to_lowercase();
            continue;
        }
        let Some(eq) = raw.find('=') else { continue };
        let key = raw[..eq].trim_start_matches(|c: char| c.is_whitespace() || c == ';' || c == '#');
        let (_, key) = command(key);
        let key = key.to_lowercase();
        if !["password", "passwordhash", "token"].contains(&key.as_str()) {
            continue;
        }
        result.push(Secret {
            index,
            slot: (section.clone(), key, comment(raw)),
            head: raw[..=eq].into(),
            value: raw[eq + 1..].trim().into(),
        })
    }
    result
}
pub fn redact(text: &str) -> String {
    let (mut lines, eol) = split(text);
    let found = secret_lines(&lines);
    if found.iter().all(|s| s.value.is_empty()) {
        return text.into();
    }
    for s in found {
        if !s.value.is_empty() {
            lines[s.index] = format!("{}{HIDDEN}", s.head)
        }
    }
    lines.join(eol)
}
pub fn restore(text: &str, live: &str) -> Result<String> {
    let (mut lines, eol) = split(text);
    let held = secret_lines(&lines);
    if !held.iter().any(|s| s.value == HIDDEN) {
        return Ok(text.into());
    }
    let real = secret_lines(&split(live).0);
    let mut taken = std::collections::HashMap::new();
    for s in held.into_iter().filter(|s| s.value == HIDDEN) {
        let nth = taken.entry(s.slot.clone()).or_insert(0);
        let source=real.iter().filter(|r|r.slot==s.slot).nth(*nth).ok_or_else(||ApiError::new(axum::http::StatusCode::BAD_REQUEST,"hidden_value","A hidden credential has no matching live value. Type a value or remove that line."))?;
        *nth += 1;
        lines[s.index] = format!("{}{}", s.head, source.value)
    }
    Ok(lines.join(eol))
}
pub fn hide(mut value: Value, texts: &[&str]) -> Value {
    let mut secrets = vec![];
    for text in texts {
        for s in secret_lines(&split(text).0) {
            for v in [&s.value, unquote(&s.value)] {
                if !v.is_empty() && v != HIDDEN && !secrets.iter().any(|x: &String| x == v) {
                    secrets.push(v.to_owned())
                }
            }
        }
    }
    secrets.sort_by_key(|s| std::cmp::Reverse(s.len()));
    fn walk(v: &mut Value, secrets: &[String]) {
        match v {
            Value::String(s) => {
                for secret in secrets {
                    *s = s.replace(secret, HIDDEN)
                }
            }
            Value::Array(v) => {
                for v in v {
                    walk(v, secrets)
                }
            }
            Value::Object(v) => {
                for v in v.values_mut() {
                    walk(v, secrets)
                }
            }
            _ => (),
        }
    }
    walk(&mut value, &secrets);
    value
}
pub fn reserved(text: &str) -> Vec<String> {
    array(text, SESSION, RESERVED)
        .into_iter()
        .map(|v| v.trim().to_owned())
        .filter(|s| !s.is_empty())
        .collect()
}
