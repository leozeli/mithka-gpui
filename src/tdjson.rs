//! Dynamic loader for Mithka's `libtdjson.so`.
//!
//! Required C symbols (stable tdjson JSON client):
//! `td_create_client_id`, `td_send`, `td_receive`, `td_execute`.
//!
//! Optional Mithka patch symbols are resolved when the loaded build exports
//! them and are never called:
//! `td_mithka_export_session_string`, `td_mithka_import_session_string`,
//! `td_mithka_last_error`, `td_mithka_set_transfer_boost`.

#![allow(unsafe_code)]

use libloading::{Library, Symbol};
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_double, c_int};
use std::path::Path;

type CreateClientId = unsafe extern "C" fn() -> c_int;
type Send = unsafe extern "C" fn(c_int, *const c_char);
type Receive = unsafe extern "C" fn(c_double) -> *const c_char;
type Execute = unsafe extern "C" fn(*const c_char) -> *const c_char;

/// `td_mithka_export_session_string(source_path, api_id, test_mode, user_id)`.
pub type ExportSessionString =
    unsafe extern "C" fn(*const c_char, c_int, c_int, i64) -> *const c_char;
/// `td_mithka_import_session_string(session_string, destination_path)` → 0 on success.
pub type ImportSessionString = unsafe extern "C" fn(*const c_char, *const c_char) -> c_int;
pub type LastError = unsafe extern "C" fn() -> *const c_char;
/// `td_mithka_set_transfer_boost(download_chunk, download_parallel, upload_chunk, upload_parallel)`.
pub type SetTransferBoost = unsafe extern "C" fn(c_int, c_int, c_int, c_int);

#[derive(Clone, Copy)]
pub struct MithkaSymbols {
    pub export_session_string: Option<ExportSessionString>,
    pub import_session_string: Option<ImportSessionString>,
    pub last_error: Option<LastError>,
    pub set_transfer_boost: Option<SetTransferBoost>,
}

impl MithkaSymbols {
    pub fn line(&self) -> String {
        format!(
            "mithka symbols: td_mithka_export_session_string={} td_mithka_import_session_string={} td_mithka_last_error={} td_mithka_set_transfer_boost={}",
            yn(self.export_session_string.is_some()),
            yn(self.import_session_string.is_some()),
            yn(self.last_error.is_some()),
            yn(self.set_transfer_boost.is_some())
        )
    }
}

fn yn(present: bool) -> &'static str {
    if present {
        "yes"
    } else {
        "no"
    }
}

#[derive(Debug)]
pub enum LoadError {
    Open {
        path: String,
        source: libloading::Error,
    },
    Symbol {
        name: &'static str,
        source: libloading::Error,
    },
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoadError::Open { path, source } => {
                write!(f, "failed to load {path}: {source}")?;
                if Path::new(path).is_file() {
                    write!(
                        f,
                        "\nhint: the file exists, so a dependency of libtdjson.so failed to load. Run `ldd {path}` and export LD_LIBRARY_PATH to the Mithka bundle directory that contains libtdjson.so."
                    )?;
                } else {
                    write!(
                        f,
                        "\nhint: pass --tdjson with the path to Mithka's libtdjson.so (Linux x86_64)."
                    )?;
                }
                Ok(())
            }
            LoadError::Symbol { name, source } => write!(
                f,
                "libtdjson is missing required symbol {name}: {source}\nhint: this spike needs td_create_client_id, td_send, td_receive, and td_execute from a Mithka 1.8.67 libtdjson.so. Optional td_mithka_* symbols are not required."
            ),
        }
    }
}

impl std::error::Error for LoadError {}

pub struct TdJson {
    create_client_id: CreateClientId,
    send_fn: Send,
    receive_fn: Receive,
    execute_fn: Execute,
    mithka: MithkaSymbols,
    /// Kept last so the library stays mapped for the function pointers above.
    _lib: Library,
}

impl TdJson {
    pub fn open(path: &Path) -> Result<Self, LoadError> {
        let path_string = path.display().to_string();
        let lib = unsafe { Library::new(path) }.map_err(|source| LoadError::Open {
            path: path_string,
            source,
        })?;

        let create_client_id = load_fn(&lib, b"td_create_client_id", "td_create_client_id")?;
        let send_fn = load_fn(&lib, b"td_send", "td_send")?;
        let receive_fn = load_fn(&lib, b"td_receive", "td_receive")?;
        let execute_fn = load_fn(&lib, b"td_execute", "td_execute")?;
        let mithka = MithkaSymbols {
            export_session_string: load_optional(&lib, b"td_mithka_export_session_string"),
            import_session_string: load_optional(&lib, b"td_mithka_import_session_string"),
            last_error: load_optional(&lib, b"td_mithka_last_error"),
            set_transfer_boost: load_optional(&lib, b"td_mithka_set_transfer_boost"),
        };

        Ok(Self {
            create_client_id,
            send_fn,
            receive_fn,
            execute_fn,
            mithka,
            _lib: lib,
        })
    }

    pub fn mithka(&self) -> &MithkaSymbols {
        &self.mithka
    }

    pub fn create_client_id(&self) -> i32 {
        unsafe { (self.create_client_id)() }
    }

    pub fn send(&self, client_id: i32, request: &str) -> Result<(), String> {
        let c = CString::new(request)
            .map_err(|_| "TDLib request contains an interior NUL".to_string())?;
        unsafe { (self.send_fn)(client_id, c.as_ptr()) };
        Ok(())
    }

    /// Copy of the next JSON event. The pointer TDLib returns is only valid
    /// until the next `td_receive` or `td_execute` on this thread.
    pub fn receive(&self, timeout_secs: f64) -> Option<String> {
        unsafe {
            let ptr = (self.receive_fn)(timeout_secs);
            if ptr.is_null() {
                return None;
            }
            Some(CStr::from_ptr(ptr).to_string_lossy().into_owned())
        }
    }

    pub fn execute(&self, request: &str) -> Result<Option<String>, String> {
        let c = CString::new(request)
            .map_err(|_| "TDLib request contains an interior NUL".to_string())?;
        unsafe {
            let ptr = (self.execute_fn)(c.as_ptr());
            if ptr.is_null() {
                return Ok(None);
            }
            Ok(Some(CStr::from_ptr(ptr).to_string_lossy().into_owned()))
        }
    }
}

fn load_fn<T: Copy>(lib: &Library, name: &[u8], label: &'static str) -> Result<T, LoadError> {
    let sym: Symbol<T> = unsafe { lib.get(name) }.map_err(|source| LoadError::Symbol {
        name: label,
        source,
    })?;
    Ok(*sym)
}

fn load_optional<T: Copy>(lib: &Library, name: &[u8]) -> Option<T> {
    let sym: Symbol<T> = unsafe { lib.get(name) }.ok()?;
    Some(*sym)
}
