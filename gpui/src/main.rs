//! GPUI shell over the shared TDLib session.
//!
//! This is the product UI. `mithka-gtk` stays in the repo as a validation
//! spike. TDLib still runs on a background thread; this process only paints
//! snapshots the worker sends.
//!
//! The window is built with [gpui-kit](https://github.com/longbridge/gpui-kit):
//! `Root` for theme and window chrome, a local-groups column, a Telegram
//! folder column (or feed sources when Subscriptions is selected), chat rows
//! with `Avatar` and `Badge`, `Message` / `Bubble` for the transcript, `Link`
//! for http(s) entities, and `Input` plus a primary `Button` for the composer.
//! Subscriptions are local RSS/Atom items, not TDLib chats. The transcript itself is
//! GPUI's virtual list — the same list `MessageScroller` wraps — so a scroll
//! to the first row can ask TDLib for an older page. `MessageScroller` does
//! not expose that offset. Charts, docks, sidebars, tables, and forms from
//! the kit are unused.

use clap::Parser;
use gpui_kit::component::avatar::Avatar;
use gpui_kit::component::badge::Badge;
use gpui_kit::component::bubble::{Bubble, BubbleVariant};
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::link::Link;
use gpui_kit::component::message::{Message, MessageAlignment, MessageContent, MessageHeader};
use gpui_kit::component::scroll::ScrollableElement;
use gpui_kit::component::{h_flex, v_flex, ActiveTheme, Disableable, Root, Sizable, Size};
use gpui_kit::{
    div, img, list, prelude::*, px, rgb, size, App, Bounds, ClickEvent, Entity, FocusHandle,
    Focusable, FollowMode, KeyDownEvent, ListAlignment, ListState, ObjectFit, Subscription,
    TitlebarOptions, Window, WindowBounds, WindowKind, WindowOptions,
};
use mithka_rss::{store_path as subscriptions_path, FeedClient, FeedEvent, SubscriptionStore};
use mithka_tdlib::{
    inspect_database, is_http_url, list_time, message_time, ChatItem, ContactItem, FolderItem,
    LiveClient, MessageKind, SessionConfig, ShellCommand, TdJson, TextLink, TextMessage, UiUpdate,
};
use std::collections::HashSet;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

mod groups;
mod icons;
mod pins;

use groups::{store_path, GroupLibrary, NestedFolder};
use icons::{hero, Icon};
use pins::{pin_sort_key, store_path as pins_path, PinLibrary};

#[derive(Parser, Debug)]
#[command(
    name = "mithka-gpui",
    version,
    about = "GPUI window for a copied Mithka TDLib database"
)]
struct Cli {
    /// Path to Mithka's libtdjson.so (1.8.67 patched build).
    #[arg(long)]
    tdjson: PathBuf,

    /// Copied TDLib database directory (contains td.binlog and files/).
    #[arg(long)]
    database: PathBuf,

    /// Telegram api_id. Same value Mithka used for this database.
    #[arg(long, env = "TDLIB_API_ID")]
    api_id: i32,

    /// Telegram api_hash. Same value Mithka used for this database.
    #[arg(long, env = "TDLIB_API_HASH")]
    api_hash: String,

    /// TDLib device_model. Mithka on Linux sends Android.
    #[arg(long, default_value = "Android")]
    device_model: String,

    /// TDLib system_language_code.
    #[arg(long, default_value = "en")]
    system_language_code: String,

    /// TDLib system_version. Empty lets TDLib detect the OS version.
    #[arg(long, default_value = "Linux")]
    system_version: String,

    /// TDLib application_version.
    #[arg(long, default_value = "mithka-gpui/0.1.0")]
    application_version: String,

    /// How many main-list chats to keep.
    #[arg(long, default_value_t = 40)]
    chat_limit: i32,

    /// TDLib log verbosity forwarded to stderr (0 = fatal, 1 = errors).
    #[arg(long, default_value_t = 1)]
    verbosity: i32,

    /// Print each incoming TDLib @type on stdout.
    #[arg(long)]
    debug: bool,
}

fn main() {
    let cli = Cli::parse();
    let prepared = match prepare(&cli) {
        Ok(prepared) => prepared,
        Err(failure) => {
            eprintln!("error: {}", failure.message);
            std::process::exit(failure.code);
        }
    };

    println!("libtdjson: {}", prepared.tdjson_path.display());
    println!("database: {}", prepared.config.database_directory);
    println!("files: {}", prepared.config.files_directory);
    println!("api_id: {}", prepared.config.api_id);
    println!(
        "api_hash: set ({} characters)",
        prepared.config.api_hash.chars().count()
    );
    println!("device_model: {}", prepared.config.device_model);
    println!(
        "system_language_code: {}",
        prepared.config.system_language_code
    );
    println!("system_version: {}", prepared.config.system_version);
    println!(
        "application_version: {}",
        prepared.config.application_version
    );
    println!("database_encryption_key: empty");
    println!("use_test_dc: false");
    let database_key = prepared.config.database_directory.clone();
    let groups_path = store_path();
    let pins_path = pins_path();
    let feeds_path = subscriptions_path();
    println!("local groups: {}", groups_path.display());
    println!("local pins: {}", pins_path.display());
    println!("subscriptions: {}", feeds_path.display());

    let client = match LiveClient::spawn(prepared.td, prepared.config, cli.verbosity, cli.debug) {
        Ok(client) => client,
        Err(message) => {
            eprintln!("error: {message}");
            std::process::exit(1);
        }
    };

    gpui_kit::application().run(move |cx: &mut App| {
        gpui_kit::init(cx);
        let bounds = Bounds::centered(None, size(px(1180.), px(720.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some("Mithka".into()),
                    appears_transparent: false,
                    traffic_light_position: None,
                }),
                app_id: Some("ad.neko.mithka.gpui".into()),
                ..Default::default()
            },
            |window, cx| {
                let view = cx.new(|cx| {
                    ShellView::new(
                        client,
                        database_key.clone(),
                        groups_path.clone(),
                        pins_path.clone(),
                        feeds_path.clone(),
                        window,
                        cx,
                    )
                });
                cx.new(|cx| Root::new(view, window, cx))
            },
        )
        .expect("open GPUI window");

        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        cx.activate(true);
    });
}

struct Prepared {
    tdjson_path: PathBuf,
    config: SessionConfig,
    td: TdJson,
}

struct StartupError {
    code: i32,
    message: String,
}

fn prepare(cli: &Cli) -> Result<Prepared, StartupError> {
    if cli.api_id <= 0 {
        return Err(fail(2, "--api-id must be a positive integer"));
    }
    let api_hash = cli.api_hash.trim().to_string();
    if api_hash.is_empty() {
        return Err(fail(2, "--api-hash / TDLIB_API_HASH is empty"));
    }
    if cli.device_model.trim().is_empty()
        || cli.system_language_code.trim().is_empty()
        || cli.application_version.trim().is_empty()
    {
        return Err(fail(
            2,
            "--device-model, --system-language-code, and --application-version must be non-empty",
        ));
    }
    if !(1..=100).contains(&cli.chat_limit) {
        return Err(fail(2, "--chat-limit must be between 1 and 100"));
    }
    if !cli.tdjson.is_file() {
        return Err(fail(
            2,
            format!("--tdjson is not a file: {}", cli.tdjson.display()),
        ));
    }
    let database = cli.database.canonicalize().map_err(|err| {
        fail(
            2,
            format!(
                "cannot resolve --database {}: {err}",
                cli.database.display()
            ),
        )
    })?;
    for warning in inspect_database(&database).map_err(|message| fail(2, message))? {
        eprintln!("warning: {warning}");
    }
    let database_directory = utf8_path(&database)?;
    let files_directory = utf8_path(&database.join("files"))?;
    let tdjson_path = cli
        .tdjson
        .canonicalize()
        .unwrap_or_else(|_| cli.tdjson.clone());
    let td = TdJson::open(&tdjson_path).map_err(|err| fail(1, err.to_string()))?;
    Ok(Prepared {
        tdjson_path,
        td,
        config: SessionConfig {
            database_directory,
            files_directory,
            api_id: cli.api_id,
            api_hash,
            device_model: cli.device_model.clone(),
            system_language_code: cli.system_language_code.clone(),
            system_version: cli.system_version.clone(),
            application_version: cli.application_version.clone(),
            chat_limit: cli.chat_limit,
        },
    })
}

fn fail(code: i32, message: impl Into<String>) -> StartupError {
    StartupError {
        code,
        message: message.into(),
    }
}

fn utf8_path(path: &std::path::Path) -> Result<String, StartupError> {
    path.to_str()
        .map(str::to_string)
        .ok_or_else(|| fail(2, format!("path is not valid UTF-8: {}", path.display())))
}

struct ShellView {
    client: LiveClient,
    status: String,
    ready: bool,
    chats: Vec<ChatItem>,
    /// Telegram folders from `updateChatFolders`. `None` in [`Self::folder`] is All.
    folders: Vec<FolderItem>,
    folder: Option<i32>,
    local_groups: GroupLibrary,
    /// `None` shows every chat the Telegram folder already allowed.
    group_id: Option<String>,
    renaming_group: bool,
    group_draft: Entity<InputState>,
    _group_events: Subscription,
    open_chat: Option<i64>,
    convo_title: String,
    messages: Vec<TextMessage>,
    /// Ids currently mounted in [`Self::transcript`], oldest first.
    shown_ids: Vec<i64>,
    transcript: ListState,
    /// Ignore scroll-to-top until the tail pin after opening a chat has settled.
    history_scroll_after: Instant,
    /// One older-page request at a time. Cleared when that page arrives.
    loading_older: bool,
    composer: Entity<InputState>,
    /// Kept so Enter on the composer stays subscribed for the life of the window.
    _composer_events: Subscription,
    search: Entity<InputState>,
    _search_events: Subscription,
    /// Last query sent to TDLib. The field filters locally before that reply.
    search_sent: String,
    search_hits: Vec<ChatItem>,
    /// Query the current [`Self::search_hits`] belong to.
    search_echo: String,
    show_contacts: bool,
    contacts: Vec<ContactItem>,
    pins: PinLibrary,
    /// Subscriptions replaces the folder, chat, and conversation columns.
    show_feeds: bool,
    feeds: SubscriptionStore,
    /// `None` is every subscribed source.
    feed_source: Option<String>,
    feed_status: String,
    refreshing: u32,
    open_item: Option<String>,
    feed_draft: Entity<InputState>,
    _feed_events: Subscription,
    feed_client: FeedClient,
}

