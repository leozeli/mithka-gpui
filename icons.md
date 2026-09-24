# Icons (Heroicons)

Main set: outline. Selected / emphasis: solid.

Align with Flutter `HeroAppIcons` / Telegram folder icon names.

Product rule: the primary set is Heroicons outline. Selected and emphasis states use the solid variant of the same name. Pin and double-check read receipts stay custom-drawn. Heroicons has no thumbtack, and a custom double-check is preferred over two `check` glyphs.

## P0 chat baseline

- List: `inbox` / `chat-bubble-left` · `magnifying-glass` · `ellipsis-horizontal`
- Row state: `bell` / `bell-slash` · unread = badge (no icon) · pin = custom draw (no thumbtack in Hero)
- Chat: `paper-airplane` · `photo` · `microphone` · `face-smile` · `paper-clip` (if attachments) · `arrow-uturn-left` reply · `arrow-uturn-right` forward · `trash` · `link`
- Nav: `chevron-left` / `chevron-right` · `x-mark` · `arrow-down` (scroll to bottom)
- Read: double-check = custom; single check may use `check`

## P1 Telegram folders (TDLib icon name → Hero)

- All → `inbox`
- Unread → `chat-bubble-left`
- Unmuted → `bell`
- Bots → `code-bracket` / `cpu-chip`
- Channels → `rss`
- Groups → `user-group`
- Private → `user-circle`
- Setup → `cog-6-tooth`
- Favorite → `star`
- Game → `puzzle-piece`
- Home → `home`
- Love → `heart`
- Party → `gift`
- Sport → `trophy`
- Study → `academic-cap`
- Trade → `arrows-right-left`
- Travel → `globe-alt`
- Work → `briefcase`
- Airplane → `paper-airplane`
- Book → `book-open`
- Light → `light-bulb`
- Like → `hand-thumb-up`
- Money → `banknotes`
- Note → `musical-note`
- Palette → `swatch`
- Default → `folder`
- Cat / Crown / Flower / Mask: Flutter uses custom paths; gpui same path or temporary `folder`

## P2 local groups three-pane

- Group: `folder` · expand/collapse `chevron-down` / `chevron-right` · edit `pencil` · delete `trash` · drag `bars-3` / `squares-2x2`

## P3 RSS

- Feed/channel: `rss` · refresh `arrow-path` · subscribe `plus` · item `document-text` · external link `arrow-top-right-on-square`

## Outline SVGs on disk

Checked against `gpui/assets/heroicons/outline/`. That directory is the only Heroicons style vendored today. There is no `solid/` set yet, so selected/emphasis solids are still missing even when the outline name below is present.

### Already vendored (on this checklist)

- `arrow-path`
- `chat-bubble-left`
- `check`
- `folder`
- `inbox`
- `paper-airplane`
- `pencil`
- `photo`
- `plus`
- `rss`
- `squares-2x2`
- `trash`
- `x-mark`

### Already vendored, not on this checklist

- `map-pin` — present from the current chat-list pin mark. The product rule above keeps pin as a custom draw, because Heroicons has no thumbtack.

### Missing outline icons

P0:

- `arrow-down`
- `arrow-uturn-left`
- `arrow-uturn-right`
- `bell`
- `bell-slash`
- `chevron-left`
- `chevron-right`
- `ellipsis-horizontal`
- `face-smile`
- `link`
- `magnifying-glass`
- `microphone`
- `paper-clip`

P1 (names not already listed under P0):

- `academic-cap`
- `arrows-right-left`
- `banknotes`
- `book-open`
- `briefcase`
- `code-bracket`
- `cog-6-tooth`
- `cpu-chip`
- `gift`
- `globe-alt`
- `hand-thumb-up`
- `heart`
- `home`
- `light-bulb`
- `musical-note`
- `puzzle-piece`
- `star`
- `swatch`
- `trophy`
- `user-circle`
- `user-group`

P2 (names not already listed above):

- `bars-3`
- `chevron-down`

P3 (names not already listed above):

- `arrow-top-right-on-square`
- `document-text`

Not SVGs: unread badge, custom pin, custom double-check, and Cat / Crown / Flower / Mask (custom paths, or temporary `folder`, which is already vendored).
