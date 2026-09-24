# Feasibility (retroactive)

This report is the Step 0 record for a migration that is already underway.
It is a redesign of the Flutter app [leozeli/mithka](https://github.com/leozeli/mithka)
onto this repository ([leozeli/mithka-gpui](https://github.com/leozeli/mithka-gpui)):
Rust, `gpui-kit`, and TDLib. It is not a structure-preserving Dart-to-Rust
translation. Nothing below authorizes file-by-file ports.

Process source: [anthropics/code-migration-kit-with-claude-code](https://github.com/anthropics/code-migration-kit-with-claude-code).
Prompt names below are that kit's `prompts/` files. This repo does not vendor
those prompts.

## Six steps, as they apply here

0. **Feasibility** (`prompts/00-feasibility.md`). This file. Verdict is
   proceed; the product shell already runs.
0b. **Judge** (`prompts/00b-judge-setup.md`). The judge is a list of
   public-surface scenarios, seeded in `parity-scenarios.md`. Run the same
   scenario on the Flutter app and on `mithka-gpui` against TDLib's observable
   behavior. Dart unit tests that import Flutter internals are not the judge:
   this tree does not contain them, and they would die with the old widgets.
1. **Map and rules** (`prompts/01-dependency-map.md`,
   `prompts/02-gap-inventory.md`). The dependency map is a module graph, not a
   file graph. The queue is `manifest.tsv`. Decisions live in `RULEBOOK.md`.
   `inventory.tsv` holds seed gap rows only; the Step 1 survey appends the rest.
2. **Stress-test the rules** (`prompts/03-stress-test.md`). The kit's
   dual-translation bakeoff is invalid for a redesign: a diff would measure
   the new architecture, not rule obedience. Substitute an adversarial review
   of `RULEBOOK.md`, plus disposable runs of one module that are thrown away
   after the rule amendments.
3. **Implement the queue** (`prompts/04-translation-kickoff.md`). One module
   from `manifest.tsv` per unit (status `next`, then `later`). The rulebook is
   the prompt. A human kicks each unit; agents do not pull the next row on
   their own.
4. **Survey build** (`prompts/05-survey-build.md`). `cargo build -p mithka-gpui`
   (and the workspace build) is the compile referee. It is cheap enough to run
   inside a module, and it still has a survey pass before behavior matching.
5. **Run it.** The cheap proof is already the product binary: `mithka-gpui` on
   a copied, logged-in TDLib database reaches `authorizationStateReady` and
   paints the chat window. `mithka-gtk` stays a validation spike.
6. **Match behavior** (`prompts/06-post-parity.md` after the scenario gate).
   Burn down `parity-scenarios.md` for modules marked `done`, then the
   `BUG(port)` / `TODO(port)` / `PERF(port)` markers. The Flutter app's
   observable behavior is the spec.

## Three calls

1. **Redesign.** Flutter screens, widget trees, and Dart files do not map onto
   this architecture. Evidence: `README.md` (this tree does not include Flutter
   sources; `mithka-gpui` is a new shell); `gpui/src/main.rs` (gpui-kit window);
   `rss/` (local feeds, no TDLib types). Work units are the modules in
   `manifest.tsv`.
2. **Verification cost is dominated by scenarios, not by `cargo`.** Compiling
   the workspace does not need `libtdjson.so` or an API hash (`README.md`,
   Build). A parity pass needs a display, a copied `td.binlog`, and the pinned
   Mithka 1.8.67 `libtdjson.so`. Token and wall-clock bands stay `unknown`
   until a module is actually run; this skeleton did not time a build. Record
   real figures in `cost-log.tsv` when a step is kicked.
3. **The inherited tests do not survive as the judge.** `tests/smoke.rs` and
   `cargo test` hit this repo's CLI spike through a stub `libtdjson.so`. They
   do not encode Flutter behavior. Dart tests that construct widgets or import
   package internals are out of scope and are not carried over. The exit
   condition is `parity-scenarios.md`, signed off by a human, run against the
   Flutter app and against `mithka-gpui`.

## Verdict

**Proceed.** The migration is already underway: session reuse, the chat
baseline, Telegram folders, local folder groups, RSS, local pins, the photo
window, sent/read ticks, and a partial Heroicons outline set are in the tree.
`icons.md` is already on `main`.

The fact that would reverse this, which this repo cannot tell you: a behavior
the Flutter app shows that TDLib 1.8.67's public JSON client cannot represent,
so a scenario cannot be checked on both sides. Check it in a day by running
one scenario from `parity-scenarios.md` on Flutter and on `mithka-gpui` with
the same account. If the Flutter-only behavior is private Dart state with no
TDLib update, stop that scenario and amend the rulebook before writing more UI.
