//! Unicode nickname policies and bounded, linear word matching.
use crate::{
    error::{ApiError, Result},
    http::{integer, string, truthy},
};
use regex::Regex;
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
};
use unicode_normalization::UnicodeNormalization;
pub const SCRIPTS: [&str; 10] = [
    "Cyrillic",
    "Greek",
    "Arabic",
    "Hebrew",
    "Thai",
    "Devanagari",
    "Han",
    "Hiragana",
    "Katakana",
    "Hangul",
];
/// Bounded compiled-expression cache. Regex execution remains linear and arbitrary
/// nickname input never becomes a regex source.
pub fn cached_regex(source: &str) -> std::result::Result<Regex, regex::Error> {
    static CACHE: OnceLock<Mutex<HashMap<String, Regex>>> = OnceLock::new();
    let mut cache = CACHE
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap();
    if let Some(re) = cache.get(source) {
        return Ok(re.clone());
    }
    let re = Regex::new(source)?;
    if cache.len() >= 512 {
        cache.clear();
    }
    cache.insert(source.to_owned(), re.clone());
    Ok(re)
}
fn ln() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"^[\pL\pN]$").unwrap())
}
fn words(v: &Value) -> Result<Vec<String>> {
    let list = v.as_array().cloned().unwrap_or_else(|| {
        string(v, usize::MAX)
            .split(['\n', ','])
            .map(|s| json!(s))
            .collect()
    });
    let rule = cached_regex(r"^[\pL\pN][\pL\pN ]{0,38}[\pL\pN]$").unwrap();
    let mut out = vec![];
    for w in list {
        let w = string(&w, 60)
            .nfkc()
            .collect::<String>()
            .to_lowercase()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        if w.is_empty() {
            continue;
        }
        if !rule.is_match(&w) {
            return Err(ApiError::bad("词条必须为2～40个字母、数字或空格。"));
        }
        if !out.contains(&w) {
            out.push(w)
        }
    }
    if out.len() > 200 {
        return Err(ApiError::bad("词条最多200项。"));
    }
    Ok(out)
}
pub fn validate(c: &Value) -> Result<Value> {
    let chars = match c["characters"].as_str() {
        Some("ascii") => "ascii",
        Some("latin") => "latin",
        _ => "off",
    };
    let scripts: Vec<_> = SCRIPTS
        .into_iter()
        .filter(|s| {
            chars == "latin"
                && c["extraScripts"]
                    .as_array()
                    .is_some_and(|v| v.contains(&json!(s)))
        })
        .collect();
    let blocked = words(&c["blocked"])?;
    let allowed = words(&c["allowed"])?;
    let min = integer(&c["minLetters"], 0, 0, 10);
    let built = truthy(&c["builtinWords"]);
    if chars == "off" && min == 0 && !built && blocked.is_empty() {
        return Err(ApiError::bad("请选择字符规则或词库。"));
    }
    let reason = string(&c["reason"], 200);
    Ok(
        json!({"characters":chars,"extraScripts":scripts,"allowSymbols":truthy(&c["allowSymbols"]),"minLetters":min,"builtinWords":built,"blocked":blocked,"allowed":allowed,"action":if c["action"]=="alert"{"alert"}else{"kick"},"spareReserved":c.get("spareReserved").is_none_or(truthy),"reason":if reason.is_empty(){"Your name is not allowed on this server: {why}.".into()}else{reason}}),
    )
}
fn look(c: char) -> String {
    match c {
        'а' | 'α' => 'a',
        'в' | 'β' => 'b',
        'е' | 'ё' | 'ε' => 'e',
        'к' | 'κ' => 'k',
        'м' => 'm',
        'н' => 'h',
        'о' | 'ο' => 'o',
        'р' | 'ρ' => 'p',
        'с' => 'c',
        'т' | 'τ' => 't',
        'у' | 'υ' => 'y',
        'х' | 'χ' => 'x',
        'і' | 'ї' | 'ι' => 'i',
        'ј' => 'j',
        'ѕ' => 's',
        'ԁ' | 'đ' => 'd',
        'ɡ' => 'g',
        'η' => 'n',
        'ν' => 'v',
        'ø' => 'o',
        'ł' => 'l',
        _ => {
            return match c {
                'ß' => "ss",
                'æ' => "ae",
                'œ' => "oe",
                _ => return c.to_string(),
            }
            .into();
        }
    }
    .to_string()
}
fn leet(s: &str) -> &str {
    match s {
        "4" | "@" => "a",
        "8" => "b",
        "3" => "e",
        "9" => "g",
        "1" | "!" | "|" | "l" => "i",
        "0" => "o",
        "5" | "$" => "s",
        "7" | "+" => "t",
        "2" => "z",
        _ => s,
    }
}
pub fn fold(s: &str, use_leet: bool) -> String {
    let plain = crate::feed::truncate(s, 100)
        .nfkc()
        .collect::<String>()
        .to_lowercase()
        .nfd()
        .collect::<String>();
    let marks = cached_regex(r"\pM").unwrap();
    let plain = marks.replace_all(&plain, "");
    let mut out = String::new();
    for c in plain.chars() {
        let ch = look(c);
        out.push_str(if use_leet { leet(&ch) } else { &ch })
    }
    // Collapse separated singleton letters only. Long words preserve their boundaries.
    let chars: Vec<_> = out.chars().collect();
    let is_ln = |c: char| ln().is_match(&c.to_string());
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        if is_ln(chars[i])
            && (i == 0 || !is_ln(chars[i - 1]))
            && (i + 1 == chars.len() || !is_ln(chars[i + 1]))
        {
            let mut end = i;
            let mut singles = vec![chars[i]];
            loop {
                let mut j = end + 1;
                while j < chars.len() && !is_ln(chars[j]) {
                    j += 1
                }
                if j == end + 1 || j == chars.len() || (j + 1 < chars.len() && is_ln(chars[j + 1]))
                {
                    break;
                }
                singles.push(chars[j]);
                end = j
            }
            if singles.len() > 1 {
                out.extend(singles);
                i = end + 1;
                continue;
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}
pub fn pattern(word: &str) -> (bool, Regex) {
    let use_leet = !cached_regex(r"\pN").unwrap().is_match(word);
    let raw: Vec<_> = fold(word, use_leet)
        .chars()
        .filter(|c| *c == ' ' || ln().is_match(&c.to_string()))
        .collect();
    let chars: Vec<_> = raw
        .iter()
        .enumerate()
        .filter(|(i, c)| **c != ' ' || i.checked_sub(1).map(|i| raw[i]) != raw.get(i + 1).copied())
        .map(|(_, c)| *c)
        .collect();
    let mut source = String::new();
    let mut i = 0;
    while i < chars.len() {
        let mut n = 1;
        while chars.get(i + n) == chars.get(i) {
            n += 1
        }
        if chars[i] == ' ' {
            source.push_str(r"[^\pL\pN]*")
        } else {
            source.push_str(&regex::escape(&chars[i].to_string()));
            source.push_str(&if n == 1 {
                "+".into()
            } else {
                format!("{{{n},}}")
            })
        }
        i += n;
    }
    (use_leet, cached_regex(&source).unwrap())
}
pub fn verdict(cfg: &Value, raw: &str) -> Option<Value> {
    let name = raw.nfc().collect::<String>();
    let policy = cfg["characters"].as_str().unwrap_or("off");
    if policy != "off" {
        let mut source = r"^[\x20-\x7E".to_owned();
        if policy == "latin" {
            source.push_str(r"\p{Latin}");
            for script in cfg["extraScripts"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
            {
                if SCRIPTS.contains(&script) {
                    source.push_str(&format!(r"\p{{{script}}}"))
                }
            }
        }
        if cfg["allowSymbols"] == true {
            source.push_str(r"\pP\pS\p{Extended_Pictographic}\x{200D}\x{FE0F}")
        }
        source.push_str("]*$");
        let allowed = cached_regex(&source).ok()?;
        if !allowed.is_match(&name) {
            let mut odd = vec![];
            for c in name.chars() {
                let c = c.to_string();
                if !allowed.is_match(&c) && !odd.contains(&c) && odd.len() < 5 {
                    odd.push(c)
                }
            }
            return Some(
                json!({"verdict":format!("characters outside the {} policy ({})",if policy=="ascii"{"ASCII"}else{"Latin"},odd.join(" ")),"why":if policy=="ascii"{"it uses characters outside plain English letters"}else{"it uses characters outside the Latin alphabet"}}),
            );
        }
    }
    let min = cfg["minLetters"].as_i64().unwrap_or(0);
    let count = cached_regex(r"\pL").unwrap().find_iter(&name).count() as i64;
    if count < min {
        return Some(
            json!({"verdict":format!("{count} letter{} (minimum {min})",if count==1{""}else{"s"}),"why":format!("it needs at least {min} letters")}),
        );
    }
    let builtin: Vec<Value> = if cfg["builtinWords"] == true {
        serde_json::from_str(include_str!("../assets/name-filter-words.json")).unwrap()
    } else {
        vec![]
    };
    let mut exceptions: Vec<String> = builtin
        .iter()
        .flat_map(|b| b["except"].as_array().into_iter().flatten())
        .chain(cfg["allowed"].as_array().into_iter().flatten())
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect();
    exceptions.sort_by_key(|s| std::cmp::Reverse(s.encode_utf16().count()));
    let reading = |l| {
        let mut text = fold(&name, l);
        for except in &exceptions {
            text = text.replace(&fold(except, l), " ")
        }
        text
    };
    let leet_text = reading(true);
    let plain = reading(false);
    for word in builtin.iter().filter_map(|b| b["word"].as_str()).chain(
        cfg["blocked"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str),
    ) {
        let (l, p) = pattern(word);
        if p.is_match(if l { &leet_text } else { &plain }) {
            return Some(
                json!({"verdict":format!("blocked word '{word}'"),"why":"it contains a word this server does not allow"}),
            );
        }
    }
    None
}
