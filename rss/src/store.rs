//! JSON store for feed URLs and the items last fetched from them.
//!
//! The file is `$XDG_DATA_HOME/ad.neko.mithka.gpui/subscriptions.json`
//! (`~/.local/share/...` when `XDG_DATA_HOME` is unset). It is not keyed by
//! the TDLib database: subscriptions belong to this app, not to a Telegram
//! account.

use crate::parse::{is_http_url, ParsedItem, MAX_ITEMS};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Source {
    pub id: String,
    pub url: String,
    pub title: String,
}

impl Source {
    pub fn display_name(&self) -> String {
        let title = self.title.trim();
        if title.is_empty() {
            self.url.clone()
        } else {
            title.to_string()
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Item {
    pub id: String,
    pub source_id: String,
    pub title: String,
    pub link: String,
    pub published: i64,
    pub body: String,
}

impl Item {
    fn from_parsed(source_id: &str, item: ParsedItem) -> Self {
        Self {
            id: item.key,
            source_id: source_id.to_string(),
            title: item.title,
            link: item.link,
            published: item.published,
            body: item.body,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct StoreFile {
    version: u32,
    next_id: u64,
    sources: Vec<Source>,
    items: Vec<Item>,
}

impl Default for StoreFile {
    fn default() -> Self {
        Self {
            version: 1,
            next_id: 0,
            sources: Vec::new(),
            items: Vec::new(),
        }
    }
}

pub struct SubscriptionStore {
    path: PathBuf,
    file: StoreFile,
}

impl SubscriptionStore {
    pub fn load(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let file = match fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_else(|err| {
                eprintln!("warning: subscriptions file is not valid JSON ({err}); starting empty");
                StoreFile::default()
            }),
            Err(err) if err.kind() == io::ErrorKind::NotFound => StoreFile::default(),
            Err(err) => {
                eprintln!("warning: could not read subscriptions ({err}); starting empty");
                StoreFile::default()
            }
        };
        Self { path, file }
    }

    pub fn sources(&self) -> &[Source] {
        &self.file.sources
    }

    pub fn source(&self, id: &str) -> Option<&Source> {
        self.file.sources.iter().find(|source| source.id == id)
    }

    /// Adds `url`, or returns the id already stored for that exact URL.
    pub fn add_source(&mut self, url: &str) -> Result<String, String> {
        let url = url.trim();
        if url.is_empty() {
            return Err("Feed URL is empty.".into());
        }
        if !is_http_url(url) {
            return Err("Feed URL must be http or https.".into());
        }
        if let Some(existing) = self.file.sources.iter().find(|source| source.url == url) {
            return Ok(existing.id.clone());
        }
        self.file.next_id = self.file.next_id.saturating_add(1);
        let id = format!("f{}", self.file.next_id);
        self.file.sources.push(Source {
            id: id.clone(),
            url: url.to_string(),
            title: provisional_title(url),
        });
        Ok(id)
    }

    pub fn set_title(&mut self, id: &str, title: &str) -> bool {
        let title = title.trim();
        if title.is_empty() {
            return false;
        }
        let Some(source) = self.file.sources.iter_mut().find(|source| source.id == id) else {
            return false;
        };
        source.title = title.to_string();
        true
    }

    pub fn remove_source(&mut self, id: &str) -> bool {
        let before = self.file.sources.len();
        self.file.sources.retain(|source| source.id != id);
        if self.file.sources.len() == before {
            return false;
        }
        self.file.items.retain(|item| item.source_id != id);
        true
    }

    /// Replaces cached items for one source. Unknown sources are ignored.
    pub fn replace_items(&mut self, source_id: &str, items: Vec<ParsedItem>) -> bool {
        if self.source(source_id).is_none() {
            return false;
        }
        self.file.items.retain(|item| item.source_id != source_id);
        let stored = items
            .into_iter()
            .take(MAX_ITEMS)
            .map(|item| Item::from_parsed(source_id, item));
        self.file.items.extend(stored);
        true
    }

    /// Newest first. `None` is every source.
    pub fn items(&self, source_id: Option<&str>) -> Vec<Item> {
        let mut items: Vec<Item> = self
            .file
            .items
            .iter()
            .filter(|item| source_id.is_none_or(|id| item.source_id == id))
            .cloned()
            .collect();
        items.sort_by(|left, right| {
            right
                .published
                .cmp(&left.published)
                .then_with(|| left.title.cmp(&right.title))
        });
        items
    }

    pub fn item(&self, id: &str) -> Option<&Item> {
        self.file.items.iter().find(|item| item.id == id)
    }

    pub fn save(&self) -> io::Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let text = serde_json::to_string_pretty(&self.file).map_err(io::Error::other)?;
        fs::write(&self.path, text)
    }
}

pub fn provisional_title(url: &str) -> String {
    let rest = url.split_once("://").map(|(_, rest)| rest).unwrap_or(url);
    let host = rest.split('/').next().unwrap_or(rest);
    if host.is_empty() {
        url.to_string()
    } else {
        host.to_string()
    }
}

pub fn data_home() -> PathBuf {
    if let Some(dir) = std::env::var_os("XDG_DATA_HOME") {
        return PathBuf::from(dir);
    }
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home).join(".local/share");
    }
    PathBuf::from(".local/share")
}

pub fn store_path_in(data_home: &Path) -> PathBuf {
    data_home.join("ad.neko.mithka.gpui/subscriptions.json")
}

pub fn store_path() -> PathBuf {
    store_path_in(&data_home())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::parse_document;

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
      <description>Plain newer</description>
    </item>
  </channel>
</rss>
"#;

    #[test]
    fn add_remove_and_items_round_trip_newest_first() {
        let dir = std::env::temp_dir().join(format!(
            "mithka-rss-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        let mut store = SubscriptionStore::load(dir.join("subscriptions.json"));
        assert!(store.add_source("ftp://example.com/feed").is_err());
        let id = store.add_source(" https://example.com/feed.xml ").unwrap();
        assert_eq!(
            store.add_source("https://example.com/feed.xml").unwrap(),
            id
        );
        let parsed = parse_document(RSS.as_bytes(), &id).unwrap();
        assert!(store.set_title(&id, &parsed.title));
        assert!(store.replace_items(&id, parsed.items));
        store.save().unwrap();

        let loaded = SubscriptionStore::load(dir.join("subscriptions.json"));
        assert_eq!(loaded.source(&id).unwrap().title, "Example Wire");
        let items = loaded.items(Some(&id));
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].title, "Newer note");
        assert_eq!(items[1].title, "Older note");
        assert_eq!(loaded.items(None).len(), 2);
        assert!(loaded.item(&items[0].id).is_some());

        let mut loaded = loaded;
        assert!(loaded.remove_source(&id));
        assert!(loaded.sources().is_empty());
        assert!(loaded.items(None).is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn store_path_is_under_the_gpui_app_id() {
        let path = store_path_in(Path::new("/var/lib/data"));
        assert_eq!(
            path,
            PathBuf::from("/var/lib/data/ad.neko.mithka.gpui/subscriptions.json")
        );
    }
}
