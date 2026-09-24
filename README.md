# mithka-gpui

[leozeli/mithka-gpui](https://github.com/leozeli/mithka-gpui) is the product mainline: a Linux Telegram desktop client in Rust + GPUI ([longbridge/gpui-kit](https://github.com/longbridge/gpui-kit)). It is separate from the Flutter app [leozeli/mithka](https://github.com/leozeli/mithka). This tree does not include Flutter sources, and the build does not link that checkout.

The workspace shares one TDLib session core, `mithka_tdlib` (package `mithka-tdlib`). It loads a pinned `libtdjson.so` and reads a **copy** of a TDLib database. Every program links that crate:

- `mithka-gpui` (crate `gpui/`) is the product desktop shell: a GPUI window with local groups, Telegram folders, a chat list, text history, a one-line composer, an HQ photo lightbox, and local RSS/Atom subscriptions (`mithka-rss`).
- `mithka-tdlib-spike` prints authorization progress and the first chat titles, then exits.
- `mithka-gtk` (crate `gtk/`) is a **validation / reference spike only**. It is not the product UI. It stays in the repo so the GTK session behavior can still be checked.

This repository does not contain secrets. `.env.example` has empty placeholders only. Copy it to a gitignored `.env` or `run.env` and fill it in locally, or export `TDLIB_API_ID` and `TDLIB_API_HASH` in the shell. Do not commit a filled-in env file, `libtdjson.so`, or a TDLib session directory.

Build and run the product shell:

```bash
cargo build --release -p mithka-gpui
export TDLIB_API_ID=123456
export TDLIB_API_HASH='your-api-hash'
./target/release/mithka-gpui \
  --tdjson /path/to/libtdjson.so \
  --database "$HOME/mithka-tdlib-copy/tdlib"
```

System packages, flags, and how to copy a session are below.

The program dynamically loads four C symbols with `libloading`:

- `td_create_client_id`
- `td_send`
- `td_receive`
- `td_execute`

If the build also exports Mithka patch symbols, they are resolved and reported. They are not required and they are not called:

- `td_mithka_export_session_string`
- `td_mithka_import_session_string`
- `td_mithka_last_error`
- `td_mithka_set_transfer_boost`

On `authorizationStateWaitTdlibParameters` it sends `setTdlibParameters` in the same shape Mithka uses: `use_test_dc` false, file database, chat-info database, message database, and secret chats enabled, `device_model` defaulting to `Android`. `database_encryption_key` is always the empty string (unencrypted Mithka database). Changing that key cannot open an existing session and returns TDLib error 401.

There is no phone-number prompt. A copied, already logged-in database should reach `authorizationStateReady`. The process prints `Ready`, then up to 20 main-list chat titles, then sends `close` (never `logOut`) and exits. Ctrl-C does the same close.

This repository cannot see your Telegram account. Point the binary at a library and a database copy on your own machine.

## Build

Linux x86_64 and stable Rust (tested with 1.98; clap 4.6 needs 1.85 or newer). Compiling does not need `libtdjson.so`, Flutter, or an API hash. The GTK validation crate needs the GTK 4.14 development headers. The GPUI shell needs the Wayland/X11 and Vulkan packages listed under [GPUI window](#gpui-window). `cargo build --release` from the repo root builds the library, the CLI, the GPUI window, and the GTK validation window:

```bash
sudo apt install build-essential pkg-config clang libgtk-4-dev \
  libfontconfig-dev libfreetype-dev libssl-dev \
  libwayland-dev wayland-protocols libxkbcommon-dev libxkbcommon-x11-dev \
  libx11-xcb-dev libxcb1-dev libxcb-render0-dev libxcb-shape0-dev \
  libxcb-xfixes0-dev libxcb-xkb-dev libvulkan-dev libvulkan1
cargo build --release
```

`build-essential` provides `gcc`, which `cargo test` uses to compile a fake `libtdjson.so`.

Binaries:

- `target/release/mithka-tdlib-spike`
- `target/release/mithka-gpui`
- `target/release/mithka-gtk`

## Run

Quit Mithka first and pass a **copy** of the database (see below). Use the same `api_id` / `api_hash` that created the session.

```bash
export TDLIB_API_ID=123456
export TDLIB_API_HASH='your-api-hash'

./target/release/mithka-tdlib-spike \
  --tdjson /path/to/libtdjson.so \
  --database "$HOME/mithka-tdlib-copy/tdlib"
```

Flags override the environment. The process does not read a `.env` or `run.env` file; `source` it yourself if you keep one. `.env.example` lists the names. Do not commit a filled-in `.env` or `run.env`, a `td.binlog`, a TDLib session directory, or `libtdjson.so`.

| Flag | Default | Role |
| --- | --- | --- |
| `--tdjson` | required | Path to Mithka's `libtdjson.so` |
| `--database` | required | Directory that contains `td.binlog` (and usually `files/`) |
| `--api-id` | env `TDLIB_API_ID` | Telegram api_id |
| `--api-hash` | env `TDLIB_API_HASH` | Telegram api_hash (never printed) |
| `--device-model` | `Android` | Mithka's Linux default |
| `--system-language-code` | `en` | IETF language tag |
| `--system-version` | `Linux` | Passed through; empty lets TDLib detect the OS |
| `--application-version` | `mithka-tdlib-spike/0.1.0` | Shown to Telegram as the client version |
| `--chat-limit` | `20` | How many titles to print |
| `--verbosity` | `1` | TDLib log verbosity (errors go to stderr) |
| `--auth-timeout` | `90` | Seconds to wait for Ready |
| `--chat-timeout` | `20` | Seconds after Ready to wait for titles |
| `--debug` | off | Print incoming `@type` only, not request bodies |

`use_test_dc` is fixed to `false`. `files_directory` is always `<database>/files`, which is where Mithka stores files.

Slot 0 is the directory that itself contains `td.binlog`. Later accounts are `account-N` subdirectories; pass that subdirectory as `--database`, not the parent.

```bash
# slot 0
--database "$HOME/mithka-tdlib-copy/tdlib"

# account 1
--database "$HOME/mithka-tdlib-copy/tdlib/account-1"
```

A parent folder that only contains `account-*` and no `td.binlog` is refused, so a new empty database is not created next to the real accounts.

## Copy the session safely

TDLib keeps an exclusive lock on `td.binlog`. Two processes on one live file fail with a lock error and can corrupt the database. Opening a database also writes to it, so this spike must not use the directory Mithka has open.

1. Quit Mithka completely. A tray icon or a Flutter debug session still holds the lock.
2. Confirm it is gone: `pgrep -a mithka` should print nothing.
3. Copy the directory, then run against the copy only.

```bash
pgrep -a mithka || true

src="${XDG_DATA_HOME:-$HOME/.local/share}/ad.neko.mithka/tdlib"
dst="$HOME/mithka-tdlib-copy/tdlib"
rm -rf "$dst"
mkdir -p "$(dirname "$dst")"
cp -a "$src" "$dst"
test -f "$dst/td.binlog"
```

Expected layout of the copy:

```text
tdlib/
  td.binlog
  files/
  account-1/          # only when a second account exists
    td.binlog
    files/
```

Do not copy the modified copy back over the live directory. Mithka may have newer messages than the copy, and replacing `td.binlog` while the app is running will break the live session. Delete the copy when you are done with the spike.

The spike sends `close`, not `logOut`, so a normal run should leave the copy logged in. A 401 encryption error or a generation mismatch should also leave the files in place; the process prints the TDLib message instead of panicking.

## Find `libtdjson.so`

The library is the pinned Mithka 1.8.67 patched build from [iebb/mithka-tdjson](https://github.com/iebb/mithka-tdjson). It is not published from this repo. Take it from the Linux app you already run.

Flutter bundle (debug or release), from a Mithka checkout:

```text
build/linux/x64/debug/bundle/lib/libtdjson.so
build/linux/x64/release/bundle/lib/libtdjson.so
```

`find build -name libtdjson.so` if the profile directory differs.

Unpacked Linux tarball from a Mithka release: `lib/libtdjson.so` next to the `mithka` binary.

AppImage:

```bash
./Mithka-*.AppImage --appimage-extract
find squashfs-root -name libtdjson.so
```

Check the four required symbols:

```bash
file /path/to/libtdjson.so
nm -D /path/to/libtdjson.so | grep -E 'td_(create_client_id|send|receive|execute|mithka_)'
```

If `dlopen` fails while the file exists, a dependency did not load. `ldd /path/to/libtdjson.so` lists them. For a bundle, put that `lib/` directory on the loader path:

```bash
export LD_LIBRARY_PATH="$(dirname /path/to/libtdjson.so)${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
```

The startup log includes `TDLib version: ...`. A matching Mithka build reports a 1.8.67 line. A stub used by `cargo test` reports `1.8.67-stub` and is not a real library.

## GPUI window

`mithka-gpui` is the product shell. It uses the same flags and environment variables as the CLI and the GTK spike (`--tdjson`, `--database`, `--api-id` / `TDLIB_API_ID`, `--api-hash` / `TDLIB_API_HASH`). `device_model` still defaults to `Android`. `database_encryption_key` is still empty. `application_version` defaults to `mithka-gpui/0.1.0`. The desktop app id is `ad.neko.mithka.gpui`, so it does not take Mithka's or the GTK spike's id.

The TDLib receive loop runs on a background thread. GPUI polls that thread about every 50ms and paints on the UI thread. After `authorizationStateReady` the chat column lists main-list chats. Each row shows an `Avatar` (the small chat photo once `downloadFile` has a local path, otherwise initials), an unread `Badge` when TDLib reports `unread_count` or `is_marked_as_unread` (`99+` above 99, a dot when only marked), a one-line last-message snippet, and a local time (`HH:MM` today, `DD Mon` this year, otherwise `YYYY-MM-DD`). Click a row to open it: the shell sends `openChat` (and `closeChat` for the previous chat), loads `getChatHistory`, then `viewMessages` so the badge clears on `updateChatReadInbox`. The transcript shows text and photo messages inside `Message` / `Bubble`. A photo draws from its local file, or the word Photo until that file exists. Clicking a photo opens a separate window (`WindowKind::Floating`: an X11 transient toplevel or a parented Wayland xdg toplevel, with a title bar). The bubble keeps a small preview (`photoSize` type `m` when Telegram sent one, otherwise `s`). The popup downloads the largest size on that message (`width` × `height`, then type `w`, `y`, `x`, `m`, `s`; the stripped `i` thumbnail is skipped) and draws that file at one image pixel per device pixel (pixel size divided by the window scale factor). It only scales down, letterboxed, when that is larger than about 90% of the display. Until that file is local the window says “Downloading photo…” and does not stretch the preview. Closing that window, or pressing Esc in it, leaves the chat window open. Outgoing messages show Read when their id is at or below the chat's `last_read_outbox_message_id` (`updateChatReadOutbox`, also copied from the chat object). Otherwise they show Sent. `message.interaction_info` / `updateMessageInteractionInfo` is view, forward, reply, and reaction counts, not a read flag, and this window does not call `getMessageViewers`. Incoming mark-read stays `viewMessages` inside `SelectChat`. `http` and `https` entities are `Link`s; clicking one calls GPUI's `open_url`, which on Linux launches the system handler (`xdg-open`, then the portal). Scrolling up to the first row loads an older `getChatHistory` page (`LoadOlder`) without jumping back to the tail. The composer still sends one line with `sendMessage`. The window is one row. All chats is the groups rail, the main chat list, and the open conversation, with no folders column. Contacts uses that same width: the folders column stays hidden and contact rows fill the list column. A selected local group adds the middle column, and only for that group's nested folders. Subscriptions keeps the middle column for feed sources. Local groups nest Telegram folders. They are stored in `$XDG_DATA_HOME/ad.neko.mithka.gpui/local-groups.json`, or `~/.local/share/ad.neko.mithka.gpui/local-groups.json` when `XDG_DATA_HOME` is unset. The file is keyed by the canonical `--database` directory, so two copies do not share a list. Schema version 2: a group is a name plus nested entries `{ "kind": "main" }` (the main chat list) and `{ "kind": "folder", "id": N }` (a folder id from `updateChatFolders`). A version 1 file that stored chat ids is rewritten on load and those ids are dropped. Type a name and Add (or press Enter) to create a group; it is selected immediately. The middle column then lists the folders inside that group. Click one to load its chats (`chatListFolder`, or the main list for All). Folders that are not in the group are listed under Attach; clicking one adds it and selects it. Remove takes the selected folder out of the group. All chats leaves the group, hides the folders column, and shows the TDLib main list. There is no chat-id filter and no extra “All folders” row. TDLib 1.8.67 has no `getChatFolders` method, so this window never sends that request. All is the main list. Choosing a folder calls `loadChats` / `getChats` with `chatListFolder` and shows only chats in that folder. Subscriptions is a row in the groups column. It is not a Telegram folder and not a chat. Choosing it replaces the middle column with feed sources, the list with a newest-first timeline (title, source, time), and the conversation with a read-only item. Paste an `http` or `https` feed URL and Add (or press Enter). Refresh fetches that source, or every source when All feeds is selected. Remove deletes the selected source and its cached items. HTML in the feed is flattened to plain text. The store is `$XDG_DATA_HOME/ad.neko.mithka.gpui/subscriptions.json`, or `~/.local/share/ad.neko.mithka.gpui/subscriptions.json` when `XDG_DATA_HOME` is unset. It is not keyed by the TDLib database. A local pin is separate from Telegram's pin: the Pin button in the open chat writes `$XDG_DATA_HOME/ad.neko.mithka.gpui/local-pins.json` (or `~/.local/share/ad.neko.mithka.gpui/local-pins.json`), keyed by the canonical `--database` directory. Pinned chats sort ahead of TDLib's order, so they sit before server pins. The window never sends `toggleChatIsPinned`. A pinned row shows a pin mark and the word Pinned. Archive is still later. Closing the window drops the client, which sends `close`, not `logOut`, then the process exits.

### Search and contacts

Same `--tdjson`, `--database`, and API flags as the rest of the window.

- **Search.** A field above the chat list. The magnifying-glass icon focuses it. Typing filters the chats already on screen by title, immediately. A non-empty query also sends `searchChats` and `searchChatsOnServer` (TDLib 1.8.67 requires `type_filter`; this window sends null, and the limit is `--chat-limit` clamped to 50). An `@username`, or one username-shaped token, also sends `searchPublicChats` with that same null `type_filter`. Hits are merged without duplicates under “Also found” when they were not already in the visible list. A click uses the existing open-chat path. Folders and local groups still choose which rows are filtered locally. Subscriptions hides the field and shows “Chat search is off in Subscriptions.”
- **Contacts.** A Contacts row in the groups column (`user-group`). It sends `getContacts` and keeps rows current from `updateUser` (`is_contact`, `usernames.active_usernames`, and `profile_photo.small`). Each row is an avatar, a display name, and `@username` when TDLib has one. A click sends `createPrivateChat` with `force` false, then opens that chat. There is no add-by-phone form. The folders column stays hidden, and no folder rows are painted. Choosing All chats or a local group leaves the contacts list.

### Profile

Same `--tdjson`, `--database`, and API flags as the rest of the window. The profile replaces the conversation pane. It does not add a column.

- **Open.** In an open chat, click the title, the avatar, or Profile (`user-circle`). That peer can be a private user, a secret chat, a basic group, or a channel/supergroup. On a Contacts row, the name and avatar still open the private chat. The Profile control on that row opens the person and does not send `createPrivateChat`.
- **Back.** `chevron-left` returns to the transcript. Message does the same when that private or secret chat is already open. Open chat, shown when the profile was opened from Contacts, uses the known private chat, or `createPrivateChat` with `force` false when none is known.
- **Fields.** A large avatar, the display name, `@username` when TDLib has one, bio or description, phone only when `phone_number` is present, and online / last seen, a member count, or a subscriber count. The large photo is `profile_photo.big`, or the largest `chatPhoto` size, via `downloadFile`. Until that file is local, the avatar is initials or the small photo already on the row.
- **TDLib.** `getUser` and `getUserFullInfo` for a user or secret chat, `getBasicGroup` and `getBasicGroupFullInfo` for a basic group, `getSupergroup` and `getSupergroupFullInfo` for a channel or other supergroup, and `getChat` for the open chat. `updateUser`, `updateUserFullInfo`, `updateUserStatus`, `updateBasicGroup`, `updateBasicGroupFullInfo`, `updateSupergroup`, and `updateSupergroupFullInfo` refresh the open profile. A failure stays on the status line as `Profile error <code>: <message>` and does not close the session. There is no gift store, QR, media gallery, block, or edit-profile form.

### Settings and notifications

Same `--tdjson`, `--database`, and API flags as the rest of the window. Settings replaces the conversation pane. It does not add a column. Mute is stored only in TDLib. There is no local notification file, and this window does not send desktop tray alerts.

- **Open.** Settings (`cog-6-tooth`) is at the bottom of the groups column. Back (`chevron-left`) returns to the transcript, or to the open Subscriptions item.
- **Sections.** Notifications, Appearance, Account, Privacy, Data & storage, and About. Notifications is the live section. Each other row shows Coming soon.
- **Scopes.** Private chats, Groups, and Channels. After Ready, and again when Settings opens, the shell sends `getScopeNotificationSettings` for `notificationSettingsScopePrivateChats`, `notificationSettingsScopeGroupChats`, and `notificationSettingsScopeChannelChats`. Mute / Unmute and Show previews / Hide previews call `setScopeNotificationSettings` with the full object, so `sound_id` and the other fields you did not change stay as TDLib last reported them.
- **This chat.** The open chat has Mute / Unmute in that section and in the conversation header (`bell` when unmuted, `bell-slash` when muted). A muted chat row also shows `bell-slash`. Both controls call `setChatNotificationSettings`. Mute sets `use_default_mute_for` to false and `mute_for` longer than 366 days, which TDLib 1.8.67 treats as forever. Unmute sets `mute_for` to 0 and `use_default_mute_for` to false, so a muted scope does not turn that chat back on. A chat that still has `use_default_mute_for` shows its scope’s mute. `updateChatNotificationSettings` and `updateScopeNotificationSettings` refresh the header.
- **Errors.** A failure stays on the status line as `Notification error <code>: <message>` and does not close the session. Before Ready, the controls do not send.

The UI crate is [gpui-kit](https://github.com/longbridge/gpui-kit) `0.6` (`gpui-kit = "0.6"`). That crate re-exports GPUI, so this package does not also depend on crates.io `gpui` 0.2. The resolved stack is `gpui-kit` 0.6.6, `gpui-component` 0.6.6, and `gpui-pre` 0.3.6. `gpui_kit::init` loads the kit theme, and the window root is `Root`. Visible pieces from the kit: text rows for local groups and Telegram folders, `Avatar`, `Badge`, `Link`, `Message` / `Bubble`, a group-name `Input`, and an `Input` with a primary Send `Button`. Chrome icons are the MIT [Heroicons](https://github.com/tailwindlabs/heroicons) v2.2.0 outline set (vendored under `gpui/assets/heroicons/`, painted with GPUI `svg().data()`). Names follow that set: `folder`, `map-pin` (there is no icon named pin), `rss`, `paper-airplane` for send, `arrow-path`, `trash`, `plus`, `pencil`, `x-mark`, `photo`, `check`, `inbox`, `chat-bubble-left`, `squares-2x2`, `magnifying-glass` for search, `user-group` for contacts, `user-circle` for Profile, `chevron-left` for Back, `cog-6-tooth` for Settings, and `bell` / `bell-slash` for mute. The transcript uses GPUI's virtual list directly, because `MessageScroller` keeps its scroll offset private and this window has to notice the top of the history. Charts, docks, sidebars, tables, and forms are left unused. The composer is the kit input, so typing, Enter, and the kit's own clipboard/IME paths are the input's, not a hand-rolled keystroke box.

`gpui-pre` 0.3 draws with wgpu. On Linux that still needs a Vulkan ICD (the GPU driver on Arch/Hyprland, or lavapipe where there is no GPU) plus the same Wayland/X11 packages as before. It uses Wayland when `WAYLAND_DISPLAY` is set, otherwise X11.

Debian/Ubuntu build packages:

```bash
sudo apt install build-essential pkg-config clang \
  libfontconfig-dev libfreetype-dev libssl-dev \
  libwayland-dev wayland-protocols \
  libxkbcommon-dev libxkbcommon-x11-dev libx11-xcb-dev \
  libxcb1-dev libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev libxcb-xkb-dev \
  libvulkan-dev libvulkan1 mesa-vulkan-drivers
```

Arch (Hyprland) is the same stack. A typical set is `base-devel pkgconf clang wayland wayland-protocols libxkbcommon libxcb xcb-util-keysyms fontconfig freetype2 openssl vulkan-devel vulkan-icd-loader` plus the GPU's Vulkan driver (`vulkan-radeon`, `vulkan-intel`, or the NVIDIA Vulkan package).

```bash
export TDLIB_API_ID=123456
export TDLIB_API_HASH='your-api-hash'

cargo build --release -p mithka-gpui
./target/release/mithka-gpui \
  --tdjson "$HOME/code/mithka/native-libs/libtdjson.so" \
  --database "$HOME/mithka-tdlib-copy/tdlib"
```

## GTK window (validation only)

`mithka-gtk` is kept so the older GTK session can still be checked. It is not the product UI. It uses the same flags and environment variables as the CLI (`--tdjson`, `--database`, `--api-id` / `TDLIB_API_ID`, `--api-hash` / `TDLIB_API_HASH`). `device_model` still defaults to `Android`. `database_encryption_key` is still empty. `application_version` defaults to `mithka-gtk/0.1.0` so a session can tell the window apart from the CLI.

The receive loop runs on a background thread. The window shows a status line until `authorizationStateReady`, then a scrollable main-list of titles ordered by TDLib's last-activity order. Each row has an unread badge when TDLib reports `unread_count` or `is_marked_as_unread`, and the small chat photo when that file is already local or after `downloadFile`. A letter stands in when the photo is missing. Chips above the list come from `updateChatFolders`, which TDLib 1.8.67 pushes after authorization. That generation has no `getChatFolders` method. All is the main list. Choosing a folder calls `loadChats` / `getChats` with `chatListFolder` and shows only chats in that folder. Mithka’s own local groups are not read. Each row also shows a short last-message snippet and a local time or date from `last_message` / `updateChatLastMessage`. Click a row to load recent messages (`getChatHistory`) on the right: sender, text or photo, and a local timestamp. Opening a chat sends `openChat` (and `closeChat` for the chat you leave). After that page arrives, `viewMessages` with `messageSourceChatHistory` and `force_read` marks those messages read, so the unread badge clears when `updateChatReadInbox` arrives. A chat marked unread is cleared with `toggleChatIsMarkedAsUnread`. Scrolling the message list to the top asks for an older page: `from_message_id` is the oldest message already shown and `offset` is 0. `http` and `https` links in text — TDLib `textEntityTypeUrl` / `textEntityTypeTextUrl`, or a plain URL in the string — open with GTK's `UriLauncher` (`gtk_uri_launcher_launch`, the portal-backed successor of `gtk_show_uri`). Photo messages use a downloaded size (`downloadFile` / `updateFile`) or a “Photo” placeholder until the file is local; the caption stays under the image. Stickers, video, and voice stay out of this slice. A one-line composer sends `sendMessage` as plain text. Below 760px the conversation replaces the list; Back returns to the list.

If the copy is logged out (`authorizationStateWaitPhoneNumber` and the other wait states), the status line says so. There is no phone, code, or QR form. Lock, 401, and database-generation errors stay on screen instead of panicking. Closing the window sends `close`, not `logOut`.

```bash
export TDLIB_API_ID=123456
export TDLIB_API_HASH='your-api-hash'

./target/release/mithka-gtk \
  --tdjson "$HOME/code/mithka/native-libs/libtdjson.so" \
  --database "$HOME/mithka-tdlib-copy/tdlib"
```

## Where Mithka stores the database

On this machine the live directory is `~/.local/share/ad.neko.mithka/tdlib`. That is the GTK application id `ad.neko.mithka` under `$XDG_DATA_HOME` (default `~/.local/share`). Copy that directory, not `~/.local/share/mithka`.

```bash
find "${XDG_DATA_HOME:-$HOME/.local/share}/ad.neko.mithka" -name td.binlog
```

## What a successful run prints

Progress goes to stdout. The api_hash is not printed. TDLib's own logs (verbosity 1) go to stderr.

```text
libtdjson: /path/to/libtdjson.so
database: /home/you/mithka-tdlib-copy/tdlib
files: /home/you/mithka-tdlib-copy/tdlib/files
api_id: 123456
api_hash: set (32 characters)
device_model: Android
system_language_code: en
system_version: Linux
application_version: mithka-tdlib-spike/0.1.0
database_encryption_key: empty
use_test_dc: false
mithka symbols: td_mithka_export_session_string=yes ...
TDLib version: 1.8.67
client_id: 1
auth: authorizationStateWaitTdlibParameters
auth: sending setTdlibParameters (empty database_encryption_key, ...)
setTdlibParameters accepted
auth: authorizationStateReady
Ready
chats (2):
 1. Alpha
 2. Beta
closing
auth: authorizationStateClosed
closed
```

Titles come from `getChats` / `loadChats` plus `updateNewChat`, `updateChatTitle`, and `getChat`. An authorized database with an empty main list prints `chats: none in the local main list` and still exits 0 after `Ready`.

## Errors

Failures are printed as `TDLib error <code>: <message>` plus a `hint:` when the text is one this spike knows. The process then sends `close` and exits. It does not panic on these.

| TDLib report | Exit | What it means |
| --- | --- | --- |
| `Can't lock file`, database is locked, resource busy | 1 | Mithka is still running, or the copy was taken while it was open. Quit, copy again, do not share one live `td.binlog`. |
| `401` / wrong database encryption key | 1 | The database was created with an empty key. This program always sends an empty `database_encryption_key`. A different key will not open the session. |
| database is from a future/older TDLib, unsupported or incompatible database | 1 | `libtdjson.so` is not the build that wrote this `td.binlog`. Use the matching Mithka 1.8.67 library. |
| `authorizationStateWaitPhoneNumber` (also code, password, email, registration) | 3 | The copy is not logged in. This spike will not ask for a code. |
| Timed out before any authorization update | 1 | The library did not start the client. Check `ldd`, the architecture (x86_64), and stderr. |
| `--tdjson` missing, `--database` missing, parent `tdlib/` without `td.binlog` | 2 | Local setup. Nothing was opened. |

Ctrl-C exits 130 after `close`.

## Tests

`cargo test` builds a fake `libtdjson.so` (needs `gcc`) and checks the receive loop, the empty encryption key, and the lock / 401 / generation / phone messages. It does not contact Telegram.

## Migration

Behavioral parity with the Flutter app [leozeli/mithka](https://github.com/leozeli/mithka) is tracked as a redesign, not a file-by-file port. The module queue, rulebook, and scenario judge live in [`migration/README.md`](migration/README.md). Icon names stay in [`icons.md`](icons.md).

## 中文

[leozeli/mithka-gpui](https://github.com/leozeli/mithka-gpui) 是产品主线：Linux 上的 Rust + GPUI（[longbridge/gpui-kit](https://github.com/longbridge/gpui-kit)）Telegram 桌面客户端，与 Flutter 仓库 [leozeli/mithka](https://github.com/leozeli/mithka) 分开，不包含 Flutter 源码。共享会话核心是 `mithka_tdlib`：用 `libloading` 加载本机的 `libtdjson.so`（TDLib 1.8.67 补丁版），读取**复制出来**的 TDLib 数据库。`mithka-gtk` 只作验证参考，不是产品 UI。仓库里没有密钥，只有 `.env.example` 的空占位。

- `mithka-tdlib-spike`：命令行，打印授权进度和大约 20 个会话标题后退出。
- `mithka-gpui`：产品窗口，界面用 [gpui-kit](https://github.com/longbridge/gpui-kit) `0.6`（解析到 0.6.6，自带 `gpui-pre` 0.3.6，不再单独依赖 crates.io 的 `gpui` 0.2）。All chats 和 Contacts 不显示文件夹中间栏（分组栏 + 列表栏 + 当前会话）。选中本地分组后，中间栏只列出该分组里的文件夹。Subscriptions 的中间栏仍是订阅源。会话行有 `Avatar`（小头像，没有本地文件时用首字母）、未读 `Badge`（超过 99 显示 `99+`，仅标记未读时是圆点）、最后一条消息的短预览和本地时间。点开会话会 `openChat`，离开时 `closeChat`，历史到达后 `viewMessages` 标为已读。右侧用 `Message` / `Bubble` 显示文本和图片；`http`/`https` 用 `Link`，点击后走系统打开（Linux 上是 `xdg-open`）。消息列表滚到顶部会再拉一页更早的 `getChatHistory`。本地分组存在 `$XDG_DATA_HOME/ad.neko.mithka.gpui/local-groups.json`（未设置时是 `~/.local/share/ad.neko.mithka.gpui/local-groups.json`），按规范化的 `--database` 路径分开。格式版本 2：分组是名称，里面嵌套 `{ "kind": "main" }`（主列表）和 `{ "kind": "folder", "id": N }`（`updateChatFolders` 的文件夹 id）。版本 1 里的会话 id 会在读取时丢掉并改写成版本 2。输入名称后 Add（或回车）创建，Rename / Delete 改当前分组。选中分组后，中间一栏只列出该分组里的文件夹；点一个就按 `chatListFolder`（All 则是主列表）过滤会话。还没放进分组的文件夹在 Attach 下面，点一下就加入并选中。Remove 把当前文件夹移出分组。All chats 退出分组，收起中间栏，会话列表回到 TDLib 主列表。不再按会话 id 过滤，也没有额外的“全部文件夹”一层。TDLib 1.8.67 没有 `getChatFolders`，所以不会发那个请求。选中文件夹后用 `chatListFolder` 调 `loadChats` / `getChats`。底部是 `Input` 加发送 `Button`。记录列表用的是 GPUI 虚拟列表，因为 `MessageScroller` 不暴露顶部偏移。图表、停靠栏、侧边栏、表格和表单没有用。左侧分组里有 Subscriptions：不是 Telegram 文件夹，也不是会话。选中后中间一栏是订阅源，列表是按时间排列的条目（标题、来源、时间），右侧是只读正文（HTML 会收成纯文本）。在 Feeds 栏粘贴 `http`/`https` 地址后 Add（或回车）添加，Refresh 拉取当前源或全部源，Remove 删除选中的源和缓存条目。订阅存在 `$XDG_DATA_HOME/ad.neko.mithka.gpui/subscriptions.json`（未设置时是 `~/.local/share/ad.neko.mithka.gpui/subscriptions.json`），不按 TDLib 数据库分开。点图片消息会另开一个系统窗口（`WindowKind::Floating`，X11 上是带标题栏的 transient 窗口，Wayland 上是带父窗口的 xdg toplevel）。气泡里仍是较小的预览（有 `m` 就用 `m`，否则 `s`）。弹窗下载这条消息里像素最多的尺寸（`width`×`height`，一样大时按 `w`、`y`、`x`、`m`、`s`，跳过剥离出来的 `i`），按窗口缩放系数把图片画成一比一的设备像素，只有比屏幕大约 90% 更大时才缩小并留边。文件还没到本地时窗口里显示“Downloading photo…”，不会把预览图拉满窗口。关掉这个窗口或在里面按 Esc 只关图片，主窗口还在。自己发出的消息在 id 不大于 `last_read_outbox_message_id`（`updateChatReadOutbox`，也会从 chat 对象读）时显示 Read，否则显示 Sent。`interaction_info` 是浏览、转发、回复和反应，不是已读，本窗口不调用 `getMessageViewers`。本地置顶写在 `$XDG_DATA_HOME/ad.neko.mithka.gpui/local-pins.json`（未设置时是 `~/.local/share/...`），按规范化的 `--database` 分开，排在 TDLib 顺序（含服务器置顶）之前，不发送 `toggleChatIsPinned`。会话行会显示 Pinned。归档还没做。界面图标是 MIT Heroicons v2.2.0 线框版（`gpui/assets/heroicons/`），用 GPUI 的 `svg().data()` 绘制；置顶用 `map-pin`（这套图标没有名为 pin 的文件）。TDLib 在后台线程。关窗口发送 `close`，不会 `logOut`。Linux 上 wgpu 仍要 Vulkan ICD，有 `WAYLAND_DISPLAY` 时走 Wayland，否则走 X11。
- **搜索**：会话列表上方，放大镜图标会聚焦输入框。输入时先按标题即时过滤当前可见会话。查询非空时再请求 `searchChats` 和 `searchChatsOnServer`（1.8.67 必须带 `type_filter`，这里传 null；条数是 `--chat-limit`，最多 50）。`@用户名` 或单个用户名样式的词还会请求 `searchPublicChats`（同样 `type_filter` 为 null）。不在当前列表里的结果去重后出现在 “Also found” 下，点击仍走原来的打开会话路径。文件夹和本地分组只影响本地那一部分。Subscriptions 不显示搜索框，并提示 “Chat search is off in Subscriptions.”。启动参数与原来相同。
- **联系人**：分组栏的 Contacts（`user-group`）。发送 `getContacts`，并用 `updateUser` 更新（`is_contact`、`usernames.active_usernames`、小头像 `profile_photo.small`）。一行是头像、名字和 `@username`（没有用户名就只显示名字）。点击会 `createPrivateChat`（`force` 为 false）再打开私聊。还没有按手机号添加联系人的表单。联系人视图不显示文件夹中间栏，也不画文件夹行。点 All chats 或本地分组会离开联系人列表。
- **资料**：点开会话后，点标题、头像或 Profile（`user-circle`）会用资料页替换右侧会话，不会多出一栏。Back（`chevron-left`）回到聊天记录。私聊或秘密聊天已经打开时显示 Message。联系人行的名字和头像仍打开私聊；行上的 Profile 只打开这个人，不发 `createPrivateChat`。Open chat 在已有私聊时打开它，否则 `createPrivateChat`（`force` 为 false）。私聊和秘密聊天请求 `getUser` / `getUserFullInfo`，普通群请求 `getBasicGroup` / `getBasicGroupFullInfo`，频道和超级群请求 `getSupergroup` / `getSupergroupFullInfo`，当前会话还会 `getChat`。大头像用 `profile_photo.big`（或 `chatPhoto` 里最大的尺寸）经 `downloadFile` 下载。只有 TDLib 给了 `phone_number` 才显示电话。失败留在状态行：`Profile error <code>: <message>`，不会关闭会话。没有礼物商店、二维码、媒体库、拉黑或编辑资料表单。
- **设置和通知**：分组栏底部的 Settings（`cog-6-tooth`）会用设置页替换右侧会话，不会多出一栏。Back（`chevron-left`）回到聊天记录。分区是 Notifications、Appearance、Account、Privacy、Data & storage、About。只有 Notifications 可用，其余显示 Coming soon。私聊、群、频道的默认静音和预览走 `getScopeNotificationSettings` / `setScopeNotificationSettings`。当前会话的静音在设置页和会话标题栏（`bell` / `bell-slash`），走 `setChatNotificationSettings`。静音超过 366 天，TDLib 1.8.67 视为永久。设置只存在 TDLib 里，不另写本地库，也不发桌面通知。失败留在状态行：`Notification error <code>: <message>`，不会关闭会话。
- `mithka-gtk`：验证用 GTK4 窗口，保留在仓库里。左侧会话有未读角标、小头像（没有照片时用首字母）、最后一条消息的短预览和本地时间。上方可以用 Telegram 聊天文件夹过滤（All 是主列表）。点开会话会 `openChat`，离开时 `closeChat`，历史到达后 `viewMessages` 标为已读，角标随 `updateChatReadInbox` 清掉。消息列表滚到顶部会用更早的 `from_message_id` 再拉一页 `getChatHistory`。文本里的 `http`/`https` 链接（含 TDLib 的 URL entity）用 GTK `UriLauncher`（`gtk_uri_launcher_launch`，即 `gtk_show_uri` 的后续接口，走 portal）打开。右侧显示文本和图片消息，底部一行输入框用 `sendMessage` 发纯文本。

必须解析的四个符号：`td_create_client_id`、`td_send`、`td_receive`、`td_execute`。`td_mithka_*` 有则记录，没有也可以运行，本程序不会调用它们。

`database_encryption_key` 固定为空字符串（Mithka 的未加密本地库）。改成别的密钥会得到 TDLib **401**，打不开已有会话。`use_test_dc` 固定为 `false`。同时打开文件库、聊天资料库、消息库和秘密聊天。Linux 上的 `device_model` 默认 `Android`，与 Mithka 一致。`files_directory` 为 `<database>/files`。

授权到 `authorizationStateReady` 后打印一行 `Ready`，再打印主聊天列表标题，然后发送 `close`（不会发送 `logOut`）并退出。未登录（等待手机号、验证码、密码）时说明原因并以状态码 3 退出，不会交互登录。

### 构建

```bash
sudo apt install build-essential pkg-config clang libgtk-4-dev \
  libfontconfig-dev libfreetype-dev libssl-dev \
  libwayland-dev wayland-protocols libxkbcommon-dev libxkbcommon-x11-dev \
  libx11-xcb-dev libxcb1-dev libxcb-render0-dev libxcb-shape0-dev \
  libxcb-xfixes0-dev libxcb-xkb-dev libvulkan-dev libvulkan1
cargo build --release
```

需要 Linux x86_64 和较新的 stable Rust（1.85 及以上；本仓库用 1.98 验证过）。GTK 开发包至少要 4.14，只给验证壳用。GPUI 壳还需要 Vulkan 和 Wayland/X11 开发包。在仓库根目录 `cargo build --release` 会同时编出库 `mithka_tdlib`、命令行、GPUI 窗口和 GTK 验证窗口。二进制：`target/release/mithka-tdlib-spike`、`target/release/mithka-gpui`、`target/release/mithka-gtk`。不需要事先放好 `libtdjson.so`。

```bash
./target/release/mithka-gpui \
  --tdjson "$HOME/code/mithka/native-libs/libtdjson.so" \
  --database "$HOME/mithka-tdlib-copy/tdlib"
```

### 先安全地复制会话

实时目录被两个进程同时打开会锁失败，也可能损坏 `td.binlog`。本程序打开数据库时会写入，所以不能指向 Mithka 正在使用的目录。

1. 完全退出 Mithka / Flutter。调试会话和托盘进程也算。
2. `pgrep -a mithka` 没有输出。
3. 复制整个目录，只把复制品传给 `--database`。
4. 不要把用过的复制品覆盖回正在使用的原目录。看完可以删掉复制品。

```bash
pgrep -a mithka || true

src="${XDG_DATA_HOME:-$HOME/.local/share}/ad.neko.mithka/tdlib"
dst="$HOME/mithka-tdlib-copy/tdlib"
rm -rf "$dst"
mkdir -p "$(dirname "$dst")"
cp -a "$src" "$dst"
test -f "$dst/td.binlog"
```

本机真实目录是 `~/.local/share/ad.neko.mithka/tdlib`（GTK application id `ad.neko.mithka`）。不要用 `~/.local/share/mithka`。找不到时：

```bash
find "${XDG_DATA_HOME:-$HOME/.local/share}" -name td.binlog -path '*mithka*'
```

- 账号槽 0：目录本身就有 `td.binlog` 和 `files/`，把这个目录传给 `--database`。
- 其他账号：`tdlib/account-N/`（里面另有自己的 `td.binlog`）。不要把只有 `account-*`、没有 `td.binlog` 的父目录传进去。

### 从哪里拿 `libtdjson.so`

用你本机 Mithka Linux 构建里的那一份（iebb/mithka-tdjson 的 1.8.67 补丁产物），不要换官方未打补丁的 TDLib。

- Flutter 调试/发布包：`build/linux/x64/debug/bundle/lib/libtdjson.so` 或 `build/linux/x64/release/bundle/lib/libtdjson.so`
- 发布 tar 包：与 `mithka` 可执行文件同级的 `lib/libtdjson.so`
- AppImage：`./Mithka-*.AppImage --appimage-extract`，再 `find squashfs-root -name libtdjson.so`

```bash
nm -D /path/to/libtdjson.so | grep -E 'td_(create_client_id|send|receive|execute|mithka_)'
```

文件在但加载失败时，先 `ldd /path/to/libtdjson.so`，并把该 `lib` 目录放进 `LD_LIBRARY_PATH`。

### 运行

`api_id` / `api_hash` 必须与创建该会话时 Mithka 使用的一致。只从参数或环境变量读取，程序不会把 hash 打出来。

```bash
export TDLIB_API_ID=123456
export TDLIB_API_HASH='your-api-hash'

./target/release/mithka-tdlib-spike \
  --tdjson "$HOME/code/mithka/native-libs/libtdjson.so" \
  --database "$HOME/mithka-tdlib-copy/tdlib"
```

GTK 窗口用同一套参数。`device_model` 默认仍是 `Android`，加密密钥仍是空字符串。未登录时状态栏会说明，不会弹出手机号或二维码登录。窗口宽度小于 760px 时，点开会话会换成对话视图，Back 回到列表。

```bash
./target/release/mithka-gtk \
  --tdjson "$HOME/code/mithka/native-libs/libtdjson.so" \
  --database "$HOME/mithka-tdlib-copy/tdlib"
```

命令行成功时 stdout 里有 `database_encryption_key: empty`、`Ready`、若干 `1. 标题`，最后是 `closed`。进程退出码 0。

失败时会看到 `TDLib error <代码>: <原文>`，并尽量附上说明，而不是直接 panic：

- 锁（Can't lock file）：Mithka 还开着，或复制时程序仍在运行。退出后重新复制。退出码 1。
- 401 / encryption key：密钥必须保持为空。退出码 1。
- 数据库代数/版本不匹配：`libtdjson.so` 与写下这个 `td.binlog` 的 Mithka 1.8.67 构建不一致。退出码 1。
- `authorizationStateWaitPhoneNumber` 等：复制品未登录。退出码 3。
- 参数或路径错误（没有库文件、父目录没有 `td.binlog`）：退出码 2。
- Ctrl-C：发送 `close` 后退出码 130。

`cargo test` 用 gcc 编译一个假的 `libtdjson.so` 来验证上述路径，不会连接 Telegram。