impl ShellView {
    fn new(
        client: LiveClient,
        database_key: String,
        groups_path: PathBuf,
        pins_path: PathBuf,
        feeds_path: PathBuf,
        window: &mut Window,
        cx: &mut gpui_kit::Context<Self>,
    ) -> Self {
        let composer = cx.new(|cx| InputState::new(window, cx).placeholder("Text message"));
        let composer_events =
            cx.subscribe_in(&composer, window, |this, _input, event, window, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    this.send_composer(window, cx);
                }
            });
        composer.read(cx).focus_handle(cx).focus(window, cx);
        let group_draft = cx.new(|cx| InputState::new(window, cx).placeholder("New group"));
        let group_events =
            cx.subscribe_in(&group_draft, window, |this, _input, event, window, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    this.commit_group_draft(window, cx);
                }
            });
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search chats"));
        let search_events = cx.subscribe_in(&search, window, |this, _input, event, _window, cx| {
            if matches!(event, InputEvent::Change) {
                this.on_search_changed(cx);
            }
        });
        let feed_draft = cx.new(|cx| InputState::new(window, cx).placeholder("Feed URL"));
        let feed_events =
            cx.subscribe_in(&feed_draft, window, |this, _input, event, window, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    this.commit_feed(window, cx);
                }
            });
        let transcript = ListState::new(0, ListAlignment::Top, px(80.));
        transcript.set_follow_mode(FollowMode::Tail);
        let view = Self {
            client,
            status: "Starting TDLib…".into(),
            ready: false,
            chats: Vec::new(),
            folders: Vec::new(),
            folder: None,
            local_groups: GroupLibrary::load(groups_path, database_key.clone()),
            pins: PinLibrary::load(pins_path, database_key),
            group_id: None,
            renaming_group: false,
            group_draft,
            _group_events: group_events,
            open_chat: None,
            convo_title: "Select a chat".into(),
            messages: Vec::new(),
            shown_ids: Vec::new(),
            transcript,
            history_scroll_after: Instant::now(),
            loading_older: false,
            composer,
            _composer_events: composer_events,
            search,
            _search_events: search_events,
            search_sent: String::new(),
            search_hits: Vec::new(),
            search_echo: String::new(),
            show_contacts: false,
            contacts: Vec::new(),
            show_feeds: false,
            feeds: SubscriptionStore::load(feeds_path),
            feed_source: None,
            feed_status: String::new(),
            refreshing: 0,
            open_item: None,
            feed_draft,
            _feed_events: feed_events,
            feed_client: FeedClient::spawn(),
        };
        view.bind_history_scroll(cx);
        view.watch(cx);
        view
    }

    fn bind_history_scroll(&self, cx: &mut gpui_kit::Context<Self>) {
        let weak = cx.weak_entity();
        self.transcript
            .set_scroll_handler(move |event, _window, cx| {
                let at_top =
                    event.visible_range.start == 0 && event.visible_range.end < event.count;
                if !at_top || event.is_following_tail {
                    return;
                }
                let weak = weak.clone();
                cx.defer(move |cx| {
                    let _ = weak.update(cx, |view, cx| view.request_older(cx));
                });
            });
    }

    fn watch(&self, cx: &mut gpui_kit::Context<Self>) {
        cx.spawn(async move |this, cx| loop {
            cx.background_executor()
                .timer(Duration::from_millis(50))
                .await;
            let alive = this
                .update(cx, |view, cx| {
                    view.poll(cx);
                })
                .is_ok();
            if !alive {
                break;
            }
        })
        .detach();
    }

    fn poll(&mut self, cx: &mut gpui_kit::Context<Self>) {
        let updates = self.client.drain();
        let feeds = self.feed_client.drain();
        if updates.is_empty() && feeds.is_empty() {
            return;
        }
        for update in updates {
            self.apply(update, cx);
        }
        for event in feeds {
            self.apply_feed(event);
        }
        cx.notify();
    }

    fn apply(&mut self, update: UiUpdate, _cx: &mut gpui_kit::Context<Self>) {
        match update {
            UiUpdate::Log(line) => println!("{line}"),
            UiUpdate::Status(text) => self.status = text,
            UiUpdate::Ready => {
                self.ready = true;
                self.status = "Ready".into();
            }
            UiUpdate::Fatal(text) => {
                self.ready = false;
                self.status = "TDLib error".into();
                self.convo_title = "TDLib error".into();
                self.messages.clear();
                self.messages.push(TextMessage {
                    id: 0,
                    chat_id: 0,
                    sender: String::new(),
                    text,
                    date: 0,
                    kind: MessageKind::Text,
                    photo: None,
                    photo_full: None,
                    photo_full_width: 0,
                    photo_full_height: 0,
                    links: Vec::new(),
                    outgoing: false,
                    read: false,
                });
                self.sync_transcript();
            }
            UiUpdate::ChatList(chats) => self.chats = chats,
            UiUpdate::Folders(folders) => {
                if self
                    .folder
                    .is_some_and(|id| !folders.iter().any(|folder| folder.id == id))
                {
                    self.folder = None;
                }
                self.folders = folders;
            }
            UiUpdate::Conversation {
                chat_id,
                title,
                messages,
            } => {
                if self.open_chat != Some(chat_id) {
                    return;
                }
                self.convo_title = title;
                self.messages = messages;
                self.sync_transcript();
            }
            UiUpdate::SearchResults { query, chats } => {
                self.search_echo = query;
                self.search_hits = chats;
            }
            UiUpdate::Contacts(contacts) => self.contacts = contacts,
            UiUpdate::OpenChat(chat_id) => {
                self.open_chat = Some(chat_id);
                self.convo_title = self
                    .chats
                    .iter()
                    .find(|chat| chat.id == chat_id)
                    .map(|chat| chat.title.clone())
                    .filter(|title| !title.is_empty() && title != "…")
                    .unwrap_or_else(|| "Loading…".into());
                self.messages.clear();
                self.loading_older = false;
                self.history_scroll_after = Instant::now() + Duration::from_millis(400);
                self.sync_transcript();
            }
        }
    }

    fn select_folder(&mut self, folder: Option<i32>, cx: &mut gpui_kit::Context<Self>) {
        let leaving_contacts = self.show_contacts;
        self.hide_contacts();
        if self.folder == folder {
            if leaving_contacts {
                cx.notify();
            }
            return;
        }
        self.folder = folder;
        self.client.send(ShellCommand::SelectFolder(folder));
        cx.notify();
    }

    fn select_group(&mut self, group_id: Option<String>, cx: &mut gpui_kit::Context<Self>) {
        if !self.show_feeds && !self.show_contacts && self.group_id == group_id {
            return;
        }
        self.show_feeds = false;
        self.hide_contacts();
        self.renaming_group = false;
        self.group_id = group_id.clone();
        if let Some(id) = group_id {
            self.focus_nested_folder(&id, cx);
        } else {
            cx.notify();
        }
    }

    /// Keep the open Telegram list when it is already inside the group.
    /// Otherwise open the first nested folder so the chat list follows it.
    fn focus_nested_folder(&mut self, group_id: &str, cx: &mut gpui_kit::Context<Self>) {
        let folders = self
            .local_groups
            .group(group_id)
            .map(|group| group.folders.clone())
            .unwrap_or_default();
        if folders.iter().any(|folder| folder.matches(self.folder)) {
            cx.notify();
            return;
        }
        if let Some(first) = folders.first().copied() {
            self.select_folder(first.telegram_id(), cx);
        } else {
            cx.notify();
        }
    }

    fn toggle_nested(&mut self, folder: NestedFolder, cx: &mut gpui_kit::Context<Self>) {
        let Some(group_id) = self.group_id.clone() else {
            return;
        };
        let Some(now_inside) = self.local_groups.toggle_folder(&group_id, folder) else {
            return;
        };
        self.persist_groups();
        if now_inside {
            if folder.matches(self.folder) {
                cx.notify();
            } else {
                self.select_folder(folder.telegram_id(), cx);
            }
            return;
        }
        if folder.matches(self.folder) {
            if let Some(next) = self
                .local_groups
                .group(&group_id)
                .and_then(|group| group.folders.first().copied())
            {
                if !next.matches(self.folder) {
                    self.select_folder(next.telegram_id(), cx);
                    return;
                }
            }
        }
        cx.notify();
    }

    fn folder_label(&self, folder: NestedFolder) -> String {
        match folder {
            NestedFolder::Main => "All".into(),
            NestedFolder::Folder { id } => self
                .folders
                .iter()
                .find(|known| known.id == id)
                .map(|known| {
                    if known.title.is_empty() {
                        format!("Folder {id}")
                    } else {
                        known.title.clone()
                    }
                })
                .unwrap_or_else(|| format!("Folder {id}")),
        }
    }

    fn known_folders(&self) -> Vec<NestedFolder> {
        let mut folders = vec![NestedFolder::Main];
        for folder in &self.folders {
            folders.push(NestedFolder::Folder { id: folder.id });
        }
        folders
    }

    fn select_subscriptions(&mut self, cx: &mut gpui_kit::Context<Self>) {
        if self.show_feeds {
            return;
        }
        self.show_feeds = true;
        self.hide_contacts();
        self.group_id = None;
        self.renaming_group = false;
        cx.notify();
    }

    fn hide_contacts(&mut self) {
        if self.show_contacts
            && self.search_sent.is_empty()
            && self.ready
            && (self.status.ends_with(" contacts")
                || self.status.starts_with("Contacts ")
                || self.status.starts_with("Opening chat"))
        {
            self.status = "Ready".into();
        }
        self.show_contacts = false;
    }

    fn select_contacts(&mut self, cx: &mut gpui_kit::Context<Self>) {
        if self.show_contacts {
            return;
        }
        self.show_contacts = true;
        self.show_feeds = false;
        self.group_id = None;
        self.renaming_group = false;
        self.client.send(ShellCommand::LoadContacts);
        cx.notify();
    }

    fn on_search_changed(&mut self, cx: &mut gpui_kit::Context<Self>) {
        let query = self.search.read(cx).value().trim().to_string();
        if query == self.search_sent {
            cx.notify();
            return;
        }
        self.search_sent = query.clone();
        if query.is_empty() {
            self.search_hits.clear();
            self.search_echo.clear();
        }
        self.client.send(ShellCommand::Search(query));
        cx.notify();
    }

    fn open_contact(
        &mut self,
        user_id: i64,
        _window: &mut Window,
        cx: &mut gpui_kit::Context<Self>,
    ) {
        if let Some(contact) = self
            .contacts
            .iter()
            .find(|contact| contact.user_id == user_id)
        {
            self.convo_title = contact.name.clone();
        }
        self.client.send(ShellCommand::OpenContact(user_id));
        cx.notify();
    }

    /// Local title matches, then TDLib hits that are not already in that set.
    fn search_rows(&self, cx: &gpui_kit::Context<Self>) -> (Vec<ChatItem>, Vec<ChatItem>) {
        let query = self.search.read(cx).value().trim().to_string();
        let visible = self.shown_chats();
        if query.is_empty() {
            return (visible, Vec::new());
        }
        let needle = query.to_lowercase();
        let local: Vec<ChatItem> = visible
            .into_iter()
            .filter(|chat| chat.title.to_lowercase().contains(&needle))
            .collect();
        let mut seen: HashSet<i64> = local.iter().map(|chat| chat.id).collect();
        let mut extra = Vec::new();
        if self.search_echo == query {
            for chat in &self.search_hits {
                if seen.insert(chat.id) {
                    extra.push(chat.clone());
                }
            }
        }
        (local, extra)
    }

    fn select_feed_source(&mut self, source_id: Option<String>, cx: &mut gpui_kit::Context<Self>) {
        if self.feed_source == source_id {
            return;
        }
        self.feed_source = source_id;
        cx.notify();
    }

    fn shown_items(&self) -> Vec<mithka_rss::Item> {
        let mut items = self.feeds.items(self.feed_source.as_deref());
        items.truncate(200);
        items
    }

    fn persist_feeds(&self) {
        if let Err(err) = self.feeds.save() {
            eprintln!("warning: could not save subscriptions: {err}");
        }
    }

    fn commit_feed(&mut self, window: &mut Window, cx: &mut gpui_kit::Context<Self>) {
        let url = self.feed_draft.read(cx).value().trim().to_string();
        if url.is_empty() {
            return;
        }
        match self.feeds.add_source(&url) {
            Ok(id) => {
                let fetch_url = self
                    .feeds
                    .source(&id)
                    .map(|source| source.url.clone())
                    .unwrap_or(url);
                self.feed_source = Some(id.clone());
                self.open_item = None;
                self.feed_status = "Refreshing…".into();
                self.refreshing = self.refreshing.saturating_add(1);
                self.persist_feeds();
                self.feed_client.refresh(id, fetch_url);
                self.feed_draft.update(cx, |input, cx| {
                    input.set_value("", window, cx);
                });
            }
            Err(message) => self.feed_status = message,
        }
        cx.notify();
    }

    fn remove_selected_feed(&mut self, cx: &mut gpui_kit::Context<Self>) {
        let Some(id) = self.feed_source.clone() else {
            return;
        };
        if self.feeds.remove_source(&id) {
            self.feed_source = None;
            self.open_item = None;
            self.feed_status.clear();
            self.persist_feeds();
            cx.notify();
        }
    }

    fn refresh_feeds(&mut self, cx: &mut gpui_kit::Context<Self>) {
        let jobs: Vec<(String, String)> = match self.feed_source.clone() {
            Some(id) => self
                .feeds
                .source(&id)
                .map(|source| vec![(source.id.clone(), source.url.clone())])
                .unwrap_or_default(),
            None => self
                .feeds
                .sources()
                .iter()
                .map(|source| (source.id.clone(), source.url.clone()))
                .collect(),
        };
        if jobs.is_empty() {
            self.feed_status = "Add a feed URL first.".into();
            cx.notify();
            return;
        }
        self.feed_status = "Refreshing…".into();
        self.refreshing = self.refreshing.saturating_add(jobs.len() as u32);
        for (id, url) in jobs {
            self.feed_client.refresh(id, url);
        }
        cx.notify();
    }

    fn apply_feed(&mut self, event: FeedEvent) {
        self.refreshing = self.refreshing.saturating_sub(1);
        match event {
            FeedEvent::Fetched { id, title, items } => {
                let count = items.len();
                if !title.is_empty() {
                    self.feeds.set_title(&id, &title);
                }
                if self.feeds.replace_items(&id, items) {
                    self.persist_feeds();
                }
                if self
                    .open_item
                    .as_deref()
                    .is_some_and(|open| self.feeds.item(open).is_none())
                {
                    self.open_item = None;
                }
                let name = self
                    .feeds
                    .source(&id)
                    .map(|source| source.display_name())
                    .unwrap_or_else(|| id.clone());
                self.feed_status = format!("Updated {name} ({count})");
            }
            FeedEvent::Failed { id, message } => {
                let name = self
                    .feeds
                    .source(&id)
                    .map(|source| source.display_name())
                    .unwrap_or(id);
                self.feed_status = format!("{name}: {message}");
            }
        }
    }

    fn open_feed_item(&mut self, id: String, cx: &mut gpui_kit::Context<Self>) {
        self.open_item = Some(id);
        cx.notify();
    }

    fn chat_list_empty(&self) -> &'static str {
        if !self.ready {
            return "Waiting for the main chat list.";
        }
        if let Some(group_id) = self.group_id.as_deref() {
            let folders = self
                .local_groups
                .group(group_id)
                .map(|group| group.folders.as_slice())
                .unwrap_or(&[]);
            if folders.is_empty() {
                return "Attach a Telegram folder in the middle column.";
            }
            if !folders.iter().any(|folder| folder.matches(self.folder)) {
                return "Pick a folder in this group.";
            }
        }
        if self.folder.is_some() {
            "No chats in this folder."
        } else {
            "No chats in the local main list yet."
        }
    }

    fn shown_chats(&self) -> Vec<ChatItem> {
        if let Some(group_id) = self.group_id.as_deref() {
            let inside = self.local_groups.group(group_id).is_some_and(|group| {
                group
                    .folders
                    .iter()
                    .any(|folder| folder.matches(self.folder))
            });
            if !inside {
                return Vec::new();
            }
        }
        let mut chats = self.chats.clone();
        chats.sort_by(|left, right| {
            pin_sort_key(&self.pins, left.id).cmp(&pin_sort_key(&self.pins, right.id))
        });
        chats
    }

    fn toggle_pin(&mut self, cx: &mut gpui_kit::Context<Self>) {
        let Some(chat_id) = self.open_chat else {
            return;
        };
        self.pins.toggle(chat_id);
        if let Err(err) = self.pins.save() {
            eprintln!("warning: could not save local pins: {err}");
        }
        cx.notify();
    }

    /// Largest downloaded photo for the popup. The bubble keeps `message.photo`.
    fn local_full_photo(&self, message_id: i64) -> Option<FullPhoto> {
        let message = self
            .messages
            .iter()
            .find(|message| message.id == message_id)?;
        let path = message
            .photo_full
            .clone()
            .filter(|path| Path::new(path).is_file())?;
        let (width, height) = if message.photo_full_width > 0 && message.photo_full_height > 0 {
            (message.photo_full_width, message.photo_full_height)
        } else {
            image_pixel_size(&path).unwrap_or((0, 0))
        };
        Some(FullPhoto {
            path,
            width,
            height,
        })
    }

    /// Opens a separate OS window. `WindowKind::Floating` is a real toplevel
    /// on Linux: X11 sets `WM_TRANSIENT_FOR`, Wayland parents the xdg toplevel,
    /// and both keep a title bar. `WindowKind::PopUp` is override-redirect on
    /// X11 and has no title bar.
    fn open_photo(&mut self, message_id: i64, cx: &mut gpui_kit::Context<Self>) {
        let shell = cx.entity();
        let title = format!("Photo · {}", self.convo_title);
        let known = self.local_full_photo(message_id);
        cx.defer(move |app| {
            let (win_w, win_h) = known
                .as_ref()
                .filter(|photo| photo.width > 0 && photo.height > 0)
                .map(|photo| {
                    let (max_w, max_h) = display_cap(app);
                    let (width, height) =
                        fit_photo_logical(photo.width, photo.height, 1.0, max_w, max_h);
                    (width.max(320.0), height.max(240.0))
                })
                .unwrap_or((960.0, 640.0));
            let bounds = Bounds::centered(None, size(px(win_w), px(win_h)), app);
            if let Err(err) = app.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    titlebar: Some(TitlebarOptions {
                        title: Some(title.into()),
                        appears_transparent: false,
                        traffic_light_position: None,
                    }),
                    focus: true,
                    show: true,
                    kind: WindowKind::Floating,
                    app_id: Some("ad.neko.mithka.gpui".into()),
                    window_min_size: Some(size(px(320.), px(240.))),
                    ..Default::default()
                },
                |window, cx| {
                    let view = cx.new(|cx| PhotoWindow::new(shell, message_id, cx));
                    cx.new(|cx| Root::new(view, window, cx))
                },
            ) {
                eprintln!("warning: could not open photo window: {err}");
            }
        });
    }

    fn persist_groups(&self) {
        if let Err(err) = self.local_groups.save() {
            eprintln!("warning: could not save local groups: {err}");
        }
    }

    fn commit_group_draft(&mut self, window: &mut Window, cx: &mut gpui_kit::Context<Self>) {
        let name = self.group_draft.read(cx).value().trim().to_string();
        if name.is_empty() {
            return;
        }
        if self.renaming_group {
            if let Some(id) = self.group_id.clone() {
                self.local_groups.rename(&id, &name);
            }
            self.renaming_group = false;
        } else if let Some(id) = self.local_groups.create(&name) {
            self.show_feeds = false;
            self.group_id = Some(id);
        }
        self.persist_groups();
        self.group_draft.update(cx, |input, cx| {
            input.set_value("", window, cx);
        });
        cx.notify();
    }

    fn begin_rename(&mut self, window: &mut Window, cx: &mut gpui_kit::Context<Self>) {
        let Some(name) = self
            .group_id
            .as_deref()
            .and_then(|id| self.local_groups.group(id))
            .map(|group| group.name.clone())
        else {
            return;
        };
        self.renaming_group = true;
        self.group_draft.update(cx, |input, cx| {
            input.set_value(&name, window, cx);
        });
        self.group_draft.read(cx).focus_handle(cx).focus(window, cx);
        cx.notify();
    }

    fn delete_selected_group(&mut self, cx: &mut gpui_kit::Context<Self>) {
        let Some(id) = self.group_id.clone() else {
            return;
        };
        if self.local_groups.delete(&id) {
            self.group_id = None;
            self.renaming_group = false;
            self.persist_groups();
            cx.notify();
        }
    }

    /// Keep the virtual list aligned with [`Self::messages`].
    ///
    /// An older page is prepended in place so the viewport stays on the rows
    /// the reader was looking at. A new tail message is appended. A photo that
    /// finishes downloading keeps the same ids and only remeasures.
    fn sync_transcript(&mut self) {
        let new_ids: Vec<i64> = self.messages.iter().map(|message| message.id).collect();
        if new_ids == self.shown_ids {
            self.transcript.remeasure();
            self.loading_older = false;
            return;
        }
        let prepended = !self.shown_ids.is_empty()
            && new_ids.len() > self.shown_ids.len()
            && new_ids.ends_with(&self.shown_ids);
        let appended = !self.shown_ids.is_empty()
            && new_ids.len() > self.shown_ids.len()
            && new_ids.starts_with(&self.shown_ids);
        if prepended {
            let added = new_ids.len() - self.shown_ids.len();
            self.transcript.splice(0..0, added);
            self.loading_older = false;
        } else if appended {
            let start = self.shown_ids.len();
            let added = new_ids.len() - start;
            self.transcript.splice(start..start, added);
            if self.transcript.is_following_tail() {
                self.transcript.scroll_to_end();
            }
        } else {
            self.transcript.reset(new_ids.len());
            self.transcript.set_follow_mode(FollowMode::Tail);
            if !new_ids.is_empty() {
                self.transcript.scroll_to_end();
            }
            self.loading_older = false;
        }
        self.shown_ids = new_ids;
    }

    fn request_older(&mut self, cx: &mut gpui_kit::Context<Self>) {
        if self.loading_older || Instant::now() < self.history_scroll_after {
            return;
        }
        let Some(chat_id) = self.open_chat else {
            return;
        };
        if self.messages.is_empty() {
            return;
        }
        self.loading_older = true;
        self.client.send(ShellCommand::LoadOlder { chat_id });
        cx.notify();
    }

    fn open_chat(&mut self, chat_id: i64, window: &mut Window, cx: &mut gpui_kit::Context<Self>) {
        self.open_chat = Some(chat_id);
        self.convo_title = self
            .chats
            .iter()
            .chain(self.search_hits.iter())
            .find(|chat| chat.id == chat_id)
            .map(|chat| chat.title.clone())
            .filter(|title| !title.is_empty() && title != "…")
            .unwrap_or_else(|| "Loading…".into());
        self.messages.clear();
        self.loading_older = false;
        self.history_scroll_after = Instant::now() + Duration::from_millis(400);
        self.sync_transcript();
        self.composer.read(cx).focus_handle(cx).focus(window, cx);
        self.client.send(ShellCommand::SelectChat(chat_id));
        cx.notify();
    }

    fn send_composer(&mut self, window: &mut Window, cx: &mut gpui_kit::Context<Self>) {
        let Some(chat_id) = self.open_chat else {
            return;
        };
        if !self.ready {
            return;
        }
        let text = self.composer.read(cx).value().trim().to_string();
        if text.is_empty() {
            return;
        }
        self.composer.update(cx, |input, cx| {
            input.set_value("", window, cx);
        });
        self.transcript.set_follow_mode(FollowMode::Tail);
        self.transcript.scroll_to_end();
        self.client.send(ShellCommand::SendText { chat_id, text });
        cx.notify();
    }
}

