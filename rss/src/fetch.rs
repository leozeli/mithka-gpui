//! Blocking HTTP fetch used by the background worker.
//!
//! The GPUI thread only sends a URL and reads the result. `ureq` 2 keeps this
//! off the UI thread without an async runtime.

use crate::parse::{is_http_url, parse_document, ParsedFeed};
use std::io::Read;
use std::time::Duration;

const MAX_BYTES: u64 = 2_000_000;
const USER_AGENT: &str = "mithka-gpui/0.1.0";

pub fn fetch_feed(url: &str, source_id: &str) -> Result<ParsedFeed, String> {
    if !is_http_url(url) {
        return Err("Feed URL must be http or https.".into());
    }
    let bytes = download(url)?;
    parse_document(&bytes, source_id)
}

fn download(url: &str) -> Result<Vec<u8>, String> {
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(20))
        .redirects(5)
        .build();
    let response = agent
        .get(url)
        .set("User-Agent", USER_AGENT)
        .call()
        .map_err(|err| shorten(&err.to_string()))?;
    let mut reader = response.into_reader().take(MAX_BYTES);
    let mut bytes = Vec::new();
    reader
        .read_to_end(&mut bytes)
        .map_err(|err| shorten(&err.to_string()))?;
    if bytes.is_empty() {
        return Err("The feed response was empty.".into());
    }
    Ok(bytes)
}

fn shorten(message: &str) -> String {
    let mut message = message.trim().to_string();
    if message.chars().count() > 180 {
        message = message.chars().take(180).collect();
        message.push('…');
    }
    message
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    const RSS: &str = r#"<?xml version="1.0"?>
<rss version="2.0">
  <channel>
    <title>Local Wire</title>
    <item>
      <title>Desk note</title>
      <link>https://example.com/desk</link>
      <guid>desk</guid>
      <pubDate>Tue, 02 Jan 2024 12:30:00 GMT</pubDate>
      <description>Fetched locally</description>
    </item>
  </channel>
</rss>
"#;

    #[test]
    fn fetch_reads_rss_from_a_local_server() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let body = RSS.as_bytes().to_vec();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut buf = [0u8; 2048];
            let _ = stream.read(&mut buf);
            let header = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/rss+xml\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            stream.write_all(header.as_bytes()).unwrap();
            stream.write_all(&body).unwrap();
        });

        let parsed = fetch_feed(&format!("http://127.0.0.1:{port}/feed.xml"), "f9").unwrap();
        server.join().unwrap();
        assert_eq!(parsed.title, "Local Wire");
        assert_eq!(parsed.items.len(), 1);
        assert_eq!(parsed.items[0].title, "Desk note");
        assert_eq!(parsed.items[0].body, "Fetched locally");
    }

    #[test]
    fn fetch_rejects_a_non_http_url() {
        let err = fetch_feed("file:///tmp/feed.xml", "f1").unwrap_err();
        assert!(err.contains("http"));
    }
}
