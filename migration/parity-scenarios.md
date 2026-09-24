# Parity scenarios (judge seed)

The judge is this list, run on the Flutter app
[leozeli/mithka](https://github.com/leozeli/mithka) and on `mithka-gpui`,
against the same account. TDLib's public updates are the shared observation
channel. Dart unit tests that import Flutter internals are not scenarios.

Scenarios below cover capabilities already live on gpui (`manifest.tsv`
status `done`). Setup for every scenario, unless a scenario says otherwise:

1. Quit Flutter completely (`pgrep -a mithka` prints nothing).
2. Copy the TDLib directory and pass the copy to `mithka-gpui --database`
   (see `README.md`, Copy the session safely). Two processes must not share
   one live `td.binlog`.
3. Point `--tdjson` at the pinned Mithka 1.8.67 `libtdjson.so`.
4. Use the same `api_id` / `api_hash` that created the session.

A scenario passes when Flutter and gpui agree on the expected column. gpui
may use gpui-kit widgets; it does not have to match Flutter's layout pixel
for pixel.

## Live scenarios

### S01 — Login and session reuse

- **id:** S01
- **area:** auth-session (M01)
- **steps:** Open the copied, already logged-in database. Do not enter a phone
  number or QR code. Close the window.
- **expected (Flutter / TDLib):** The copied database reaches
  `authorizationStateReady`. Shutdown sends `close`, not `logOut`, so the copy
  stays logged in. A logged-out copy stops on
  `authorizationStateWaitPhoneNumber` (and the other wait states) with no
  phone form. `database_encryption_key` stays empty.
- **how to check on gpui:** The window leaves the status line and shows the
  chat columns. Closing the window exits after `close`. Opening the same copy
  again is still Ready.

### S02 — Chat list unread, avatar, preview

- **id:** S02
- **area:** chat-baseline (M02)
- **steps:** After Ready, read the main chat list. Include a chat with unread
  messages, a chat only marked unread, a chat whose small photo is already
  local, and a chat with no local photo.
- **expected (Flutter / TDLib):** Each row shows the title, a one-line
  `last_message` preview, and a local time (`HH:MM` today, `DD Mon` this year,
  otherwise `YYYY-MM-DD`). Unread uses `unread_count` (above 99 shows `99+`).
  `is_marked_as_unread` with a zero count is a dot. The small photo comes from
  `downloadFile` when TDLib has a local path; otherwise initials.
- **how to check on gpui:** The chat column uses `Avatar` and `Badge` the same
  way. Missing files stay initials. The preview is one line.

### S03 — Open a chat and scroll history up

- **id:** S03
- **area:** chat-baseline (M02)
- **steps:** Click a long chat. Read the newest page. Scroll until the oldest
  loaded row is in view. Stay there.
- **expected (Flutter / TDLib):** Opening sends `openChat` and `closeChat` for
  the previous chat, then `getChatHistory`. After that page arrives,
  `viewMessages` clears the unread badge via `updateChatReadInbox`. Scrolling
  to the oldest loaded message requests another `getChatHistory` page
  (`from_message_id` of the oldest message, `offset` 0) and keeps the viewport
  on that older page. The list does not jump back to the tail.
- **how to check on gpui:** The transcript grows upward. The scroll position
  stays on the older rows (`LoadOlder`). The badge clears when
  `updateChatReadInbox` arrives.

### S04 — Text send and receive

- **id:** S04
- **area:** chat-baseline (M02)
- **steps:** In an open chat, type one line and send it. From the other party
  (Flutter, or another client on this account before the copy), deliver a text
  reply into the copied database's view of that chat.
- **expected (Flutter / TDLib):** The outgoing message is `sendMessage` with
  `inputMessageText`. It appears in the thread as an outgoing bubble. An
  incoming text update appears as an incoming bubble, with sender and local
  time.
- **how to check on gpui:** The composer is one line. Enter or Send submits it
  and clears the input. Outgoing bubbles align to the end; incoming bubbles
  align to the start.

### S05 — Links

- **id:** S05
- **area:** chat-baseline (M02)
- **steps:** Open a message whose text contains an `http` or `https` URL
  (a TDLib `textEntityTypeUrl` / `textEntityTypeTextUrl`, or a plain URL in
  the string). Click it.
- **expected (Flutter / TDLib):** The URL opens in the system handler. Other
  schemes and strings with spaces are not links.
- **how to check on gpui:** The span is a gpui-kit `Link`. The click uses
  GPUI `open_url` (on Linux, `xdg-open`, then the portal).

### S06 — Photo send, receive, and the HD window

- **id:** S06
- **area:** chat-baseline (M02) and photo-hd-window (M07)
- **steps:** Open a chat that already contains an outgoing photo and an
  incoming photo (`messagePhoto` from this account and from the other party).
  Click each photo. Close the viewer with the window control, then open one
  again and press Esc.
- **expected (Flutter / TDLib):** Both directions render as photo messages.
  The bubble uses a small size (`photoSize` type `m` when present, otherwise
  `s`), or a Photo placeholder until `downloadFile` finishes. The viewer
  downloads the largest size (`width` × `height`, then type `w`, `y`, `x`,
  `m`, `s`; the stripped `i` thumbnail is skipped) and shows it at one image
  pixel per device pixel, letterboxed only when that is larger than about 90%
  of the display. Closing the viewer leaves the chat open.
- **how to check on gpui:** The bubble shows the local preview or the word
  Photo. The click opens a separate `WindowKind::Floating` window (X11
  transient toplevel, or a parented Wayland xdg toplevel) with a title bar.
  Until the full file is local the window says “Downloading photo…” and does
  not stretch the preview. Esc or close affects only that window.

### S07 — Telegram folder switch

- **id:** S07
- **area:** tg-folders (M03)
- **steps:** From All chats (no local group selected), click a Telegram folder,
  then click All.
- **expected (Flutter / TDLib):** Folder chips come from `updateChatFolders`.
  TDLib 1.8.67 does not offer `getChatFolders`, so the client never sends it.
  All is the main list. A folder calls `loadChats` / `getChats` with
  `chatListFolder` and lists only chats in that folder.
- **how to check on gpui:** The middle column lists those folders. The chat
  column follows the selected folder. All restores the main list.

### S08 — Local folder group nesting

- **id:** S08
- **area:** local-folder-groups (M04)
- **steps:** Add a group (type a name, Add or Enter). In the middle column,
  under Attach, click a Telegram folder so it joins the group. Click that
  nested folder. Remove it. Click All chats.
- **expected (Flutter / TDLib):** Flutter's local groups are a client-side
  nesting of Telegram folders, not a TDLib chat list and not a filter on chat
  ids. Main is `{ "kind": "main" }`. A folder is `{ "kind": "folder", "id": N }`
  with N from `updateChatFolders`. Choosing the nested folder shows that
  folder's chats.
- **how to check on gpui:** The file is `local-groups.json`, schema version 2,
  keyed by the canonical `--database` path. The middle column lists only
  folders inside the selected group. Attach adds and selects. Remove drops the
  selected folder. All chats clears the group and shows every Telegram folder
  again.

### S09 — RSS add, refresh, open

- **id:** S09
- **area:** rss-mvp (M05)
- **steps:** Click Subscriptions. Paste an `http` or `https` feed URL and Add
  (or Enter). Refresh. Open an item. Remove the source.
- **expected (Flutter / TDLib):** Feeds are local. They are not Telegram
  folders, not chats, and they do not go through TDLib. HTML in the item
  becomes plain text. Refresh updates the selected source, or every source
  when All feeds is selected. Remove deletes the source and its cached items.
- **how to check on gpui:** The middle column becomes feed sources, the list a
  newest-first timeline (title, source, time), and the conversation a
  read-only item. The store is `subscriptions.json` and is not keyed by the
  TDLib database.

### S10 — Local pin ordering

- **id:** S10
- **area:** local-pin (M06)
- **steps:** Open a chat that is not pinned on the server. Press Pin. Return
  to the list. Pin a second chat. Unpin the first.
- **expected (Flutter / TDLib):** A local pin is not a Telegram pin. Server
  pins stay inside TDLib's `chatPosition.order`. Locally pinned chats sort
  ahead of that order. The client does not send `toggleChatIsPinned` for this
  gesture.
- **how to check on gpui:** The button label toggles Pin / Unpin. The row
  shows a pin mark and the word Pinned. Order is stored in `local-pins.json`,
  keyed by the canonical `--database` path. Restarting on the same copy keeps
  the order.

### S11 — Sent and read ticks

- **id:** S11
- **area:** sent-read-checks (M08)
- **steps:** Send a text message (S04). Watch it before the other party reads
  it, then after `last_read_outbox_message_id` moves to that id or past it.
- **expected (Flutter / TDLib):** Outgoing messages at or below
  `last_read_outbox_message_id` (`updateChatReadOutbox`, also the field on the
  chat object) show Read. Newer outgoing messages show Sent.
  `message.interaction_info` is view, forward, reply, and reaction counts, not
  a read flag. The client does not call `getMessageViewers` for this tick.
- **how to check on gpui:** The outgoing bubble shows the word Sent or Read.
  Read also draws the `check` glyph. Incoming mark-read stays the `viewMessages`
  path from S03.

## Upcoming

These are not judge scenarios yet. They are named so a later pass does not
treat them as already live.

- **Contacts and global search (M11, `in_progress`).** Another agent owns the
  implementation. Do not add search or contact scenarios here until that work
  lands and a human moves the manifest row to `done`.
- **Profile page (M12, `next`) and settings shell plus notifications (M13,
  `next`).** No profile or settings surface is in the live window. Scenarios
  for them wait until those modules are kicked.