impl Render for ShellView {
    fn render(
        &mut self,
        _window: &mut Window,
        cx: &mut gpui_kit::Context<Self>,
    ) -> impl IntoElement {
        let theme = cx.theme();
        let border = theme.border;
        let muted = theme.muted_foreground;
        let background = theme.background;
        let foreground = theme.foreground;
        let can_send = self.ready && self.open_chat.is_some();
        let middle = if self.show_feeds {
            self.sources_pane(cx, border, muted).into_any_element()
        } else {
            self.folders_pane(cx, border, muted).into_any_element()
        };
        let list = if self.show_feeds {
            self.items_pane(cx, border, muted).into_any_element()
        } else if self.show_contacts {
            self.contacts_pane(cx, border, muted).into_any_element()
        } else {
            self.chat_pane(cx, border, muted).into_any_element()
        };
        let detail = if self.show_feeds {
            self.article_pane(cx, border, muted).into_any_element()
        } else {
            self.conversation_pane(cx, can_send, border, muted)
                .into_any_element()
        };

        h_flex()
            .size_full()
            .items_stretch()
            .bg(background)
            .text_color(foreground)
            .text_size(px(14.))
            .child(self.groups_pane(cx, border, muted))
            .child(middle)
            .child(list)
            .child(detail)
    }
}

