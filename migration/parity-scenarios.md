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
- **steps:** With no local group selected, confirm the folders column is
  hidden and the chat column is the main list. Select a local group, attach a
  Telegram folder if it is not already nested, and click that folder. Then
  click All chats.
- **expected (Flutter / TDLib):** Folder names come from `updateChatFolders`.
  TDLib 1.8.67 does not offer `getChatFolders`, so the client never sends it.
  All is the main list. A folder calls `loadChats` / `getChats` with
  `chatListFolder` and lists only chats in that folder.
- **how to check on gpui:** All chats hides the folders column and lists the
  main chat list. Inside a local group the middle column lists that group's
  nested folders (and Attach). The chat column follows the selected nested
  folder. All chats hides the column again and restores the main list.

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
  keyed by the canonical `--database` path. The middle column appears only
  while that group is selected, and lists only folders inside it. Attach adds
  and selects. Remove drops the selected folder. All chats clears the group,
  hides the folders column, and shows the main list. It does not list every
  Telegram folder.

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

### S12 — Global search

- **id:** S12
- **area:** contacts-search (M11)
- **steps:** From All chats, type a substring of a title that is already in
  the visible list. Clear it. Type a query that is not only that substring
  (a known chat's username, or an `@username`). Click one result. Open
  Subscriptions.
- **expected (Flutter / TDLib):** The visible list filters by title substring
  as the query changes. Folder and local-group selection still decide which
  chats are in that visible set. A non-empty query also sends `searchChats`
  and `searchChatsOnServer`. TDLib 1.8.67 requires `type_filter` on both;
  the client sends null. An `@username`, or one username-shaped token, also
  sends `searchPublicChats` with `query` (not `username_prefix`) and the same
  null `type_filter`. That method omits chats already in the chat list.
  Results are unique. Clicking one opens that chat (`openChat`, same path as
  S03). In Subscriptions, chat search does not run.
- **how to check on gpui:** The field is above the chat list. The
  magnifying-glass icon focuses it. Title matches update immediately. Hits
  that were not already visible appear under “Also found”. A click opens the
  conversation. Subscriptions hides the field and shows “Chat search is off
  in Subscriptions.” A search error stays on the status line.

### S13 — Contacts list

- **id:** S13
- **area:** contacts-search (M11)
- **steps:** In the groups column, click Contacts. Read the list. Click a
  contact. Then click All chats, or a local group.
- **expected (Flutter / TDLib):** Opening the list sends `getContacts`.
  `updateUser` keeps it current (`is_contact` adds or removes a row;
  `usernames.active_usernames` is the `@username`; `profile_photo.small` is
  the avatar once `downloadFile` has a local path). A click sends
  `createPrivateChat` with `force` false, then opens that private chat
  (same `openChat` path as S03). There is no add-by-phone form. Choosing All
  chats or a local group leaves the contacts list and shows chats again.
- **how to check on gpui:** The row is Contacts, with the `user-group` icon.
  The folders column is hidden (same width as All chats). No folder chips or
  rows are painted. Each contact row is an avatar (initials until the small
  photo is local), a display name, and `@username` when TDLib has one. The
  conversation opens for that person. A `getContacts` or `createPrivateChat`
  error stays on the status line and does not close the session. All chats
  returns to the main chat list. A local group shows that group's nested
  folders and chats. Profile on a contact row opens that person's profile
  (S14) and does not send `createPrivateChat`.

### S14 — Profile

- **id:** S14
- **area:** profile (M12)
- **steps:** Open a private chat. Click the title, the avatar, or Profile.
  Read the name, `@username`, bio, phone when that number is visible, and
  status. Press Back. Open a basic group, then a channel, the same way. In
  Contacts, click Profile on a row, then Open chat.
- **expected (Flutter / TDLib):** The conversation pane is replaced by the
  profile. No fifth column appears. A private or secret peer sends `getUser`
  and `getUserFullInfo`. A basic group sends `getBasicGroup` and
  `getBasicGroupFullInfo`. A channel or other supergroup sends `getSupergroup`
  and `getSupergroupFullInfo`. The open chat also sends `getChat`. The large
  photo is `profile_photo.big`, or the largest `chatPhoto` size, via
  `downloadFile`. Until that file is local, the avatar is initials or the
  small photo already on the row. Phone is shown only when TDLib sends
  `phone_number`. Status is online or last seen for a user, a member count
  for a group, or a subscriber count for a channel. `updateUser`,
  `updateUserFullInfo`, `updateUserStatus`, `updateBasicGroup`,
  `updateBasicGroupFullInfo`, `updateSupergroup`, and `updateSupergroupFullInfo`
  refresh the open profile. Back returns to the transcript. Message does the
  same when that private chat is already open. Profile on a contact row does
  not open the chat. Open chat does: `createPrivateChat` with `force` false
  when no private chat is known yet, otherwise the existing chat. A profile
  error stays on the status line (`Profile error <code>: <message>`) and does
  not close the session. There is no gift store, QR, media gallery, block, or
  edit-profile form.
- **how to check on gpui:** Profile is the `user-circle` control. Back is
  `chevron-left`. The pane shows a large avatar, the display name, `@username`
  when TDLib has one, Group / Channel / Secret chat when that applies, the
  status line, `Phone` plus the number when it is visible, and the bio or
  description. Clicking the contact’s name or avatar still opens the chat
  (S13). Choosing All chats, a local group, or Subscriptions leaves the
  profile and shows that view’s conversation again.

### S15 — Settings shell

- **id:** S15
- **area:** settings-notifications (M13)
- **steps:** Click Settings in the groups column. Read the section list.
  Click Appearance, then Account, Privacy, Data & storage, and About. Press
  Back.
- **expected (Flutter / TDLib):** Settings replaces the conversation pane.
  No fifth column appears. Back returns to the transcript, or to the
  Subscriptions item when that view is open. The sections are Notifications,
  Appearance, Account, Privacy, Data & storage, and About. Notifications is
  the live section (S16). Each other section shows “Coming soon”. A click
  does not close the session.
- **how to check on gpui:** Settings is the `cog-6-tooth` control at the
  bottom of the groups column. Back is `chevron-left`. The pane title is
  Settings. The status line is under that title. Choosing a stub section
  replaces the detail with Coming soon and leaves the chat list in place.

### S16 — Notification mute

- **id:** S16
- **area:** settings-notifications (M13)
- **steps:** After TDLib is ready, open Settings → Notifications. Read
  Private chats, Groups, and Channels. Mute one scope, then unmute it.
  Toggle previews. Open a chat. Mute it from the header. Open Settings
  again and unmute that same chat from This chat. Mute a chat whose
  settings still use the scope default, and confirm the header follows
  the scope until the chat is muted on its own.
- **expected (Flutter / TDLib):** Ready sends `getScopeNotificationSettings`
  for `notificationSettingsScopePrivateChats`,
  `notificationSettingsScopeGroupChats`, and
  `notificationSettingsScopeChannelChats`. Opening Settings sends those
  three again. Mute and preview call `setScopeNotificationSettings` with
  the full `scopeNotificationSettings` object, keeping fields that were
  not toggled (including `sound_id`). Per-chat mute and unmute call
  `setChatNotificationSettings` with the full `chatNotificationSettings`
  object. Mute sets `use_default_mute_for` false and `mute_for` longer
  than 366 days, which TDLib 1.8.67 treats as forever. Unmute sets
  `mute_for` to 0 and `use_default_mute_for` false, so a muted scope does
  not turn the chat back on. A chat that still has `use_default_mute_for`
  shows the scope’s mute. `updateChatNotificationSettings` and
  `updateScopeNotificationSettings` refresh the same state. Nothing is
  written to a local notification file. Desktop tray alerts are not sent.
  A failure stays on the status line as
  `Notification error <code>: <message>` and does not close the session.
  There is no `logOut` and no `getChatFolders`.
- **how to check on gpui:** The header control is `bell` when the open
  chat is unmuted and `bell-slash` when it is muted, labeled Mute or
  Unmute. A muted row in the chat list shows `bell-slash`. Notifications
  lists Private chats, Groups, and Channels with Muted or Unmuted,
  Previews on or off, and the same Mute / Unmute and Show previews /
  Hide previews controls. This chat names the open chat, or says to open
  one. A line says desktop alerts are not sent and mute is saved in TDLib.
  Before Ready, the controls do not send and the status line says
  notifications are available after TDLib is ready.

## Upcoming

These are not judge scenarios yet. They are named so a later pass does not
treat them as already live.

- **Richer messaging (M14, `next`).** Stickers, voice, reply, and forward
  are not in the live window. Settings and notifications are already S15
  and S16. Scenarios for richer messaging wait until that module is kicked.
