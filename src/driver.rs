//! TDLib authorization and chat-list state machine.
//!
//! Requests are JSON values. The receive loop sends them; this module never
//! talks to the network or the native library, so the auth path can be tested
//! without a session.

use serde_json::{json, Value};
use std::collections::HashMap;

#[derive(Clone, Debug)]
pub struct SessionConfig {
    pub database_directory: String,
    pub files_directory: String,
    pub api_id: i32,
    pub api_hash: String,
    pub device_model: String,
    pub system_language_code: String,
    pub system_version: String,
    pub application_version: String,
    pub chat_limit: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    WaitAuth,
    Collecting,
    Closing,
    Finished,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimeoutKind {
    Auth,
    Chats,
}

#[derive(Default, Debug)]
pub struct Effect {
    pub send: Vec<Value>,
    pub lines: Vec<String>,
    pub exit: Option<i32>,
}

pub struct Driver {
    cfg: SessionConfig,
    phase: Phase,
    sent_parameters: bool,
    sent_encryption_check: bool,
    saw_ready: bool,
    saw_chats: bool,
    did_load: bool,
    get_chats_sent: u8,
    pending_code: Option<i32>,
    last_auth: String,
    last_error: Option<String>,
    order: Vec<i64>,
    seen_order: Vec<i64>,
    titles: HashMap<i64, String>,
    waiting: Vec<i64>,
}

impl Driver {
    pub fn new(cfg: SessionConfig) -> Self {
        Self {
            cfg,
            phase: Phase::WaitAuth,
            sent_parameters: false,
            sent_encryption_check: false,
            saw_ready: false,
            saw_chats: false,
            did_load: false,
            get_chats_sent: 0,
            pending_code: None,
            last_auth: String::new(),
            last_error: None,
            order: Vec::new(),
            seen_order: Vec::new(),
            titles: HashMap::new(),
            waiting: Vec::new(),
        }
    }

    pub fn saw_ready(&self) -> bool {
        self.saw_ready
    }

    pub fn is_collecting(&self) -> bool {
        self.phase == Phase::Collecting
    }

    pub fn is_closing(&self) -> bool {
        matches!(self.phase, Phase::Closing | Phase::Finished)
    }

    pub fn pending_code(&self) -> i32 {
        self.pending_code.unwrap_or(1)
    }

    pub fn on_event(&mut self, event: &Value) -> Effect {
        if self.phase == Phase::Finished {
            return Effect::default();
        }
        let typ = event_type(event);
        if self.phase == Phase::Closing {
            return self.while_closing(typ, event);
        }
        match typ {
            "updateAuthorizationState" => self.on_auth(event),
            "error" => self.on_error(event),
            "chats" => self.on_chats(event),
            "chat" => self.on_chat_object(event),
            "updateNewChat" => self.on_new_chat(event),
            "updateChatTitle" => self.on_title_update(event),
            "ok" => self.on_ok(event),
            _ => Effect::default(),
        }
    }

    pub fn interrupt(&mut self) -> Effect {
        if self.is_closing() {
            return Effect::default();
        }
        self.begin_close(
            130,
            vec!["interrupt: closing TDLib (close, not logOut)".to_string()],
        )
    }

    pub fn on_timeout(&mut self, kind: TimeoutKind) -> Effect {
        if self.is_closing() {
            return Effect::default();
        }
        match kind {
            TimeoutKind::Auth => {
                let state = if self.last_auth.is_empty() {
                    "(no authorization update yet)".to_string()
                } else {
                    self.last_auth.clone()
                };
                let mut lines = vec![format!(
                    "timed out waiting for authorizationStateReady (last state: {state})"
                )];
                if let Some(err) = &self.last_error {
                    lines.push(format!("last TDLib error: {err}"));
                }
                self.begin_close(1, lines)
            }
            TimeoutKind::Chats => {
                let mut lines =
                    vec!["chat list wait ended; printing titles received so far".to_string()];
                lines.extend(self.chat_lines());
                self.begin_close(0, lines)
            }
        }
    }