struct FullPhoto {
    path: String,
    width: i32,
    height: i32,
}

struct PhotoWindow {
    path: Option<String>,
    width: i32,
    height: i32,
    focus: FocusHandle,
    focused: bool,
    sized: bool,
    _updates: Subscription,
}

impl PhotoWindow {
    fn new(shell: Entity<ShellView>, message_id: i64, cx: &mut gpui_kit::Context<Self>) -> Self {
        let full = shell.read(cx).local_full_photo(message_id);
        let updates = cx.observe(&shell, move |this, shell, cx| {
            let Some(full) = shell.read(cx).local_full_photo(message_id) else {
                return;
            };
            if this.path.as_deref() == Some(full.path.as_str())
                && this.width == full.width
                && this.height == full.height
            {
                return;
            }
            this.path = Some(full.path);
            this.width = full.width;
            this.height = full.height;
            this.sized = false;
            cx.notify();
        });
        Self {
            path: full.as_ref().map(|photo| photo.path.clone()),
            width: full.as_ref().map(|photo| photo.width).unwrap_or(0),
            height: full.as_ref().map(|photo| photo.height).unwrap_or(0),
            focus: cx.focus_handle(),
            focused: false,
            sized: false,
            _updates: updates,
        }
    }

    fn ensure_pixels(&mut self) {
        if self.width > 0 && self.height > 0 {
            return;
        }
        let Some(path) = self.path.as_deref() else {
            return;
        };
        let Some((width, height)) = image_pixel_size(path) else {
            return;
        };
        self.width = width;
        self.height = height;
        self.sized = false;
    }
}

impl Render for PhotoWindow {
    fn render(
        &mut self,
        window: &mut Window,
        cx: &mut gpui_kit::Context<Self>,
    ) -> impl IntoElement {
        if !self.focused {
            self.focused = true;
            self.focus.focus(window, cx);
        }
        self.ensure_pixels();
        let ink = gpui_kit::hsla(0.0, 0.0, 0.96, 1.0);
        let body = if let Some(path) = self.path.clone() {
            if self.width > 0 && self.height > 0 {
                let scale = window.scale_factor();
                let (max_w, max_h) = window_display_cap(window, cx);
                let (img_w, img_h) =
                    fit_photo_logical(self.width, self.height, scale, max_w, max_h);
                if !self.sized {
                    window.resize(size(px(img_w.max(320.0)), px(img_h.max(240.0))));
                    self.sized = true;
                }
                img(PathBuf::from(path))
                    .w(px(img_w))
                    .h(px(img_h))
                    .flex_shrink_0()
                    .object_fit(ObjectFit::Contain)
                    .with_fallback(|| div().child("Photo").into_any_element())
                    .into_any_element()
            } else {
                img(PathBuf::from(path))
                    .size_full()
                    .object_fit(ObjectFit::ScaleDown)
                    .with_fallback(|| div().child("Photo").into_any_element())
                    .into_any_element()
            }
        } else {
            div()
                .text_color(ink)
                .child("Downloading photo…")
                .into_any_element()
        };
        div()
            .id("photo-window")
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(gpui_kit::hsla(0.0, 0.0, 0.08, 1.0))
            .text_color(ink)
            .track_focus(&self.focus)
            .on_key_down(|event: &KeyDownEvent, window, _app| {
                if event.keystroke.key == "escape" {
                    window.remove_window();
                }
            })
            .child(body)
    }
}

