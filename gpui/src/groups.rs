//! Local folder groups for the GPUI shell.
//!
//! These are not Telegram folders. They live in
//! `$XDG_DATA_HOME/ad.neko.mithka.gpui/local-groups.json` (or
//! `~/.local/share/...` when `XDG_DATA_HOME` is unset). Each copied TDLib
//! database has its own list, keyed by the canonical database directory.
//!
//! Schema version 2: a group is a name plus nested Telegram folders. `main`
//! is the main chat list. `folder` is a `chatFolderInfo` id from
//! `updateChatFolders`. Version 1 stored chat ids; loading that file drops
//! those ids and rewrites version 2. Chat membership is not inferred.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const SCHEMA_VERSION: u32 = 2;

/// A Telegram list nested inside a local group.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum NestedFolder {
    /// The main chat list (`chatListMain`).
    Main,
    /// A folder id from `updateChatFolders`.
    Folder { id: i32 },
}

impl NestedFolder {
    /// `None` is the main list, which is what `SelectFolder(None)` loads.
    pub fn telegram_id(self) -> Option<i32> {
        match self {
            NestedFolder::Main => None,
            NestedFolder::Folder { id } => Some(id),
        }
    }

    pub fn matches(self, active: Option<i32>) -> bool {
        self.telegram_id() == active
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalGroup {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub folders: Vec<NestedFolder>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct SessionRecord {
    next_id: u64,
    groups: Vec<LocalGroup>,
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

pub struct GroupLibrary {
    path: PathBuf,
    database_key: String,
    file: StoreFile,
}

impl GroupLibrary {
    pub fn load(path: impl Into<PathBuf>, database_key: impl Into<String>) -> Self {
        let path = path.into();
        let database_key = database_key.into();
        let (file, rewrite) = match fs::read_to_string(&path) {
            Ok(text) => match serde_json::from_str::<StoreFile>(&text) {
                Ok(mut file) => {
                    let rewrite = file.version != SCHEMA_VERSION;
                    file.version = SCHEMA_VERSION;
                    (file, rewrite)
                }
                Err(err) => {
                    eprintln!(
                        "warning: local groups file is not valid JSON ({err}); starting empty"
                    );
                    (StoreFile::default(), false)
                }
            },
            Err(err) if err.kind() == io::ErrorKind::NotFound => (StoreFile::default(), false),
            Err(err) => {
                eprintln!("warning: could not read local groups ({err}); starting empty");
                (StoreFile::default(), false)
            }
        };
        let library = Self {
            path,
            database_key,
            file,
        };
        if rewrite {
            if let Err(err) = library.save() {
                eprintln!("warning: could not rewrite local groups as version 2 ({err})");
            }
        }
        library
    }

    pub fn groups(&self) -> &[LocalGroup] {
        self.record()
            .map(|record| record.groups.as_slice())
            .unwrap_or(&[])
    }

    pub fn group(&self, id: &str) -> Option<&LocalGroup> {
        self.groups().iter().find(|group| group.id == id)
    }

    pub fn create(&mut self, name: &str) -> Option<String> {
        let name = name.trim();
        if name.is_empty() {
            return None;
        }
        let record = self.record_mut();
        record.next_id = record.next_id.saturating_add(1);
        let id = format!("g{}", record.next_id);
        record.groups.push(LocalGroup {
            id: id.clone(),
            name: name.to_string(),
            folders: Vec::new(),
        });
        Some(id)
    }

    pub fn rename(&mut self, id: &str, name: &str) -> bool {
        let name = name.trim();
        if name.is_empty() {
            return false;
        }
        let Some(group) = self
            .record_mut()
            .groups
            .iter_mut()
            .find(|group| group.id == id)
        else {
            return false;
        };
        group.name = name.to_string();
        true
    }

    pub fn delete(&mut self, id: &str) -> bool {
        let record = self.record_mut();
        let before = record.groups.len();
        record.groups.retain(|group| group.id != id);
        record.groups.len() != before
    }

    /// Returns whether `folder` is nested in the group after the toggle.
    pub fn toggle_folder(&mut self, group_id: &str, folder: NestedFolder) -> Option<bool> {
        let group = self
            .record_mut()
            .groups
            .iter_mut()
            .find(|group| group.id == group_id)?;
        if let Some(index) = group
            .folders
            .iter()
            .position(|existing| *existing == folder)
        {
            group.folders.remove(index);
            Some(false)
        } else {
            group.folders.push(folder);
            Some(true)
        }
    }

    pub fn save(&self) -> io::Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let text = serde_json::to_string_pretty(&self.file).map_err(io::Error::other)?;
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
    data_home.join("ad.neko.mithka.gpui/local-groups.json")
}

pub fn store_path() -> PathBuf {
    store_path_in(&data_home())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn library(dir: &Path) -> GroupLibrary {
        GroupLibrary::load(dir.join("local-groups.json"), "/tmp/mithka-session")
    }

    fn scratch_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "mithka-groups-{}-{}",
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
    fn create_rename_and_nest_folders_round_trip() {
        let dir = scratch_dir();
        let mut groups = library(&dir);
        let id = groups.create("  Family  ").unwrap();
        assert!(groups.create("   ").is_none());
        assert_eq!(
            groups.toggle_folder(&id, NestedFolder::Folder { id: 2 }),
            Some(true)
        );
        assert_eq!(
            groups.toggle_folder(&id, NestedFolder::Folder { id: 2 }),
            Some(false)
        );
        assert_eq!(groups.toggle_folder(&id, NestedFolder::Main), Some(true));
        assert_eq!(
            groups.toggle_folder(&id, NestedFolder::Folder { id: 2 }),
            Some(true)
        );
        assert!(groups.rename(&id, "Kin"));
        groups.save().unwrap();

        let mut loaded = library(&dir);
        let group = loaded.group(&id).unwrap();
        assert_eq!(group.name, "Kin");
        assert_eq!(
            group.folders,
            vec![NestedFolder::Main, NestedFolder::Folder { id: 2 }]
        );
        assert!(group.folders.contains(&NestedFolder::Main));
        assert!(!group.folders.contains(&NestedFolder::Folder { id: 9 }));
        assert!(loaded.delete(&id));
        assert!(loaded.group(&id).is_none());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn version_one_chat_ids_are_dropped_on_load() {
        let dir = scratch_dir();
        let path = dir.join("local-groups.json");
        fs::write(
            &path,
            r#"{
              "version": 1,
              "sessions": {
                "/tmp/mithka-session": {
                  "next_id": 3,
                  "groups": [
                    {"id": "g1", "name": "Old", "chat_ids": [101, 102]}
                  ]
                }
              }
            }"#,
        )
        .unwrap();
        let loaded = GroupLibrary::load(&path, "/tmp/mithka-session");
        let group = loaded.group("g1").unwrap();
        assert_eq!(group.name, "Old");
        assert!(group.folders.is_empty());
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("\"version\": 2"));
        assert!(!text.contains("chat_ids"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn store_path_is_under_the_gpui_app_id() {
        let path = store_path_in(Path::new("/var/lib/data"));
        assert_eq!(
            path,
            PathBuf::from("/var/lib/data/ad.neko.mithka.gpui/local-groups.json")
        );
    }
}