    fn while_closing(&mut self, typ: &str, event: &Value) -> Effect {
        match typ {
            "updateAuthorizationState" => self.on_auth(event),
            "error" => {
                let code = event["code"].as_i64().unwrap_or(0);
                let message = event["message"].as_str().unwrap_or("");
                if code == 406 || message.to_ascii_lowercase().contains("aborted") {
                    return Effect::default();
                }
                Effect {
                    lines: vec![format!("TDLib error {code}: {message}")],
                    ..Effect::default()
                }
            }
            _ => Effect::default(),
        }
    }

    fn on_auth(&mut self, event: &Value) -> Effect {
        let state = event["authorization_state"]["@type"]
            .as_str()
            .unwrap_or("unknown");
        self.last_auth = state.to_string();
        let mut lines = vec![format!("auth: {state}")];

        match state {
            "authorizationStateWaitTdlibParameters" if self.phase == Phase::WaitAuth => {
                if self.sent_parameters {
                    return Effect { lines, ..Effect::default() };
                }
                self.sent_parameters = true;
                lines.push(
                    "auth: sending setTdlibParameters (empty database_encryption_key, use_test_dc=false, file database, chat info database, message database, secret chats on)"
                        .to_string(),
                );
                Effect {
                    send: vec![set_tdlib_parameters(&self.cfg)],
                    lines,
                    exit: None,
                }
            }
            "authorizationStateWaitEncryptionKey" if self.phase == Phase::WaitAuth => {
                let encrypted = event["authorization_state"]["is_encrypted"]
                    .as_bool()
                    .unwrap_or(false);
                lines.push(format!(
                    "auth: legacy authorizationStateWaitEncryptionKey (is_encrypted={encrypted}); sending checkDatabaseEncryptionKey with an empty key"
                ));
                if encrypted {
                    lines.push(
                        "auth: TDLib reports the database is encrypted. An empty key will not open it."
                            .to_string(),
                    );
                }
                if self.sent_encryption_check {
                    return Effect { lines, ..Effect::default() };
                }
                self.sent_encryption_check = true;
                Effect {
                    send: vec![check_database_encryption_key()],
                    lines,
                    exit: None,
                }
            }
            "authorizationStateReady" if self.phase == Phase::WaitAuth => {
                self.saw_ready = true;
                self.phase = Phase::Collecting;
                lines.push("Ready".to_string());
                let mut effect = self.request_chats();
                effect.lines.splice(0..0, lines);
                effect
            }
            "authorizationStateWaitPhoneNumber"
            | "authorizationStateWaitCode"
            | "authorizationStateWaitPassword"
            | "authorizationStateWaitRegistration"
            | "authorizationStateWaitEmailAddress"
            | "authorizationStateWaitEmailCode"
            | "authorizationStateWaitOtherDeviceConfirmation"
            | "authorizationStateWaitPremiumPurchase" => self.begin_close(
                3,
                vec![
                    format!("auth: {state}"),
                    "authorization is incomplete. This spike does not ask for a phone number, code, or password. Point --database at a copy of a logged-in Mithka tdlib directory.".to_string(),
                ],
            ),
            "authorizationStateLoggingOut" => self.begin_close(
                1,
                vec![
                    format!("auth: {state}"),
                    "TDLib is logging out. This spike never sends logOut. If another client revoked the session, this copy can no longer open it.".to_string(),
                ],
            ),
            "authorizationStateClosing" => {
                lines.push("auth: TDLib is closing".to_string());
                if self.phase != Phase::Closing {
                    self.phase = Phase::Closing;
                    if self.pending_code.is_none() {
                        self.pending_code = Some(1);
                    }
                }
                Effect { lines, ..Effect::default() }
            }
            "authorizationStateClosed" => {
                let code = self.pending_code.unwrap_or(1);
                if self.pending_code.is_none() {
                    if self.saw_ready {
                        lines.push(
                            "TDLib closed before the chat list was printed.".to_string(),
                        );
                    } else {
                        lines.push("TDLib closed before Ready.".to_string());
                        if let Some(err) = &self.last_error {
                            lines.push(format!("last TDLib error: {err}"));
                        }
                    }
                }
                self.phase = Phase::Finished;
                self.pending_code = Some(code);
                lines.push("closed".to_string());
                Effect {
                    lines,
                    exit: Some(code),
                    ..Effect::default()
                }
            }
            _ => Effect { lines, ..Effect::default() },
        }
    }