impl ShellView {
    fn sources_pane(
        &self,
        cx: &mut gpui_kit::Context<Self>,
        border: gpui_kit::Hsla,
        muted: gpui_kit::Hsla,
    ) -> impl IntoElement {
        let fill = cx.theme().secondary;
        let glyph = cx.theme().foreground;
        let mut rows = vec![self.side_row(
            "feed-all",
            "All feeds",
            Icon::Rss,
            self.feed_source.is_none(),
            fill,
            glyph,
            cx.listener(|this, _: &ClickEvent, _window, cx| {
                this.select_feed_source(None, cx);
            }),
        )];
        for source in self.feeds.sources() {
            let id = source.id.clone();
            rows.push(self.side_row(
                format!("feed-{}", source.id),
                source.display_name(),
                Icon::Rss,
                self.feed_source.as_deref() == Some(source.id.as_str()),
                fill,
                glyph,
                cx.listener(move |this, _: &ClickEvent, _window, cx| {
                    this.select_feed_source(Some(id.clone()), cx);
                }),
            ));
        }
        if self.feeds.sources().is_empty() {
            rows.push(
                div()
                    .px_3()
                    .py_2()
                    .text_size(px(12.))
                    .text_color(muted)
                    .child("Paste an http(s) feed URL below.")
                    .into_any_element(),
            );
        }

        v_flex()
            .w(px(200.))
            .h_full()
            .flex_shrink_0()
            .border_r_1()
            .border_color(border)
            .child(column_title("Feeds", muted))
            .child(
                v_flex()
                    .id("feed-sources")
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .overflow_y_scrollbar()
                    .children(rows),
            )
            .child(
                v_flex()
                    .w_full()
                    .gap_2()
                    .p_2()
                    .border_t_1()
                    .border_color(border)
                    .child(Input::new(&self.feed_draft))
                    .child(
                        h_flex()
                            .w_full()
                            .gap_1()
                            .child(with_icon(
                                Icon::Plus,
                                glyph,
                                Button::new("add-feed")
                                    .primary()
                                    .small()
                                    .label("Add")
                                    .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                        this.commit_feed(window, cx);
                                    })),
                            ))
                            .child(with_icon(
                                Icon::ArrowPath,
                                glyph,
                                Button::new("refresh-feeds")
                                    .ghost()
                                    .small()
                                    .label("Refresh")
                                    .on_click(cx.listener(|this, _: &ClickEvent, _window, cx| {
                                        this.refresh_feeds(cx);
                                    })),
                            )),
                    )
                    .when(self.feed_source.is_some(), |col| {
                        col.child(with_icon(
                            Icon::Trash,
                            glyph,
                            Button::new("remove-feed")
                                .ghost()
                                .small()
                                .label("Remove")
                                .on_click(cx.listener(|this, _: &ClickEvent, _window, cx| {
                                    this.remove_selected_feed(cx);
                                })),
                        ))
                    }),
            )
    }

    fn items_pane(
        &self,
        cx: &mut gpui_kit::Context<Self>,
        border: gpui_kit::Hsla,
        muted: gpui_kit::Hsla,
    ) -> impl IntoElement {
        let shown = self.shown_items();
        let rows = if shown.is_empty() {
            let empty = if self.feeds.sources().is_empty() {
                "Add a feed URL to start the timeline."
            } else if self.refreshing > 0 {
                "Fetching items…"
            } else if self.feed_source.is_some() {
                "No items from this feed yet."
            } else {
                "No items yet. Refresh to fetch."
            };
            vec![div()
                .p_3()
                .text_color(muted)
                .child(empty.to_string())
                .into_any_element()]
        } else {
            shown.iter().map(|item| self.item_row(item, cx)).collect()
        };
        let status = if self.feed_status.is_empty() {
            format!("{} items", shown.len())
        } else {
            self.feed_status.clone()
        };

        v_flex()
            .w(px(280.))
            .h_full()
            .flex_shrink_0()
            .border_r_1()
            .border_color(border)
            .child(
                v_flex()
                    .px_3()
                    .py_3()
                    .gap_1()
                    .child(div().text_size(px(16.)).child("Feed"))
                    .child(div().text_size(px(12.)).text_color(muted).child(status))
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(muted)
                            .child("Chat search is off in Subscriptions."),
                    ),
            )
            .child(
                v_flex()
                    .id("feed-items")
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .overflow_y_scrollbar()
                    .children(rows),
            )
    }

    fn item_row(
        &self,
        item: &mithka_rss::Item,
        cx: &mut gpui_kit::Context<Self>,
    ) -> gpui_kit::AnyElement {
        let id = item.id.clone();
        let selected = self.open_item.as_deref() == Some(item.id.as_str());
        let source = self
            .feeds
            .source(&item.source_id)
            .map(|source| source.display_name())
            .unwrap_or_else(|| "Feed".into());
        let hover = cx.theme().secondary;
        let muted = cx.theme().muted_foreground;
        let glyph = cx.theme().foreground;
        div()
            .id(gpui_kit::SharedString::from(format!("item-{}", item.id)))
            .w_full()
            .flex()
            .items_start()
            .gap_2()
            .px_3()
            .py_2()
            .min_h(px(56.))
            .cursor_pointer()
            .hover(move |style| style.bg(hover))
            .when(selected, move |style| style.bg(hover))
            .on_click(cx.listener(move |this, _: &ClickEvent, _window, cx| {
                this.open_feed_item(id.clone(), cx);
            }))
            .child(hero(Icon::Rss, glyph))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_0p5()
                    .child(
                        h_flex()
                            .w_full()
                            .min_w_0()
                            .gap_2()
                            .items_center()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_ellipsis()
                                    .child(item.title.clone()),
                            )
                            .when(item.published > 0, |row| {
                                row.child(
                                    div()
                                        .flex_shrink_0()
                                        .text_size(px(12.))
                                        .text_color(muted)
                                        .child(list_time(item.published)),
                                )
                            }),
                    )
                    .child(
                        div()
                            .w_full()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .text_size(px(12.))
                            .text_color(muted)
                            .child(source),
                    ),
            )
            .into_any_element()
    }

    fn article_pane(
        &self,
        _cx: &mut gpui_kit::Context<Self>,
        border: gpui_kit::Hsla,
        muted: gpui_kit::Hsla,
    ) -> impl IntoElement {
        let item = self
            .open_item
            .as_deref()
            .and_then(|id| self.feeds.item(id))
            .cloned();
        let body = if let Some(item) = item {
            let source = self
                .feeds
                .source(&item.source_id)
                .map(|source| source.display_name())
                .unwrap_or_else(|| "Feed".into());
            let when = if item.published > 0 {
                message_time(item.published)
            } else {
                String::new()
            };
            let meta = if when.is_empty() {
                source.clone()
            } else {
                format!("{source} · {when}")
            };
            let paragraphs = article_paragraphs(&item.body);
            let link = if is_http_url(&item.link) {
                Some(item.link.clone())
            } else {
                None
            };
            v_flex()
                .flex_1()
                .min_h_0()
                .w_full()
                .child(
                    v_flex()
                        .w_full()
                        .px_4()
                        .py_3()
                        .gap_1()
                        .border_b_1()
                        .border_color(border)
                        .child(div().text_size(px(16.)).child(item.title.clone()))
                        .child(div().text_size(px(12.)).text_color(muted).child(meta))
                        .when_some(link, |col, url| {
                            col.child(Link::new("feed-item-link").href(url.clone()).child(url))
                        }),
                )
                .child(
                    v_flex()
                        .id("feed-article")
                        .flex_1()
                        .min_h_0()
                        .w_full()
                        .px_4()
                        .py_3()
                        .gap_3()
                        .overflow_y_scrollbar()
                        .children(if paragraphs.is_empty() {
                            vec![div()
                                .text_color(muted)
                                .child("This item has no summary.")
                                .into_any_element()]
                        } else {
                            paragraphs
                                .into_iter()
                                .map(|paragraph| {
                                    div()
                                        .w_full()
                                        .whitespace_normal()
                                        .child(paragraph)
                                        .into_any_element()
                                })
                                .collect()
                        }),
                )
                .into_any_element()
        } else {
            div()
                .flex_1()
                .p_4()
                .text_color(muted)
                .child("Open an item to read it. The text stays on this machine.")
                .into_any_element()
        };

        v_flex().flex_1().h_full().min_w_0().child(body)
    }

    fn chat_pane(
        &self,
        cx: &mut gpui_kit::Context<Self>,
        border: gpui_kit::Hsla,
        muted: gpui_kit::Hsla,
    ) -> impl IntoElement {
        let (local, extra) = self.search_rows(cx);
        let querying = !self.search.read(cx).value().trim().is_empty();
        let mut rows = Vec::new();
        if local.is_empty() && extra.is_empty() {
            let empty = if querying {
                "No chats match this search."
            } else {
                self.chat_list_empty()
            };
            rows.push(
                div()
                    .p_3()
                    .text_color(muted)
                    .child(empty.to_string())
                    .into_any_element(),
            );
        } else {
            rows.extend(local.iter().map(|chat| self.chat_row(chat, cx)));
            if !extra.is_empty() {
                rows.push(
                    div()
                        .px_3()
                        .pt_3()
                        .pb_1()
                        .text_size(px(12.))
                        .text_color(muted)
                        .child("Also found")
                        .into_any_element(),
                );
                rows.extend(extra.iter().map(|chat| self.chat_row(chat, cx)));
            }
        }

        v_flex()
            .w(px(280.))
            .h_full()
            .flex_shrink_0()
            .border_r_1()
            .border_color(border)
            .child(
                v_flex()
                    .px_3()
                    .py_3()
                    .gap_2()
                    .child(div().text_size(px(16.)).child("Chats"))
                    .child(
                        h_flex()
                            .w_full()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .id("focus-search")
                                    .cursor_pointer()
                                    .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                        this.search.read(cx).focus_handle(cx).focus(window, cx);
                                    }))
                                    .child(hero(Icon::MagnifyingGlass, muted)),
                            )
                            .child(div().flex_1().min_w_0().child(Input::new(&self.search))),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(muted)
                            .child(self.status.clone()),
                    ),
            )
            .child(
                v_flex()
                    .id("chat-list")
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .overflow_y_scrollbar()
                    .children(rows),
            )
    }

    fn contacts_pane(
        &self,
        cx: &mut gpui_kit::Context<Self>,
        border: gpui_kit::Hsla,
        muted: gpui_kit::Hsla,
    ) -> impl IntoElement {
        let rows = if self.contacts.is_empty() {
            let empty = if self.ready {
                "No contacts yet."
            } else {
                "Waiting for TDLib before loading contacts."
            };
            vec![div()
                .p_3()
                .text_color(muted)
                .child(empty.to_string())
                .into_any_element()]
        } else {
            self.contacts
                .iter()
                .map(|contact| self.contact_row(contact, cx))
                .collect()
        };

        v_flex()
            .w(px(280.))
            .h_full()
            .flex_shrink_0()
            .border_r_1()
            .border_color(border)
            .child(
                v_flex()
                    .px_3()
                    .py_3()
                    .gap_1()
                    .child(div().text_size(px(16.)).child("Contacts"))
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(muted)
                            .child(self.status.clone()),
                    ),
            )
            .child(
                v_flex()
                    .id("contact-list")
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .overflow_y_scrollbar()
                    .children(rows),
            )
    }

    fn contact_row(
        &self,
        contact: &ContactItem,
        cx: &mut gpui_kit::Context<Self>,
    ) -> gpui_kit::AnyElement {
        let user_id = contact.user_id;
        let hover = cx.theme().secondary;
        let muted = cx.theme().muted_foreground;
        let username = if contact.username.is_empty() {
            String::new()
        } else {
            format!("@{}", contact.username)
        };
        div()
            .id(gpui_kit::SharedString::from(format!("contact-{user_id}")))
            .w_full()
            .flex()
            .items_center()
            .gap_2()
            .px_3()
            .py_2()
            .min_h(px(56.))
            .cursor_pointer()
            .hover(move |style| style.bg(hover))
            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                this.open_contact(user_id, window, cx);
            }))
            .child(named_avatar(&contact.name, contact.avatar.as_deref()))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_0p5()
                    .child(
                        div()
                            .w_full()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .child(contact.name.clone()),
                    )
                    .when(!username.is_empty(), |col| {
                        col.child(
                            div()
                                .w_full()
                                .min_w_0()
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .text_ellipsis()
                                .text_size(px(12.))
                                .text_color(muted)
                                .child(username),
                        )
                    }),
            )
            .into_any_element()
    }

    fn groups_pane(
        &self,
        cx: &mut gpui_kit::Context<Self>,
        border: gpui_kit::Hsla,
        muted: gpui_kit::Hsla,
    ) -> impl IntoElement {
        let fill = cx.theme().secondary;
        let glyph = cx.theme().foreground;
        let mut rows = vec![
            self.side_row(
                "group-all",
                "All chats",
                Icon::ChatBubble,
                !self.show_feeds && !self.show_contacts && self.group_id.is_none(),
                fill,
                glyph,
                cx.listener(|this, _: &ClickEvent, _window, cx| {
                    this.select_group(None, cx);
                }),
            ),
            self.side_row(
                "group-contacts",
                "Contacts",
                Icon::UserGroup,
                self.show_contacts,
                fill,
                glyph,
                cx.listener(|this, _: &ClickEvent, _window, cx| {
                    this.select_contacts(cx);
                }),
            ),
            self.side_row(
                "group-subscriptions",
                "Subscriptions",
                Icon::Rss,
                self.show_feeds,
                fill,
                glyph,
                cx.listener(|this, _: &ClickEvent, _window, cx| {
                    this.select_subscriptions(cx);
                }),
            ),
        ];
        for group in self.local_groups.groups() {
            let id = group.id.clone();
            let selected = !self.show_feeds
                && !self.show_contacts
                && self.group_id.as_deref() == Some(group.id.as_str());
            rows.push(self.side_row(
                format!("group-{}", group.id),
                group.name.clone(),
                Icon::Squares,
                selected,
                fill,
                glyph,
                cx.listener(move |this, _: &ClickEvent, _window, cx| {
                    this.select_group(Some(id.clone()), cx);
                }),
            ));
        }
        if !self.show_feeds && self.local_groups.groups().is_empty() {
            rows.push(
                div()
                    .px_3()
                    .py_2()
                    .text_size(px(12.))
                    .text_color(muted)
                    .child("Name a group below, then attach Telegram folders to it.")
                    .into_any_element(),
            );
        }

        let action_label = if self.renaming_group { "Save" } else { "Add" };
        let selected = self.group_id.is_some();
        v_flex()
            .w(px(188.))
            .h_full()
            .flex_shrink_0()
            .border_r_1()
            .border_color(border)
            .child(column_title("Groups", muted))
            .child(
                v_flex()
                    .id("local-groups")
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .overflow_y_scrollbar()
                    .children(rows),
            )
            .child(
                v_flex()
                    .w_full()
                    .gap_2()
                    .p_2()
                    .border_t_1()
                    .border_color(border)
                    .when(selected, |col| {
                        col.child(
                            h_flex()
                                .w_full()
                                .gap_1()
                                .child(with_icon(
                                    Icon::Pencil,
                                    glyph,
                                    Button::new("rename-group")
                                        .ghost()
                                        .small()
                                        .label("Rename")
                                        .on_click(cx.listener(
                                            |this, _: &ClickEvent, window, cx| {
                                                this.begin_rename(window, cx);
                                            },
                                        )),
                                ))
                                .child(with_icon(
                                    Icon::Trash,
                                    glyph,
                                    Button::new("delete-group")
                                        .ghost()
                                        .small()
                                        .label("Delete")
                                        .on_click(cx.listener(
                                            |this, _: &ClickEvent, _window, cx| {
                                                this.delete_selected_group(cx);
                                            },
                                        )),
                                )),
                        )
                    })
                    .child(Input::new(&self.group_draft))
                    .child(with_icon(
                        Icon::Plus,
                        glyph,
                        Button::new("save-group")
                            .primary()
                            .small()
                            .label(action_label)
                            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                this.commit_group_draft(window, cx);
                            })),
                    )),
            )
    }

    fn folders_pane(
        &self,
        cx: &mut gpui_kit::Context<Self>,
        border: gpui_kit::Hsla,
        muted: gpui_kit::Hsla,
    ) -> impl IntoElement {
        let fill = cx.theme().secondary;
        let glyph = cx.theme().foreground;
        let nested = self
            .group_id
            .as_deref()
            .and_then(|id| self.local_groups.group(id))
            .map(|group| group.folders.clone())
            .unwrap_or_default();
        let in_group = self.group_id.is_some();
        let mut rows = Vec::new();
        if in_group {
            if nested.is_empty() {
                rows.push(
                    div()
                        .px_3()
                        .py_2()
                        .text_size(px(12.))
                        .text_color(muted)
                        .child("No folders in this group yet.")
                        .into_any_element(),
                );
            }
            for folder in &nested {
                rows.push(self.folder_row(*folder, true, fill, cx));
            }
            let available: Vec<NestedFolder> = self
                .known_folders()
                .into_iter()
                .filter(|folder| !nested.contains(folder))
                .collect();
            if !available.is_empty() {
                rows.push(
                    div()
                        .px_3()
                        .pt_3()
                        .pb_1()
                        .text_size(px(12.))
                        .text_color(muted)
                        .child("Attach")
                        .into_any_element(),
                );
                for folder in available {
                    rows.push(self.folder_row(folder, false, fill, cx));
                }
            }
        } else {
            for folder in self.known_folders() {
                rows.push(self.folder_row(folder, true, fill, cx));
            }
        }
        let can_remove = in_group && nested.iter().any(|folder| folder.matches(self.folder));

        v_flex()
            .w(px(168.))
            .h_full()
            .flex_shrink_0()
            .border_r_1()
            .border_color(border)
            .child(column_title(
                if in_group { "In group" } else { "Folders" },
                muted,
            ))
            .child(
                v_flex()
                    .id("telegram-folders")
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .overflow_y_scrollbar()
                    .children(rows),
            )
            .when(can_remove, |col| {
                col.child(
                    div()
                        .w_full()
                        .p_2()
                        .border_t_1()
                        .border_color(border)
                        .child(with_icon(
                            Icon::Trash,
                            glyph,
                            Button::new("remove-nested-folder")
                                .ghost()
                                .small()
                                .label("Remove")
                                .on_click(cx.listener(|this, _: &ClickEvent, _window, cx| {
                                    let Some(folder) = this.group_id.as_deref().and_then(|id| {
                                        this.local_groups.group(id).and_then(|group| {
                                            group
                                                .folders
                                                .iter()
                                                .copied()
                                                .find(|folder| folder.matches(this.folder))
                                        })
                                    }) else {
                                        return;
                                    };
                                    this.toggle_nested(folder, cx);
                                })),
                        )),
                )
            })
    }

    fn folder_row(
        &self,
        folder: NestedFolder,
        nested: bool,
        fill: gpui_kit::Hsla,
        cx: &mut gpui_kit::Context<Self>,
    ) -> gpui_kit::AnyElement {
        let label = self.folder_label(folder);
        let icon = match folder {
            NestedFolder::Main => Icon::Inbox,
            NestedFolder::Folder { .. } => Icon::Folder,
        };
        let selected = nested && folder.matches(self.folder);
        let key = match folder {
            NestedFolder::Main => "main".to_string(),
            NestedFolder::Folder { id } => id.to_string(),
        };
        let id = if nested {
            format!("folder-{key}")
        } else {
            format!("attach-{key}")
        };
        let glyph = cx.theme().foreground;
        self.side_row(
            id,
            label,
            icon,
            selected,
            fill,
            glyph,
            cx.listener(move |this, _: &ClickEvent, _window, cx| {
                if nested {
                    this.select_folder(folder.telegram_id(), cx);
                } else {
                    this.toggle_nested(folder, cx);
                }
            }),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn side_row(
        &self,
        id: impl Into<gpui_kit::ElementId>,
        label: impl Into<String>,
        icon: Icon,
        selected: bool,
        fill: gpui_kit::Hsla,
        glyph: gpui_kit::Hsla,
        on_click: impl Fn(&ClickEvent, &mut Window, &mut gpui_kit::App) + 'static,
    ) -> gpui_kit::AnyElement {
        let label = label.into();
        let selected_fill = fill;
        let hover_fill = fill.opacity(0.55);
        div()
            .id(id.into())
            .w_full()
            .flex()
            .items_center()
            .gap_2()
            .px_2()
            .py_2()
            .min_h(px(40.))
            .cursor_pointer()
            .when(selected, move |style| style.bg(selected_fill))
            .hover(move |style| style.bg(hover_fill))
            .on_click(on_click)
            .child(hero(icon, glyph))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .child(label),
            )
            .into_any_element()
    }

    fn chat_row(&self, chat: &ChatItem, cx: &mut gpui_kit::Context<Self>) -> gpui_kit::AnyElement {
        let chat_id = chat.id;
        let selected = self.open_chat == Some(chat_id);
        let title = if chat.title.is_empty() {
            "Untitled".to_string()
        } else {
            chat.title.clone()
        };
        let hover = cx.theme().secondary;
        let muted = cx.theme().muted_foreground;
        div()
            .id(gpui_kit::SharedString::from(format!("chat-{chat_id}")))
            .w_full()
            .flex()
            .items_center()
            .gap_2()
            .px_3()
            .py_2()
            .min_h(px(56.))
            .cursor_pointer()
            .hover(move |style| style.bg(hover))
            .when(selected, move |style| style.bg(hover))
            .on_click(cx.listener(move |this, _: &ClickEvent, window, cx| {
                this.open_chat(chat_id, window, cx);
            }))
            .child(avatar_with_badge(chat))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_0p5()
                    .child(
                        h_flex()
                            .w_full()
                            .min_w_0()
                            .gap_2()
                            .items_center()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_ellipsis()
                                    .child(title),
                            )
                            .when(self.pins.is_pinned(chat_id), |row| {
                                row.child(hero(Icon::MapPin, muted)).child(
                                    div()
                                        .flex_shrink_0()
                                        .text_size(px(12.))
                                        .text_color(muted)
                                        .child("Pinned"),
                                )
                            })
                            .when(chat.preview_date > 0, |row| {
                                row.child(
                                    div()
                                        .flex_shrink_0()
                                        .text_size(px(12.))
                                        .text_color(muted)
                                        .child(list_time(chat.preview_date)),
                                )
                            }),
                    )
                    .when(!chat.preview.is_empty(), |col| {
                        col.child(
                            div()
                                .w_full()
                                .min_w_0()
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .text_ellipsis()
                                .text_size(px(12.))
                                .text_color(muted)
                                .child(chat.preview.clone()),
                        )
                    }),
            )
            .into_any_element()
    }

    fn conversation_pane(
        &self,
        cx: &mut gpui_kit::Context<Self>,
        can_send: bool,
        border: gpui_kit::Hsla,
        muted: gpui_kit::Hsla,
    ) -> impl IntoElement {
        let transcript = if self.messages.is_empty() {
            let empty = if self.open_chat.is_some() {
                "No messages in the latest history."
            } else if self.ready {
                "Open a chat to read recent messages."
            } else {
                "Chat titles show up here after TDLib reports Ready."
            };
            div()
                .flex_1()
                .p_4()
                .text_color(muted)
                .child(empty.to_string())
                .into_any_element()
        } else {
            let rows = self.messages.clone();
            let transcript = self.transcript.clone();
            let view = cx.entity();
            let receipt_color = muted;
            div()
                .id("messages")
                .flex_1()
                .min_h_0()
                .w_full()
                .child(
                    list(transcript.clone(), move |index, _window, _app| {
                        let Some(message) = rows.get(index) else {
                            return Message::new().into_any_element();
                        };
                        let on_photo = (message.kind == MessageKind::Photo).then(|| {
                            let id = message.id;
                            let view = view.clone();
                            Box::new(
                                move |_event: &ClickEvent,
                                      window: &mut Window,
                                      app: &mut gpui_kit::App| {
                                    let _ = window;
                                    view.update(app, |this, cx| {
                                        this.open_photo(id, cx);
                                    });
                                },
                            ) as PhotoClick
                        });
                        message_row(message, receipt_color, on_photo).into_any_element()
                    })
                    .size_full()
                    .px_3()
                    .py_2(),
                )
                .vertical_scrollbar(&transcript)
                .into_any_element()
        };

        v_flex()
            .flex_1()
            .h_full()
            .min_w_0()
            .child(
                v_flex()
                    .w_full()
                    .px_4()
                    .py_3()
                    .gap_2()
                    .border_b_1()
                    .border_color(border)
                    .child(
                        h_flex()
                            .w_full()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_size(px(16.))
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_ellipsis()
                                    .child(self.convo_title.clone()),
                            )
                            .when(self.open_chat.is_some(), |row| {
                                let pinned =
                                    self.open_chat.is_some_and(|id| self.pins.is_pinned(id));
                                let label = if pinned { "Unpin" } else { "Pin" };
                                row.child(with_icon(
                                    Icon::MapPin,
                                    muted,
                                    Button::new("pin-chat")
                                        .ghost()
                                        .small()
                                        .label(label)
                                        .on_click(cx.listener(
                                            |this, _: &ClickEvent, _window, cx| {
                                                this.toggle_pin(cx);
                                            },
                                        )),
                                ))
                            }),
                    ),
            )
            .child(transcript)
            .child(
                h_flex()
                    .w_full()
                    .gap_2()
                    .p_3()
                    .border_t_1()
                    .border_color(border)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(Input::new(&self.composer).disabled(!can_send)),
                    )
                    .child(with_icon(
                        Icon::PaperAirplane,
                        muted,
                        Button::new("send")
                            .primary()
                            .label("Send")
                            .disabled(!can_send)
                            .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                                this.send_composer(window, cx);
                            })),
                    )),
            )
    }
}

