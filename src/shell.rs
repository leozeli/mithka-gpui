//! Long-lived TDLib session shared by the desktop shells.
//!
//! The CLI driver exits after printing titles. This one stays open: it keeps
//! the main chat list, loads text and photo history, and sends plain-text messages.
//! The receive loop runs on a background thread so the UI can own the main thread.

use crate::driver::{
    bootstrap_request, explain_error, parse_version, set_tdlib_parameters, set_verbosity_request,
    version_request, SessionConfig,
};
use crate::tdjson::TdJson;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const HISTORY_LIMIT: i32 = 40;
const SYNTHETIC_ORDER: i64 = 1_000_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChatItem {
    pub id: i64,
    pub title: String,
    pub order: i64,
    /// `unread_count` from the chat, or the latest `updateChatReadInbox`.
    pub unread: i32,
    /// TDLib `is_marked_as_unread` when the numeric count is zero.
    pub marked_unread: bool,
    /// Local path of the small chat photo, once TDLib has the file.
    pub avatar: Option<String>,
    /// One-line preview of `last_message` / `updateChatLastMessage`.
    pub preview: String,
    /// Unix time of that last message. `0` when TDLib has none.
    pub preview_date: i64,
    real_order: bool,
}

fn empty_chat(id: i64) -> ChatItem {
    ChatItem {
        id,
        title: "…".into(),
        order: 0,
        unread: 0,
        marked_unread: false,
        avatar: None,
        preview: String::new(),
        preview_date: 0,
        real_order: false,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MessageKind {
    Text,
    Photo,
}

/// Which local path a finished `downloadFile` should fill.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PhotoSlot {
    /// Chat-bubble preview. Prefer `m`, then `s`.
    Preview,
    /// Popup. The largest `photoSize` on the message.
    Full,
    /// Preview and popup are the same file id, so one download fills both.
    Both,
}

#[derive(Clone, Debug)]
enum FileRole {
    Avatar(i64),
    ContactAvatar(i64),
    MessagePhoto {
        chat_id: i64,
        message_id: i64,
        slot: PhotoSlot,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FolderItem {
    pub id: i32,
    pub title: String,
}

/// One row in the contacts panel. `username` is empty when TDLib has none.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContactItem {
    pub user_id: i64,
    pub name: String,
    /// Active username without a leading `@`.
    pub username: String,
    /// Local path of `profile_photo.small`, once that file has downloaded.
    pub avatar: Option<String>,
}

#[derive(Clone, Debug)]
struct ContactMeta {
    name: String,
    username: String,
    avatar: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextMessage {
    pub id: i64,
    pub chat_id: i64,
    pub sender: String,
    /// Plain text, or the photo caption when `kind` is [`MessageKind::Photo`].
    pub text: String,
    pub date: i64,
    pub kind: MessageKind,
    /// Local path of the chat-bubble preview (`m`, else `s`, else a larger size).
    /// Empty until that `downloadFile` finishes.
    pub photo: Option<String>,
    /// Local path of the largest `photoSize` on the message. The popup uses this,
    /// not `photo`, so a 320px preview is not stretched to the window.
    pub photo_full: Option<String>,
    /// Pixel width of the size chosen for `photo_full`. `0` when TDLib omitted it.
    pub photo_full_width: i32,
    /// Pixel height of the size chosen for `photo_full`. `0` when TDLib omitted it.
    pub photo_full_height: i32,
    /// Clickable `http`/`https` spans. Offsets are UTF-8 byte indexes into `text`.
    pub links: Vec<TextLink>,
    /// TDLib `message.is_outgoing`. Receipts are only drawn for these.
    pub outgoing: bool,
    /// Outgoing message whose id is at or below the chat's
    /// `last_read_outbox_message_id`. Incoming messages stay false.
    /// `interaction_info` is views, forwards, replies, and reactions, not a receipt.
    pub read: bool,
}

/// A URL span inside a message body. `start`/`end` are UTF-8 byte offsets.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextLink {
    pub start: usize,
    pub end: usize,
    pub url: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UiUpdate {
    Log(String),
    Status(String),
    Ready,
    ChatList(Vec<ChatItem>),
    Conversation {
        chat_id: i64,
        title: String,
        messages: Vec<TextMessage>,
    },
    /// Telegram chat folders. The main list is not included; the UI adds All.
    Folders(Vec<FolderItem>),
    /// Chats returned for one search query. The UI merges these with the
    /// folder-filtered list; this does not replace [`UiUpdate::ChatList`].
    SearchResults {
        query: String,
        chats: Vec<ChatItem>,
    },
    /// Full contact list, in `getContacts` order, refreshed from `updateUser`.
    Contacts(Vec<ContactItem>),
    /// `createPrivateChat` returned a chat. The UI should select it, then the
    /// following [`UiUpdate::Conversation`] fills the transcript.
    OpenChat(i64),
    Fatal(String),
}

#[derive(Clone, Debug)]
pub enum ShellCommand {
    SelectChat(i64),
    SendText {
        chat_id: i64,
        text: String,
    },
    /// `None` is the main chat list.
    SelectFolder(Option<i32>),
    /// Older `getChatHistory` page for the chat that is already open.
    LoadOlder {
        chat_id: i64,
    },
    /// Global chat search. An empty query clears remote hits.
    Search(String),
    /// `getContacts`. Later `updateUser` events keep the list current.
    LoadContacts,
    /// `createPrivateChat`, then the same open path as [`ShellCommand::SelectChat`].
    OpenContact(i64),
    Close,
}

struct StoredMessage {
    sender_id: Option<i64>,
    sender_chat: Option<i64>,
    outgoing: bool,
    text: String,
    date: i64,
    kind: MessageKind,
    photo: Option<String>,
    photo_full: Option<String>,
    photo_full_width: i32,
    photo_full_height: i32,
    links: Vec<TextLink>,
}

pub struct Shell {
    cfg: SessionConfig,
    ready: bool,
    closing: bool,
    finished: bool,
    failed: bool,
    sent_parameters: bool,
    sent_encryption_check: bool,
    did_load: bool,
    get_chats_sent: u8,
    my_user_id: Option<i64>,
    open_chat: Option<i64>,
    users: HashMap<i64, String>,
    chats: HashMap<i64, ChatItem>,
    listed: Vec<i64>,
    messages: HashMap<i64, HashMap<i64, StoredMessage>>,
    file_roles: HashMap<i32, FileRole>,
    downloads_sent: HashSet<i32>,
    queued_sends: Vec<Value>,
    folders: Vec<FolderItem>,
    /// `None` shows `chatListMain`.
    active_folder: Option<i32>,
    /// chat id -> folder id -> order in that folder.
    folder_orders: HashMap<i64, HashMap<i32, i64>>,
    folder_fetches: HashSet<i32>,
    /// Chats with a `getChatHistory` request that has not answered yet.
    history_pending: HashSet<i64>,
    /// `from_message_id` of that in-flight request. `0` is the latest page.
    history_from: HashMap<i64, i64>,
    /// Older pages that came back empty or already known.
    history_exhausted: HashSet<i64>,
    /// `chat.last_read_outbox_message_id` / `updateChatReadOutbox`.
    last_read_outbox: HashMap<i64, i64>,
    /// Trimmed query of the in-flight global search. Empty means no search.
    search_query: String,
    search_gen: u64,
    /// How many search responses are still outstanding for `search_gen`.
    search_pending: u8,
    search_error: bool,
    /// Chat ids for `search_query`, in the order replies arrived, unique.
    search_ids: Vec<i64>,
    /// `getContacts` order. `updateUser` with `is_contact` inserts or removes.
    contact_ids: Vec<i64>,
    contact_meta: HashMap<i64, ContactMeta>,
}

struct Effect {
    send: Vec<Value>,
    ui: Vec<UiUpdate>,
}

impl Shell {
    fn new(cfg: SessionConfig) -> Self {
        Self {
            cfg,
            ready: false,
            closing: false,
            finished: false,
            failed: false,
            sent_parameters: false,
            sent_encryption_check: false,
            did_load: false,
            get_chats_sent: 0,
            my_user_id: None,
            open_chat: None,
            users: HashMap::new(),
            chats: HashMap::new(),
            listed: Vec::new(),
            messages: HashMap::new(),
            file_roles: HashMap::new(),
            downloads_sent: HashSet::new(),
            queued_sends: Vec::new(),
            folders: Vec::new(),
            active_folder: None,
            folder_orders: HashMap::new(),
            folder_fetches: HashSet::new(),
            history_pending: HashSet::new(),
            history_from: HashMap::new(),
            history_exhausted: HashSet::new(),
            last_read_outbox: HashMap::new(),
            search_query: String::new(),
            search_gen: 0,
            search_pending: 0,
            search_error: false,
            search_ids: Vec::new(),
            contact_ids: Vec::new(),
            contact_meta: HashMap::new(),
        }
    }

    fn on_command(&mut self, command: ShellCommand) -> Effect {
        if self.finished {
            return Effect::none();
        }
        match command {
            ShellCommand::Close => self.begin_close(Vec::new()),
            ShellCommand::SelectChat(chat_id) => self.select_chat(chat_id),
            ShellCommand::LoadOlder { chat_id } => self.load_older(chat_id),
            ShellCommand::SendText { chat_id, text } => {
                let text = text.trim().to_string();
                if text.is_empty() || !self.ready || self.failed {
                    return Effect::none();
                }
                Effect::send(vec![send_text_request(chat_id, &text)])
            }
            ShellCommand::SelectFolder(folder) => self.select_folder(folder),
            ShellCommand::Search(query) => self.search(query),
            ShellCommand::LoadContacts => self.load_contacts(),
            ShellCommand::OpenContact(user_id) => self.open_contact(user_id),
        }
    }

    fn on_event(&mut self, event: &Value) -> Effect {
        if self.finished {
            return Effect::none();
        }
        let typ = event["@type"].as_str().unwrap_or("");
        if self.closing {
            return match typ {
                "updateAuthorizationState" => self.on_auth(event),
                "error" => {
                    let code = event["code"].as_i64().unwrap_or(0);
                    let message = event["message"].as_str().unwrap_or("");
                    if code == 406 || message.to_ascii_lowercase().contains("aborted") {
                        Effect::none()
                    } else {
                        Effect::ui(vec![UiUpdate::Log(format!(
                            "TDLib error {code}: {message}"
                        ))])
                    }
                }
                _ => Effect::none(),
            };
        }
        match typ {
            "updateAuthorizationState" => self.on_auth(event),
            "error" => self.on_error(event),
            "ok" => self.on_ok(event),
            "chats" => self.on_chats(event),
            "chat" | "updateNewChat" => self.on_chat_event(event),
            "updateChatTitle" => self.on_title(event),
            "updateChatPosition" => self.on_position(event),
            "updateChatReadInbox" => self.on_read_inbox(event),
            "updateChatReadOutbox" => self.on_read_outbox(event),
            "updateChatIsMarkedAsUnread" => self.on_marked_unread(event),
            "updateChatLastMessage" => self.on_last_message(event),
            "updateChatPhoto" => self.on_chat_photo(event),
            "file" | "updateFile" => self.on_file(event),
            "chatFolders" | "updateChatFolders" => self.on_folders(event),
            "user" | "updateUser" => self.on_user(event),
            "users" => self.on_users(event),
            "messages" => self.on_messages(event),
            "message" | "updateNewMessage" => self.on_message_event(event),
            "updateMessageContent" => self.on_message_content(event),
            _ => Effect::none(),
        }
    }

    fn on_auth(&mut self, event: &Value) -> Effect {
        let state = event["authorization_state"]["@type"]
            .as_str()
            .unwrap_or("unknown");
        let mut ui = vec![
            UiUpdate::Log(format!("auth: {state}")),
            UiUpdate::Status(status_for(state)),
        ];
        match state {
            "authorizationStateWaitTdlibParameters" if !self.sent_parameters && !self.closing => {
                self.sent_parameters = true;
                ui.push(UiUpdate::Log(
                    "auth: sending setTdlibParameters (empty database_encryption_key, use_test_dc=false, file database, chat info database, message database, secret chats on)".into(),
                ));
                Effect {
                    send: vec![set_tdlib_parameters(&self.cfg)],
                    ui,
                }
            }
            "authorizationStateWaitEncryptionKey" if !self.closing => {
                let encrypted = event["authorization_state"]["is_encrypted"]
                    .as_bool()
                    .unwrap_or(false);
                ui.push(UiUpdate::Log(format!(
                    "auth: legacy authorizationStateWaitEncryptionKey (is_encrypted={encrypted}); sending an empty checkDatabaseEncryptionKey"
                )));
                if self.sent_encryption_check {
                    return Effect { send: Vec::new(), ui };
                }
                self.sent_encryption_check = true;
                Effect {
                    send: vec![json!({
                        "@type": "checkDatabaseEncryptionKey",
                        "encryption_key": "",
                        "@extra": "checkDatabaseEncryptionKey"
                    })],
                    ui,
                }
            }
            "authorizationStateReady" if !self.ready && !self.closing => {
                self.ready = true;
                ui.push(UiUpdate::Log("Ready".into()));
                ui.push(UiUpdate::Ready);
                ui.push(UiUpdate::Status("Ready".into()));
                let mut effect = self.request_chats();
                effect.send.insert(0, get_me_request());
                effect.send.insert(1, load_chats(self.cfg.chat_limit));
                effect.ui.splice(0..0, ui);
                effect
            }
            "authorizationStateWaitPhoneNumber"
            | "authorizationStateWaitCode"
            | "authorizationStateWaitPassword"
            | "authorizationStateWaitRegistration"
            | "authorizationStateWaitEmailAddress"
            | "authorizationStateWaitEmailCode"
            | "authorizationStateWaitOtherDeviceConfirmation"
            | "authorizationStateWaitPremiumPurchase" => Effect::ui(vec![
                UiUpdate::Log(format!("auth: {state}")),
                UiUpdate::Status(
                    "Not logged in. This window does not ask for a phone number, code, or password. Point --database at a copy of a logged-in Mithka database."
                        .into(),
                ),
            ]),
            "authorizationStateLoggingOut" => self.fail(
                "TDLib is logging out. This program never sends logOut.".into(),
            ),
            "authorizationStateClosed" => {
                self.finished = true;
                ui.push(UiUpdate::Log("closed".into()));
                Effect { send: Vec::new(), ui }
            }
            _ => Effect { send: Vec::new(), ui },
        }
    }

    fn on_error(&mut self, event: &Value) -> Effect {
        let code = event["code"].as_i64().unwrap_or(0);
        let message = event["message"].as_str().unwrap_or("");
        let extra = event["@extra"].as_str().unwrap_or("");
        let mut lines = vec![format!("TDLib error {code}: {message}")];
        let hint = explain_error(code, message, extra);
        if let Some(hint) = hint {
            lines.push(format!("hint: {hint}"));
        }
        let rendered = lines.join("\n");
        if extra == "loadChats" && code == 404 {
            let mut effect = self.request_chats();
            effect.ui.insert(
                0,
                UiUpdate::Log("loadChats: local chat list has no further pages".into()),
            );
            return effect;
        }
        if let Some(chat_id) = extra.strip_prefix("history:") {
            if let Ok(chat_id) = chat_id.parse::<i64>() {
                self.history_pending.remove(&chat_id);
                self.history_from.remove(&chat_id);
            }
        }
        if let Some(gen) = search_generation(extra) {
            let current = gen == self.search_gen;
            if current {
                self.search_error = true;
            }
            self.note_search_done(gen);
            let mut ui = vec![UiUpdate::Log(rendered)];
            if current {
                ui.push(UiUpdate::Status(format!("Search error {code}: {message}")));
            }
            return Effect::ui(ui);
        }
        if extra == "getContacts" {
            return Effect::ui(vec![
                UiUpdate::Log(rendered),
                UiUpdate::Status(format!("Contacts error {code}: {message}")),
            ]);
        }
        if extra.starts_with("createPrivateChat:") {
            return Effect::ui(vec![
                UiUpdate::Log(rendered),
                UiUpdate::Status(format!("Could not open contact: {message}")),
            ]);
        }
        if extra.starts_with("getChat:")
            || extra.starts_with("history:")
            || extra.starts_with("getUser:")
            || extra.starts_with("download:")
            || extra.starts_with("loadChats:folder:")
            || extra.starts_with("getChats:folder:")
            || extra.starts_with("view:")
            || extra.starts_with("openChat:")
            || extra.starts_with("closeChat:")
            || extra.starts_with("marked:")
        {
            return Effect::ui(vec![UiUpdate::Log(rendered)]);
        }
        let fatal = extra == "setTdlibParameters"
            || extra == "checkDatabaseEncryptionKey"
            || hint.is_some()
            || code == 401;
        if fatal {
            return self.fail(rendered);
        }
        Effect::ui(vec![UiUpdate::Log(rendered)])
    }

    fn on_ok(&mut self, event: &Value) -> Effect {
        match event["@extra"].as_str().unwrap_or("") {
            "setTdlibParameters" => {
                Effect::ui(vec![UiUpdate::Log("setTdlibParameters accepted".into())])
            }
            "checkDatabaseEncryptionKey" => Effect::ui(vec![UiUpdate::Log(
                "checkDatabaseEncryptionKey accepted".into(),
            )]),
            "loadChats" if self.ready => self.request_chats(),
            _ => Effect::none(),
        }
    }

    fn on_chats(&mut self, event: &Value) -> Effect {
        let extra = event["@extra"].as_str().unwrap_or("");
        if let Some(folder_id) = extra.strip_prefix("getChats:folder:") {
            return self.on_folder_chats(folder_id, event);
        }
        if let Some(gen) = search_generation(extra) {
            return self.on_search_chats(gen, event);
        }
        let mut ids = Vec::new();
        if let Some(arr) = event["chat_ids"].as_array() {
            for id in arr {
                if let Some(id) = json_i64(id) {
                    ids.push(id);
                }
            }
        }
        let limit = self.cfg.chat_limit.max(0) as usize;
        self.listed = ids.into_iter().take(limit).collect();
        let mut missing = Vec::new();
        for (index, id) in self.listed.iter().copied().enumerate() {
            let chat = self.chats.entry(id).or_insert_with(|| empty_chat(id));
            if !chat.real_order {
                chat.order = SYNTHETIC_ORDER - index as i64;
            }
            if chat.title == "…" || chat.title.is_empty() {
                missing.push(id);
            }
        }
        if self.listed.is_empty() && !self.did_load {
            self.did_load = true;
            return Effect::send(vec![load_chats(self.cfg.chat_limit)]);
        }
        let mut effect = Effect::send(missing.into_iter().map(get_chat_request).collect());
        effect.ui.push(self.chat_list_update());
        effect
    }

    fn on_folder_chats(&mut self, folder_id: &str, event: &Value) -> Effect {
        let Some(folder_id) = folder_id.parse::<i32>().ok() else {
            return Effect::none();
        };
        let mut ids = Vec::new();
        if let Some(arr) = event["chat_ids"].as_array() {
            for id in arr {
                if let Some(id) = json_i64(id) {
                    ids.push(id);
                }
            }
        }
        let limit = self.cfg.chat_limit.max(0) as usize;
        let mut missing = Vec::new();
        for (index, id) in ids.into_iter().take(limit).enumerate() {
            let orders = self.folder_orders.entry(id).or_default();
            orders
                .entry(folder_id)
                .or_insert(SYNTHETIC_ORDER - index as i64);
            let chat = self.chats.entry(id).or_insert_with(|| empty_chat(id));
            if chat.title == "…" || chat.title.is_empty() {
                missing.push(id);
            }
        }
        let mut effect = Effect::send(missing.into_iter().map(get_chat_request).collect());
        effect.ui.push(self.chat_list_update());
        effect
    }

    fn select_folder(&mut self, folder: Option<i32>) -> Effect {
        if !self.ready || self.failed {
            return Effect::none();
        }
        self.active_folder = folder;
        let mut effect = Effect::ui(vec![self.chat_list_update()]);
        if let Some(folder_id) = folder {
            if self.folder_fetches.insert(folder_id) {
                let limit = self.cfg.chat_limit;
                effect.send.push(load_folder_chats(folder_id, limit));
                effect.send.push(get_folder_chats(folder_id, limit));
            }
        }
        effect
    }

    fn on_folders(&mut self, event: &Value) -> Effect {
        let mut folders = Vec::new();
        if let Some(arr) = event["chat_folders"].as_array() {
            for info in arr {
                let Some(id) = json_i32(&info["id"]) else {
                    continue;
                };
                folders.push(FolderItem {
                    id,
                    title: folder_title(info),
                });
            }
        }
        if let Some(active) = self.active_folder {
            if !folders.iter().any(|folder| folder.id == active) {
                self.active_folder = None;
            }
        }
        self.folders = folders.clone();
        Effect::ui(vec![UiUpdate::Folders(folders), self.chat_list_update()])
    }

    fn on_chat_event(&mut self, event: &Value) -> Effect {
        let extra = event["@extra"].as_str().unwrap_or("").to_string();
        let chat = event.get("chat").unwrap_or(event);
        let chat_id = json_i64(&chat["id"]);
        let was_listed = chat_id.is_some_and(|id| self.listed.contains(&id));
        let outbox_changed = if chat["@type"].as_str() == Some("chat")
            || event["@type"].as_str() == Some("updateNewChat")
        {
            self.remember_chat(chat)
        } else {
            false
        };
        if extra.starts_with("getChat:search:") {
            if let Some(id) = chat_id {
                self.detach_unlisted_search_chat(id, was_listed);
            }
        }
        let mut effect = Effect::send(self.drain_sends());
        effect.ui.push(self.chat_list_update());
        if let Some(id) = chat_id {
            if !self.search_query.is_empty() && self.search_ids.contains(&id) {
                effect.ui.push(self.search_update());
                if self.search_pending == 0 && !self.search_error {
                    effect
                        .ui
                        .push(UiUpdate::Status(search_status(self.search_chats().len())));
                }
            }
            if outbox_changed && self.open_chat == Some(id) {
                effect.ui.push(self.conversation_update(id));
            }
        }
        if extra
            .strip_prefix("createPrivateChat:")
            .is_some_and(|rest| rest.parse::<i64>().is_ok())
        {
            if let Some(id) = chat_id {
                let opened = self.select_chat(id);
                effect.send.extend(opened.send);
                effect.ui.insert(0, UiUpdate::OpenChat(id));
                effect.ui.extend(opened.ui);
                if self.ready {
                    effect.ui.push(UiUpdate::Status("Ready".into()));
                }
            }
        }
        effect
    }

    /// A search hit that was not already on a chat list stays out of All chats.
    fn detach_unlisted_search_chat(&mut self, id: i64, was_listed: bool) {
        if was_listed {
            return;
        }
        let on_main = self
            .chats
            .get(&id)
            .is_some_and(|chat| chat.real_order && chat.order != 0);
        if on_main || self.chat_in_some_folder(id) {
            return;
        }
        self.listed.retain(|chat_id| *chat_id != id);
    }

    fn on_title(&mut self, event: &Value) -> Effect {
        let Some(id) = json_i64(&event["chat_id"]) else {
            return Effect::none();
        };
        let Some(title) = event["title"].as_str() else {
            return Effect::none();
        };
        if let Some(chat) = self.chats.get_mut(&id) {
            chat.title = title.to_string();
        }
        let mut ui = vec![self.chat_list_update()];
        if self.open_chat == Some(id) {
            ui.push(self.conversation_update(id));
        }
        Effect::ui(ui)
    }

    fn on_position(&mut self, event: &Value) -> Effect {
        let Some(id) = json_i64(&event["chat_id"]) else {
            return Effect::none();
        };
        let position = &event["position"];
        let list_type = position["list"]["@type"].as_str().unwrap_or("");
        if list_type == "chatListFolder" {
            let Some(folder_id) = json_i32(&position["list"]["chat_folder_id"]) else {
                return Effect::none();
            };
            let order = json_i64(&position["order"]).unwrap_or(0);
            if order == 0 {
                if let Some(orders) = self.folder_orders.get_mut(&id) {
                    orders.remove(&folder_id);
                }
            } else {
                self.folder_orders
                    .entry(id)
                    .or_default()
                    .insert(folder_id, order);
                self.chats.entry(id).or_insert_with(|| empty_chat(id));
            }
            return Effect::ui(vec![self.chat_list_update()]);
        }
        if list_type != "chatListMain" {
            return Effect::none();
        }
        let order = json_i64(&position["order"]).unwrap_or(0);
        self.apply_main_order(id, order);
        Effect::ui(vec![self.chat_list_update()])
    }

    fn apply_main_order(&mut self, id: i64, order: i64) {
        if order == 0 {
            self.listed.retain(|chat_id| *chat_id != id);
            if let Some(chat) = self.chats.get_mut(&id) {
                chat.order = 0;
                chat.real_order = true;
            }
            if !self.chat_in_some_folder(id) {
                self.chats.remove(&id);
            }
        } else if let Some(chat) = self.chats.get_mut(&id) {
            chat.order = order;
            chat.real_order = true;
            if !self.listed.contains(&id) {
                self.listed.push(id);
            }
        } else {
            let mut chat = empty_chat(id);
            chat.order = order;
            chat.real_order = true;
            self.chats.insert(id, chat);
            self.listed.push(id);
        }
    }

    fn select_chat(&mut self, chat_id: i64) -> Effect {
        if !self.ready || self.failed {
            return Effect::none();
        }
        let previous = self.open_chat;
        self.open_chat = Some(chat_id);
        self.history_exhausted.remove(&chat_id);
        self.history_pending.insert(chat_id);
        self.history_from.insert(chat_id, 0);
        let mut send = Vec::new();
        if let Some(previous) = previous {
            if previous != chat_id {
                send.push(close_chat_request(previous));
            }
        }
        send.push(open_chat_request(chat_id));
        if self
            .chats
            .get(&chat_id)
            .is_some_and(|chat| chat.marked_unread)
        {
            send.push(toggle_marked_unread_request(chat_id, false));
        }
        send.push(history_request(chat_id, 0));
        let mut effect = Effect::send(send);
        effect.ui.push(self.conversation_update(chat_id));
        effect
    }

    fn load_older(&mut self, chat_id: i64) -> Effect {
        if !self.ready || self.failed || self.open_chat != Some(chat_id) {
            return Effect::none();
        }
        if self.history_exhausted.contains(&chat_id) || self.history_pending.contains(&chat_id) {
            return Effect::none();
        }
        let Some(from) = self.oldest_message_id(chat_id) else {
            return Effect::none();
        };
        self.history_pending.insert(chat_id);
        self.history_from.insert(chat_id, from);
        Effect::send(vec![history_request(chat_id, from)])
    }

    fn on_last_message(&mut self, event: &Value) -> Effect {
        let Some(id) = json_i64(&event["chat_id"]) else {
            return Effect::none();
        };
        if event.get("last_message").is_some() {
            let chat = self.chats.entry(id).or_insert_with(|| empty_chat(id));
            apply_preview(chat, &event["last_message"]);
        }
        self.remember_folder_positions(id, event.get("positions"));
        if let Some(order) = main_list_order(event.get("positions")) {
            self.apply_main_order(id, order);
        }
        Effect::ui(vec![self.chat_list_update()])
    }

    fn on_read_inbox(&mut self, event: &Value) -> Effect {
        let Some(id) = json_i64(&event["chat_id"]) else {
            return Effect::none();
        };
        let Some(chat) = self.chats.get_mut(&id) else {
            return Effect::none();
        };
        chat.unread = unread_count(&event["unread_count"]);
        Effect::ui(vec![self.chat_list_update()])
    }

    /// Peer read cursor for outgoing messages. TDLib 1.8.67 pushes this for
    /// private chats and basic groups. `getMessageViewers` is a separate
    /// request for recent viewers and is not used.
    fn on_read_outbox(&mut self, event: &Value) -> Effect {
        let Some(chat_id) = json_i64(&event["chat_id"]) else {
            return Effect::none();
        };
        let Some(message_id) = json_i64(&event["last_read_outbox_message_id"]) else {
            return Effect::none();
        };
        if !self.note_outbox(chat_id, message_id) {
            return Effect::none();
        }
        self.maybe_conversation(chat_id)
    }

    fn note_outbox(&mut self, chat_id: i64, message_id: i64) -> bool {
        let previous = self.last_read_outbox.insert(chat_id, message_id);
        previous != Some(message_id)
    }

    fn on_marked_unread(&mut self, event: &Value) -> Effect {
        let Some(id) = json_i64(&event["chat_id"]) else {
            return Effect::none();
        };
        let Some(chat) = self.chats.get_mut(&id) else {
            return Effect::none();
        };
        chat.marked_unread = event["is_marked_as_unread"].as_bool().unwrap_or(false);
        Effect::ui(vec![self.chat_list_update()])
    }

    fn on_chat_photo(&mut self, event: &Value) -> Effect {
        let Some(id) = json_i64(&event["chat_id"]) else {
            return Effect::none();
        };
        if !self.chats.contains_key(&id) {
            return Effect::none();
        }
        if event.get("photo").is_some_and(Value::is_null) {
            if let Some(chat) = self.chats.get_mut(&id) {
                chat.avatar = None;
            }
            return Effect::ui(vec![self.chat_list_update()]);
        }
        if let Some(chat) = self.chats.get_mut(&id) {
            chat.avatar = None;
        }
        if let Some(file) = event.get("photo").and_then(|photo| photo.get("small")) {
            self.watch_file(file, FileRole::Avatar(id));
        }
        let mut effect = Effect::send(self.drain_sends());
        effect.ui.push(self.chat_list_update());
        effect
    }

    fn on_file(&mut self, event: &Value) -> Effect {
        let file = event.get("file").unwrap_or(event);
        let Some(path) = completed_local_path(file) else {
            return Effect::none();
        };
        let Some(id) = file_id(file) else {
            return Effect::none();
        };
        let role = self.file_roles.get(&id).cloned();
        if !self.apply_downloaded(id, &path) {
            return Effect::none();
        }
        match role {
            Some(FileRole::Avatar(_)) => Effect::ui(vec![self.chat_list_update()]),
            Some(FileRole::ContactAvatar(_)) => Effect::ui(vec![self.contacts_update()]),
            Some(FileRole::MessagePhoto { chat_id, .. }) => self.maybe_conversation(chat_id),
            None => Effect::none(),
        }
    }

    fn on_user(&mut self, event: &Value) -> Effect {
        let user = event.get("user").unwrap_or(event);
        let Some(id) = json_i64(&user["id"]) else {
            return Effect::none();
        };
        if event["@extra"].as_str() == Some("getMe") {
            self.my_user_id = Some(id);
        }
        let contacts_changed = self.note_contact(user);
        let mut effect = Effect::send(self.drain_sends());
        if contacts_changed {
            effect.ui.push(self.contacts_update());
        }
        if self.users.contains_key(&id) {
            if let Some(chat_id) = self.open_chat {
                effect.ui.push(self.conversation_update(chat_id));
            }
        }
        effect
    }

    fn on_users(&mut self, event: &Value) -> Effect {
        if event["@extra"].as_str() != Some("getContacts") {
            return Effect::none();
        }
        let mut ids = Vec::new();
        if let Some(arr) = event["user_ids"].as_array() {
            for id in arr {
                if let Some(id) = json_i64(id) {
                    ids.push(id);
                }
            }
        }
        self.contact_ids = ids;
        self.contact_meta
            .retain(|id, _| self.contact_ids.contains(id));
        let missing: Vec<i64> = self
            .contact_ids
            .iter()
            .copied()
            .filter(|id| !self.contact_meta.contains_key(id))
            .collect();
        let mut effect = Effect::send(missing.into_iter().map(get_user_request).collect());
        effect.ui.push(self.contacts_update());
        effect.ui.push(UiUpdate::Status(format!(
            "{} contacts",
            self.contact_ids.len()
        )));
        effect
    }

    /// Returns whether the contact list changed. Also queues a small-photo download.
    fn note_contact(&mut self, user: &Value) -> bool {
        let Some(id) = json_i64(&user["id"]) else {
            return false;
        };
        let name = user_name(user);
        if !name.is_empty() {
            self.users.insert(id, name.clone());
        }
        let username = user_username(user);
        let mut changed = false;
        match user.get("is_contact").and_then(Value::as_bool) {
            Some(true) if !self.contact_ids.contains(&id) => {
                self.contact_ids.push(id);
                changed = true;
            }
            Some(false) if self.contact_ids.contains(&id) => {
                self.contact_ids.retain(|known| *known != id);
                self.contact_meta.remove(&id);
                changed = true;
            }
            Some(_) | None => {}
        }
        if !self.contact_ids.contains(&id) {
            return changed;
        }
        let display = if !name.is_empty() {
            name
        } else if !username.is_empty() {
            username.clone()
        } else {
            format!("User {id}")
        };
        let photo_null = user.get("profile_photo").is_some_and(Value::is_null);
        let before = self
            .contact_meta
            .get(&id)
            .and_then(|meta| meta.avatar.clone());
        {
            let entry = self.contact_meta.entry(id).or_insert_with(|| ContactMeta {
                name: String::new(),
                username: String::new(),
                avatar: None,
            });
            if entry.name != display || entry.username != username {
                entry.name = display;
                entry.username = username;
                changed = true;
            }
            if photo_null && entry.avatar.take().is_some() {
                changed = true;
            }
        }
        if photo_null {
            return changed;
        }
        if let Some(file) = user.pointer("/profile_photo/small").cloned() {
            self.watch_file(&file, FileRole::ContactAvatar(id));
        }
        let after = self
            .contact_meta
            .get(&id)
            .and_then(|meta| meta.avatar.clone());
        changed || before != after
    }

    fn on_messages(&mut self, event: &Value) -> Effect {
        let Some(arr) = event["messages"].as_array() else {
            return Effect::none();
        };
        let history_chat = history_extra(event);
        let mut chat_id = None;
        let mut changed = false;
        let mut fetch_users = Vec::new();
        let mut inserted = Vec::new();
        for message in arr.iter().filter(|value| value.is_object()) {
            if let Some(remembered) = self.remember_message(message) {
                chat_id = Some(remembered.chat_id);
                changed |= remembered.changed;
                if remembered.inserted {
                    inserted.push((remembered.chat_id, remembered.message_id));
                }
                if let Some(user_id) = json_i64(&message["sender_id"]["user_id"]) {
                    if !self.users.contains_key(&user_id) && self.my_user_id != Some(user_id) {
                        fetch_users.push(user_id);
                    }
                }
            }
        }
        if let Some(id) = history_chat {
            let ids: Vec<i64> = inserted
                .into_iter()
                .filter(|(chat, _)| *chat == id)
                .map(|(_, message_id)| message_id)
                .collect();
            self.finish_history(id, &ids);
            chat_id = chat_id.or(Some(id));
        }
        fetch_users.sort_unstable();
        fetch_users.dedup();
        let mut effect = Effect::send(fetch_users.into_iter().map(get_user_request).collect());
        effect.send.extend(self.drain_sends());
        if changed {
            if let Some(chat_id) = chat_id.or(self.open_chat) {
                if self.open_chat == Some(chat_id) || self.open_chat.is_none() {
                    effect.ui.push(self.conversation_update(chat_id));
                }
            }
        }
        if let Some(chat_id) = chat_id {
            self.push_view(&mut effect, chat_id);
        }
        effect
    }

    fn on_message_event(&mut self, event: &Value) -> Effect {
        let message = event.get("message").unwrap_or(event);
        let Some(remembered) = self.remember_message(message) else {
            return Effect::none();
        };
        let mut effect = Effect::send(self.drain_sends());
        if remembered.changed && self.open_chat == Some(remembered.chat_id) {
            effect.ui.push(self.conversation_update(remembered.chat_id));
        }
        if remembered.changed || remembered.inserted {
            self.push_view(&mut effect, remembered.chat_id);
        }
        effect
    }

    fn finish_history(&mut self, chat_id: i64, inserted_ids: &[i64]) {
        let from = self.history_from.remove(&chat_id);
        self.history_pending.remove(&chat_id);
        let Some(from) = from else {
            return;
        };
        if from == 0 {
            if inserted_ids.is_empty()
                && self
                    .messages
                    .get(&chat_id)
                    .is_none_or(|bucket| bucket.is_empty())
            {
                self.history_exhausted.insert(chat_id);
            }
            return;
        }
        if !inserted_ids.iter().any(|id| *id < from) {
            self.history_exhausted.insert(chat_id);
        }
    }

    fn push_view(&self, effect: &mut Effect, chat_id: i64) {
        if self.open_chat != Some(chat_id) {
            return;
        }
        let ids = self.open_message_ids(chat_id);
        if ids.is_empty() {
            return;
        }
        effect.send.push(view_messages_request(chat_id, &ids));
    }

    fn open_message_ids(&self, chat_id: i64) -> Vec<i64> {
        let Some(bucket) = self.messages.get(&chat_id) else {
            return Vec::new();
        };
        let mut ids: Vec<i64> = bucket.keys().copied().collect();
        ids.sort_unstable();
        if ids.len() > 100 {
            ids = ids.split_off(ids.len() - 100);
        }
        ids
    }

    fn oldest_message_id(&self, chat_id: i64) -> Option<i64> {
        self.messages.get(&chat_id)?.keys().copied().min()
    }

    fn on_message_content(&mut self, event: &Value) -> Effect {
        let Some(chat_id) = json_i64(&event["chat_id"]) else {
            return Effect::none();
        };
        let Some(message_id) = json_i64(&event["message_id"]) else {
            return Effect::none();
        };
        let parsed = parse_visible_content(&event["new_content"]);
        let known = {
            let Some(bucket) = self.messages.get_mut(&chat_id) else {
                return Effect::none();
            };
            match &parsed {
                None => {
                    bucket.remove(&message_id);
                    true
                }
                Some(parsed) => {
                    if let Some(stored) = bucket.get_mut(&message_id) {
                        stored.text = parsed.text.clone();
                        stored.kind = parsed.kind;
                        stored.photo = parsed.photo.clone();
                        stored.photo_full = parsed.photo_full.clone();
                        stored.photo_full_width = parsed.photo_full_width;
                        stored.photo_full_height = parsed.photo_full_height;
                        stored.links = parsed.links.clone();
                        true
                    } else {
                        false
                    }
                }
            }
        };
        if !known {
            return Effect::none();
        }
        if let Some(parsed) = parsed.as_ref() {
            self.watch_message_photos(parsed, chat_id, message_id);
        }
        let mut effect = Effect::send(self.drain_sends());
        if self.open_chat == Some(chat_id) {
            effect.ui.push(self.conversation_update(chat_id));
        }
        effect
    }

    fn remember_chat(&mut self, chat: &Value) -> bool {
        let Some(id) = json_i64(&chat["id"]) else {
            return false;
        };
        let outbox_changed = json_i64(&chat["last_read_outbox_message_id"])
            .is_some_and(|message_id| self.note_outbox(id, message_id));
        let title = chat["title"].as_str().unwrap_or("…").to_string();
        let real = main_list_order(chat.get("positions"));
        let photo = {
            let entry = self.chats.entry(id).or_insert_with(|| empty_chat(id));
            if !title.is_empty() && title != "…" {
                entry.title = title;
            }
            if chat.get("unread_count").is_some() {
                entry.unread = unread_count(&chat["unread_count"]);
            }
            if chat.get("is_marked_as_unread").is_some() {
                entry.marked_unread = chat["is_marked_as_unread"].as_bool().unwrap_or(false);
            }
            if chat.get("last_message").is_some() {
                apply_preview(entry, &chat["last_message"]);
            }
            if chat.get("photo").is_some_and(Value::is_null) {
                entry.avatar = None;
            }
            chat.get("photo").filter(|value| !value.is_null()).cloned()
        };
        if let Some(photo) = photo.as_ref() {
            if let Some(file) = photo.get("small") {
                self.watch_file(file, FileRole::Avatar(id));
            }
        }
        self.remember_folder_positions(id, chat.get("positions"));
        if let Some(order) = real {
            if order == 0 {
                self.listed.retain(|chat_id| *chat_id != id);
                if let Some(entry) = self.chats.get_mut(&id) {
                    entry.order = 0;
                    entry.real_order = true;
                }
                if !self.chat_in_some_folder(id) {
                    self.chats.remove(&id);
                }
                return outbox_changed;
            }
            if let Some(entry) = self.chats.get_mut(&id) {
                entry.order = order;
                entry.real_order = true;
            }
        }
        if !self.listed.contains(&id) && self.listed.len() < self.cfg.chat_limit.max(0) as usize {
            self.listed.push(id);
        }
        outbox_changed
    }

    fn remember_folder_positions(&mut self, chat_id: i64, positions: Option<&Value>) {
        let Some(positions) = positions.and_then(Value::as_array) else {
            return;
        };
        for position in positions {
            if position["list"]["@type"].as_str() != Some("chatListFolder") {
                continue;
            }
            let Some(folder_id) = json_i32(&position["list"]["chat_folder_id"]) else {
                continue;
            };
            let order = json_i64(&position["order"]).unwrap_or(0);
            let orders = self.folder_orders.entry(chat_id).or_default();
            if order == 0 {
                orders.remove(&folder_id);
            } else {
                orders.insert(folder_id, order);
            }
        }
    }

    fn chat_in_some_folder(&self, chat_id: i64) -> bool {
        self.folder_orders
            .get(&chat_id)
            .is_some_and(|orders| orders.values().any(|order| *order != 0))
    }

    fn remember_message(&mut self, message: &Value) -> Option<Remembered> {
        if message["@type"].as_str() != Some("message") && message.get("content").is_none() {
            return None;
        }
        let chat_id = json_i64(&message["chat_id"])?;
        let id = json_i64(&message["id"])?;
        let parsed = parse_visible_content(&message["content"])?;
        let date = json_i64(&message["date"]).unwrap_or(0);
        let sender = &message["sender_id"];
        let sender_id = match sender["@type"].as_str() {
            Some("messageSenderUser") => json_i64(&sender["user_id"]),
            _ => None,
        };
        let sender_chat = match sender["@type"].as_str() {
            Some("messageSenderChat") => json_i64(&sender["chat_id"]),
            _ => None,
        };
        let outgoing = message["is_outgoing"].as_bool().unwrap_or(false);
        let stored = StoredMessage {
            sender_id,
            sender_chat,
            outgoing,
            text: parsed.text.clone(),
            date,
            kind: parsed.kind,
            photo: parsed.photo.clone(),
            photo_full: parsed.photo_full.clone(),
            photo_full_width: parsed.photo_full_width,
            photo_full_height: parsed.photo_full_height,
            links: parsed.links.clone(),
        };
        let bucket = self.messages.entry(chat_id).or_default();
        let inserted = !bucket.contains_key(&id);
        let changed = match bucket.get(&id) {
            Some(existing) => {
                existing.text != stored.text
                    || existing.date != stored.date
                    || existing.outgoing != stored.outgoing
                    || existing.sender_id != stored.sender_id
                    || existing.sender_chat != stored.sender_chat
                    || existing.kind != stored.kind
                    || existing.photo != stored.photo
                    || existing.photo_full != stored.photo_full
                    || existing.photo_full_width != stored.photo_full_width
                    || existing.photo_full_height != stored.photo_full_height
                    || existing.links != stored.links
            }
            None => true,
        };
        if changed {
            bucket.insert(id, stored);
        }
        self.watch_message_photos(&parsed, chat_id, id);
        Some(Remembered {
            chat_id,
            message_id: id,
            changed,
            inserted,
        })
    }

    fn drain_sends(&mut self) -> Vec<Value> {
        std::mem::take(&mut self.queued_sends)
    }

    fn watch_message_photos(&mut self, parsed: &ParsedContent, chat_id: i64, message_id: i64) {
        let preview_id = parsed.file.as_ref().and_then(file_id);
        let full_id = parsed.full_file.as_ref().and_then(file_id);
        if preview_id.is_some() && preview_id == full_id {
            if let Some(file) = parsed.file.as_ref() {
                self.watch_file(
                    file,
                    FileRole::MessagePhoto {
                        chat_id,
                        message_id,
                        slot: PhotoSlot::Both,
                    },
                );
            }
            return;
        }
        if let Some(file) = parsed.file.as_ref() {
            self.watch_file(
                file,
                FileRole::MessagePhoto {
                    chat_id,
                    message_id,
                    slot: PhotoSlot::Preview,
                },
            );
        }
        if let Some(file) = parsed.full_file.as_ref() {
            self.watch_file(
                file,
                FileRole::MessagePhoto {
                    chat_id,
                    message_id,
                    slot: PhotoSlot::Full,
                },
            );
        }
    }

    fn watch_file(&mut self, file: &Value, role: FileRole) {
        let Some(id) = file_id(file) else {
            return;
        };
        self.file_roles.insert(id, role);
        if let Some(path) = completed_local_path(file) {
            self.apply_downloaded(id, &path);
            return;
        }
        if self.downloads_sent.contains(&id) {
            return;
        }
        if file["local"]["can_be_downloaded"].as_bool() == Some(false) {
            return;
        }
        self.downloads_sent.insert(id);
        self.queued_sends.push(download_file_request(id));
    }

    /// Applies a finished download. Returns whether the visible chat changed.
    fn apply_downloaded(&mut self, file_id: i32, path: &str) -> bool {
        match self.file_roles.get(&file_id).cloned() {
            Some(FileRole::Avatar(chat_id)) => {
                let Some(chat) = self.chats.get_mut(&chat_id) else {
                    return false;
                };
                if chat.avatar.as_deref() == Some(path) {
                    return false;
                }
                chat.avatar = Some(path.to_string());
                true
            }
            Some(FileRole::ContactAvatar(user_id)) => {
                let Some(meta) = self.contact_meta.get_mut(&user_id) else {
                    return false;
                };
                if meta.avatar.as_deref() == Some(path) {
                    return false;
                }
                meta.avatar = Some(path.to_string());
                true
            }
            Some(FileRole::MessagePhoto {
                chat_id,
                message_id,
                slot,
            }) => {
                let Some(stored) = self
                    .messages
                    .get_mut(&chat_id)
                    .and_then(|bucket| bucket.get_mut(&message_id))
                else {
                    return false;
                };
                let mut changed = false;
                if matches!(slot, PhotoSlot::Preview | PhotoSlot::Both)
                    && stored.photo.as_deref() != Some(path)
                {
                    stored.photo = Some(path.to_string());
                    changed = true;
                }
                if matches!(slot, PhotoSlot::Full | PhotoSlot::Both)
                    && stored.photo_full.as_deref() != Some(path)
                {
                    stored.photo_full = Some(path.to_string());
                    changed = true;
                }
                changed
            }
            None => false,
        }
    }

    fn search(&mut self, query: String) -> Effect {
        let query = query.trim().to_string();
        self.search_gen = self.search_gen.wrapping_add(1);
        self.search_query = query.clone();
        self.search_ids.clear();
        self.search_pending = 0;
        self.search_error = false;
        if query.is_empty() {
            let mut ui = vec![self.search_update()];
            if self.ready && !self.failed {
                ui.push(UiUpdate::Status("Ready".into()));
            }
            return Effect::ui(ui);
        }
        if !self.ready || self.failed {
            return Effect::ui(vec![
                self.search_update(),
                UiUpdate::Status("Search is available after TDLib is ready.".into()),
            ]);
        }
        let gen = self.search_gen;
        let limit = self.cfg.chat_limit.clamp(1, 50);
        let mut send = vec![
            search_chats_request(&query, limit, gen),
            search_chats_on_server_request(&query, limit, gen),
        ];
        self.search_pending = 2;
        if let Some(public_query) = public_search_query(&query) {
            send.push(search_public_chats_request(&public_query, gen));
            self.search_pending = 3;
        }
        Effect {
            send,
            ui: vec![self.search_update(), UiUpdate::Status("Searching…".into())],
        }
    }

    fn on_search_chats(&mut self, gen: u64, event: &Value) -> Effect {
        if gen != self.search_gen || self.search_query.is_empty() {
            return Effect::none();
        }
        let mut missing = Vec::new();
        if let Some(arr) = event["chat_ids"].as_array() {
            for id in arr {
                let Some(id) = json_i64(id) else {
                    continue;
                };
                if self.search_ids.contains(&id) {
                    continue;
                }
                self.search_ids.push(id);
                let needs_title = self
                    .chats
                    .get(&id)
                    .is_none_or(|chat| chat.title.is_empty() || chat.title == "…");
                if needs_title {
                    missing.push(id);
                }
            }
        }
        let mut effect = Effect::send(missing.into_iter().map(get_chat_search_request).collect());
        effect.ui.push(self.search_update());
        if self.note_search_done(gen) {
            effect
                .ui
                .push(UiUpdate::Status(search_status(self.search_chats().len())));
        }
        effect
    }

    /// `true` when this generation's search requests have all returned and none failed.
    fn note_search_done(&mut self, gen: u64) -> bool {
        if gen != self.search_gen {
            return false;
        }
        self.search_pending = self.search_pending.saturating_sub(1);
        self.search_pending == 0 && !self.search_error
    }

    fn search_chats(&self) -> Vec<ChatItem> {
        self.search_ids
            .iter()
            .filter_map(|id| {
                let chat = self.chats.get(id)?;
                if chat.title.is_empty() || chat.title == "…" {
                    return None;
                }
                Some(chat.clone())
            })
            .collect()
    }

    fn search_update(&self) -> UiUpdate {
        UiUpdate::SearchResults {
            query: self.search_query.clone(),
            chats: self.search_chats(),
        }
    }

    fn load_contacts(&mut self) -> Effect {
        if !self.ready || self.failed {
            return Effect::ui(vec![UiUpdate::Status(
                "Contacts are available after TDLib is ready.".into(),
            )]);
        }
        Effect::send(vec![get_contacts_request()])
    }

    fn open_contact(&mut self, user_id: i64) -> Effect {
        if !self.ready || self.failed || user_id == 0 {
            return Effect::ui(vec![UiUpdate::Status(
                "Contacts are available after TDLib is ready.".into(),
            )]);
        }
        Effect {
            send: vec![create_private_chat_request(user_id)],
            ui: vec![UiUpdate::Status("Opening chat…".into())],
        }
    }

    fn contacts_update(&self) -> UiUpdate {
        let contacts = self
            .contact_ids
            .iter()
            .map(|id| {
                if let Some(meta) = self.contact_meta.get(id) {
                    ContactItem {
                        user_id: *id,
                        name: meta.name.clone(),
                        username: meta.username.clone(),
                        avatar: meta.avatar.clone(),
                    }
                } else {
                    ContactItem {
                        user_id: *id,
                        name: format!("User {id}"),
                        username: String::new(),
                        avatar: None,
                    }
                }
            })
            .collect();
        UiUpdate::Contacts(contacts)
    }

    fn request_chats(&mut self) -> Effect {
        if self.get_chats_sent >= 3 {
            return Effect::ui(vec![self.chat_list_update()]);
        }
        self.get_chats_sent += 1;
        Effect::send(vec![get_chats(self.cfg.chat_limit)])
    }

    fn chat_list_update(&self) -> UiUpdate {
        let limit = self.cfg.chat_limit.max(0) as usize;
        let mut items = if let Some(folder_id) = self.active_folder {
            let mut items: Vec<ChatItem> = self
                .folder_orders
                .iter()
                .filter_map(|(id, orders)| {
                    let order = *orders.get(&folder_id)?;
                    if order == 0 {
                        return None;
                    }
                    let mut chat = self.chats.get(id)?.clone();
                    if chat.title == "…" || chat.title.is_empty() {
                        return None;
                    }
                    chat.order = order;
                    Some(chat)
                })
                .collect();
            items.sort_by(|a, b| b.order.cmp(&a.order).then(b.id.cmp(&a.id)));
            items
        } else {
            let mut items: Vec<ChatItem> = self
                .listed
                .iter()
                .filter_map(|id| self.chats.get(id).cloned())
                .filter(|chat| chat.order != 0 || chat.title != "…")
                .collect();
            items.sort_by(|a, b| b.order.cmp(&a.order).then(b.id.cmp(&a.id)));
            items
        };
        items.truncate(limit);
        UiUpdate::ChatList(items)
    }

    fn conversation_update(&self, chat_id: i64) -> UiUpdate {
        let title = self
            .chats
            .get(&chat_id)
            .map(|chat| chat.title.clone())
            .unwrap_or_else(|| format!("Chat {chat_id}"));
        let read_cursor = self.last_read_outbox.get(&chat_id).copied().unwrap_or(0);
        let mut messages: Vec<TextMessage> = self
            .messages
            .get(&chat_id)
            .map(|bucket| {
                bucket
                    .iter()
                    .map(|(id, stored)| TextMessage {
                        id: *id,
                        chat_id,
                        sender: self.sender_label(stored),
                        text: stored.text.clone(),
                        date: stored.date,
                        kind: stored.kind,
                        photo: stored.photo.clone(),
                        photo_full: stored.photo_full.clone(),
                        photo_full_width: stored.photo_full_width,
                        photo_full_height: stored.photo_full_height,
                        links: stored.links.clone(),
                        outgoing: stored.outgoing,
                        read: stored.outgoing && read_cursor > 0 && *id <= read_cursor,
                    })
                    .collect()
            })
            .unwrap_or_default();
        messages.sort_by(|a, b| a.date.cmp(&b.date).then(a.id.cmp(&b.id)));
        UiUpdate::Conversation {
            chat_id,
            title,
            messages,
        }
    }

    fn sender_label(&self, stored: &StoredMessage) -> String {
        if stored.outgoing {
            return "You".into();
        }
        if let Some(user_id) = stored.sender_id {
            if self.my_user_id == Some(user_id) {
                return "You".into();
            }
            if let Some(name) = self.users.get(&user_id) {
                return name.clone();
            }
            return format!("User {user_id}");
        }
        if let Some(chat_id) = stored.sender_chat {
            if let Some(chat) = self.chats.get(&chat_id) {
                if !chat.title.is_empty() && chat.title != "…" {
                    return chat.title.clone();
                }
            }
            return format!("Chat {chat_id}");
        }
        "Unknown".into()
    }

    fn maybe_conversation(&self, chat_id: i64) -> Effect {
        if self.open_chat == Some(chat_id) {
            Effect::ui(vec![self.conversation_update(chat_id)])
        } else {
            Effect::none()
        }
    }

    fn fail(&mut self, message: String) -> Effect {
        self.failed = true;
        let mut effect = self.begin_close(vec![
            UiUpdate::Fatal(message.clone()),
            UiUpdate::Status(message),
        ]);
        effect
            .ui
            .retain(|update| !matches!(update, UiUpdate::Log(line) if line == "closing"));
        effect
    }

    fn begin_close(&mut self, mut ui: Vec<UiUpdate>) -> Effect {
        if self.closing || self.finished {
            return Effect {
                send: Vec::new(),
                ui,
            };
        }
        self.closing = true;
        ui.push(UiUpdate::Log("closing".into()));
        Effect {
            send: vec![json!({"@type": "close", "@extra": "close"})],
            ui,
        }
    }
}

impl Effect {
    fn none() -> Self {
        Self {
            send: Vec::new(),
            ui: Vec::new(),
        }
    }

    fn send(send: Vec<Value>) -> Self {
        Self {
            send,
            ui: Vec::new(),
        }
    }

    fn ui(ui: Vec<UiUpdate>) -> Self {
        Self {
            send: Vec::new(),
            ui,
        }
    }
}

fn status_for(state: &str) -> String {
    match state {
        "authorizationStateWaitTdlibParameters" => "Starting TDLib…".into(),
        "authorizationStateWaitEncryptionKey" => "Checking the empty database key…".into(),
        "authorizationStateReady" => "Ready".into(),
        "authorizationStateClosing" => "Closing…".into(),
        "authorizationStateClosed" => "Closed".into(),
        other => other.trim_start_matches("authorizationState").to_string(),
    }
}

fn user_username(user: &Value) -> String {
    if let Some(name) = user["usernames"]["active_usernames"]
        .as_array()
        .and_then(|names| names.iter().find_map(Value::as_str))
        .map(str::trim)
        .filter(|name| !name.is_empty())
    {
        return name.trim_start_matches('@').to_string();
    }
    if let Some(name) = user["usernames"]["editable_username"]
        .as_str()
        .map(str::trim)
        .filter(|name| !name.is_empty())
    {
        return name.trim_start_matches('@').to_string();
    }
    user["username"]
        .as_str()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(|name| name.trim_start_matches('@').to_string())
        .unwrap_or_default()
}

/// `@name` and a single username-shaped token. Titles with spaces stay local
/// plus `searchChats` / `searchChatsOnServer`.
fn public_search_query(query: &str) -> Option<String> {
    let trimmed = query.trim();
    let (body, explicit) = if let Some(rest) = trimmed.strip_prefix('@') {
        (rest, true)
    } else {
        (trimmed, false)
    };
    if body.is_empty()
        || !body
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
    {
        return None;
    }
    if explicit || body.chars().count() >= 5 {
        Some(body.to_string())
    } else {
        None
    }
}

fn search_status(count: usize) -> String {
    match count {
        1 => "Search: 1 chat".into(),
        n => format!("Search: {n} chats"),
    }
}

fn search_generation(extra: &str) -> Option<u64> {
    let rest = extra
        .strip_prefix("searchChatsOnServer:")
        .or_else(|| extra.strip_prefix("searchPublicChats:"))
        .or_else(|| extra.strip_prefix("searchChats:"))?;
    rest.parse().ok()
}

fn user_name(user: &Value) -> String {
    let first = user["first_name"].as_str().unwrap_or("").trim();
    let last = user["last_name"].as_str().unwrap_or("").trim();
    match (first.is_empty(), last.is_empty()) {
        (true, true) => String::new(),
        (false, true) => first.to_string(),
        (true, false) => last.to_string(),
        (false, false) => format!("{first} {last}"),
    }
}

struct Remembered {
    chat_id: i64,
    message_id: i64,
    changed: bool,
    inserted: bool,
}

struct ParsedContent {
    kind: MessageKind,
    text: String,
    photo: Option<String>,
    photo_full: Option<String>,
    photo_full_width: i32,
    photo_full_height: i32,
    file: Option<Value>,
    full_file: Option<Value>,
    links: Vec<TextLink>,
}

fn parse_visible_content(content: &Value) -> Option<ParsedContent> {
    match content["@type"].as_str() {
        Some("messageText") => {
            let text = content["text"]["text"].as_str().unwrap_or("").to_string();
            let links = links_in_formatted(&text, &content["text"]);
            Some(ParsedContent {
                kind: MessageKind::Text,
                text,
                photo: None,
                photo_full: None,
                photo_full_width: 0,
                photo_full_height: 0,
                file: None,
                full_file: None,
                links,
            })
        }
        Some("messagePhoto") => {
            let full = pick_full_size(&content["photo"]);
            let photo_full_width = full
                .and_then(|size| json_i64(&size["width"]))
                .map(clamp_i32)
                .unwrap_or(0);
            let photo_full_height = full
                .and_then(|size| json_i64(&size["height"]))
                .map(clamp_i32)
                .unwrap_or(0);
            let full_file = full.and_then(|size| size.get("photo")).cloned();
            let file = pick_preview_file(&content["photo"]).cloned();
            let photo = file.as_ref().and_then(completed_local_path);
            let photo_full = full_file.as_ref().and_then(completed_local_path);
            let text = content["caption"]["text"]
                .as_str()
                .unwrap_or("")
                .to_string();
            let links = links_in_formatted(&text, &content["caption"]);
            Some(ParsedContent {
                kind: MessageKind::Photo,
                text,
                photo,
                photo_full,
                photo_full_width,
                photo_full_height,
                file,
                full_file,
                links,
            })
        }
        _ => None,
    }
}

fn apply_preview(chat: &mut ChatItem, last_message: &Value) {
    if last_message.is_null() {
        chat.preview.clear();
        chat.preview_date = 0;
        return;
    }
    let Some((snippet, date)) = message_snippet(last_message) else {
        return;
    };
    chat.preview = snippet;
    chat.preview_date = date;
}

fn message_snippet(message: &Value) -> Option<(String, i64)> {
    let content = message.get("content")?;
    let date = json_i64(&message["date"]).unwrap_or(0);
    let snippet = match content["@type"].as_str()? {
        "messageText" => collapse_preview(content["text"]["text"].as_str().unwrap_or("")),
        "messagePhoto" => {
            let caption = collapse_preview(content["caption"]["text"].as_str().unwrap_or(""));
            if caption.is_empty() {
                "Photo".into()
            } else {
                caption
            }
        }
        "messageVideo" | "messageVideoNote" => "Video".into(),
        "messageAnimation" => "GIF".into(),
        "messageVoiceNote" => "Voice message".into(),
        "messageAudio" => "Audio".into(),
        "messageDocument" => "File".into(),
        "messageSticker" | "messageAnimatedEmoji" => "Sticker".into(),
        "messageCall" => "Call".into(),
        "messagePoll" => "Poll".into(),
        "messageLocation" => "Location".into(),
        "messageContact" => "Contact".into(),
        _ => String::new(),
    };
    Some((snippet, date))
}

fn collapse_preview(text: &str) -> String {
    const MAX: usize = 80;
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() <= MAX {
        return collapsed;
    }
    let mut cut: String = collapsed.chars().take(MAX).collect();
    cut.push('…');
    cut
}

fn links_in_formatted(text: &str, formatted: &Value) -> Vec<TextLink> {
    let mut links = Vec::new();
    if let Some(entities) = formatted["entities"].as_array() {
        for entity in entities {
            let offset =
                usize::try_from(json_i64(&entity["offset"]).unwrap_or(0).max(0)).unwrap_or(0);
            let length =
                usize::try_from(json_i64(&entity["length"]).unwrap_or(0).max(0)).unwrap_or(0);
            let kind = &entity["type"];
            let (start, end, url) = match kind["@type"].as_str() {
                Some("textEntityTypeUrl") => {
                    let Some((start, end)) = utf16_span(text, offset, length) else {
                        continue;
                    };
                    let url = text[start..end].to_string();
                    (start, end, url)
                }
                Some("textEntityTypeTextUrl") => {
                    let Some((start, end)) = utf16_span(text, offset, length) else {
                        continue;
                    };
                    let url = kind["url"].as_str().unwrap_or("").to_string();
                    (start, end, url)
                }
                _ => continue,
            };
            if start >= end || !is_http_url(&url) {
                continue;
            }
            links.push(TextLink { start, end, url });
        }
    }
    for plain in plain_http_links(text) {
        if links
            .iter()
            .any(|link| ranges_overlap(link.start, link.end, plain.start, plain.end))
        {
            continue;
        }
        links.push(plain);
    }
    links.sort_by_key(|link| link.start);
    links
}

/// TDLib entity offsets are UTF-16 code units.
fn utf16_span(text: &str, offset: usize, length: usize) -> Option<(usize, usize)> {
    if length == 0 {
        return None;
    }
    let end_unit = offset + length;
    let mut units = 0usize;
    let mut start_byte = None;
    for (byte, ch) in text.char_indices() {
        if units == offset || (start_byte.is_none() && units > offset) {
            start_byte = Some(byte);
        }
        let next = units + ch.len_utf16();
        if next >= end_unit {
            let end_byte = byte + ch.len_utf8();
            return Some((start_byte?, end_byte));
        }
        units = next;
    }
    None
}

fn plain_http_links(text: &str) -> Vec<TextLink> {
    let mut links = Vec::new();
    let mut base = 0usize;
    while base < text.len() {
        let rest = &text[base..];
        let http = rest.find("http://");
        let https = rest.find("https://");
        let rel = match (http, https) {
            (Some(left), Some(right)) => left.min(right),
            (Some(left), None) => left,
            (None, Some(right)) => right,
            (None, None) => break,
        };
        let start = base + rel;
        let tail = &text[start..];
        let stop = tail
            .find(|ch: char| ch.is_whitespace() || matches!(ch, '<' | '>' | '"' | '\''))
            .unwrap_or(tail.len());
        let mut end = start + stop;
        while end > start
            && matches!(
                text.as_bytes()[end - 1],
                b'.' | b',' | b';' | b':' | b')' | b']' | b'!'
            )
        {
            end -= 1;
        }
        if end > start && is_http_url(&text[start..end]) {
            links.push(TextLink {
                start,
                end,
                url: text[start..end].to_string(),
            });
        }
        base = end.max(start + 1);
    }
    links
}

fn is_http_url(url: &str) -> bool {
    let lower = url.as_bytes();
    let https = lower.len() > "https://".len() && lower[..8].eq_ignore_ascii_case(b"https://");
    let http = lower.len() > "http://".len() && lower[..7].eq_ignore_ascii_case(b"http://");
    (https || http)
        && !url
            .chars()
            .any(|ch| ch.is_whitespace() || ch == '<' || ch == '>')
}

fn ranges_overlap(left: usize, left_end: usize, right: usize, right_end: usize) -> bool {
    left < right_end && right < left_end
}

fn history_extra(event: &Value) -> Option<i64> {
    event["@extra"]
        .as_str()
        .and_then(|extra| extra.strip_prefix("history:"))
        .and_then(|id| id.parse().ok())
}

fn clamp_i32(value: i64) -> i32 {
    value.clamp(0, i64::from(i32::MAX)) as i32
}

fn size_type_name(size: &Value) -> &str {
    size["type"].as_str().unwrap_or("")
}

/// `i` is a tiny stripped thumbnail. Skip it when any other size exists.
fn candidate_sizes(photo: &Value) -> Vec<&Value> {
    let Some(sizes) = photo.get("sizes").and_then(Value::as_array) else {
        return Vec::new();
    };
    let without_stripped: Vec<&Value> = sizes
        .iter()
        .filter(|size| size_type_name(size) != "i")
        .collect();
    if without_stripped.is_empty() {
        sizes.iter().collect()
    } else {
        without_stripped
    }
}

fn size_area(size: &Value) -> i64 {
    let width = json_i64(&size["width"]).unwrap_or(0).max(0);
    let height = json_i64(&size["height"]).unwrap_or(0).max(0);
    width.saturating_mul(height)
}

/// Tie-break when width/height are missing or equal. Larger means sharper.
fn size_rank(size: &Value) -> i32 {
    match size_type_name(size) {
        "w" => 60,
        "y" => 50,
        "x" => 40,
        "m" => 30,
        "c" => 20,
        "b" => 15,
        "a" => 10,
        "s" => 5,
        "i" => 0,
        _ => 1,
    }
}

/// Bubble thumbnail. `m` is about 320px; fall back toward larger types only
/// when Telegram did not send `m` or `s`.
fn pick_preview_file(photo: &Value) -> Option<&Value> {
    let sizes = candidate_sizes(photo);
    if sizes.is_empty() {
        return None;
    }
    for prefer in ["m", "s", "x", "y", "w"] {
        if let Some(size) = sizes.iter().find(|size| size_type_name(size) == prefer) {
            return size.get("photo");
        }
    }
    sizes.first().and_then(|size| size.get("photo"))
}

/// Popup file. Largest pixel area, then type rank (`w` over `y` over `x`).
fn pick_full_size(photo: &Value) -> Option<&Value> {
    candidate_sizes(photo).into_iter().max_by(|left, right| {
        size_area(left)
            .cmp(&size_area(right))
            .then(size_rank(left).cmp(&size_rank(right)))
    })
}

fn main_list_order(positions: Option<&Value>) -> Option<i64> {
    let positions = positions?.as_array()?;
    for position in positions {
        if position["list"]["@type"].as_str() == Some("chatListMain") {
            return json_i64(&position["order"]);
        }
    }
    None
}

fn unread_count(value: &Value) -> i32 {
    json_i64(value).unwrap_or(0).clamp(0, i64::from(i32::MAX)) as i32
}

fn file_id(file: &Value) -> Option<i32> {
    let id = json_i64(&file["id"])?;
    i32::try_from(id).ok().filter(|id| *id > 0)
}

fn completed_local_path(file: &Value) -> Option<String> {
    let local = &file["local"];
    if local["is_downloading_completed"].as_bool() != Some(true) {
        return None;
    }
    let path = local["path"].as_str().unwrap_or("").trim();
    if path.is_empty() {
        None
    } else {
        Some(path.to_string())
    }
}

fn download_file_request(file_id: i32) -> Value {
    json!({
        "@type": "downloadFile",
        "file_id": file_id,
        "priority": 16,
        "offset": 0,
        "limit": 0,
        "synchronous": false,
        "@extra": format!("download:{file_id}")
    })
}

fn json_i64(value: &Value) -> Option<i64> {
    if let Some(number) = value.as_i64() {
        return Some(number);
    }
    if let Some(number) = value.as_u64() {
        return i64::try_from(number).ok();
    }
    value.as_str().and_then(|text| text.parse().ok())
}

fn get_me_request() -> Value {
    json!({"@type": "getMe", "@extra": "getMe"})
}

fn load_chats(limit: i32) -> Value {
    json!({
        "@type": "loadChats",
        "chat_list": {"@type": "chatListMain"},
        "limit": limit,
        "@extra": "loadChats"
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

fn load_folder_chats(folder_id: i32, limit: i32) -> Value {
    json!({
        "@type": "loadChats",
        "chat_list": {"@type": "chatListFolder", "chat_folder_id": folder_id},
        "limit": limit,
        "@extra": format!("loadChats:folder:{folder_id}")
    })
}

fn get_folder_chats(folder_id: i32, limit: i32) -> Value {
    json!({
        "@type": "getChats",
        "chat_list": {"@type": "chatListFolder", "chat_folder_id": folder_id},
        "limit": limit,
        "@extra": format!("getChats:folder:{folder_id}")
    })
}

fn folder_title(info: &Value) -> String {
    if let Some(title) = info["title"].as_str().filter(|text| !text.is_empty()) {
        return title.to_string();
    }
    if let Some(title) = info["name"]["text"]["text"]
        .as_str()
        .filter(|text| !text.is_empty())
    {
        return title.to_string();
    }
    if let Some(title) = info["name"]["text"]
        .as_str()
        .filter(|text| !text.is_empty())
    {
        return title.to_string();
    }
    if let Some(title) = info["name"].as_str().filter(|text| !text.is_empty()) {
        return title.to_string();
    }
    let id = json_i64(&info["id"]).unwrap_or(0);
    format!("Folder {id}")
}

fn json_i32(value: &Value) -> Option<i32> {
    i32::try_from(json_i64(value)?).ok()
}

fn search_chats_request(query: &str, limit: i32, gen: u64) -> Value {
    json!({
        "@type": "searchChats",
        "query": query,
        "type_filter": null,
        "limit": limit,
        "@extra": format!("searchChats:{gen}")
    })
}

fn search_chats_on_server_request(query: &str, limit: i32, gen: u64) -> Value {
    json!({
        "@type": "searchChatsOnServer",
        "query": query,
        "type_filter": null,
        "limit": limit,
        "@extra": format!("searchChatsOnServer:{gen}")
    })
}

fn search_public_chats_request(query: &str, gen: u64) -> Value {
    json!({
        "@type": "searchPublicChats",
        "query": query,
        "type_filter": null,
        "@extra": format!("searchPublicChats:{gen}")
    })
}

fn get_contacts_request() -> Value {
    json!({"@type": "getContacts", "@extra": "getContacts"})
}

fn create_private_chat_request(user_id: i64) -> Value {
    json!({
        "@type": "createPrivateChat",
        "user_id": user_id,
        "force": false,
        "@extra": format!("createPrivateChat:{user_id}")
    })
}

fn get_chat_search_request(chat_id: i64) -> Value {
    json!({
        "@type": "getChat",
        "chat_id": chat_id,
        "@extra": format!("getChat:search:{chat_id}")
    })
}

fn get_chat_request(chat_id: i64) -> Value {
    json!({
        "@type": "getChat",
        "chat_id": chat_id,
        "@extra": format!("getChat:{chat_id}")
    })
}

fn get_user_request(user_id: i64) -> Value {
    json!({
        "@type": "getUser",
        "user_id": user_id,
        "@extra": format!("getUser:{user_id}")
    })
}

fn history_request(chat_id: i64, from_message_id: i64) -> Value {
    json!({
        "@type": "getChatHistory",
        "chat_id": chat_id,
        "from_message_id": from_message_id,
        "offset": 0,
        "limit": HISTORY_LIMIT,
        "only_local": false,
        "@extra": format!("history:{chat_id}")
    })
}

fn open_chat_request(chat_id: i64) -> Value {
    json!({
        "@type": "openChat",
        "chat_id": chat_id,
        "@extra": format!("openChat:{chat_id}")
    })
}

fn close_chat_request(chat_id: i64) -> Value {
    json!({
        "@type": "closeChat",
        "chat_id": chat_id,
        "@extra": format!("closeChat:{chat_id}")
    })
}

fn view_messages_request(chat_id: i64, message_ids: &[i64]) -> Value {
    json!({
        "@type": "viewMessages",
        "chat_id": chat_id,
        "message_ids": message_ids,
        "source": {"@type": "messageSourceChatHistory"},
        "force_read": true,
        "@extra": format!("view:{chat_id}")
    })
}

fn toggle_marked_unread_request(chat_id: i64, marked: bool) -> Value {
    json!({
        "@type": "toggleChatIsMarkedAsUnread",
        "chat_id": chat_id,
        "is_marked_as_unread": marked,
        "@extra": format!("marked:{chat_id}")
    })
}

fn send_text_request(chat_id: i64, text: &str) -> Value {
    json!({
        "@type": "sendMessage",
        "chat_id": chat_id,
        "input_message_content": {
            "@type": "inputMessageText",
            "text": {"@type": "formattedText", "text": text}
        },
        "@extra": format!("send:{chat_id}")
    })
}

/// Background TDLib client. Dropping it asks the worker to `close`.
pub struct LiveClient {
    commands: Sender<ShellCommand>,
    updates: Receiver<UiUpdate>,
    join: Option<JoinHandle<()>>,
}

impl LiveClient {
    pub fn spawn(
        td: TdJson,
        cfg: SessionConfig,
        verbosity: i32,
        debug: bool,
    ) -> Result<Self, String> {
        let (cmd_tx, cmd_rx) = mpsc::channel();
        let (ui_tx, ui_rx) = mpsc::channel();
        let join = thread::Builder::new()
            .name("tdlib-receive".into())
            .spawn(move || worker(td, cfg, verbosity, debug, cmd_rx, ui_tx))
            .map_err(|err| format!("failed to start the TDLib thread: {err}"))?;
        Ok(Self {
            commands: cmd_tx,
            updates: ui_rx,
            join: Some(join),
        })
    }

    pub fn send(&self, command: ShellCommand) {
        let _ = self.commands.send(command);
    }

    pub fn drain(&self) -> Vec<UiUpdate> {
        let mut updates = Vec::new();
        // Bound a single GTK tick so a burst cannot stall painting.
        while updates.len() < 64 {
            match self.updates.try_recv() {
                Ok(update) => updates.push(update),
                Err(_) => break,
            }
        }
        updates
    }
}

impl Drop for LiveClient {
    fn drop(&mut self) {
        let _ = self.commands.send(ShellCommand::Close);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

fn worker(
    td: TdJson,
    cfg: SessionConfig,
    verbosity: i32,
    debug: bool,
    commands: Receiver<ShellCommand>,
    updates: Sender<UiUpdate>,
) {
    let verbosity_request = match serde_json::to_string(&set_verbosity_request(verbosity)) {
        Ok(body) => body,
        Err(err) => {
            let _ = updates.send(UiUpdate::Fatal(err.to_string()));
            return;
        }
    };
    if let Ok(Some(response)) = td.execute(&verbosity_request) {
        if response.contains("\"@type\":\"error\"") {
            let _ = updates.send(UiUpdate::Log(format!(
                "TDLib log verbosity was rejected: {response}"
            )));
        }
    }
    if let Ok(body) = serde_json::to_string(&version_request()) {
        match td.execute(&body) {
            Ok(Some(response)) => match parse_version(&response) {
                Some(version) => {
                    let _ = updates.send(UiUpdate::Log(format!("TDLib version: {version}")));
                }
                None => {
                    let _ = updates.send(UiUpdate::Log(format!(
                        "TDLib version: (unparsed) {response}"
                    )));
                }
            },
            _ => {
                let _ = updates.send(UiUpdate::Log("TDLib version: (no response)".into()));
            }
        }
    }
    let _ = updates.send(UiUpdate::Log(td.mithka().line()));

    let client_id = td.create_client_id();
    if client_id <= 0 {
        let _ = updates.send(UiUpdate::Fatal(format!(
            "td_create_client_id returned {client_id}"
        )));
        return;
    }
    let _ = updates.send(UiUpdate::Log(format!("client_id: {client_id}")));

    let mut shell = Shell::new(cfg);
    if !dispatch(
        &td,
        client_id,
        &updates,
        &Effect::send(vec![bootstrap_request()]),
    )
    .is_ok()
    {
        return;
    }

    let mut closing_since: Option<Instant> = None;
    loop {
        loop {
            match commands.try_recv() {
                Ok(command) => {
                    let effect = shell.on_command(command);
                    if !dispatch(&td, client_id, &updates, &effect).is_ok() {
                        return;
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    let effect = shell.on_command(ShellCommand::Close);
                    let _ = dispatch(&td, client_id, &updates, &effect);
                    return;
                }
            }
        }
        if shell.closing && closing_since.is_none() {
            closing_since = Some(Instant::now());
        }
        if shell.finished {
            break;
        }
        if closing_since.is_some_and(|started| started.elapsed() > Duration::from_secs(8)) {
            let _ = updates.send(UiUpdate::Log("close timed out".into()));
            break;
        }

        let text = match td.receive(0.2) {
            Some(text) => text,
            None => continue,
        };
        let event: Value = match serde_json::from_str::<Value>(&text) {
            Ok(value) if value.is_object() => value,
            _ => {
                let _ = updates.send(UiUpdate::Log(format!(
                    "ignored non-json TDLib event ({} bytes)",
                    text.len()
                )));
                continue;
            }
        };
        if debug {
            let _ = updates.send(UiUpdate::Log(format!(
                "event: {}",
                event["@type"].as_str().unwrap_or("?")
            )));
        }
        let effect = shell.on_event(&event);
        if !dispatch(&td, client_id, &updates, &effect).is_ok() {
            return;
        }
    }
}

fn dispatch(
    td: &TdJson,
    client_id: i32,
    updates: &Sender<UiUpdate>,
    effect: &Effect,
) -> Result<(), ()> {
    for update in &effect.ui {
        if matches!(update, UiUpdate::Log(line) if line.contains("api_hash")) {
            continue;
        }
        if updates.send(update.clone()).is_err() {
            return Err(());
        }
    }
    for request in &effect.send {
        let body = serde_json::to_string(request).map_err(|_| ())?;
        td.send(client_id, &body).map_err(|_| ())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> SessionConfig {
        SessionConfig {
            database_directory: "/tmp/mithka-copy/tdlib".into(),
            files_directory: "/tmp/mithka-copy/tdlib/files".into(),
            api_id: 100,
            api_hash: "unit-test-hash-not-a-secret".into(),
            device_model: "Android".into(),
            system_language_code: "en".into(),
            system_version: "Linux".into(),
            application_version: "mithka-gtk/0.1.0".into(),
            chat_limit: 20,
        }
    }

    fn drive(shell: &mut Shell, event: Value) -> Effect {
        shell.on_event(&event)
    }

    fn conversation_messages(effect: &Effect) -> &[TextMessage] {
        effect
            .ui
            .iter()
            .find_map(|update| match update {
                UiUpdate::Conversation { messages, .. } => Some(messages.as_slice()),
                _ => None,
            })
            .expect("missing conversation")
    }

    fn ready(shell: &mut Shell) {
        drive(
            shell,
            json!({
                "@type": "updateAuthorizationState",
                "authorization_state": {"@type": "authorizationStateWaitTdlibParameters"}
            }),
        );
        drive(
            shell,
            json!({
                "@type": "updateAuthorizationState",
                "authorization_state": {"@type": "authorizationStateReady"}
            }),
        );
    }

    #[test]
    fn parameters_keep_the_empty_mithka_key() {
        let mut shell = Shell::new(cfg());
        let effect = drive(
            &mut shell,
            json!({
                "@type": "updateAuthorizationState",
                "authorization_state": {"@type": "authorizationStateWaitTdlibParameters"}
            }),
        );
        assert_eq!(effect.send[0]["database_encryption_key"], "");
        assert_eq!(effect.send[0]["use_test_dc"], false);
        assert_eq!(effect.send[0]["device_model"], "Android");
        assert_eq!(effect.send[0]["use_secret_chats"], true);
        let text = format!("{:?}", effect.ui);
        assert!(!text.contains("unit-test-hash-not-a-secret"));
    }

    #[test]
    fn chat_list_follows_last_activity_order() {
        let mut shell = Shell::new(cfg());
        ready(&mut shell);
        drive(
            &mut shell,
            json!({
                "@type": "updateNewChat",
                "chat": {
                    "id": 1,
                    "title": "Older",
                    "positions": [{
                        "list": {"@type": "chatListMain"},
                        "order": "10"
                    }]
                }
            }),
        );
        let effect = drive(
            &mut shell,
            json!({
                "@type": "updateNewChat",
                "chat": {
                    "id": 2,
                    "title": "Newer",
                    "positions": [{
                        "list": {"@type": "chatListMain"},
                        "order": "50"
                    }]
                }
            }),
        );
        let UiUpdate::ChatList(items) = effect
            .ui
            .iter()
            .find(|update| matches!(update, UiUpdate::ChatList(_)))
            .unwrap()
        else {
            panic!("missing list");
        };
        assert_eq!(items[0].title, "Newer");
        assert_eq!(items[1].title, "Older");
    }

    #[test]
    fn unread_badge_and_avatar_follow_chat_and_file_updates() {
        let mut shell = Shell::new(cfg());
        ready(&mut shell);
        let effect = drive(
            &mut shell,
            json!({
                "@type": "updateNewChat",
                "chat": {
                    "id": 1,
                    "title": "Alpha",
                    "unread_count": 4,
                    "is_marked_as_unread": false,
                    "photo": {
                        "@type": "chatPhotoInfo",
                        "small": {
                            "@type": "file",
                            "id": 11,
                            "local": {
                                "@type": "localFile",
                                "path": "",
                                "is_downloading_completed": false,
                                "can_be_downloaded": true
                            }
                        }
                    }
                }
            }),
        );
        assert_eq!(effect.send[0]["@type"], "downloadFile");
        assert_eq!(effect.send[0]["file_id"], 11);
        assert_eq!(effect.send[0]["synchronous"], false);
        let UiUpdate::ChatList(items) = effect
            .ui
            .iter()
            .find(|u| matches!(u, UiUpdate::ChatList(_)))
            .unwrap()
        else {
            panic!("missing list");
        };
        assert_eq!(items[0].unread, 4);
        assert!(items[0].avatar.is_none());

        let effect = drive(
            &mut shell,
            json!({
                "@type": "updateFile",
                "file": {
                    "@type": "file",
                    "id": 11,
                    "local": {
                        "path": "/tmp/mithka-stub-avatar.png",
                        "is_downloading_completed": true
                    }
                }
            }),
        );
        let UiUpdate::ChatList(items) = effect
            .ui
            .iter()
            .find(|u| matches!(u, UiUpdate::ChatList(_)))
            .unwrap()
        else {
            panic!("missing list");
        };
        assert_eq!(
            items[0].avatar.as_deref(),
            Some("/tmp/mithka-stub-avatar.png")
        );

        let effect = drive(
            &mut shell,
            json!({
                "@type": "updateChatReadInbox",
                "chat_id": 1,
                "unread_count": 0
            }),
        );
        let UiUpdate::ChatList(items) = effect
            .ui
            .iter()
            .find(|u| matches!(u, UiUpdate::ChatList(_)))
            .unwrap()
        else {
            panic!("missing list");
        };
        assert_eq!(items[0].unread, 0);

        let effect = drive(
            &mut shell,
            json!({
                "@type": "updateChatIsMarkedAsUnread",
                "chat_id": 1,
                "is_marked_as_unread": true
            }),
        );
        let UiUpdate::ChatList(items) = effect
            .ui
            .iter()
            .find(|u| matches!(u, UiUpdate::ChatList(_)))
            .unwrap()
        else {
            panic!("missing list");
        };
        assert!(items[0].marked_unread);

        let effect = drive(
            &mut shell,
            json!({
                "@type": "updateNewChat",
                "chat": {"id": 2, "title": "Beta"}
            }),
        );
        let UiUpdate::ChatList(items) = effect
            .ui
            .iter()
            .find(|u| matches!(u, UiUpdate::ChatList(_)))
            .unwrap()
        else {
            panic!("missing list");
        };
        let beta = items.iter().find(|chat| chat.id == 2).unwrap();
        assert_eq!(beta.unread, 0);
        assert!(beta.avatar.is_none());
        assert!(effect.send.is_empty());
    }

    #[test]
    fn startup_waits_for_update_chat_folders_and_does_not_call_get_chat_folders() {
        let mut shell = Shell::new(cfg());
        let steps = [
            json!({
                "@type": "updateAuthorizationState",
                "authorization_state": {"@type": "authorizationStateWaitTdlibParameters"}
            }),
            json!({
                "@type": "updateAuthorizationState",
                "authorization_state": {"@type": "authorizationStateReady"}
            }),
            json!({"@type": "chats", "chat_ids": [1, 2], "@extra": "getChats"}),
            json!({"@type": "chat", "id": 2, "title": "Beta"}),
        ];
        for event in steps {
            let effect = drive(&mut shell, event);
            let body = serde_json::to_string(&effect.send).unwrap();
            assert!(
                !body.contains("getChatFolders"),
                "TDLib 1.8.67 has no getChatFolders: {body}"
            );
        }
        let effect = drive(
            &mut shell,
            json!({
                "@type": "updateChatFolders",
                "chat_folders": [{
                    "@type": "chatFolderInfo",
                    "id": 2,
                    "name": {
                        "@type": "chatFolderName",
                        "text": {"@type": "formattedText", "text": "Work"}
                    }
                }],
                "main_chat_list_position": 0,
                "are_tags_enabled": false
            }),
        );
        let UiUpdate::Folders(folders) = effect
            .ui
            .iter()
            .find(|update| matches!(update, UiUpdate::Folders(_)))
            .unwrap()
        else {
            panic!("missing folders");
        };
        assert_eq!(folders[0].title, "Work");
        assert!(effect.send.is_empty());
    }

    #[test]
    fn folder_filter_keeps_main_list_and_requests_that_folder() {
        let mut shell = Shell::new(cfg());
        ready(&mut shell);
        drive(
            &mut shell,
            json!({
                "@type": "updateNewChat",
                "chat": {
                    "id": 1,
                    "title": "Alpha",
                    "positions": [{"list": {"@type": "chatListMain"}, "order": "10"}]
                }
            }),
        );
        drive(
            &mut shell,
            json!({
                "@type": "updateNewChat",
                "chat": {
                    "id": 2,
                    "title": "Beta",
                    "positions": [
                        {"list": {"@type": "chatListMain"}, "order": "40"},
                        {"list": {"@type": "chatListFolder", "chat_folder_id": 2}, "order": "5"}
                    ]
                }
            }),
        );
        let effect = drive(
            &mut shell,
            json!({
                "@type": "updateChatFolders",
                "chat_folders": [
                    {"@type": "chatFolderInfo", "id": 2, "title": "Work"},
                    {
                        "@type": "chatFolderInfo",
                        "id": 3,
                        "name": {"@type": "chatFolderName", "text": {"text": "Personal"}}
                    }
                ]
            }),
        );
        let UiUpdate::Folders(folders) = effect
            .ui
            .iter()
            .find(|u| matches!(u, UiUpdate::Folders(_)))
            .unwrap()
        else {
            panic!("missing folders");
        };
        assert_eq!(folders[0].title, "Work");
        assert_eq!(folders[1].title, "Personal");

        let effect = shell.on_command(ShellCommand::SelectFolder(Some(2)));
        assert_eq!(effect.send[0]["@type"], "loadChats");
        assert_eq!(effect.send[0]["chat_list"]["chat_folder_id"], 2);
        assert_eq!(effect.send[1]["@type"], "getChats");
        assert_eq!(effect.send[1]["chat_list"]["@type"], "chatListFolder");
        let UiUpdate::ChatList(items) = effect
            .ui
            .iter()
            .find(|u| matches!(u, UiUpdate::ChatList(_)))
            .unwrap()
        else {
            panic!("missing list");
        };
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "Beta");

        let effect = shell.on_command(ShellCommand::SelectFolder(None));
        assert!(effect.send.is_empty());
        let UiUpdate::ChatList(items) = effect
            .ui
            .iter()
            .find(|u| matches!(u, UiUpdate::ChatList(_)))
            .unwrap()
        else {
            panic!("missing list");
        };
        assert_eq!(items[0].title, "Beta");
        assert_eq!(items[1].title, "Alpha");
    }

    #[test]
    fn history_keeps_text_and_orders_oldest_first() {
        let mut shell = Shell::new(cfg());
        ready(&mut shell);
        drive(
            &mut shell,
            json!({"@type": "updateNewChat", "chat": {"id": 5, "title": "Ada"}}),
        );
        shell.my_user_id = Some(7);
        let effect = shell.on_command(ShellCommand::SelectChat(5));
        assert!(effect
            .send
            .iter()
            .any(|request| request["@type"] == "openChat" && request["chat_id"] == 5));
        let history = effect
            .send
            .iter()
            .find(|request| request["@type"] == "getChatHistory")
            .unwrap();
        assert_eq!(history["from_message_id"], 0);
        assert_eq!(history["offset"], 0);
        assert!(!effect
            .send
            .iter()
            .any(|request| request["@type"] == "logOut" || request["@type"] == "getChatFolders"));
        let effect = drive(
            &mut shell,
            json!({
                "@type": "messages",
                "messages": [
                    {
                        "@type": "message",
                        "id": 2,
                        "chat_id": 5,
                        "date": 200,
                        "sender_id": {"@type": "messageSenderUser", "user_id": 7},
                        "content": {"@type": "messageText", "text": {"text": "Later"}}
                    },
                    {
                        "@type": "message",
                        "id": 1,
                        "chat_id": 5,
                        "date": 100,
                        "sender_id": {"@type": "messageSenderUser", "user_id": 9},
                        "content": {"@type": "messageText", "text": {"text": "Earlier\nline"}}
                    },
                    {
                        "@type": "message",
                        "id": 3,
                        "chat_id": 5,
                        "date": 150,
                        "content": {"@type": "messagePhoto"}
                    }
                ]
            }),
        );
        let UiUpdate::Conversation { messages, .. } = effect
            .ui
            .iter()
            .find(|update| matches!(update, UiUpdate::Conversation { .. }))
            .unwrap()
        else {
            panic!("missing conversation");
        };
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0].text, "Earlier\nline");
        assert_eq!(messages[0].kind, MessageKind::Text);
        assert_eq!(messages[0].sender, "User 9");
        assert_eq!(messages[1].kind, MessageKind::Photo);
        assert!(messages[1].photo.is_none());
        assert_eq!(messages[1].text, "");
        assert_eq!(messages[2].text, "Later");
        assert_eq!(messages[2].sender, "You");
        assert!(!messages[2].outgoing);
        assert!(!messages[2].read);
    }

    #[test]
    fn outgoing_receipts_follow_last_read_outbox() {
        let mut shell = Shell::new(cfg());
        ready(&mut shell);
        drive(
            &mut shell,
            json!({
                "@type": "updateNewChat",
                "chat": {
                    "id": 5,
                    "title": "Ada",
                    "last_read_outbox_message_id": 10
                }
            }),
        );
        shell.on_command(ShellCommand::SelectChat(5));
        let effect = drive(
            &mut shell,
            json!({
                "@type": "messages",
                "messages": [
                    {
                        "@type": "message",
                        "id": 10,
                        "chat_id": 5,
                        "date": 1,
                        "is_outgoing": true,
                        "content": {"@type": "messageText", "text": {"text": "Seen"}}
                    },
                    {
                        "@type": "message",
                        "id": 12,
                        "chat_id": 5,
                        "date": 2,
                        "is_outgoing": true,
                        "interaction_info": {"@type": "messageInteractionInfo", "view_count": 4},
                        "content": {"@type": "messageText", "text": {"text": "Waiting"}}
                    },
                    {
                        "@type": "message",
                        "id": 9,
                        "chat_id": 5,
                        "date": 3,
                        "is_outgoing": false,
                        "content": {"@type": "messageText", "text": {"text": "In"}}
                    }
                ]
            }),
        );
        let messages = conversation_messages(&effect);
        let seen = messages.iter().find(|message| message.id == 10).unwrap();
        let waiting = messages.iter().find(|message| message.id == 12).unwrap();
        let incoming = messages.iter().find(|message| message.id == 9).unwrap();
        assert!(seen.outgoing && seen.read);
        assert!(waiting.outgoing && !waiting.read);
        assert!(!incoming.outgoing && !incoming.read);
        assert!(!effect.send.iter().any(|request| {
            request["@type"] == "getMessageViewers" || request["@type"] == "toggleChatIsPinned"
        }));

        let effect = drive(
            &mut shell,
            json!({
                "@type": "updateChatReadOutbox",
                "chat_id": 5,
                "last_read_outbox_message_id": 12
            }),
        );
        let messages = conversation_messages(&effect);
        assert!(messages
            .iter()
            .any(|message| message.id == 12 && message.read));
        assert!(messages
            .iter()
            .any(|message| message.id == 10 && message.read));
    }

    #[test]
    fn photo_message_downloads_then_keeps_the_local_path() {
        let mut shell = Shell::new(cfg());
        ready(&mut shell);
        drive(
            &mut shell,
            json!({"@type": "updateNewChat", "chat": {"id": 5, "title": "Ada"}}),
        );
        shell.on_command(ShellCommand::SelectChat(5));
        let effect = drive(
            &mut shell,
            json!({
                "@type": "messages",
                "messages": [{
                    "@type": "message",
                    "id": 8,
                    "chat_id": 5,
                    "date": 10,
                    "content": {
                        "@type": "messagePhoto",
                        "caption": {"@type": "formattedText", "text": "Pier"},
                        "photo": {
                            "@type": "photo",
                            "sizes": [
                                {"@type": "photoSize", "type": "s", "photo": {"@type": "file", "id": 1, "local": {"is_downloading_completed": false, "can_be_downloaded": true, "path": ""}}},
                                {"@type": "photoSize", "type": "m", "photo": {"@type": "file", "id": 2, "local": {"is_downloading_completed": false, "can_be_downloaded": true, "path": ""}}}
                            ]
                        }
                    }
                }]
            }),
        );
        assert_eq!(effect.send[0]["@type"], "downloadFile");
        assert_eq!(effect.send[0]["file_id"], 2);
        let UiUpdate::Conversation { messages, .. } = effect
            .ui
            .iter()
            .find(|u| matches!(u, UiUpdate::Conversation { .. }))
            .unwrap()
        else {
            panic!("missing conversation");
        };
        assert_eq!(messages[0].kind, MessageKind::Photo);
        assert_eq!(messages[0].text, "Pier");
        assert!(messages[0].photo.is_none());
        assert!(messages[0].photo_full.is_none());
        assert_eq!(
            effect
                .send
                .iter()
                .filter(|request| request["@type"] == "downloadFile")
                .count(),
            1
        );

        let effect = drive(
            &mut shell,
            json!({
                "@type": "updateFile",
                "file": {
                    "id": 2,
                    "local": {"path": "/tmp/mithka-stub-photo.png", "is_downloading_completed": true}
                }
            }),
        );
        let UiUpdate::Conversation { messages, .. } = effect
            .ui
            .iter()
            .find(|u| matches!(u, UiUpdate::Conversation { .. }))
            .unwrap()
        else {
            panic!("missing conversation");
        };
        assert_eq!(
            messages[0].photo.as_deref(),
            Some("/tmp/mithka-stub-photo.png")
        );
        assert_eq!(
            messages[0].photo_full.as_deref(),
            Some("/tmp/mithka-stub-photo.png")
        );
    }

    #[test]
    fn popup_downloads_the_largest_photo_size_and_keeps_the_preview() {
        let mut shell = Shell::new(cfg());
        ready(&mut shell);
        drive(
            &mut shell,
            json!({"@type": "updateNewChat", "chat": {"id": 5, "title": "Ada"}}),
        );
        shell.on_command(ShellCommand::SelectChat(5));
        let effect = drive(
            &mut shell,
            json!({
                "@type": "messages",
                "messages": [{
                    "@type": "message",
                    "id": 8,
                    "chat_id": 5,
                    "date": 10,
                    "content": {
                        "@type": "messagePhoto",
                        "caption": {"@type": "formattedText", "text": "Pier"},
                        "photo": {
                            "@type": "photo",
                            "sizes": [
                                {"@type": "photoSize", "type": "i", "width": 5000, "height": 5000, "photo": {"@type": "file", "id": 3, "local": {"is_downloading_completed": false, "can_be_downloaded": true, "path": ""}}},
                                {"@type": "photoSize", "type": "s", "width": 100, "height": 67, "photo": {"@type": "file", "id": 1, "local": {"is_downloading_completed": false, "can_be_downloaded": true, "path": ""}}},
                                {"@type": "photoSize", "type": "m", "width": 320, "height": 214, "photo": {"@type": "file", "id": 2, "local": {"is_downloading_completed": false, "can_be_downloaded": true, "path": ""}}},
                                {"@type": "photoSize", "type": "y", "width": 1280, "height": 854, "photo": {"@type": "file", "id": 8, "local": {"is_downloading_completed": false, "can_be_downloaded": true, "path": ""}}},
                                {"@type": "photoSize", "type": "w", "width": 2560, "height": 1706, "photo": {"@type": "file", "id": 9, "local": {"is_downloading_completed": false, "can_be_downloaded": true, "path": ""}}}
                            ]
                        }
                    }
                }]
            }),
        );
        let downloads: Vec<i64> = effect
            .send
            .iter()
            .filter(|request| request["@type"] == "downloadFile")
            .map(|request| request["file_id"].as_i64().unwrap())
            .collect();
        assert_eq!(downloads, vec![2, 9]);
        let messages = conversation_messages(&effect);
        assert_eq!(messages[0].photo_full_width, 2560);
        assert_eq!(messages[0].photo_full_height, 1706);
        assert!(messages[0].photo.is_none());
        assert!(messages[0].photo_full.is_none());

        let effect = drive(
            &mut shell,
            json!({
                "@type": "updateFile",
                "file": {
                    "id": 2,
                    "local": {"path": "/tmp/preview.png", "is_downloading_completed": true}
                }
            }),
        );
        let messages = conversation_messages(&effect);
        assert_eq!(messages[0].photo.as_deref(), Some("/tmp/preview.png"));
        assert!(messages[0].photo_full.is_none());

        let effect = drive(
            &mut shell,
            json!({
                "@type": "updateFile",
                "file": {
                    "id": 9,
                    "local": {"path": "/tmp/full.png", "is_downloading_completed": true}
                }
            }),
        );
        let messages = conversation_messages(&effect);
        assert_eq!(messages[0].photo.as_deref(), Some("/tmp/preview.png"));
        assert_eq!(messages[0].photo_full.as_deref(), Some("/tmp/full.png"));
        assert_eq!(messages[0].photo_full_width, 2560);
    }

    #[test]
    fn popup_prefers_type_w_when_dimensions_are_missing() {
        let mut shell = Shell::new(cfg());
        ready(&mut shell);
        drive(
            &mut shell,
            json!({"@type": "updateNewChat", "chat": {"id": 5, "title": "Ada"}}),
        );
        shell.on_command(ShellCommand::SelectChat(5));
        let effect = drive(
            &mut shell,
            json!({
                "@type": "messages",
                "messages": [{
                    "@type": "message",
                    "id": 8,
                    "chat_id": 5,
                    "date": 10,
                    "content": {
                        "@type": "messagePhoto",
                        "photo": {
                            "@type": "photo",
                            "sizes": [
                                {"@type": "photoSize", "type": "m", "photo": {"@type": "file", "id": 2, "local": {"is_downloading_completed": false, "can_be_downloaded": true, "path": ""}}},
                                {"@type": "photoSize", "type": "x", "photo": {"@type": "file", "id": 4, "local": {"is_downloading_completed": false, "can_be_downloaded": true, "path": ""}}},
                                {"@type": "photoSize", "type": "w", "photo": {"@type": "file", "id": 9, "local": {"is_downloading_completed": false, "can_be_downloaded": true, "path": ""}}}
                            ]
                        }
                    }
                }]
            }),
        );
        let downloads: Vec<i64> = effect
            .send
            .iter()
            .filter(|request| request["@type"] == "downloadFile")
            .map(|request| request["file_id"].as_i64().unwrap())
            .collect();
        assert_eq!(downloads, vec![2, 9]);
    }

    #[test]
    fn send_is_plain_text_and_never_logs_out() {
        let mut shell = Shell::new(cfg());
        ready(&mut shell);
        let effect = shell.on_command(ShellCommand::SendText {
            chat_id: 5,
            text: "  hello  ".into(),
        });
        assert_eq!(effect.send[0]["@type"], "sendMessage");
        assert_eq!(
            effect.send[0]["input_message_content"]["text"]["text"],
            "hello"
        );
        assert_ne!(effect.send[0]["@type"], "logOut");
        let empty = shell.on_command(ShellCommand::SendText {
            chat_id: 5,
            text: "   ".into(),
        });
        assert!(empty.send.is_empty());
    }

    #[test]
    fn logged_out_session_sets_a_status_and_does_not_ask_for_a_phone() {
        let mut shell = Shell::new(cfg());
        let effect = drive(
            &mut shell,
            json!({
                "@type": "updateAuthorizationState",
                "authorization_state": {"@type": "authorizationStateWaitPhoneNumber"}
            }),
        );
        let text = format!("{:?}", effect.ui);
        assert!(text.contains("does not ask"));
        assert!(effect.send.is_empty());
        assert!(!text.contains("setAuthenticationPhoneNumber"));
    }

    #[test]
    fn encryption_error_is_visible() {
        let mut shell = Shell::new(cfg());
        let effect = drive(
            &mut shell,
            json!({
                "@type": "error",
                "code": 401,
                "message": "Wrong database encryption key",
                "@extra": "setTdlibParameters"
            }),
        );
        assert!(effect.ui.iter().any(|update| matches!(update, UiUpdate::Fatal(text) if text.contains("401") && text.contains("empty"))));
        assert_eq!(effect.send[0]["@type"], "close");
    }

    fn list_items(effect: &Effect) -> &Vec<ChatItem> {
        let UiUpdate::ChatList(items) = effect
            .ui
            .iter()
            .find(|update| matches!(update, UiUpdate::ChatList(_)))
            .unwrap()
        else {
            panic!("missing list");
        };
        items
    }

    #[test]
    fn last_message_preview_follows_the_chat_and_later_updates() {
        let mut shell = Shell::new(cfg());
        ready(&mut shell);
        let effect = drive(
            &mut shell,
            json!({
                "@type": "updateNewChat",
                "chat": {
                    "id": 1,
                    "title": "Alpha",
                    "unread_count": 2,
                    "last_message": {
                        "@type": "message",
                        "id": 9,
                        "chat_id": 1,
                        "date": 1700000000,
                        "content": {
                            "@type": "messageText",
                            "text": {"@type": "formattedText", "text": "Hello\nfrom Alpha"}
                        }
                    }
                }
            }),
        );
        let items = list_items(&effect);
        assert_eq!(items[0].preview, "Hello from Alpha");
        assert_eq!(items[0].preview_date, 1700000000);
        assert_eq!(items[0].unread, 2);

        let effect = drive(
            &mut shell,
            json!({
                "@type": "updateChatLastMessage",
                "chat_id": 1,
                "last_message": {
                    "@type": "message",
                    "id": 10,
                    "chat_id": 1,
                    "date": 1700001111,
                    "content": {"@type": "messagePhoto", "caption": {"@type": "formattedText", "text": ""}}
                },
                "positions": [{
                    "list": {"@type": "chatListMain"},
                    "order": "80"
                }]
            }),
        );
        let items = list_items(&effect);
        assert_eq!(items[0].preview, "Photo");
        assert_eq!(items[0].preview_date, 1700001111);
        assert_eq!(items[0].order, 80);
    }

    #[test]
    fn opening_a_chat_marks_it_read_after_history_syncs() {
        let mut shell = Shell::new(cfg());
        ready(&mut shell);
        drive(
            &mut shell,
            json!({
                "@type": "updateNewChat",
                "chat": {
                    "id": 1,
                    "title": "Alpha",
                    "unread_count": 3,
                    "is_marked_as_unread": true,
                    "positions": [{"list": {"@type": "chatListMain"}, "order": "20"}]
                }
            }),
        );
        drive(
            &mut shell,
            json!({
                "@type": "updateNewChat",
                "chat": {
                    "id": 2,
                    "title": "Beta",
                    "positions": [{"list": {"@type": "chatListMain"}, "order": "10"}]
                }
            }),
        );
        let effect = shell.on_command(ShellCommand::SelectChat(1));
        assert!(effect
            .send
            .iter()
            .any(|request| request["@type"] == "openChat" && request["chat_id"] == 1));
        assert!(effect
            .send
            .iter()
            .any(|request| request["@type"] == "toggleChatIsMarkedAsUnread"
                && request["chat_id"] == 1
                && request["is_marked_as_unread"] == false));
        assert!(effect
            .send
            .iter()
            .any(|request| request["@type"] == "getChatHistory"
                && request["chat_id"] == 1
                && request["from_message_id"] == 0));
        assert!(!effect
            .send
            .iter()
            .any(|request| request["@type"] == "closeChat" || request["@type"] == "logOut"));

        let effect = drive(
            &mut shell,
            json!({
                "@type": "messages",
                "@extra": "history:1",
                "messages": [
                    {"@type": "message", "id": 12, "chat_id": 1, "date": 20, "content": {"@type": "messageText", "text": {"text": "New"}}},
                    {"@type": "message", "id": 11, "chat_id": 1, "date": 10, "content": {"@type": "messageText", "text": {"text": "Old"}}}
                ]
            }),
        );
        let view = effect
            .send
            .iter()
            .find(|request| request["@type"] == "viewMessages")
            .unwrap();
        assert_eq!(view["chat_id"], 1);
        assert_eq!(view["force_read"], true);
        assert_eq!(view["source"]["@type"], "messageSourceChatHistory");
        let ids = view["message_ids"].as_array().unwrap();
        assert_eq!(ids, &vec![json!(11), json!(12)]);

        let effect = drive(
            &mut shell,
            json!({"@type": "updateChatReadInbox", "chat_id": 1, "unread_count": 0}),
        );
        assert_eq!(list_items(&effect)[0].unread, 0);

        let effect = shell.on_command(ShellCommand::SelectChat(2));
        assert!(effect
            .send
            .iter()
            .any(|request| request["@type"] == "closeChat" && request["chat_id"] == 1));
        assert!(effect
            .send
            .iter()
            .any(|request| request["@type"] == "openChat" && request["chat_id"] == 2));
    }

    #[test]
    fn load_older_history_anchors_on_the_oldest_id() {
        let mut shell = Shell::new(cfg());
        ready(&mut shell);
        drive(
            &mut shell,
            json!({"@type": "updateNewChat", "chat": {"id": 5, "title": "Ada"}}),
        );
        shell.on_command(ShellCommand::SelectChat(5));
        drive(
            &mut shell,
            json!({
                "@type": "messages",
                "@extra": "history:5",
                "messages": [
                    {"@type": "message", "id": 20, "chat_id": 5, "date": 200, "content": {"@type": "messageText", "text": {"text": "Newer"}}},
                    {"@type": "message", "id": 10, "chat_id": 5, "date": 100, "content": {"@type": "messageText", "text": {"text": "Older"}}}
                ]
            }),
        );
        let effect = shell.on_command(ShellCommand::LoadOlder { chat_id: 5 });
        assert_eq!(effect.send.len(), 1);
        assert_eq!(effect.send[0]["@type"], "getChatHistory");
        assert_eq!(effect.send[0]["from_message_id"], 10);
        assert_eq!(effect.send[0]["offset"], 0);
        assert_eq!(effect.send[0]["limit"], HISTORY_LIMIT);
        assert!(shell
            .on_command(ShellCommand::LoadOlder { chat_id: 5 })
            .send
            .is_empty());

        drive(
            &mut shell,
            json!({
                "@type": "messages",
                "@extra": "history:5",
                "messages": [
                    {"@type": "message", "id": 10, "chat_id": 5, "date": 100, "content": {"@type": "messageText", "text": {"text": "Older"}}},
                    {"@type": "message", "id": 4, "chat_id": 5, "date": 40, "content": {"@type": "messagePhoto", "caption": {"text": "Way back"}}}
                ]
            }),
        );
        let effect = shell.on_command(ShellCommand::LoadOlder { chat_id: 5 });
        assert_eq!(effect.send[0]["from_message_id"], 4);

        drive(
            &mut shell,
            json!({
                "@type": "messages",
                "@extra": "history:5",
                "messages": [
                    {"@type": "message", "id": 4, "chat_id": 5, "date": 40, "content": {"@type": "messagePhoto", "caption": {"text": "Way back"}}}
                ]
            }),
        );
        assert!(shell
            .on_command(ShellCommand::LoadOlder { chat_id: 5 })
            .send
            .is_empty());
    }

    #[test]
    fn text_links_use_entities_and_plain_urls() {
        let mut shell = Shell::new(cfg());
        ready(&mut shell);
        drive(
            &mut shell,
            json!({"@type": "updateNewChat", "chat": {"id": 5, "title": "Ada"}}),
        );
        shell.on_command(ShellCommand::SelectChat(5));
        let body = "hi 😀 see https://example.com/a and docs.";
        let emoji_units = "hi 😀 see ".encode_utf16().count();
        let url = "https://example.com/a";
        let effect = drive(
            &mut shell,
            json!({
                "@type": "messages",
                "@extra": "history:5",
                "messages": [{
                    "@type": "message",
                    "id": 3,
                    "chat_id": 5,
                    "date": 10,
                    "content": {
                        "@type": "messageText",
                        "text": {
                            "@type": "formattedText",
                            "text": body,
                            "entities": [{
                                "@type": "textEntity",
                                "offset": emoji_units + url.encode_utf16().count() + " and ".encode_utf16().count(),
                                "length": "docs".encode_utf16().count(),
                                "type": {"@type": "textEntityTypeTextUrl", "url": "https://example.com/docs"}
                            }]
                        }
                    }
                }]
            }),
        );
        let UiUpdate::Conversation { messages, .. } = effect
            .ui
            .iter()
            .find(|update| matches!(update, UiUpdate::Conversation { .. }))
            .unwrap()
        else {
            panic!("missing conversation");
        };
        assert_eq!(messages[0].links.len(), 2);
        assert_eq!(messages[0].links[0].url, "https://example.com/a");
        assert_eq!(
            &body[messages[0].links[0].start..messages[0].links[0].end],
            url
        );
        assert_eq!(messages[0].links[1].url, "https://example.com/docs");
        assert_eq!(
            &body[messages[0].links[1].start..messages[0].links[1].end],
            "docs"
        );
    }

    fn search_requests(effect: &Effect) -> Vec<&str> {
        effect
            .send
            .iter()
            .filter_map(|request| request["@type"].as_str())
            .collect()
    }

    #[test]
    fn search_uses_1_8_67_filters_and_merges_unique_chats() {
        let mut shell = Shell::new(cfg());
        ready(&mut shell);
        drive(
            &mut shell,
            json!({
                "@type": "updateNewChat",
                "chat": {"id": 1, "title": "Alpha"}
            }),
        );

        let spaced = shell.on_command(ShellCommand::Search("hello world".into()));
        assert_eq!(
            search_requests(&spaced),
            ["searchChats", "searchChatsOnServer"]
        );
        assert!(spaced
            .send
            .iter()
            .all(|request| request["type_filter"].is_null()));
        assert_eq!(spaced.send[0]["query"], "hello world");
        assert!(spaced
            .ui
            .iter()
            .any(|update| matches!(update, UiUpdate::Status(text) if text == "Searching…")));

        let at = shell.on_command(ShellCommand::Search("@Telegram".into()));
        assert_eq!(
            search_requests(&at),
            ["searchChats", "searchChatsOnServer", "searchPublicChats"]
        );
        assert_eq!(at.send[2]["query"], "Telegram");
        assert!(at.send[2]["type_filter"].is_null());
        let gen = at.send[0]["@extra"]
            .as_str()
            .unwrap()
            .strip_prefix("searchChats:")
            .unwrap();

        let local = drive(
            &mut shell,
            json!({
                "@type": "chats",
                "chat_ids": [1, 9],
                "@extra": format!("searchChats:{gen}")
            }),
        );
        assert_eq!(local.send[0]["@type"], "getChat");
        assert_eq!(local.send[0]["@extra"], "getChat:search:9");
        assert!(local
            .ui
            .iter()
            .any(|update| matches!(update, UiUpdate::SearchResults { chats, .. } if chats.iter().any(|chat| chat.id == 1))));

        let server = drive(
            &mut shell,
            json!({
                "@type": "chats",
                "chat_ids": [1, 9],
                "@extra": format!("searchChatsOnServer:{gen}")
            }),
        );
        assert!(server.send.is_empty());

        drive(
            &mut shell,
            json!({
                "@type": "chat",
                "id": 9,
                "title": "Public",
                "@extra": "getChat:search:9"
            }),
        );
        let public = drive(
            &mut shell,
            json!({
                "@type": "chats",
                "chat_ids": [9, 11],
                "@extra": format!("searchPublicChats:{gen}")
            }),
        );
        let UiUpdate::SearchResults { query, chats } = public
            .ui
            .iter()
            .find(|update| matches!(update, UiUpdate::SearchResults { .. }))
            .unwrap()
        else {
            panic!("missing search results");
        };
        assert_eq!(query, "@Telegram");
        assert_eq!(
            chats.iter().map(|chat| chat.id).collect::<Vec<_>>(),
            vec![1, 9]
        );
        assert!(public.ui.iter().any(|update| {
            matches!(update, UiUpdate::Status(text) if text == "Search: 2 chats")
        }));

        let stale = drive(
            &mut shell,
            json!({
                "@type": "chats",
                "chat_ids": [42],
                "@extra": "searchChats:0"
            }),
        );
        assert!(stale.ui.is_empty());
        assert!(stale.send.is_empty());

        let cleared = shell.on_command(ShellCommand::Search("  ".into()));
        assert!(cleared.send.is_empty());
        assert!(cleared.ui.iter().any(|update| {
            matches!(update, UiUpdate::SearchResults { query, chats } if query.is_empty() && chats.is_empty())
        }));
        assert!(cleared
            .ui
            .iter()
            .any(|update| matches!(update, UiUpdate::Status(text) if text == "Ready")));
    }

    #[test]
    fn search_hit_without_a_list_position_stays_out_of_all_chats() {
        let mut shell = Shell::new(cfg());
        ready(&mut shell);
        drive(
            &mut shell,
            json!({"@type": "chats", "chat_ids": [1], "@extra": "getChats"}),
        );
        drive(
            &mut shell,
            json!({"@type": "chat", "id": 1, "title": "Alpha", "@extra": "getChat:1"}),
        );
        let effect = shell.on_command(ShellCommand::Search("public".into()));
        let gen = effect.send[0]["@extra"]
            .as_str()
            .unwrap()
            .strip_prefix("searchChats:")
            .unwrap()
            .to_string();
        drive(
            &mut shell,
            json!({
                "@type": "chats",
                "chat_ids": [77],
                "@extra": format!("searchChats:{gen}")
            }),
        );
        let effect = drive(
            &mut shell,
            json!({
                "@type": "chat",
                "id": 77,
                "title": "Elsewhere",
                "@extra": "getChat:search:77"
            }),
        );
        let UiUpdate::ChatList(items) = effect
            .ui
            .iter()
            .find(|update| matches!(update, UiUpdate::ChatList(_)))
            .unwrap()
        else {
            panic!("missing list");
        };
        assert!(items.iter().all(|chat| chat.id != 77));
        assert!(items.iter().any(|chat| chat.id == 1));
        let UiUpdate::SearchResults { chats, .. } = effect
            .ui
            .iter()
            .find(|update| matches!(update, UiUpdate::SearchResults { .. }))
            .unwrap()
        else {
            panic!("missing search");
        };
        assert_eq!(chats[0].title, "Elsewhere");
    }

    #[test]
    fn search_error_is_status_and_not_fatal() {
        let mut shell = Shell::new(cfg());
        ready(&mut shell);
        let effect = shell.on_command(ShellCommand::Search("ada".into()));
        let gen = effect.send[0]["@extra"]
            .as_str()
            .unwrap()
            .strip_prefix("searchChats:")
            .unwrap()
            .to_string();
        let effect = drive(
            &mut shell,
            json!({
                "@type": "error",
                "code": 400,
                "message": "QUERY_TOO_SHORT",
                "@extra": format!("searchChatsOnServer:{gen}")
            }),
        );
        assert!(!shell.failed);
        assert!(effect.ui.iter().any(|update| {
            matches!(update, UiUpdate::Status(text) if text.contains("400") && text.contains("QUERY_TOO_SHORT"))
        }));
        assert!(!effect
            .ui
            .iter()
            .any(|update| matches!(update, UiUpdate::Fatal(_))));
    }

    #[test]
    fn contacts_follow_get_contacts_and_update_user() {
        let mut shell = Shell::new(cfg());
        ready(&mut shell);
        let effect = shell.on_command(ShellCommand::LoadContacts);
        assert_eq!(effect.send[0]["@type"], "getContacts");
        assert_eq!(effect.send[0]["@extra"], "getContacts");

        let effect = drive(
            &mut shell,
            json!({
                "@type": "updateUser",
                "user": {
                    "@type": "user",
                    "id": 4,
                    "first_name": "Ada",
                    "last_name": "Lovelace",
                    "is_contact": true,
                    "usernames": {
                        "@type": "usernames",
                        "active_usernames": ["ada"],
                        "disabled_usernames": [],
                        "editable_username": "ada"
                    },
                    "profile_photo": {
                        "@type": "profilePhoto",
                        "small": {
                            "@type": "file",
                            "id": 21,
                            "local": {
                                "path": "",
                                "is_downloading_completed": false,
                                "can_be_downloaded": true
                            }
                        }
                    }
                }
            }),
        );
        assert_eq!(effect.send[0]["@type"], "downloadFile");
        assert_eq!(effect.send[0]["file_id"], 21);

        let effect = drive(
            &mut shell,
            json!({
                "@type": "users",
                "total_count": 1,
                "user_ids": [4],
                "@extra": "getContacts"
            }),
        );
        assert!(effect.send.is_empty());
        let UiUpdate::Contacts(contacts) = effect
            .ui
            .iter()
            .find(|update| matches!(update, UiUpdate::Contacts(_)))
            .unwrap()
        else {
            panic!("missing contacts");
        };
        assert_eq!(contacts.len(), 1);
        assert_eq!(contacts[0].name, "Ada Lovelace");
        assert_eq!(contacts[0].username, "ada");

        let effect = drive(
            &mut shell,
            json!({
                "@type": "updateUser",
                "user": {
                    "id": 4,
                    "first_name": "Ada",
                    "last_name": "Lovelace",
                    "is_contact": false,
                    "usernames": {"active_usernames": ["ada"]}
                }
            }),
        );
        let UiUpdate::Contacts(contacts) = effect
            .ui
            .iter()
            .find(|update| matches!(update, UiUpdate::Contacts(_)))
            .unwrap()
        else {
            panic!("missing contacts");
        };
        assert!(contacts.is_empty());

        let effect = drive(
            &mut shell,
            json!({
                "@type": "error",
                "code": 401,
                "message": "Unauthorized",
                "@extra": "getContacts"
            }),
        );
        assert!(!shell.failed);
        assert!(effect.ui.iter().any(|update| {
            matches!(update, UiUpdate::Status(text) if text.contains("Contacts error 401"))
        }));
    }

    #[test]
    fn open_contact_creates_a_private_chat_then_selects_it() {
        let mut shell = Shell::new(cfg());
        ready(&mut shell);
        let effect = shell.on_command(ShellCommand::OpenContact(4));
        assert_eq!(effect.send[0]["@type"], "createPrivateChat");
        assert_eq!(effect.send[0]["user_id"], 4);
        assert_eq!(effect.send[0]["force"], false);
        assert_eq!(effect.send[0]["@extra"], "createPrivateChat:4");

        let effect = drive(
            &mut shell,
            json!({
                "@type": "chat",
                "id": 80,
                "title": "Ada Lovelace",
                "@extra": "createPrivateChat:4"
            }),
        );
        assert!(effect
            .send
            .iter()
            .any(|request| request["@type"] == "openChat" && request["chat_id"] == 80));
        assert!(effect
            .ui
            .iter()
            .any(|update| matches!(update, UiUpdate::OpenChat(80))));
        assert!(effect.ui.iter().any(|update| {
            matches!(update, UiUpdate::Conversation { chat_id: 80, title, .. } if title == "Ada Lovelace")
        }));

        let effect = drive(
            &mut shell,
            json!({
                "@type": "error",
                "code": 400,
                "message": "User not found",
                "@extra": "createPrivateChat:4"
            }),
        );
        assert!(!shell.failed);
        assert!(effect.ui.iter().any(|update| {
            matches!(update, UiUpdate::Status(text) if text.contains("User not found"))
        }));
    }
}
