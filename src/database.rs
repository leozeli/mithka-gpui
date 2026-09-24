use std::fs;
use std::path::Path;

/// Check that `dir` looks like a Mithka TDLib database directory.
///
/// Slot 0 is the directory that itself contains `td.binlog` and `files/`.
/// Other accounts live in `account-N` children of that folder. A parent that
/// only contains `account-*` and no `td.binlog` is refused so TDLib does not
/// create a fresh database beside the real accounts.
pub fn inspect_database(dir: &Path) -> Result<Vec<String>, String> {
    if !dir.exists() {
        return Err(format!(
            "database directory does not exist: {}",
            dir.display()
        ));
    }
    if !dir.is_dir() {
        return Err(format!(
            "database path is not a directory: {}",
            dir.display()
        ));
    }

    let binlog = dir.join("td.binlog");
    if binlog.is_file() {
        let mut warnings = Vec::new();
        if !dir.join("files").is_dir() {
            warnings.push(format!(
                "no files/ directory in {}. Mithka sets files_directory to this path; TDLib may create it.",
                dir.display()
            ));
        }
        return Ok(warnings);
    }

    let mut accounts = Vec::new();
    if let Ok(read) = fs::read_dir(dir) {
        for entry in read.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with("account-") && entry.path().is_dir() {
                accounts.push(name.into_owned());
            }
        }
    }
    accounts.sort();
    if !accounts.is_empty() {
        return Err(format!(
            "no td.binlog in {}. This looks like the parent tdlib folder with accounts ({}). Pass the slot directory that itself contains td.binlog: the slot 0 directory, or one of those account-N directories.",
            dir.display(),
            accounts.join(", ")
        ));
    }

    Ok(vec![format!(
        "no td.binlog in {}. TDLib will create a new empty database and will not reuse a Mithka login.",
        dir.display()
    )])
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn scratch(label: &str) -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("mithka-db-{label}-{nanos}"));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn missing_directory_is_an_error() {
        let err = inspect_database(Path::new("/no/such/mithka-tdlib-dir")).unwrap_err();
        assert!(err.contains("does not exist"));
    }

    #[test]
    fn parent_with_accounts_is_refused() {
        let dir = scratch("parent");
        fs::create_dir(dir.join("account-1")).unwrap();
        let err = inspect_database(&dir).unwrap_err();
        assert!(err.contains("account-1"));
        assert!(err.contains("td.binlog"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn slot_directory_with_binlog_is_accepted() {
        let dir = scratch("slot");
        fs::write(dir.join("td.binlog"), b"").unwrap();
        fs::create_dir(dir.join("files")).unwrap();
        let warnings = inspect_database(&dir).unwrap();
        assert!(warnings.is_empty(), "{warnings:?}");
        let _ = fs::remove_dir_all(&dir);
    }
}
