use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static STUBS: AtomicU64 = AtomicU64::new(0);

fn bin() -> PathBuf {
    PathBuf::from(
        std::env::var("CARGO_BIN_EXE_mithka-tdlib-spike")
            .expect("Cargo sets CARGO_BIN_EXE_mithka-tdlib-spike for integration tests"),
    )
}

fn scratch(label: &str) -> PathBuf {
    let n = STUBS.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("mithka-spike-{label}-{nanos}-{n}"));
    fs::create_dir_all(&path).unwrap();
    path
}

fn compile_stub() -> PathBuf {
    let so = scratch("so").join("libtdjson_stub.so");
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/stub_tdjson.c");
    let output = Command::new("gcc")
        .args(["-shared", "-fPIC", "-O2", "-o"])
        .arg(&so)
        .arg(&src)
        .output()
        .expect("gcc");
    assert!(
        output.status.success(),
        "gcc failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    so
}

fn database_dir() -> PathBuf {
    let dir = scratch("db");
    fs::write(dir.join("td.binlog"), b"stub").unwrap();
    fs::create_dir(dir.join("files")).unwrap();
    dir
}

fn run(mode: Option<&str>, extra: &[&str]) -> std::process::Output {
    let so = compile_stub();
    let db = database_dir();
    let mut cmd = Command::new(bin());
    cmd.args([
        "--tdjson",
        so.to_str().unwrap(),
        "--database",
        db.to_str().unwrap(),
        "--api-id",
        "100",
        "--api-hash",
        "spike-test-hash-do-not-log",
        "--auth-timeout",
        "5",
        "--chat-timeout",
        "5",
    ])
    .args(extra);
    if let Some(mode) = mode {
        cmd.env("MITHKA_STUB_MODE", mode);
    }
    cmd.output().unwrap()
}

fn combined(output: &std::process::Output) -> String {
    format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

#[test]
fn help_lists_the_session_flags() {
    let output = Command::new(bin()).arg("--help").output().unwrap();
    assert!(output.status.success());
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("--tdjson"));
    assert!(text.contains("--database"));
    assert!(text.contains("--api-id"));
    assert!(text.contains("--api-hash"));
    assert!(text.contains("TDLIB_API_ID"));
    assert!(text.contains("TDLIB_API_HASH"));
    assert!(text.contains("Android"));
}

#[test]
fn stub_prints_ready_and_titles_without_the_api_hash() {
    let output = run(None, &[]);
    let text = combined(&output);
    assert!(
        output.status.success(),
        "status {:?}\n{text}",
        output.status.code()
    );
    assert!(text.contains("TDLib version: 1.8.67-stub"), "{text}");
    assert!(text.contains("database_encryption_key: empty"), "{text}");
    assert!(text.contains("use_test_dc: false"), "{text}");
    assert!(text.contains("td_mithka_last_error=yes"), "{text}");
    assert!(
        text.contains("td_mithka_export_session_string=no"),
        "{text}"
    );
    assert!(
        text.contains("auth: authorizationStateWaitTdlibParameters"),
        "{text}"
    );
    assert!(text.contains("\nReady\n"), "{text}");
    assert!(text.contains("1. Alpha"), "{text}");
    assert!(text.contains("2. Beta"), "{text}");
    assert!(text.contains("closed"), "{text}");
    assert!(!text.contains("spike-test-hash-do-not-log"), "{text}");
    assert!(!text.contains("logOut"), "{text}");
}

#[test]
fn lock_error_is_explicit() {
    let output = run(Some("lock"), &[]);
    let text = combined(&output);
    assert_eq!(output.status.code(), Some(1), "{text}");
    assert!(text.contains("TDLib error 400"), "{text}");
    assert!(text.contains("Can't lock file"), "{text}");
    assert!(text.to_ascii_lowercase().contains("copy"), "{text}");
    assert!(text.contains("closed"), "{text}");
    assert!(!text.contains("spike-test-hash-do-not-log"), "{text}");
}

#[test]
fn encryption_401_is_explicit() {
    let output = run(Some("encryption"), &[]);
    let text = combined(&output);
    assert_eq!(output.status.code(), Some(1), "{text}");
    assert!(text.contains("TDLib error 401"), "{text}");
    assert!(text.contains("encryption key"), "{text}");
    assert!(text.contains("empty"), "{text}");
}

#[test]
fn generation_mismatch_is_explicit() {
    let output = run(Some("generation"), &[]);
    let text = combined(&output);
    assert_eq!(output.status.code(), Some(1), "{text}");
    assert!(text.contains("future TDLib version"), "{text}");
    assert!(text.contains("1.8.67"), "{text}");
}

#[test]
fn phone_state_exits_cleanly() {
    let output = run(Some("phone"), &[]);
    let text = combined(&output);
    assert_eq!(output.status.code(), Some(3), "{text}");
    assert!(text.contains("authorizationStateWaitPhoneNumber"), "{text}");
    assert!(text.contains("does not ask"), "{text}");
    assert!(!text.contains("\nReady\n"), "{text}");
}

#[test]
fn missing_library_is_a_usage_error() {
    let db = database_dir();
    let output = Command::new(bin())
        .args([
            "--tdjson",
            "/no/such/libtdjson.so",
            "--database",
            db.to_str().unwrap(),
            "--api-id",
            "1",
            "--api-hash",
            "hash",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let err = String::from_utf8_lossy(&output.stderr);
    assert!(err.contains("not a file"), "{err}");
}

#[test]
fn parent_tdlib_directory_is_refused() {
    let root = scratch("parent");
    fs::create_dir(root.join("account-1")).unwrap();
    let dummy = root.join("placeholder.so");
    fs::write(&dummy, b"").unwrap();
    let output = Command::new(bin())
        .args([
            "--tdjson",
            dummy.to_str().unwrap(),
            "--database",
            root.to_str().unwrap(),
            "--api-id",
            "1",
            "--api-hash",
            "hash",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let err = String::from_utf8_lossy(&output.stderr);
    assert!(err.contains("account-1"), "{err}");
    assert!(err.contains("td.binlog"), "{err}");
}