    fn on_error(&mut self, event: &Value) -> Effect {
        let code = event["code"].as_i64().unwrap_or(0);
        let message = event["message"].as_str().unwrap_or("");
        let extra = extra_of(event);
        let rendered = format!("TDLib error {code}: {message}");
        self.last_error = Some(rendered.clone());
        let mut lines = vec![rendered];
        let hint = explain_error(code, message, extra);
        if let Some(hint) = hint {
            lines.push(format!("hint: {hint}"));
        }

        if extra.starts_with("getChat:") && hint.is_none() && code != 401 {
            if let Some(id) = parse_get_chat_extra(extra) {
                self.titles
                    .entry(id)
                    .or_insert_with(|| "(title unavailable)".to_string());
                self.waiting.retain(|waiting| *waiting != id);
                let mut effect = self.maybe_finish();
                effect.lines.splice(0..0, lines);
                return effect;
            }
        }

        if extra == "loadChats" && code == 404 {
            lines.push("loadChats: local chat list has no further pages".to_string());
            let mut effect = self.request_chats();
            effect.lines.splice(0..0, lines);
            return effect;
        }

        let fatal = extra == "setTdlibParameters"
            || extra == "checkDatabaseEncryptionKey"
            || hint.is_some()
            || code == 401;
        if fatal {
            return self.begin_close(1, lines);
        }
        Effect {
            lines,
            ..Effect::default()
        }
    }

    fn on_ok(&mut self, event: &Value) -> Effect {
        match extra_of(event) {
            "setTdlibParameters" => Effect {
                lines: vec!["setTdlibParameters accepted".to_string()],
                ..Effect::default()
            },
            "checkDatabaseEncryptionKey" => Effect {
                lines: vec!["checkDatabaseEncryptionKey accepted".to_string()],
                ..Effect::default()
            },
            "loadChats" if self.phase == Phase::Collecting => {
                let mut effect = self.request_chats();
                effect.lines.insert(0, "loadChats accepted".to_string());
                effect
            }
            _ => Effect::default(),
        }
    }

    fn on_chats(&mut self, event: &Value) -> Effect {
        if self.phase != Phase::Collecting {
            return Effect::default();
        }
        let mut ids = Vec::new();
        if let Some(arr) = event["chat_ids"].as_array() {
            for id in arr {
                if let Some(id) = id.as_i64() {
                    ids.push(id);
                }
            }
        }
        let limit = self.cfg.chat_limit.max(0) as usize;
        self.order = ids.into_iter().take(limit).collect();
        self.saw_chats = true;
        if self.order.is_empty() {
            if !self.did_load {
                self.did_load = true;
                return Effect {
                    send: vec![load_chats(self.cfg.chat_limit)],
                    lines: vec![
                        "getChats returned no chats; loading the local main list".to_string()
                    ],
                    exit: None,
                };
            }
            return self.finish_success();
        }
        self.waiting = self
            .order
            .iter()
            .copied()
            .filter(|id| !self.titles.contains_key(id))
            .collect();
        self.maybe_finish_with_fetches()
    }

    fn on_new_chat(&mut self, event: &Value) -> Effect {
        if let Some(chat) = event.get("chat") {
            self.remember_chat(chat);
        }
        self.maybe_finish()
    }

    fn on_chat_object(&mut self, event: &Value) -> Effect {
        self.remember_chat(event);
        self.maybe_finish()
    }

    fn on_title_update(&mut self, event: &Value) -> Effect {
        let id = event["chat_id"].as_i64();
        let title = event["title"].as_str();
        if let (Some(id), Some(title)) = (id, title) {
            self.remember(id, title);
        }
        self.maybe_finish()
    }

    fn remember_chat(&mut self, chat: &Value) {
        let id = chat["id"].as_i64();
        let title = chat["title"].as_str();
        if let (Some(id), Some(title)) = (id, title) {
            self.remember(id, title);
        }
    }

    fn remember(&mut self, id: i64, title: &str) {
        if !self.titles.contains_key(&id) {
            self.seen_order.push(id);
        }
        self.titles.insert(id, title.to_string());
        self.waiting.retain(|waiting| *waiting != id);
    }

