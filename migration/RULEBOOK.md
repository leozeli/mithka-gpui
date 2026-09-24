# Redesign rulebook — Flutter Mithka behavior on Rust / GPUI / TDLib

> **The meta-rule: if two agents could answer a question differently, the
> answer goes in this file.** Implementers, reviewers, and fixers read it
> before they touch a module. Inside a loop it is read-only. Amendments are
> queued for the human and applied between modules.
>
> This is a **design document plus a lookup table**. The
> [migration kit](https://github.com/anthropics/code-migration-kit-with-claude-code)
> uses a pure lookup table for structure-preserving ports. That shape does not
> fit here. Read `FEASIBILITY.md` for why.

Read this whole document before writing any code.

## 0. Scope and posture

- Flutter [leozeli/mithka](https://github.com/leozeli/mithka) is the
  behavioral spec. This repository is the new architecture: `mithka_tdlib`
  (`src/`), the product shell `mithka-gpui` (`gpui/`), and local feeds
  `mithka-rss` (`rss/`). `mithka-gtk` (`gtk/`) is a validation spike, not the
  product UI.
- **The old code is the spec.** When a guess and the running Flutter app
  disagree, match what Flutter shows on the public surface: the window, and
  the TDLib requests and updates both clients can see. This tree does not
  vendor Flutter sources. Dart tests that import internals are not the spec
  and are not the judge (`parity-scenarios.md` is).
- Do not translate Dart files one-to-one. The unit of work is a row in
  `manifest.tsv` (a module or subsystem).
- The kit's dual-translation bakeoff is invalid. Stress-test this document
  with adversarial review and disposable module runs (`prompts/03-stress-test.md`).
  Throw the disposable run away; keep the rule amendments.
- A human kicks the next manifest row. The next module is profile (M12).
  Contacts and global search (M11) are `done`.
- First pass of a new module optimizes for behavioral fidelity on its
  scenarios. Mark a known slow-but-faithful path `PERF(port):` with one line
  on the fast version, and move on.

## 1. Ecosystem — what this architecture uses

Decide these once. A second choice belongs in an amendment, not in a module branch.

| Area | Decision | Why |
|---|---|---|
| Product UI | `gpui-kit` 0.6 (`gpui-component`, `gpui-pre`). Flutter widgets are the behavior reference. GTK is the validation spike. | `README.md` GPUI window; `gpui/Cargo.toml` |
| Telegram behavior | TDLib JSON client, pinned Mithka `libtdjson.so` 1.8.67. Requests and updates are the source of truth for authorization, chats, folders, messages, files, and read state. | `src/tdjson.rs`, `src/shell.rs` |
| Session | A **copy** of the Mithka database. Empty `database_encryption_key`. `close`, never `logOut`, on shutdown. Same `api_id` / `api_hash` that created the session. `device_model` default `Android`. | `README.md` Run; `src/driver.rs` |
| Threads | TDLib `td_receive` stays on a background thread. GPUI owns the main thread and paints snapshots. | `src/shell.rs`, `gpui/src/main.rs` |
| RSS | Local crate `mithka-rss`. Not a TDLib chat and not a Telegram folder. | `rss/`, Subscriptions row in `gpui/src/main.rs` |
| Icons | Heroicons outline, solid for selected/emphasis. The name map is [`icons.md`](../icons.md) at the repo root. It is already on `main`. Do not rewrite it from a migration pass. | `icons.md`; `gpui/src/icons.rs` |
| Dependencies | No new crate unless this table names it. | Keeps agents from each picking a different HTTP, image, or UI stack |
| Errors at the TDLib boundary | Surface `TDLib error <code>: <message>` and the hints the spike already prints. Do not panic on lock, 401, generation mismatch, or a logged-out copy. | `README.md` Errors |

## 2. Design lookups

One row per decision two agents would otherwise split. An unlisted case uses
the UNKNOWN rule in the next section.

| Topic | Decision | Evidence |
|---|---|---|
| GPUI UI vs Flutter reference | Paint with gpui-kit (`Root`, `Avatar`, `Badge`, `Link`, `Message` / `Bubble`, `Input`, `Button`). Match Flutter on `parity-scenarios.md`. Do not recreate Flutter's element tree, state classes, or file layout. The transcript uses GPUI's virtual list so a scroll to the top can request older history. | `gpui/src/main.rs` module docs |
| TDLib is the source of truth | Chat order (before local pins), unread counts, folder membership, history pages, sends, and read state come from TDLib methods and updates. TDLib 1.8.67 has no `getChatFolders`; folders arrive as `updateChatFolders`. All is `chatListMain`. A chosen folder uses `loadChats` / `getChats` with `chatListFolder`. | `src/shell.rs`; `README.md` GPUI window |
| Heroicons outline / solid | Primary glyphs are Heroicons v2 outline, vendored under `gpui/assets/heroicons/outline/` and painted with `svg().data()`. Selected and emphasis states use the solid variant of the same name. There is no `solid/` directory yet, so solids are still missing. Names, including which outline files are still absent, are in [`icons.md`](../icons.md). Pin and the double-check read receipt stay custom-drawn: Heroicons has no thumbtack, and a custom double-check is preferred over two `check` glyphs. `map-pin` is on disk from the current pin mark; the product rule in `icons.md` still wants a custom pin. | `icons.md`; `gpui/src/icons.rs` |
| Local pins vs Telegram pins | A local pin is `PinLibrary` in `$XDG_DATA_HOME/ad.neko.mithka.gpui/local-pins.json` (or `~/.local/share/...`), keyed by the canonical `--database` path. Locally pinned chats sort ahead of TDLib `chatPosition.order` (server pins stay inside that TDLib order). The Pin button writes only that file. It does not send `toggleChatIsPinned`. A pinned row shows a pin mark and the word Pinned. | `gpui/src/pins.rs` |
| Telegram folder vs local folder group | A Telegram folder is a TDLib chat folder (`FolderItem`, id from `updateChatFolders`). A local folder group (design name LocalFolderGroup; Rust type `LocalGroup` in `gpui/src/groups.rs`) nests those folders. It is a name plus `NestedFolder::Main` (`{ "kind": "main" }`) and `NestedFolder::Folder { id }` (`{ "kind": "folder", "id": N }`). Schema version 2. A version 1 file that stored chat ids is rewritten on load and those ids are dropped. Groups do not filter by chat id. The window is four panes: groups, folders inside the selected group, the chat list, the open chat. Attach adds a folder to the group. Remove takes it out. All chats clears the group selection. | `gpui/src/groups.rs` |
| RSS is local / non-TDLib | Subscriptions is a row in the groups column. Choosing it replaces the folder column with feed sources, the list with a newest-first timeline, and the conversation with a read-only item. HTML is flattened to plain text. The store is `subscriptions.json` under the same XDG directory and is **not** keyed by the TDLib database. Add accepts an `http` or `https` URL. Refresh fetches the selected source, or every source when All feeds is selected. | `rss/src/store.rs`; `README.md` GPUI window |

**The BUG rule.** When Flutter's visible behavior is itself defective, reproduce
it and mark `BUG(port):` with a one-line repro. Behavior matching asserts that
output. Fixes ship only in a flagged change after the scenario gate. Fidelity
and improvement are different commits.

**The UNKNOWN rule.** When neither this table nor `inventory.tsv` decides a
case, use the most conservative representation already in the crate, mark
`TODO(port):` with the open question, and keep moving. UNKNOWN is an answer.
A stalled module is not.

**TDLib vs Flutter.** If Flutter and a TDLib update disagree, follow the TDLib
update for Telegram state, and record the disagreement as `BUG(port):` (or
`TODO(port):` when it is unclear which side a user would call correct). Do not
invent a parallel chat list, folder list, or read cursor in the UI.

## 3. Markers and the escape hatch

Greppable markers are the post-parity queue. The spelling is load-bearing:

- `BUG(port):` — faithful reproduction of a known defect, with a repro.
- `TODO(port):` — an open decision. Apply the UNKNOWN rule and continue.
- `PERF(port):` — faithful but slow. One line on the fast version.

`unsafe` is allowed at the `libtdjson` FFI boundary in `src/tdjson.rs`, where
the C API is already unsafe. It is not a way to silence the borrow checker on
UI state. Every new `unsafe` block carries `// SAFETY:`.

## 4. Where a module lands

Done-ness for the migration queue is the `status` column in `manifest.tsv`,
not "a translated file appeared."

| Module kind | Home |
|---|---|
| TDLib session, chats, history, send, folders, files | `src/` (`mithka_tdlib`), consumed by `gpui/` |
| Product window, local groups, local pins, icons | `gpui/` |
| Feed fetch, parse, store | `rss/` |
| Icon name map | [`icons.md`](../icons.md) — already on `main`; link it, do not rewrite it |
| Validation spike | `gtk/` — do not grow product features here |

Shared types (`ChatItem`, `FolderItem`, `TextMessage`, `NestedFolder`) keep
the home they have now. A second copy in another crate is a rule violation.

## 5. Gap inventory

Ownership, nullability, platform, and FFI decisions are looked up in
`inventory.tsv`, one row per site. The file on disk is a **seed**. Rows the
Step 1 survey has not written yet are inventory bugs: flag them, apply the
UNKNOWN rule, keep moving. Do not treat the seed as a finished survey.

## 6. Per-module trailer

A module change ends with a status trailer in the file that owns the behavior:

```
// PORT STATUS: module=<manifest id> confidence=[high|medium|low] todos=[N]
```

Reviewers check `todos` against the file's `TODO(port)` count. An undercount
is a finding.

## 7. Deviation log

Written at gates, by the human or with the human's sign-off.

| ID | Date | Deviation | Sanctioned by |
|---|---|---|---|
| DEV-001 | 2026-09-24 | Retroactive skeleton. Bakeoff waived (redesign). Step 1 gap survey not run; `inventory.tsv` is seed rows only. `cost-log.tsv` has a header and no measured rows. | Migration-kit brief for this repo |
