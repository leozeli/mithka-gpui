//! Minimal GTK4 shell over the TDLib spike.
//!
//! The TDLib receive loop runs on a background thread. This process only
//! builds widgets and applies snapshots that the worker sends.

use clap::Parser;
use gtk4::gio::prelude::ApplicationExtManual;
use gtk4::glib;
use gtk4::prelude::*;
use gtk4::{
    Application, ApplicationWindow, Box, Button, Entry, HeaderBar, Label, ListBox, ListBoxRow,
    Orientation, PolicyType, ScrolledWindow, Separator, ToggleButton,
};
use mithka_tdlib::{
    inspect_database, LiveClient, SessionConfig, ShellCommand, TdJson, TextMessage, UiUpdate,
};
use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

#[derive(Parser, Debug)]
#[command(
    name = "mithka-gtk",
    version,
    about = "Minimal GTK4 window for a copied Mithka TDLib database"
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
    #[arg(long, default_value = "mithka-gtk/0.1.0")]
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

    let client = match LiveClient::spawn(prepared.td, prepared.config, cli.verbosity, cli.debug) {
        Ok(client) => Rc::new(client),
        Err(message) => {
            eprintln!("error: {message}");
            std::process::exit(1);
        }
    };

    let app = Application::builder()
        .application_id("ad.neko.mithka.tdlib-spike")
        .flags(gtk4::gio::ApplicationFlags::NON_UNIQUE)
        .build();
    let client_for_app = Rc::clone(&client);
    app.connect_activate(move |app| build_window(app, Rc::clone(&client_for_app)));
    // Clap already consumed argv. GApplication would treat --tdjson as its own flag.
    app.run_with_args(&[] as &[&str]);
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

struct Ui {
    status: Label,
    back: Button,
    chat_list: ListBox,
    chat_ids: RefCell<Vec<i64>>,
    folder_box: Box,
    list_pane: Box,
    folder: Cell<Option<i32>>,
    rebuilding_folders: Cell<bool>,
    right: Box,
    convo_title: Label,
    placeholder: Label,
    messages: ListBox,
    message_scroll: ScrolledWindow,
    entry: Entry,
    send: Button,
    open_chat: Cell<Option<i64>>,
    ready: Cell<bool>,
    force_list: Cell<bool>,
    /// The next conversation snapshot was requested by scrolling to the top.
    loading_older: Cell<bool>,
    suppress_scroll: Cell<bool>,
    message_count: Cell<usize>,
    /// Keep the message list on the latest line for a few frames after open.
    pin_bottom: Cell<bool>,
    pin_ticks: Cell<u8>,
    pin_scheduled: Cell<bool>,
}