fn article_paragraphs(body: &str) -> Vec<String> {
    body.split("\n\n")
        .map(str::trim)
        .filter(|paragraph| !paragraph.is_empty())
        .map(str::to_string)
        .collect()
}

fn column_title(title: &str, muted: gpui_kit::Hsla) -> impl IntoElement {
    v_flex().px_3().py_3().gap_1().child(
        div()
            .text_size(px(16.))
            .text_color(muted)
            .child(title.to_string()),
    )
}

fn named_avatar(name: &str, path: Option<&str>) -> Avatar {
    let title = if name.is_empty() {
        "Untitled".to_string()
    } else {
        name.to_string()
    };
    let mut avatar = Avatar::new().name(title).with_size(Size::Size(px(36.)));
    if let Some(path) = path.filter(|path| Path::new(path).is_file()) {
        avatar = avatar.src(PathBuf::from(path));
    }
    avatar
}

fn avatar_for(chat: &ChatItem) -> Avatar {
    named_avatar(&chat.title, chat.avatar.as_deref())
}

fn avatar_with_badge(chat: &ChatItem) -> gpui_kit::AnyElement {
    let avatar = avatar_for(chat);
    let badge = Badge::new().with_size(Size::Large).color(rgb(0x2ea043));
    if chat.unread > 0 {
        badge
            .count(usize::try_from(chat.unread).unwrap_or(usize::MAX))
            .max(99)
            .child(avatar)
            .into_any_element()
    } else if chat.marked_unread {
        badge.dot().child(avatar).into_any_element()
    } else {
        avatar.into_any_element()
    }
}

