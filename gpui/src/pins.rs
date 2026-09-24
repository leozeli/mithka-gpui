//! Local chat pins for the GPUI shell.
//!
//! These are not Telegram pins. They live in
//! `$XDG_DATA_HOME/ad.neko.mithka.gpui/local-pins.json` (or
//! `~/.local/share/...` when `XDG_DATA_HOME` is unset), keyed by the
//! canonical database directory. The UI sorts them ahead of TDLib's
//! `chatPosition.order`, which already puts server pins first among
//! chats that are not locally pinned. Nothing here sends
//! `toggleChatIsPinned`.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct SessionRecord {
    #[serde(default)]
    chat_ids: Vec<i64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct StoreFile {
    #[serde(default = "schema_version")]
    version: u32,
    #[serde(default)]
    sessions: BTreeMap<String, SessionRecord>,
}

fn schema_version() -> u32 {
    SCHEMA_VERSION
}

impl Default for StoreFile {
    fn default() -> Self {
        Self {
            version: SCHEMA_VERSION,
            sessions: BTreeMap::new(),
        }
    }
}

pub struct PinLibrary {
    path: PathBuf,
    database_key: String,
    file: StoreFile,
}

impl PinLibrary {
    pub fn load(path: impl Into<PathBuf>, database_key: impl Into<String>) -> Self {
        let path = path.into();
        let database_key = database_key.into();
        let file = match fs::read_to_string(&path) {
            Ok(text) => match serde_json::from_str::<StoreFile>(&text) {
                Ok(mut file) => {
                    file.version = SCHEMA_VERSION;
                    file
                }
                Err(err) => {
                    eprintln!("warning: local pins file is not valid JSON ({err}); starting empty");
                    StoreFile::default()
                }
            },
            Err(err) if err.kind() == io::ErrorKind::NotFound => StoreFile::default(),
            Err(err) => {
                eprintln!("warning: could not read local pins ({err}); starting empty");
                StoreFile::default()
            }
        };
        Self {
            path,
            database_key,
            file,
        }
    }

    pub fn is_pinned(&self, chat_id: i64) -> bool {
        self.rank(chat_id).is_some()
    }

    /// `Some(0)` is the most recently pinned chat. `None` is not pinned.
    pub fn rank(&self, chat_id: i64) -> Option<usize> {
        self.record()
            .and_then(|record| record.chat_ids.iter().position(|id| *id == chat_id))
    }

    /// Returns whether the chat is pinned after the toggle.
    pub fn toggle(&mut self, chat_id: i64) -> bool {
        let ids = &mut self.record_mut().chat_ids;
        if let Some(position) = ids.iter().position(|id| *id == chat_id) {
            ids.remove(position);
            false
        } else {
            ids.insert(0, chat_id);
            true
        }
    }

    pub fn save(&self) -> io::Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let text = serde_json::to_string_pretty(&self.file)
            .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
        fs::write(&self.path, text)
    }

    fn record(&self) -> Option<&SessionRecord> {
        self.file.sessions.get(&self.database_key)
    }

    fn record_mut(&mut self) -> &mut SessionRecord {
        self.file
            .sessions
            .entry(self.database_key.clone())
            .or_default()
    }
}

/// Sort key that places local pins before every other chat.
/// Lower rank stays ahead. Unpinned chats share one key so a stable sort
/// keeps TDLib's existing order, including server pins.
pub fn pin_sort_key(pins: &PinLibrary, chat_id: i64) -> (u8, usize) {
    match pins.rank(chat_id) {
        Some(rank) => (0, rank),
        None => (1, 0),
    }
}

pub fn store_path_in(data_home: &Path) -> PathBuf {
    data_home.join("ad.neko.mithka.gpui/local-pins.json")
}

pub fn store_path() -> PathBuf {
    store_path_in(&crate::groups::data_home())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "mithka-pins-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn toggle_round_trip_and_sort_ahead_of_higher_order() {
        let dir = scratch_dir();
        let path = dir.join("local-pins.json");
        let mut pins = PinLibrary::load(&path, "/tmp/mithka-session");
        assert!(pins.toggle(101));
        assert!(pins.toggle(102));
        assert_eq!(pins.rank(102), Some(0));
        assert_eq!(pins.rank(101), Some(1));
        assert!(!pins.toggle(102));
        assert!(!pins.is_pinned(102));
        assert!(pins.is_pinned(101));
        pins.save().unwrap();

        let loaded = PinLibrary::load(&path, "/tmp/mithka-session");
        assert!(loaded.is_pinned(101));
        let mut chats = [(102_i64, 99_i64), (101, 10)];
        chats.sort_by(|left, right| {
            pin_sort_key(&loaded, left.0)
                .cmp(&pin_sort_key(&loaded, right.0))
                .then(right.1.cmp(&left.1))
        });
        assert_eq!(chats[0].0, 101);
        assert_eq!(
            store_path_in(Path::new("/var/lib/data")),
            PathBuf::from("/var/lib/data/ad.neko.mithka.gpui/local-pins.json")
        );
        let _ = fs::remove_dir_all(&dir);
    }
}
