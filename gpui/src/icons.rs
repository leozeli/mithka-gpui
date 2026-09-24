//! Outline icons from [Heroicons](https://github.com/tailwindlabs/heroicons)
//! v2.2.0. The set is MIT, copyright Tailwind Labs, Inc. The license is
//! `assets/heroicons/LICENSE`.
//!
//! GPUI paints `svg().data()` only when a text color is set. Heroicons has
//! no icon named `pin`; `map-pin` is the pin mark used in the chat list.

use gpui_kit::{px, svg, Hsla, IntoElement, Styled};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    ArrowPath,
    Bell,
    BellSlash,
    ChatBubble,
    Check,
    ChevronLeft,
    Cog6Tooth,
    Folder,
    Inbox,
    MagnifyingGlass,
    MapPin,
    PaperAirplane,
    Pencil,
    Photo,
    Plus,
    Rss,
    Squares,
    Trash,
    UserCircle,
    UserGroup,
    /// Kept with the vendored set. The photo viewer uses the window title bar.
    #[allow(dead_code)]
    XMark,
}

impl Icon {
    fn bytes(self) -> &'static [u8] {
        match self {
            Icon::ArrowPath => include_bytes!("../assets/heroicons/outline/arrow-path.svg"),
            Icon::Bell => include_bytes!("../assets/heroicons/outline/bell.svg"),
            Icon::BellSlash => include_bytes!("../assets/heroicons/outline/bell-slash.svg"),
            Icon::ChatBubble => include_bytes!("../assets/heroicons/outline/chat-bubble-left.svg"),
            Icon::Check => include_bytes!("../assets/heroicons/outline/check.svg"),
            Icon::ChevronLeft => include_bytes!("../assets/heroicons/outline/chevron-left.svg"),
            Icon::Cog6Tooth => include_bytes!("../assets/heroicons/outline/cog-6-tooth.svg"),
            Icon::Folder => include_bytes!("../assets/heroicons/outline/folder.svg"),
            Icon::Inbox => include_bytes!("../assets/heroicons/outline/inbox.svg"),
            Icon::MagnifyingGlass => {
                include_bytes!("../assets/heroicons/outline/magnifying-glass.svg")
            }
            Icon::MapPin => include_bytes!("../assets/heroicons/outline/map-pin.svg"),
            Icon::PaperAirplane => {
                include_bytes!("../assets/heroicons/outline/paper-airplane.svg")
            }
            Icon::Pencil => include_bytes!("../assets/heroicons/outline/pencil.svg"),
            Icon::Photo => include_bytes!("../assets/heroicons/outline/photo.svg"),
            Icon::Plus => include_bytes!("../assets/heroicons/outline/plus.svg"),
            Icon::Rss => include_bytes!("../assets/heroicons/outline/rss.svg"),
            Icon::Squares => include_bytes!("../assets/heroicons/outline/squares-2x2.svg"),
            Icon::Trash => include_bytes!("../assets/heroicons/outline/trash.svg"),
            Icon::UserCircle => include_bytes!("../assets/heroicons/outline/user-circle.svg"),
            Icon::UserGroup => include_bytes!("../assets/heroicons/outline/user-group.svg"),
            Icon::XMark => include_bytes!("../assets/heroicons/outline/x-mark.svg"),
        }
    }
}

pub fn hero(icon: Icon, color: Hsla) -> impl IntoElement {
    svg()
        .data(icon.bytes())
        .size(px(16.))
        .flex_shrink_0()
        .text_color(color)
}