type PhotoClick = Box<dyn Fn(&ClickEvent, &mut Window, &mut gpui_kit::App) + 'static>;

fn message_row(
    message: &TextMessage,
    icon_color: gpui_kit::Hsla,
    on_photo: Option<PhotoClick>,
) -> Message {
    let outgoing = message.outgoing;
    let alignment = if outgoing {
        MessageAlignment::End
    } else {
        MessageAlignment::Start
    };
    let variant = if outgoing {
        BubbleVariant::Filled
    } else {
        BubbleVariant::Secondary
    };
    let mut row = Message::new().alignment(alignment);
    let header = message_header(message);
    if !header.is_empty() {
        row = row.header(MessageHeader::new().child(header));
    }
    row.content(
        MessageContent::new().bubble(message_bubble(message, variant, icon_color, on_photo)),
    )
}

fn message_header(message: &TextMessage) -> String {
    let time = if message.date > 0 {
        message_time(message.date)
    } else {
        String::new()
    };
    match (message.sender.is_empty(), time.is_empty()) {
        (true, true) => String::new(),
        (false, true) => message.sender.clone(),
        (true, false) => time,
        (false, false) => format!("{} · {time}", message.sender),
    }
}

fn message_bubble(
    message: &TextMessage,
    variant: BubbleVariant,
    icon_color: gpui_kit::Hsla,
    on_photo: Option<PhotoClick>,
) -> Bubble {
    let mut bubble = Bubble::new().with_variant(variant);
    if message.kind == MessageKind::Photo {
        bubble = bubble.child(photo_element(message, icon_color, on_photo));
    }
    if !message.text.is_empty() {
        bubble = bubble.child(message_text(message));
    }
    if message.outgoing {
        bubble = bubble.child(receipt_line(message, icon_color));
    }
    bubble
}

fn receipt_line(message: &TextMessage, icon_color: gpui_kit::Hsla) -> gpui_kit::AnyElement {
    let label = if message.read { "Read" } else { "Sent" };
    h_flex()
        .gap_1()
        .items_center()
        .text_size(px(11.))
        .text_color(icon_color)
        .when(message.read, |row| row.child(hero(Icon::Check, icon_color)))
        .child(label)
        .into_any_element()
}

fn photo_element(
    message: &TextMessage,
    icon_color: gpui_kit::Hsla,
    on_photo: Option<PhotoClick>,
) -> gpui_kit::AnyElement {
    let body = if let Some(path) = message
        .photo
        .as_deref()
        .filter(|path| Path::new(path).is_file())
    {
        img(PathBuf::from(path))
            .w(px(280.))
            .h(px(180.))
            .object_fit(ObjectFit::Contain)
            .with_fallback(|| div().child("Photo").into_any_element())
            .into_any_element()
    } else {
        h_flex()
            .gap_1()
            .items_center()
            .child(hero(Icon::Photo, icon_color))
            .child("Photo")
            .into_any_element()
    };
    let Some(on_photo) = on_photo else {
        return body;
    };
    div()
        .id(gpui_kit::SharedString::from(format!(
            "photo-{}",
            message.id
        )))
        .cursor_pointer()
        .on_click(on_photo)
        .child(body)
        .into_any_element()
}