fn build_window(app: &Application, client: Rc<LiveClient>) {
    let window = ApplicationWindow::builder()
        .application(app)
        .title("Mithka")
        .default_width(980)
        .default_height(680)
        .build();

    let status = Label::builder()
        .label("Starting TDLib…")
        .xalign(0.0)
        .css_classes(["dim-label", "caption"])
        .build();
    let title = Label::builder()
        .label("Mithka")
        .xalign(0.0)
        .css_classes(["heading"])
        .build();
    let title_box = Box::new(Orientation::Vertical, 0);
    title_box.append(&title);
    title_box.append(&status);

    let back = Button::with_label("Back");
    back.set_visible(false);
    let header = HeaderBar::new();
    header.pack_start(&back);
    header.set_title_widget(Some(&title_box));

    let chat_list = ListBox::new();
    chat_list.set_selection_mode(gtk4::SelectionMode::Single);
    chat_list.set_activate_on_single_click(true);
    let list_scroll = ScrolledWindow::builder()
        .child(&chat_list)
        .hscrollbar_policy(PolicyType::Never)
        .width_request(300)
        .vexpand(true)
        .build();
    let folder_box = Box::builder()
        .orientation(Orientation::Horizontal)
        .spacing(6)
        .margin_start(8)
        .margin_end(8)
        .margin_top(8)
        .margin_bottom(4)
        .build();
    let folder_scroll = ScrolledWindow::builder()
        .child(&folder_box)
        .hscrollbar_policy(PolicyType::Automatic)
        .vscrollbar_policy(PolicyType::Never)
        .propagate_natural_height(true)
        .build();
    let list_pane = Box::new(Orientation::Vertical, 0);
    list_pane.append(&folder_scroll);
    list_pane.append(&list_scroll);
    list_pane.set_width_request(300);

    let convo_title = Label::builder()
        .label("Select a chat")
        .xalign(0.0)
        .css_classes(["title-3"])
        .margin_start(16)
        .margin_end(16)
        .margin_top(12)
        .margin_bottom(8)
        .build();
    let placeholder = Label::builder()
        .label("Chat titles show up here after TDLib reports Ready.")
        .wrap(true)
        .xalign(0.0)
        .margin_start(16)
        .margin_end(16)
        .margin_bottom(12)
        .css_classes(["dim-label"])
        .build();
    let messages = ListBox::new();
    messages.set_selection_mode(gtk4::SelectionMode::None);
    let message_scroll = ScrolledWindow::builder()
        .child(&messages)
        .hscrollbar_policy(PolicyType::Never)
        .vexpand(true)
        .build();

    let entry = Entry::builder()
        .placeholder_text("Text message")
        .hexpand(true)
        .sensitive(false)
        .build();
    let send = Button::with_label("Send");
    send.set_sensitive(false);
    let composer = Box::builder()
        .orientation(Orientation::Horizontal)
        .spacing(8)
        .margin_start(12)
        .margin_end(12)
        .margin_top(8)
        .margin_bottom(12)
        .build();
    composer.append(&entry);
    composer.append(&send);

    let right = Box::new(Orientation::Vertical, 0);
    right.append(&convo_title);
    right.append(&placeholder);
    right.append(&message_scroll);
    right.append(&composer);

    let content = Box::new(Orientation::Horizontal, 0);
    content.append(&list_pane);
    content.append(&Separator::new(Orientation::Vertical));
    content.append(&right);
    right.set_hexpand(true);

    window.set_titlebar(Some(&header));
    window.set_child(Some(&content));

    let ui = Rc::new(Ui {
        status,
        back,
        chat_list,
        chat_ids: RefCell::new(Vec::new()),
        folder_box,
        list_pane,
        folder: Cell::new(None),
        rebuilding_folders: Cell::new(false),
        right,
        convo_title,
        placeholder,
        messages,
        message_scroll,
        entry,
        send,
        open_chat: Cell::new(None),
        ready: Cell::new(false),
        force_list: Cell::new(false),
        loading_older: Cell::new(false),
        suppress_scroll: Cell::new(false),
        message_count: Cell::new(0),
        pin_bottom: Cell::new(false),
        pin_ticks: Cell::new(0),
        pin_scheduled: Cell::new(false),
    });

    let activate_client = Rc::clone(&client);
    let activate_ui = Rc::clone(&ui);
    ui.chat_list.connect_row_activated(move |_, row| {
        let index = row.index();
        if index < 0 {
            return;
        }
        let Some(chat_id) = activate_ui.chat_ids.borrow().get(index as usize).copied() else {
            return;
        };
        activate_ui.open_chat.set(Some(chat_id));
        activate_ui.loading_older.set(false);
        activate_ui.force_list.set(false);
        activate_ui.convo_title.set_label("Loading…");
        refresh_composer(&activate_ui);
        apply_narrow(window_width_of(row), &activate_ui);
        activate_client.send(ShellCommand::SelectChat(chat_id));
    });

    let send_ui = Rc::clone(&ui);
    let send_client = Rc::clone(&client);
    ui.send
        .connect_clicked(move |_| send_current(&send_ui, &send_client));
    let entry_ui = Rc::clone(&ui);
    let entry_client = Rc::clone(&client);
    ui.entry
        .connect_activate(move |_| send_current(&entry_ui, &entry_client));

    let scroll_ui = Rc::clone(&ui);
    let scroll_client = Rc::clone(&client);
    ui.message_scroll
        .vadjustment()
        .connect_value_changed(move |adj| {
            if scroll_ui.suppress_scroll.get() || scroll_ui.loading_older.get() {
                return;
            }
            let Some(chat_id) = scroll_ui.open_chat.get() else {
                return;
            };
            if scroll_ui.message_count.get() == 0 {
                return;
            }
            if adj.upper() - adj.lower() <= adj.page_size() + 1.0 {
                return;
            }
            if adj.value() <= adj.lower() + 24.0 {
                scroll_ui.loading_older.set(true);
                scroll_client.send(ShellCommand::LoadOlder { chat_id });
            }
        });

    let back_ui = Rc::clone(&ui);
    let back_window = window.clone();
    ui.back.connect_clicked(move |_| {
        back_ui.force_list.set(true);
        apply_narrow(back_window.width(), &back_ui);
    });

    let tick_ui = Rc::clone(&ui);
    let tick_client = Rc::clone(&client);
    let tick_window = window.clone();
    glib::timeout_add_local(Duration::from_millis(50), move || {
        for update in tick_client.drain() {
            apply_update(&tick_ui, &tick_client, update);
        }
        apply_narrow(tick_window.width(), &tick_ui);
        glib::ControlFlow::Continue
    });

    let close_client = Rc::clone(&client);
    window.connect_close_request(move |_| {
        close_client.send(ShellCommand::Close);
        glib::Propagation::Proceed
    });

    refill_folders(&ui, &client, &[]);
    install_css();
    window.present();
}