    fn maybe_finish(&mut self) -> Effect {
        if self.phase != Phase::Collecting
            || !self.saw_chats
            || self.order.is_empty()
            || !self.waiting.is_empty()
        {
            return Effect::default();
        }
        self.finish_success()
    }

    fn maybe_finish_with_fetches(&mut self) -> Effect {
        if self.waiting.is_empty() {
            return self.finish_success();
        }
        let send = self
            .waiting
            .iter()
            .copied()
            .map(get_chat)
            .collect::<Vec<_>>();
        Effect {
            send,
            ..Effect::default()
        }
    }

    fn request_chats(&mut self) -> Effect {
        if self.get_chats_sent >= 3 {
            return self.finish_success();
        }
        self.get_chats_sent += 1;
        Effect {
            send: vec![get_chats(self.cfg.chat_limit)],
            ..Effect::default()
        }
    }

    fn finish_success(&mut self) -> Effect {
        if self.phase != Phase::Collecting {
            return Effect::default();
        }
        self.begin_close(0, self.chat_lines())
    }

    fn chat_lines(&self) -> Vec<String> {
        let order = if !self.order.is_empty() {
            self.order.clone()
        } else {
            self.seen_order
                .iter()
                .copied()
                .take(self.cfg.chat_limit.max(0) as usize)
                .collect::<Vec<_>>()
        };
        if order.is_empty() {
            return vec!["chats: none in the local main list".to_string()];
        }
        let mut lines = vec![format!("chats ({}):", order.len())];
        for (index, id) in order.iter().enumerate() {
            let title = self
                .titles
                .get(id)
                .map(|title| one_line(title))
                .unwrap_or_else(|| "(title unavailable)".to_string());
            lines.push(format!("{:>2}. {title}", index + 1));
        }
        lines
    }

    fn begin_close(&mut self, code: i32, mut lines: Vec<String>) -> Effect {
        if matches!(self.phase, Phase::Closing | Phase::Finished) {
            return Effect {
                lines,
                ..Effect::default()
            };
        }
        self.phase = Phase::Closing;
        self.pending_code = Some(code);
        lines.push("closing".to_string());
        Effect {
            send: vec![close_request()],
            lines,
            exit: None,
        }
    }
}

pub fn set_tdlib_parameters(cfg: &SessionConfig) -> Value {
    json!({
        "@type": "setTdlibParameters",
        "use_test_dc": false,
        "database_directory": cfg.database_directory,
        "files_directory": cfg.files_directory,
        "database_encryption_key": "",
        "use_file_database": true,
        "use_chat_info_database": true,
        "use_message_database": true,
        "use_secret_chats": true,
        "api_id": cfg.api_id,
        "api_hash": cfg.api_hash,
        "system_language_code": cfg.system_language_code,
        "device_model": cfg.device_model,
        "system_version": cfg.system_version,
        "application_version": cfg.application_version,
        "@extra": "setTdlibParameters"
    })
}

pub fn set_verbosity_request(level: i32) -> Value {
    json!({
        "@type": "setLogVerbosityLevel",
        "new_verbosity_level": level
    })
}

pub fn version_request() -> Value {
    json!({
        "@type": "getOption",
        "name": "version"
    })
}

pub fn bootstrap_request() -> Value {
    json!({
        "@type": "getOption",
        "name": "version",
        "@extra": "bootstrap"
    })
}

fn check_database_encryption_key() -> Value {
    json!({
        "@type": "checkDatabaseEncryptionKey",
        "encryption_key": "",
        "@extra": "checkDatabaseEncryptionKey"
    })
}

fn get_chats(limit: i32) -> Value {
    json!({
        "@type": "getChats",
        "chat_list": {"@type": "chatListMain"},
        "limit": limit,
        "@extra": "getChats"
    })
}

fn load_chats(limit: i32) -> Value {
    json!({
        "@type": "loadChats",
        "chat_list": {"@type": "chatListMain"},
        "limit": limit,
        "@extra": "loadChats"
    })
}

fn get_chat(chat_id: i64) -> Value {
    json!({
        "@type": "getChat",
        "chat_id": chat_id,
        "@extra": format!("getChat:{chat_id}")
    })
}

fn close_request() -> Value {
    json!({
        "@type": "close",
        "@extra": "close"
    })
}

pub fn parse_version(response: &str) -> Option<String> {
    let value: Value = serde_json::from_str(response).ok()?;
    if value["@type"] == "optionValueString" {
        return value["value"].as_str().map(str::to_string);
    }
    None
}

pub fn explain_error(code: i64, message: &str, extra: &str) -> Option<&'static str> {
    let m = message.to_ascii_lowercase();
    if is_lock(&m) {
        return Some(LOCK_HINT);
    }
    if is_generation(&m) {
        return Some(GENERATION_HINT);
    }
    if is_session(&m) {
        return Some(SESSION_HINT);
    }
    if is_encryption(code, &m, extra) {
        return Some(ENCRYPTION_HINT);
    }
    None
}