fn with_icon(icon: Icon, color: gpui_kit::Hsla, control: impl IntoElement) -> gpui_kit::AnyElement {
    h_flex()
        .gap_1()
        .items_center()
        .child(hero(icon, color))
        .child(control)
        .into_any_element()
}

fn message_text(message: &TextMessage) -> gpui_kit::AnyElement {
    let spans = http_spans(&message.text, &message.links);
    let linked = spans
        .iter()
        .any(|span| matches!(span, TextSpan::Link { .. }));
    if !linked {
        return div()
            .max_w(px(420.))
            .whitespace_normal()
            .child(message.text.clone())
            .into_any_element();
    }
    let mut children = Vec::new();
    for (index, span) in spans.into_iter().enumerate() {
        match span {
            TextSpan::Text(text) => push_plain_text(&mut children, &text),
            TextSpan::Link { label, url } => {
                children.push(
                    Link::new(gpui_kit::SharedString::from(format!(
                        "link-{}-{index}",
                        message.id
                    )))
                    .href(url)
                    .child(label)
                    .into_any_element(),
                );
            }
        }
    }
    div()
        .max_w(px(420.))
        .flex()
        .flex_wrap()
        .items_center()
        .children(children)
        .into_any_element()
}

enum TextSpan {
    Text(String),
    Link { label: String, url: String },
}

fn http_spans(text: &str, links: &[TextLink]) -> Vec<TextSpan> {
    let mut spans: Vec<&TextLink> = links
        .iter()
        .filter(|link| {
            link.start < link.end
                && link.end <= text.len()
                && text.is_char_boundary(link.start)
                && text.is_char_boundary(link.end)
                && is_http_url(&link.url)
        })
        .collect();
    spans.sort_by_key(|link| link.start);
    let mut out = Vec::new();
    let mut cursor = 0usize;
    for link in spans {
        if link.start < cursor {
            continue;
        }
        if cursor < link.start {
            out.push(TextSpan::Text(text[cursor..link.start].to_string()));
        }
        out.push(TextSpan::Link {
            label: text[link.start..link.end].to_string(),
            url: link.url.clone(),
        });
        cursor = link.end;
    }
    if cursor < text.len() {
        out.push(TextSpan::Text(text[cursor..].to_string()));
    }
    out
}

fn push_plain_text(children: &mut Vec<gpui_kit::AnyElement>, text: &str) {
    for (line_index, line) in text.split('\n').enumerate() {
        if line_index > 0 {
            children.push(div().w_full().h(px(0.)).into_any_element());
        }
        let mut word = String::new();
        for ch in line.chars() {
            if ch.is_whitespace() {
                if !word.is_empty() {
                    children.push(div().child(std::mem::take(&mut word)).into_any_element());
                }
                children.push(div().child(ch.to_string()).into_any_element());
            } else {
                word.push(ch);
            }
        }
        if !word.is_empty() {
            children.push(div().child(word).into_any_element());
        }
    }
}

/// Logical size of a photo so one image pixel lands on one device pixel.
/// `scale` is the window scale factor. The result never grows the bitmap,
/// and it shrinks only to stay inside `max_w` × `max_h`.
fn fit_photo_logical(width: i32, height: i32, scale: f32, max_w: f32, max_h: f32) -> (f32, f32) {
    let scale = if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    };
    let mut fitted_w = (width.max(1) as f32) / scale;
    let mut fitted_h = (height.max(1) as f32) / scale;
    let max_w = if max_w.is_finite() && max_w > 1.0 {
        max_w
    } else {
        fitted_w
    };
    let max_h = if max_h.is_finite() && max_h > 1.0 {
        max_h
    } else {
        fitted_h
    };
    let fit = (max_w / fitted_w).min(max_h / fitted_h).min(1.0);
    if fit < 1.0 {
        fitted_w *= fit;
        fitted_h *= fit;
    }
    (fitted_w, fitted_h)
}

fn display_cap(app: &App) -> (f32, f32) {
    app.primary_display()
        .map(|display| {
            let size = display.visible_bounds().size;
            (size.width.as_f32() * 0.9, size.height.as_f32() * 0.9)
        })
        .unwrap_or((1600.0, 1000.0))
}

fn window_display_cap(window: &Window, app: &App) -> (f32, f32) {
    window
        .display(app)
        .or_else(|| app.primary_display())
        .map(|display| {
            let size = display.visible_bounds().size;
            (size.width.as_f32() * 0.9, size.height.as_f32() * 0.9)
        })
        .unwrap_or((1600.0, 1000.0))
}

fn image_pixel_size(path: &str) -> Option<(i32, i32)> {
    let mut file = std::fs::File::open(path).ok()?;
    let mut bytes = Vec::new();
    file.by_ref()
        .take(2 * 1024 * 1024)
        .read_to_end(&mut bytes)
        .ok()?;
    png_pixel_size(&bytes)
        .or_else(|| jpeg_pixel_size(&bytes))
        .or_else(|| webp_pixel_size(&bytes))
}

fn png_pixel_size(data: &[u8]) -> Option<(i32, i32)> {
    const SIG: &[u8] = &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    if data.len() < 24 || &data[..8] != SIG || &data[12..16] != b"IHDR" {
        return None;
    }
    let width = i32::try_from(u32::from_be_bytes(data[16..20].try_into().ok()?)).ok()?;
    let height = i32::try_from(u32::from_be_bytes(data[20..24].try_into().ok()?)).ok()?;
    (width > 0 && height > 0).then_some((width, height))
}

fn jpeg_pixel_size(data: &[u8]) -> Option<(i32, i32)> {
    if data.len() < 4 || data[0] != 0xff || data[1] != 0xd8 {
        return None;
    }
    let mut index = 2usize;
    while index + 3 < data.len() {
        if data[index] != 0xff {
            return None;
        }
        while index < data.len() && data[index] == 0xff {
            index += 1;
        }
        if index >= data.len() {
            return None;
        }
        let marker = data[index];
        index += 1;
        if marker == 0xd8 || marker == 0xd9 || marker == 0xda || (0xd0..=0xd7).contains(&marker) {
            if marker == 0xda || marker == 0xd9 {
                return None;
            }
            continue;
        }
        if index + 1 >= data.len() {
            return None;
        }
        let len = usize::from(u16::from_be_bytes([data[index], data[index + 1]]));
        if len < 2 || index + len > data.len() {
            return None;
        }
        if matches!(
            marker,
            0xc0 | 0xc1
                | 0xc2
                | 0xc3
                | 0xc5
                | 0xc6
                | 0xc7
                | 0xc9
                | 0xca
                | 0xcb
                | 0xcd
                | 0xce
                | 0xcf
        ) && len >= 7
        {
            let height = i32::from(u16::from_be_bytes([data[index + 3], data[index + 4]]));
            let width = i32::from(u16::from_be_bytes([data[index + 5], data[index + 6]]));
            if width > 0 && height > 0 {
                return Some((width, height));
            }
            return None;
        }
        index += len;
    }
    None
}

fn webp_pixel_size(data: &[u8]) -> Option<(i32, i32)> {
    if data.len() < 30 || &data[0..4] != b"RIFF" || &data[8..12] != b"WEBP" {
        return None;
    }
    match &data[12..16] {
        b"VP8X" => {
            let width = 1 + i32::from(u32::from_le_bytes([data[24], data[25], data[26], 0]) as u16);
            let height =
                1 + i32::from(u32::from_le_bytes([data[27], data[28], data[29], 0]) as u16);
            (width > 0 && height > 0).then_some((width, height))
        }
        b"VP8 " => {
            let payload = &data[20..];
            if payload.len() < 10 || payload[3] != 0x9d || payload[4] != 0x01 || payload[5] != 0x2a
            {
                return None;
            }
            let width = i32::from(u16::from_le_bytes([payload[6], payload[7]]) & 0x3fff);
            let height = i32::from(u16::from_le_bytes([payload[8], payload[9]]) & 0x3fff);
            (width > 0 && height > 0).then_some((width, height))
        }
        b"VP8L" => {
            let payload = &data[20..];
            if payload.len() < 5 || payload[0] != 0x2f {
                return None;
            }
            let bits = u32::from_le_bytes([payload[1], payload[2], payload[3], payload[4]]);
            let width = i32::try_from((bits & 0x3fff) + 1).ok()?;
            let height = i32::try_from(((bits >> 14) & 0x3fff) + 1).ok()?;
            Some((width, height))
        }
        _ => None,
    }
}

impl Drop for ShellView {
    fn drop(&mut self) {
        // LiveClient::drop also sends close. Sending it here starts TDLib's
        // shutdown before the join, and never sends logOut.
        self.client.send(ShellCommand::Close);
    }
}

#[cfg(test)]
mod photo_fit_tests {
    use super::{fit_photo_logical, png_pixel_size};

    #[test]
    fn native_size_divides_by_scale_and_does_not_upscale() {
        assert_eq!(
            fit_photo_logical(1280, 800, 2.0, 1728.0, 1080.0),
            (640.0, 400.0)
        );
        assert_eq!(
            fit_photo_logical(280, 180, 1.0, 1728.0, 1080.0),
            (280.0, 180.0)
        );
    }

    #[test]
    fn oversized_bitmap_only_scales_down_to_the_display() {
        let (width, height) = fit_photo_logical(2560, 1440, 1.0, 1728.0, 1080.0);
        assert!((width - 1728.0).abs() < 0.01);
        assert!((height - 972.0).abs() < 0.01);
    }

    #[test]
    fn png_header_reports_pixel_size() {
        let mut bytes = vec![
            0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, b'I', b'H',
            b'D', b'R',
        ];
        bytes.extend_from_slice(&1280u32.to_be_bytes());
        bytes.extend_from_slice(&800u32.to_be_bytes());
        assert_eq!(png_pixel_size(&bytes), Some((1280, 800)));
    }
}
