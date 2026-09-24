//! RSS, Atom, and JSON Feed documents, via `feed-rs` 2.4.

use crate::text::html_to_text;
use feed_rs::model::{Content, Entry, Feed, Text};
use feed_rs::parser;
use std::io::Cursor;

pub const MAX_ITEMS: usize = 40;
const MAX_BODY: usize = 12_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParsedFeed {
    pub title: String,
    pub items: Vec<ParsedItem>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParsedItem {
    pub key: String,
    pub title: String,
    pub link: String,
    pub published: i64,
    pub body: String,
}

pub fn parse_document(bytes: &[u8], source_id: &str) -> Result<ParsedFeed, String> {
    let feed = parser::parse(Cursor::new(bytes)).map_err(|err| err.to_string())?;
    Ok(from_feed(feed, source_id))
}

fn from_feed(feed: Feed, source_id: &str) -> ParsedFeed {
    let mut items: Vec<ParsedItem> = feed
        .entries
        .iter()
        .map(|entry| parsed_item(source_id, entry))
        .filter(|item| !item.title.is_empty() || !item.body.is_empty())
        .collect();
    items.truncate(MAX_ITEMS);
    ParsedFeed {
        title: feed.title.as_ref().map(plain_text).unwrap_or_default(),
        items,
    }
}

fn parsed_item(source_id: &str, entry: &Entry) -> ParsedItem {
    let title = entry
        .title
        .as_ref()
        .map(plain_text)
        .filter(|title| !title.is_empty())
        .unwrap_or_else(|| "Untitled".into());
    let link = entry_link(entry);
    let identity = if entry.id.is_empty() {
        format!("{link}|{title}")
    } else {
        entry.id.clone()
    };
    ParsedItem {
        key: stable_key(source_id, &identity),
        title,
        link,
        published: entry
            .published
            .or(entry.updated)
            .map(|when| when.timestamp())
            .unwrap_or(0),
        body: entry_body(entry),
    }
}

fn entry_link(entry: &Entry) -> String {
    let http = |href: &str| is_http_url(href);
    entry
        .links
        .iter()
        .find(|link| link.rel.as_deref() == Some("alternate") && http(&link.href))
        .or_else(|| entry.links.iter().find(|link| http(&link.href)))
        .map(|link| link.href.clone())
        .unwrap_or_default()
}

fn entry_body(entry: &Entry) -> String {
    if let Some(content) = &entry.content {
        if let Some(body) = content_body(content) {
            return body;
        }
    }
    entry.summary.as_ref().map(plain_text).unwrap_or_default()
}

fn content_body(content: &Content) -> Option<String> {
    let body = content.body.as_deref()?;
    if body.trim().is_empty() {
        return None;
    }
    Some(to_plain(body, content.content_type.as_str()))
}

fn plain_text(text: &Text) -> String {
    to_plain(&text.content, text.content_type.as_str())
}

fn to_plain(text: &str, _content_type: &str) -> String {
    let mut plain = html_to_text(text);
    if plain.chars().count() > MAX_BODY {
        plain = plain.chars().take(MAX_BODY).collect();
        plain.push('…');
    }
    plain
}

pub fn stable_key(source_id: &str, identity: &str) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in identity.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{source_id}:{hash:016x}")
}

pub fn is_http_url(url: &str) -> bool {
    let bytes = url.as_bytes();
    let https = bytes.len() > 8 && bytes[..8].eq_ignore_ascii_case(b"https://");
    let http = bytes.len() > 7 && bytes[..7].eq_ignore_ascii_case(b"http://");
    (https || http) && !url.chars().any(|ch| ch.is_whitespace())
}

#[cfg(test)]
mod tests {
    use super::*;

    const RSS: &str = r#"<?xml version="1.0"?>
<rss version="2.0">
  <channel>
    <title>Example Wire</title>
    <item>
      <title>Older note</title>
      <link>https://example.com/older</link>
      <guid>older</guid>
      <pubDate>Mon, 01 Jan 2024 00:00:00 GMT</pubDate>
      <description>Plain older</description>
    </item>
    <item>
      <title>Newer note</title>
      <link>https://example.com/newer</link>
      <guid>newer</guid>
      <pubDate>Tue, 02 Jan 2024 12:30:00 GMT</pubDate>
      <description><![CDATA[<p>Hello <b>there</b></p><p>Second</p>]]></description>
    </item>
  </channel>
</rss>
"#;

    const ATOM: &str = r#"<?xml version="1.0"?>
<feed xmlns="http://www.w3.org/2005/Atom">
  <title>Atom Wire</title>
  <entry>
    <id>urn:atom:1</id>
    <title>Atom title</title>
    <updated>2024-03-01T08:00:00Z</updated>
    <link rel="alternate" href="https://example.com/atom"/>
    <content type="html">&lt;p&gt;Atom &amp; body&lt;/p&gt;</content>
  </entry>
</feed>
"#;

    #[test]
    fn rss_items_keep_title_link_and_plain_body() {
        let parsed = parse_document(RSS.as_bytes(), "f1").unwrap();
        assert_eq!(parsed.title, "Example Wire");
        assert_eq!(parsed.items.len(), 2);
        let newer = parsed
            .items
            .iter()
            .find(|item| item.title == "Newer note")
            .unwrap();
        assert_eq!(newer.link, "https://example.com/newer");
        assert!(newer.published > 0);
        assert_eq!(newer.body, "Hello there\n\nSecond");
        assert!(newer.key.starts_with("f1:"));
    }

    #[test]
    fn atom_content_is_plain_text() {
        let parsed = parse_document(ATOM.as_bytes(), "f2").unwrap();
        assert_eq!(parsed.title, "Atom Wire");
        assert_eq!(parsed.items.len(), 1);
        assert_eq!(parsed.items[0].title, "Atom title");
        assert_eq!(parsed.items[0].link, "https://example.com/atom");
        assert_eq!(parsed.items[0].body, "Atom & body");
    }

    #[test]
    fn junk_is_not_a_feed() {
        let err = parse_document(b"<html>nope</html>", "f1").unwrap_err();
        assert!(!err.is_empty());
    }
}