const LOCK_HINT: &str = "Another process has this database open, or the copy was taken while Mithka was still running. Quit Mithka completely, copy the directory again, and point --database at the copy. Never run two processes on one live td.binlog.";
const ENCRYPTION_HINT: &str = "database_encryption_key must stay empty for a Mithka database. A different key returns 401 and cannot open the existing session. This spike always sends an empty key; do not change it.";
const GENERATION_HINT: &str = "This libtdjson.so does not match the database generation. Use the same Mithka 1.8.67 patched build that wrote the session (the libtdjson.so from that app bundle, iebb/mithka-tdjson).";
const SESSION_HINT: &str = "Telegram rejected the copied session (logged out or revoked). This is separate from the local encryption key when the message does not mention encryption.";

fn is_lock(message: &str) -> bool {
    message.contains("can't lock")
        || message.contains("cannot lock")
        || message.contains("database is locked")
        || message.contains("resource busy")
        || message.contains("already opened")
        || message.contains("file is locked")
        || message.contains("unable to lock")
}

fn is_generation(message: &str) -> bool {
    message.contains("future tdlib")
        || message.contains("from a newer")
        || message.contains("from a future")
        || message.contains("incompatible database")
        || message.contains("unsupported database")
        || message.contains("wrong database version")
        || message.contains("database version")
        || message.contains("unknown database format")
        || message.contains("downgrade")
        || message.contains("generation")
}

fn is_session(message: &str) -> bool {
    message.contains("session revoked")
        || message.contains("session expired")
        || message.contains("logged out")
        || message.contains("auth_key_unregistered")
        || message.contains("unauthorized")
}

fn is_encryption(code: i64, message: &str, extra: &str) -> bool {
    if message.contains("encryption key") || message.contains("wrong key") {
        return true;
    }
    code == 401 && (extra == "setTdlibParameters" || extra == "checkDatabaseEncryptionKey")
}

fn event_type(event: &Value) -> &str {
    event["@type"].as_str().unwrap_or("")
}

fn extra_of(event: &Value) -> &str {
    event["@extra"].as_str().unwrap_or("")
}

fn parse_get_chat_extra(extra: &str) -> Option<i64> {
    extra.strip_prefix("getChat:")?.parse().ok()
}

