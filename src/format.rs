//! Display helpers shared by the desktop shells.
//!
//! Time strings are local. URL checks only accept `http` and `https`, which is
//! what both shells open in the system browser.

use chrono::{DateTime, Datelike, Local};

/// Chat-list clock: `HH:MM` today, `DD Mon` this year, otherwise `YYYY-MM-DD`.
pub fn list_time(unix: i64) -> String {
    let Some(when) = local_time(unix) else {
        return String::new();
    };
    let now = Local::now();
    let pattern = if when.date_naive() == now.date_naive() {
        "%H:%M"
    } else if when.year() == now.year() {
        "%d %b"
    } else {
        "%Y-%m-%d"
    };
    when.format(pattern).to_string()
}

/// Message header time, `YYYY-MM-DD HH:MM` in the local zone.
pub fn message_time(unix: i64) -> String {
    local_time(unix)
        .map(|when| when.format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_else(|| unix.to_string())
}

fn local_time(unix: i64) -> Option<DateTime<Local>> {
    DateTime::from_timestamp(unix, 0).map(|when| when.with_timezone(&Local))
}

/// `true` when `url` is an `http` or `https` URL with no whitespace.
pub fn is_http_url(url: &str) -> bool {
    let bytes = url.as_bytes();
    let https = bytes.len() > 8 && bytes[..8].eq_ignore_ascii_case(b"https://");
    let http = bytes.len() > 7 && bytes[..7].eq_ignore_ascii_case(b"http://");
    (https || http) && !url.chars().any(|ch| ch.is_whitespace())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn http_urls_reject_other_schemes_and_spaces() {
        assert!(is_http_url("https://example.com/mithka"));
        assert!(is_http_url("http://example.com"));
        assert!(is_http_url("HTTPS://example.com/a"));
        assert!(!is_http_url("https://example.com/a b"));
        assert!(!is_http_url("ftp://example.com"));
        assert!(!is_http_url("javascript:alert(1)"));
        assert!(!is_http_url("tg://resolve"));
        assert!(!is_http_url(""));
    }

    #[test]
    fn list_time_is_a_clock_today_and_a_date_in_2023() {
        let today = list_time(Local::now().timestamp());
        assert_eq!(today.len(), 5);
        assert!(today.as_bytes()[2] == b':');

        let archived = list_time(1_700_000_000);
        assert!(
            archived.contains('-'),
            "a 2023 timestamp should use a numeric date, got {archived}"
        );
        let full = message_time(1_700_000_000);
        assert!(full.contains(' ') && full.contains(':'), "{full}");
    }
}
