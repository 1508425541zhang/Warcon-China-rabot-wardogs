use chrono::{DateTime, Utc};
use serde::Deserialize;
use std::collections::HashMap;
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Facts {
    pub entry_id: String,
    pub reason: String,
    pub added_by_name: String,
    pub added_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
}
pub fn uid(id: &str) -> String {
    format!(
        "B-{}",
        id.chars()
            .filter(char::is_ascii_alphanumeric)
            .take(6)
            .collect::<String>()
            .to_ascii_uppercase()
    )
}
pub fn render(template: &str, f: &Facts) -> String {
    let stamp = |d: DateTime<Utc>| d.format("%-d %b %Y %H:%M UTC").to_string();
    let duration = if let Some(to) = f.expires_at {
        let m = ((to - f.added_at).num_milliseconds() as f64 / 60000.)
            .round()
            .max(1.) as i64;
        if m % 1440 == 0 {
            format!("{}d", m / 1440)
        } else if m >= 60 {
            format!("{}h", (m as f64 / 60.).round() as i64)
        } else {
            format!("{m}m")
        }
    } else {
        "Perm".into()
    };
    let vars = HashMap::from([
        ("reason", f.reason.clone()),
        ("duration", duration),
        (
            "expires",
            f.expires_at.map(stamp).unwrap_or_else(|| "never".into()),
        ),
        ("banned", stamp(f.added_at)),
        ("uid", uid(&f.entry_id)),
        ("admin", f.added_by_name.clone()),
    ]);
    let mut input = if template.is_empty() {
        "{reason}"
    } else {
        template
    };
    let mut output = String::new();
    while let Some(start) = input.find('{') {
        output.push_str(&input[..start]);
        input = &input[start..];
        if let Some(end) = input.find('}') {
            let key = &input[1..end];
            if !key.is_empty() && key.bytes().all(|b| b.is_ascii_alphabetic() || b == b'_') {
                if let Some(value) = vars.get(key.to_ascii_lowercase().as_str()) {
                    output.push_str(value)
                } else {
                    output.push_str(&input[..=end])
                }
                input = &input[end + 1..];
                continue;
            }
        }
        output.push('{');
        input = &input[1..];
    }
    output.push_str(input);
    crate::feed::truncate(
        output.trim_matches(|c: char| c.is_whitespace() || "|·:,;-".contains(c)),
        200,
    )
}