fn install_css() {
    let Some(display) = gtk4::gdk::Display::default() else {
        return;
    };
    let provider = gtk4::CssProvider::new();
    provider.load_from_string(
        ".avatar { border-radius: 18px; }
         .avatar-fallback {
            border-radius: 18px;
            background-color: #3d6f8c;
            color: white;
            min-width: 36px;
            min-height: 36px;
            font-weight: 600;
         }
         .unread-badge {
            border-radius: 999px;
            background-color: #2ea043;
            color: white;
            padding: 0 6px;
            font-size: 11px;
            font-weight: 700;
            min-width: 16px;
         }
         .chat-snippet { font-size: 12px; }",
    );
    gtk4::style_context_add_provider_for_display(
        &display,
        &provider,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}

fn window_width_of(widget: &impl WidgetExt) -> i32 {
    widget
        .root()
        .map(|root| root.width())
        .filter(|width| *width > 0)
        .unwrap_or(980)
}

fn apply_narrow(width: i32, ui: &Ui) {
    let narrow = width > 0 && width < 760;
    let open = ui.open_chat.get().is_some();
    if !narrow {
        ui.list_pane.set_visible(true);
        ui.right.set_visible(true);
        ui.back.set_visible(false);
        return;
    }
    if open && !ui.force_list.get() {
        ui.list_pane.set_visible(false);
        ui.right.set_visible(true);
        ui.back.set_visible(true);
    } else {
        ui.list_pane.set_visible(true);
        ui.right.set_visible(false);
        ui.back.set_visible(false);
    }
}

fn apply_update(ui: &Rc<Ui>, client: &Rc<LiveClient>, update: UiUpdate) {
    match update {
        UiUpdate::Log(line) => println!("{line}"),
        UiUpdate::Status(text) => ui.status.set_text(&text),
        UiUpdate::Ready => {
            ui.ready.set(true);
            ui.status.set_text("Ready");
            refresh_composer(ui);
        }
        UiUpdate::Fatal(text) => {
            ui.ready.set(false);
            ui.status.set_text("TDLib error");
            ui.placeholder.set_text(&text);
            ui.placeholder.set_visible(true);
            refresh_composer(ui);
        }
        UiUpdate::ChatList(chats) => {
            refill_chats(ui, &chats);
            if ui.open_chat.get().is_none() {
                if chats.is_empty() && ui.ready.get() {
                    let text = if ui.folder.get().is_some() {
                        "No chats in this folder."
                    } else {
                        "No chats in the local main list yet."
                    };
                    ui.placeholder.set_text(text);
                    ui.placeholder.set_visible(true);
                } else if !chats.is_empty() {
                    ui.placeholder
                        .set_text("Open a chat to read recent text messages.");
                    ui.placeholder.set_visible(true);
                }
            }
        }
        UiUpdate::Conversation {
            chat_id,
            title,
            messages,
        } => {
            if ui.open_chat.get() != Some(chat_id) {
                return;
            }
            let preserve = ui.loading_older.get();
            let adj = ui.message_scroll.vadjustment();
            let before_upper = adj.upper();
            let before_value = adj.value();
            ui.suppress_scroll.set(true);
            ui.convo_title.set_text(&title);
            refill_messages(ui, &messages);
            ui.message_count.set(messages.len());
            ui.placeholder.set_visible(messages.is_empty());
            if messages.is_empty() {
                ui.placeholder
                    .set_text("No messages in the latest history.");
            }
            refresh_composer(ui);
            if preserve {
                ui.pin_bottom.set(false);
                let ui_scroll = Rc::clone(ui);
                glib::idle_add_local_once(move || {
                    let adj = ui_scroll.message_scroll.vadjustment();
                    ui_scroll.suppress_scroll.set(true);
                    let delta = (adj.upper() - before_upper).max(0.0);
                    let max_value = (adj.upper() - adj.page_size()).max(adj.lower());
                    adj.set_value((before_value + delta).clamp(adj.lower(), max_value));
                    ui_scroll.suppress_scroll.set(false);
                    ui_scroll.loading_older.set(false);
                });
            } else {
                ui.pin_ticks.set(0);
                ui.pin_bottom.set(true);
                schedule_pin_bottom(ui);
            }
        }
        UiUpdate::Folders(folders) => refill_folders(ui, client, &folders),
        UiUpdate::SearchResults { .. }
        | UiUpdate::Contacts(_)
        | UiUpdate::OpenChat(_)
        | UiUpdate::Profile(_) => {}
    }
}

fn refill_folders(ui: &Rc<Ui>, client: &Rc<LiveClient>, folders: &[mithka_tdlib::FolderItem]) {
    ui.rebuilding_folders.set(true);
    while let Some(child) = ui.folder_box.first_child() {
        ui.folder_box.remove(&child);
    }
    let active = ui.folder.get();
    let all = ToggleButton::with_label("All");
    all.set_active(active.is_none());
    ui.folder_box.append(&all);
    let mut anchor = all.clone();
    {
        let ui = Rc::clone(ui);
        let client = Rc::clone(client);
        all.connect_toggled(move |button| {
            if ui.rebuilding_folders.get() || !button.is_active() {
                return;
            }
            ui.folder.set(None);
            client.send(ShellCommand::SelectFolder(None));
        });
    }
    for folder in folders {
        let button = ToggleButton::with_label(&folder.title);
        button.set_group(Some(&anchor));
        button.set_active(active == Some(folder.id));
        ui.folder_box.append(&button);
        let ui = Rc::clone(ui);
        let client = Rc::clone(client);
        let id = folder.id;
        button.connect_toggled(move |button| {
            if ui.rebuilding_folders.get() || !button.is_active() {
                return;
            }
            ui.folder.set(Some(id));
            client.send(ShellCommand::SelectFolder(Some(id)));
        });
        anchor = button;
    }
    ui.rebuilding_folders.set(false);
}

fn avatar_widget(chat: &mithka_tdlib::ChatItem) -> gtk4::Widget {
    if let Some(path) = chat.avatar.as_deref() {
        if std::path::Path::new(path).is_file() {
            let picture = gtk4::Picture::for_filename(path);
            picture.set_content_fit(gtk4::ContentFit::Cover);
            picture.set_size_request(36, 36);
            picture.add_css_class("avatar");
            return picture.upcast();
        }
    }
    let letter = chat
        .title
        .chars()
        .find(|ch| !ch.is_whitespace())
        .map(|ch| ch.to_uppercase().to_string())
        .unwrap_or_else(|| "?".into());
    Label::builder()
        .label(letter)
        .css_classes(["avatar-fallback"])
        .halign(gtk4::Align::Center)
        .valign(gtk4::Align::Center)
        .width_request(36)
        .height_request(36)
        .build()
        .upcast()
}

fn unread_badge(chat: &mithka_tdlib::ChatItem) -> Option<String> {
    if chat.unread > 99 {
        Some("99+".into())
    } else if chat.unread > 0 {
        Some(chat.unread.to_string())
    } else if chat.marked_unread {
        Some("•".into())
    } else {
        None
    }
}

fn refill_chats(ui: &Ui, chats: &[mithka_tdlib::ChatItem]) {
    while let Some(row) = ui.chat_list.row_at_index(0) {
        ui.chat_list.remove(&row);
    }
    ui.chat_ids.borrow_mut().clear();
    let selected = ui.open_chat.get();
    let mut select_index = None;
    for (index, chat) in chats.iter().enumerate() {
        let title = if chat.title.is_empty() {
            "Untitled"
        } else {
            chat.title.as_str()
        };
        let label = Label::builder()
            .label(title)
            .xalign(0.0)
            .hexpand(true)
            .ellipsize(gtk4::pango::EllipsizeMode::End)
            .build();
        let title_row = Box::new(Orientation::Horizontal, 8);
        title_row.append(&label);
        if chat.preview_date > 0 {
            let time = Label::builder()
                .label(list_time(chat.preview_date))
                .xalign(1.0)
                .valign(gtk4::Align::Center)
                .css_classes(["dim-label", "caption"])
                .build();
            title_row.append(&time);
        }
        let text_col = Box::builder()
            .orientation(Orientation::Vertical)
            .spacing(2)
            .hexpand(true)
            .build();
        text_col.append(&title_row);
        if !chat.preview.is_empty() {
            let snippet = Label::builder()
                .label(&chat.preview)
                .xalign(0.0)
                .hexpand(true)
                .ellipsize(gtk4::pango::EllipsizeMode::End)
                .css_classes(["dim-label", "chat-snippet"])
                .build();
            text_col.append(&snippet);
        }
        let row_box = Box::builder()
            .orientation(Orientation::Horizontal)
            .spacing(10)
            .margin_top(8)
            .margin_bottom(8)
            .margin_start(10)
            .margin_end(10)
            .build();
        row_box.append(&avatar_widget(chat));
        row_box.append(&text_col);
        if let Some(badge) = unread_badge(chat) {
            let badge = Label::builder()
                .label(badge)
                .css_classes(["unread-badge"])
                .valign(gtk4::Align::Center)
                .build();
            row_box.append(&badge);
        }
        let row = ListBoxRow::new();
        row.set_child(Some(&row_box));
        ui.chat_list.append(&row);
        ui.chat_ids.borrow_mut().push(chat.id);
        if selected == Some(chat.id) {
            select_index = Some(index);
        }
    }
    if let Some(index) = select_index {
        if let Some(row) = ui.chat_list.row_at_index(index as i32) {
            ui.chat_list.select_row(Some(&row));
        }
    }
}

fn refill_messages(ui: &Ui, messages: &[TextMessage]) {
    while let Some(row) = ui.messages.row_at_index(0) {
        ui.messages.remove(&row);
    }
    for message in messages {
        let sender = Label::builder()
            .label(&message.sender)
            .xalign(0.0)
            .hexpand(true)
            .css_classes(["heading"])
            .build();
        let time = Label::builder()
            .label(rough_time(message.date))
            .xalign(1.0)
            .css_classes(["dim-label", "caption"])
            .build();
        let head = Box::new(Orientation::Horizontal, 8);
        head.append(&sender);
        head.append(&time);
        let row_box = Box::builder()
            .orientation(Orientation::Vertical)
            .spacing(2)
            .margin_top(8)
            .margin_bottom(8)
            .margin_start(16)
            .margin_end(16)
            .build();
        row_box.append(&head);
        match message.kind {
            mithka_tdlib::MessageKind::Text => {
                row_box.append(&message_body(&message.text, &message.links));
            }
            mithka_tdlib::MessageKind::Photo => {
                if let Some(path) = message
                    .photo
                    .as_deref()
                    .filter(|path| std::path::Path::new(path).is_file())
                {
                    let picture = gtk4::Picture::for_filename(path);
                    picture.set_content_fit(gtk4::ContentFit::Contain);
                    picture.set_can_shrink(true);
                    picture.set_halign(gtk4::Align::Start);
                    picture.set_size_request(280, 180);
                    row_box.append(&picture);
                } else {
                    let waiting = Label::builder()
                        .label("Photo")
                        .xalign(0.0)
                        .css_classes(["dim-label"])
                        .build();
                    row_box.append(&waiting);
                }
                if !message.text.is_empty() {
                    row_box.append(&message_body(&message.text, &message.links));
                }
            }
        }
        let row = ListBoxRow::new();
        row.set_activatable(false);
        row.set_selectable(false);
        row.set_child(Some(&row_box));
        ui.messages.append(&row);
    }
}

fn message_body(text: &str, links: &[mithka_tdlib::TextLink]) -> Label {
    let body = Label::builder()
        .xalign(0.0)
        .wrap(true)
        .wrap_mode(gtk4::pango::WrapMode::WordChar)
        .selectable(true)
        .build();
    body.set_max_width_chars(60);
    if links.is_empty() {
        body.set_text(text);
        return body;
    }
    body.set_markup(&link_markup(text, links));
    body.connect_activate_link(|label, uri| {
        open_http_uri(label, uri);
        glib::Propagation::Stop
    });
    body
}

fn link_markup(text: &str, links: &[mithka_tdlib::TextLink]) -> String {
    let mut spans: Vec<&mithka_tdlib::TextLink> = links
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
    let mut out = String::new();
    let mut cursor = 0usize;
    for link in spans {
        if link.start < cursor {
            continue;
        }
        out.push_str(&escape_markup(&text[cursor..link.start]));
        out.push_str("<a href=\"");
        out.push_str(&escape_markup(&link.url).replace('"', "&quot;"));
        out.push_str("\">");
        out.push_str(&escape_markup(&text[link.start..link.end]));
        out.push_str("</a>");
        cursor = link.end;
    }
    out.push_str(&escape_markup(&text[cursor..]));
    out
}

fn escape_markup(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn is_http_url(url: &str) -> bool {
    mithka_tdlib::is_http_url(url)
}

fn open_http_uri(widget: &impl WidgetExt, uri: &str) {
    if !is_http_url(uri) {
        return;
    }
    let uri = uri.to_string();
    let window = widget
        .root()
        .and_then(|root| root.downcast::<gtk4::Window>().ok());
    // GTK 4.10 replaced gtk_show_uri with GtkUriLauncher. It still opens through the portal.
    gtk4::UriLauncher::new(&uri).launch(
        window.as_ref(),
        None::<&gtk4::gio::Cancellable>,
        move |result| {
            if let Err(err) = result {
                eprintln!("could not open {uri}: {err}");
            }
        },
    );
}

fn send_current(ui: &Ui, client: &LiveClient) {
    let Some(chat_id) = ui.open_chat.get() else {
        return;
    };
    let text = ui.entry.text().to_string();
    if text.trim().is_empty() {
        return;
    }
    ui.entry.set_text("");
    client.send(ShellCommand::SendText { chat_id, text });
}

fn refresh_composer(ui: &Ui) {
    let enabled = ui.ready.get() && ui.open_chat.get().is_some();
    ui.entry.set_sensitive(enabled);
    ui.send.set_sensitive(enabled);
}

fn schedule_pin_bottom(ui: &Rc<Ui>) {
    if ui.pin_scheduled.get() {
        return;
    }
    ui.pin_scheduled.set(true);
    let ui = Rc::clone(ui);
    glib::timeout_add_local(Duration::from_millis(16), move || {
        if !ui.pin_bottom.get() {
            ui.pin_scheduled.set(false);
            return glib::ControlFlow::Break;
        }
        let adj = ui.message_scroll.vadjustment();
        ui.suppress_scroll.set(true);
        adj.set_value(adj.upper());
        ui.suppress_scroll.set(false);
        let ticks = ui.pin_ticks.get().saturating_add(1);
        ui.pin_ticks.set(ticks);
        if ticks >= 8 {
            ui.pin_bottom.set(false);
            ui.pin_scheduled.set(false);
            glib::ControlFlow::Break
        } else {
            glib::ControlFlow::Continue
        }
    });
}

fn list_time(unix: i64) -> String {
    mithka_tdlib::list_time(unix)
}

fn rough_time(unix: i64) -> String {
    mithka_tdlib::message_time(unix)
}
