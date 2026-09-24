# Migration kit (redesign)

This folder is the on-disk process for bringing Flutter
[leozeli/mithka](https://github.com/leozeli/mithka) behavior onto this
Rust / GPUI / TDLib app. It follows
[anthropics/code-migration-kit-with-claude-code](https://github.com/anthropics/code-migration-kit-with-claude-code),
adapted for a **redesign**:

- The rulebook is a design document plus a lookup table (`RULEBOOK.md`).
- The dual-translation bakeoff in the kit's Step 2 is invalid. Review the
  rulebook, and throw disposable module runs away.
- The unit of work is a module in `manifest.tsv`, not a Dart file.
- Behavior matching is unchanged in spirit: the running Flutter app is the
  spec, checked through `parity-scenarios.md` and TDLib's public surface.

Prompt filenames below (`prompts/00-feasibility.md` and the rest) live in the
kit repo. This folder does not vendor them.

Icon names already live at the repo root in [`icons.md`](../icons.md). That
file is on `main`. Link it. Do not rewrite it from a migration pass.

Desktop UI inspiration for the live three-pane shell, conversation, and RSS
timeline is in [`design-refs.md`](design-refs.md). Those links do not add an
icon stack. Implementation stays Heroicons plus gpui-kit.

## How the files map onto steps 0–6

| Step | Kit prompt | What this folder holds |
|---|---|---|
| 0. Feasibility | `prompts/00-feasibility.md` | `FEASIBILITY.md` — retroactive. Verdict: proceed. |
| 0b. Judge | `prompts/00b-judge-setup.md` | `parity-scenarios.md`. Dart unit tests that import internals are not the judge. |
| 1. Map and rules | `prompts/01-dependency-map.md`, `prompts/02-gap-inventory.md` | `RULEBOOK.md`, `manifest.tsv` (module queue), `inventory.tsv` (seed gaps; the survey appends rows). |
| 2. Stress-test | `prompts/03-stress-test.md` | No bakeoff artifacts. Amendments land in `RULEBOOK.md` between modules. |
| 3. Implement | `prompts/04-translation-kickoff.md` | The queue is `manifest.tsv`. Work `next`, then `later`. The next module is richer messaging (M14). Settings and notifications (M13) are `done`. Profile (M12) is `done`. |
| 4. Survey build | `prompts/05-survey-build.md` | `cargo build -p mithka-gpui`. Record the run in `cost-log.tsv`. |
| 5. Run | (kit has no separate prompt) | `mithka-gpui` on a copied TDLib database. |
| 6. Match behavior | `prompts/06-post-parity.md` after the gate | `parity-scenarios.md` for `done` modules, then `BUG(port)` / `TODO(port)` / `PERF(port)` markers. |

## Gates and queues

A step runs to completion, leaves its artifacts on disk, and stops. **The
human's sign-off is kicking the next step.** Queues are files, not chat
memory:

- `manifest.tsv` — which module is `done`, `in_progress`, `next`, or `later`.
- `inventory.tsv` — gap sites. Seed rows are examples. Step 1 adds the rest.
- `parity-scenarios.md` — the judge. Live scenarios first; upcoming modules
  are listed and are not treated as done.
- `cost-log.tsv` — header only until a step is actually run. Append one
  tab-separated row per step:
  `step`, `timestamp`, `wall_clock_min`, `tokens`, `subagents`, `model`.
  Use `unknown` when a figure was not measured.

Contacts and global search (`M11`) are `done` (landed in `5e4f6bc`). Profile
(`M12`) is `done`. Settings and notifications (`M13`) are `done`. The next
module to kick is richer messaging (`M14`).
