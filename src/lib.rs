//! Shared TDLib session for Mithka desktop shells.
//!
//! `mithka-tdlib-spike` is the headless binary in this package. `mithka-gpui`
//! is the product shell. `mithka-gtk` is a separate validation spike. Both
//! shells depend on `mithka_tdlib` and neither secret lives in this crate.

#![deny(unsafe_code)]

mod database;
mod driver;
mod format;
mod run;
mod shell;
mod tdjson;

pub use database::inspect_database;
pub use driver::{explain_error, set_tdlib_parameters, SessionConfig};
pub use format::{is_http_url, list_time, message_time};
pub use run::{run, RunOptions};
pub use shell::{
    ChatItem, ContactItem, FolderItem, LiveClient, MessageKind, ShellCommand, TextLink,
    TextMessage, UiUpdate,
};
pub use tdjson::{LoadError, TdJson};