fn one_line(title: &str) -> String {
    title.replace(['\n', '\r'], " ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> SessionConfig {
        SessionConfig {
            database_directory: "/tmp/mithka-copy/tdlib".to_string(),
            files_directory: "/tmp/mithka-copy/tdlib/files".to_string(),
            api_id: 100,
            api_hash: "unit-test-hash-not-a-secret".to_string(),
            device_model: "Android".to_string(),
            system_language_code: "en".to_string(),
            system_version: "Linux".to_string(),
            application_version: "mithka-tdlib-spike/0.1.0".to_string(),
            chat_limit: 20,
        }
    }

    fn assert_no_logout(send: &[Value]) {
        for request in send {
            assert_ne!(request["@type"], "logOut", "{request}");
        }
    }

    fn assert_lines_hide_hash(lines: &[String], hash: &str) {
        for line in lines {
            assert!(!line.contains(hash), "{line}");
        }
    }

    #[test]
    fn parameters_use_an_empty_key_and_mithka_flags() {
        let value = set_tdlib_parameters(&cfg());
        assert_eq!(value["@type"], "setTdlibParameters");
        assert_eq!(value["use_test_dc"], false);
        assert_eq!(value["database_encryption_key"], "");
        assert_eq!(value["use_file_database"], true);
        assert_eq!(value["use_chat_info_database"], true);
        assert_eq!(value["use_message_database"], true);
        assert_eq!(value["use_secret_chats"], true);
        assert_eq!(value["device_model"], "Android");
        assert_eq!(value["system_language_code"], "en");
        assert_eq!(value["application_version"], "mithka-tdlib-spike/0.1.0");
        assert_eq!(value["database_directory"], "/tmp/mithka-copy/tdlib");
        assert_eq!(value["files_directory"], "/tmp/mithka-copy/tdlib/files");
        assert_eq!(value["api_id"], 100);
        assert_eq!(value["api_hash"], "unit-test-hash-not-a-secret");
        assert!(value.get("parameters").is_none());
    }

    #[test]
    fn happy_path_prints_titles_then_closes() {
        let hash = cfg().api_hash.clone();
        let mut driver = Driver::new(cfg());
        let wait = json!({
            "@type": "updateAuthorizationState",
            "authorization_state": {"@type": "authorizationStateWaitTdlibParameters"}
        });
        let first = driver.on_event(&wait);
        assert_eq!(first.send.len(), 1);
        assert_eq!(first.send[0]["database_encryption_key"], "");
        assert_lines_hide_hash(&first.lines, &hash);
        assert_no_logout(&first.send);

        let again = driver.on_event(&wait);
        assert!(again.send.is_empty());

        let accepted = driver.on_event(&json!({"@type": "ok", "@extra": "setTdlibParameters"}));
        assert!(accepted.lines.iter().any(|line| line.contains("accepted")));

        let ready = driver.on_event(&json!({
            "@type": "updateAuthorizationState",
            "authorization_state": {"@type": "authorizationStateReady"}
        }));
        assert!(ready.lines.iter().any(|line| line == "Ready"));
        assert_eq!(ready.send[0]["@type"], "getChats");
        assert_eq!(ready.send[0]["chat_list"]["@type"], "chatListMain");

        let news = driver.on_event(&json!({
            "@type": "updateNewChat",
            "chat": {"@type": "chat", "id": 101, "title": "Alpha"}
        }));
        assert!(news.send.is_empty());

        let chats = driver.on_event(&json!({
            "@type": "chats",
            "total_count": 2,
            "chat_ids": [101, 102],
            "@extra": "getChats"
        }));
        assert_eq!(chats.send.len(), 1);
        assert_eq!(chats.send[0]["@type"], "getChat");
        assert_eq!(chats.send[0]["chat_id"], 102);

        let title = driver.on_event(&json!({
            "@type": "chat",
            "id": 102,
            "title": "Beta\nline",
            "@extra": "getChat:102"
        }));
        let printed = title.lines.join("\n");
        assert!(printed.contains("1. Alpha"), "{printed}");
        assert!(printed.contains("2. Beta line"), "{printed}");
        assert_eq!(title.send[0]["@type"], "close");
        assert!(title.exit.is_none());
        assert_no_logout(&title.send);

        let closed = driver.on_event(&json!({
            "@type": "updateAuthorizationState",
            "authorization_state": {"@type": "authorizationStateClosed"}
        }));
        assert_eq!(closed.exit, Some(0));
    }

    #[test]
    fn empty_chat_list_loads_then_reports_none() {
        let mut driver = Driver::new(cfg());
        driver.on_event(&json!({
            "@type": "updateAuthorizationState",
            "authorization_state": {"@type": "authorizationStateWaitTdlibParameters"}
        }));
        driver.on_event(&json!({
            "@type": "updateAuthorizationState",
            "authorization_state": {"@type": "authorizationStateReady"}
        }));
        let empty =
            driver.on_event(&json!({"@type": "chats", "chat_ids": [], "@extra": "getChats"}));
        assert_eq!(empty.send[0]["@type"], "loadChats");
        let again = driver.on_event(&json!({"@type": "ok", "@extra": "loadChats"}));
        assert_eq!(again.send[0]["@type"], "getChats");
        let done =
            driver.on_event(&json!({"@type": "chats", "chat_ids": [], "@extra": "getChats"}));
        assert!(done.lines.iter().any(|line| line.contains("none")));
        assert_eq!(done.send[0]["@type"], "close");
    }

    #[test]
    fn load_chats_404_retries_get_chats() {
        let mut driver = Driver::new(cfg());
        driver.on_event(&json!({
            "@type": "updateAuthorizationState",
            "authorization_state": {"@type": "authorizationStateReady"}
        }));
        driver.on_event(&json!({"@type": "chats", "chat_ids": [], "@extra": "getChats"}));
        let missing = driver.on_event(&json!({
            "@type": "error",
            "code": 404,
            "message": "Not Found",
            "@extra": "loadChats"
        }));
        assert!(missing.lines.iter().any(|line| line.contains("404")));
        assert_eq!(missing.send[0]["@type"], "getChats");
        assert!(missing.exit.is_none());
    }

    #[test]
    fn lock_error_is_explained_and_not_a_panic_path() {
        let mut driver = Driver::new(cfg());
        driver.on_event(&json!({
            "@type": "updateAuthorizationState",
            "authorization_state": {"@type": "authorizationStateWaitTdlibParameters"}
        }));
        let err = driver.on_event(&json!({
            "@type": "error",
            "code": 400,
            "message": "Can't lock file \"/tmp/td.binlog\"",
            "@extra": "setTdlibParameters"
        }));
        let text = err.lines.join("\n");
        assert!(text.contains("TDLib error 400"), "{text}");
        assert!(text.contains("hint:"), "{text}");
        assert!(text.to_ascii_lowercase().contains("copy"), "{text}");
        assert_eq!(err.send[0]["@type"], "close");
        let closed = driver.on_event(&json!({
            "@type": "updateAuthorizationState",
            "authorization_state": {"@type": "authorizationStateClosed"}
        }));
        assert_eq!(closed.exit, Some(1));
    }

    #[test]
    fn encryption_401_names_the_empty_key() {
        let mut driver = Driver::new(cfg());
        let err = driver.on_event(&json!({
            "@type": "error",
            "code": 401,
            "message": "Wrong database encryption key",
            "@extra": "setTdlibParameters"
        }));
        let text = err.lines.join("\n");
        assert!(text.contains("401"), "{text}");
        assert!(text.contains("empty"), "{text}");
        assert_eq!(err.send[0]["@type"], "close");
    }

    #[test]
    fn generation_mismatch_is_explained() {
        let hint = explain_error(
            400,
            "database is from a future TDLib version",
            "setTdlibParameters",
        );
        assert!(hint.unwrap().contains("1.8.67"));
    }

    #[test]
    fn phone_state_exits_without_asking_for_a_code() {
        let mut driver = Driver::new(cfg());
        let effect = driver.on_event(&json!({
            "@type": "updateAuthorizationState",
            "authorization_state": {"@type": "authorizationStateWaitPhoneNumber"}
        }));
        let text = effect.lines.join("\n");
        assert!(text.contains("authorizationStateWaitPhoneNumber"), "{text}");
        assert!(text.contains("does not ask"), "{text}");
        assert_eq!(effect.send[0]["@type"], "close");
        assert_no_logout(&effect.send);
        let closed = driver.on_event(&json!({
            "@type": "updateAuthorizationState",
            "authorization_state": {"@type": "authorizationStateClosed"}
        }));
        assert_eq!(closed.exit, Some(3));
    }

    #[test]
    fn legacy_encryption_state_sends_an_empty_check() {
        let mut driver = Driver::new(cfg());
        let effect = driver.on_event(&json!({
            "@type": "updateAuthorizationState",
            "authorization_state": {
                "@type": "authorizationStateWaitEncryptionKey",
                "is_encrypted": false
            }
        }));
        assert_eq!(effect.send[0]["@type"], "checkDatabaseEncryptionKey");
        assert_eq!(effect.send[0]["encryption_key"], "");
    }

    #[test]
    fn interrupt_closes_instead_of_logging_out() {
        let mut driver = Driver::new(cfg());
        let effect = driver.interrupt();
        assert_eq!(effect.send.len(), 1);
        assert_eq!(effect.send[0]["@type"], "close");
        assert_no_logout(&effect.send);
    }
}
