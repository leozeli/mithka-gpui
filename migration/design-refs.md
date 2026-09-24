# Design references

Curated UI inspiration for `mithka-gpui`. These sites train the eye. They are
not a second design system.

Use them for the live window: a three-pane desktop layout (local groups,
Telegram folders or feed sources, chat list) plus the open conversation, and
the RSS timeline when Subscriptions is selected. Prefer desktop and web
multi-column chat and reader flows.

Study these microinteractions: sent/read ticks, the independent photo window,
and scroll that still feels continuous on a 160Hz display.

Implementation stays [Heroicons](../icons.md) plus gpui-kit. A reference must
not add an icon font, a second SVG set, or a component library. `icons.md` at
the repo root remains the name map.

## Use

1. [Mobbin](https://mobbin.com/explore/web) — desktop and web multi-column
   messaging and reader flows from shipped products. Filter to web apps; skip
   phone frames.
2. [Screenlane](https://screenlane.com) — search chat, sidebar, and split-screen
   flows. The host currently redirects to Page Flows; stay on web split views.
3. [Collect UI](https://collectui.com/) — list rows, unread treatment, and
   composer pieces. Borrow the arrangement, not a new control style.
4. [Details](https://details.so/) — micro-interaction analysis: how a state
   change, overlay, or scroll is staged. Apply that to read ticks, the photo
   window, and history scroll.
5. [60fps](https://60fps.design/) — motion and scroll timing that should still
   feel smooth on a 160Hz display. Study list and scroll continuity, not
   gesture reels.
6. [Sombra](https://sombra.design/) and [Dark Mode Design](https://www.darkmodedesign.com/) —
   dark desktop density: layered surfaces, tight columns, and text that stays
   readable. Sombra is the gradient and surface study; Dark Mode Design is the
   gallery of dark layouts. Neither replaces the gpui-kit theme.
7. [Refactoring UI](https://www.refactoringui.com/) — the book, for spacing,
   contrast, and hierarchy. It is a craft reference, not a visual style to copy.

## Skip

Skip mobile-fullscreen galleries and landing-page CTA galleries. Full-screen
phone flows, story viewers, and marketing hero or pricing pages do not map onto
this three-pane desktop shell. Do not import their iconography.
