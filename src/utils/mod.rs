use once_cell::sync::Lazy;
use regex::Regex;
use std::time::Duration;

pub mod launcher;

#[derive(Debug, Clone, Copy)]
pub enum IndicatorState {
    Normal,
    Success,
    Warning,
    Danger,
}

pub fn format_duration(duration: &Duration) -> String {
    let h = duration.as_secs() / 60 / 60;
    let m = duration.as_secs() / 60 % 60;
    if h > 0 {
        format!("{h}h {m:>2}m")
    } else {
        format!("{m:>2}m")
    }
}

/// Truncate a string to at most `max_chars` characters (not bytes),
/// avoiding panics on multi-byte UTF-8.
pub fn truncate_chars(s: &str, max_chars: usize) -> &str {
    match s.char_indices().nth(max_chars) {
        Some((byte_idx, _)) => &s[..byte_idx],
        None => s,
    }
}

static STRIP_TAGS_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"<[^>]+>").unwrap());

/// Strip HTML/markup tags from notification body text.
/// Converts `<br>` / `<br/>` to newlines, removes all other tags,
/// and decodes basic HTML entities.
pub fn strip_markup_tags(s: &str) -> String {
    // Convert <br> variants to newlines before stripping
    let s = s.replace("<br>", "\n").replace("<br/>", "\n").replace("<br />", "\n");
    let stripped = STRIP_TAGS_RE.replace_all(&s, "");
    stripped
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
}

pub fn truncate_text(value: &str, max_length: u32) -> String {
    let length = value.len();

    if length > max_length as usize {
        let split = max_length as usize / 2;
        let first_part = value.chars().take(split).collect::<String>();
        let last_part = value.chars().skip(length - split).collect::<String>();
        format!("{first_part}...{last_part}")
    } else {
        value.to_string()
    }
}
