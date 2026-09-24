//! TDLib 1.8.67 notification settings.
//!
//! Mute and preview are stored by TDLib (`setChatNotificationSettings`,
//! `getScopeNotificationSettings`, `setScopeNotificationSettings`). This module
//! does not keep a second database. Desktop push is not sent from here.
//!
//! A mute longer than 366 days is forever (`setChatNotificationSettings`).

use serde_json::{json, Value};
use std::collections::HashMap;

/// Seconds. TDLib 1.8.67 treats a mute longer than 366 days as forever.
pub const MUTE_FOREVER_SECONDS: i32 = 366 * 24 * 60 * 60 + 1;

/// `notificationSettingsScope*` in TDLib 1.8.67.
/// Private includes secret chats. Group includes basic groups and non-channel
/// supergroups. Channel is a supergroup with `is_channel`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NotificationScope {
    Private,
    Group,
    Channel,
}

impl NotificationScope {
    pub fn key(self) -> &'static str {
        match self {
            NotificationScope::Private => "private",
            NotificationScope::Group => "group",
            NotificationScope::Channel => "channel",
        }
    }

    pub fn type_name(self) -> &'static str {
        match self {
            NotificationScope::Private => "notificationSettingsScopePrivateChats",
            NotificationScope::Group => "notificationSettingsScopeGroupChats",
            NotificationScope::Channel => "notificationSettingsScopeChannelChats",
        }
    }

    pub fn all() -> [NotificationScope; 3] {
        [
            NotificationScope::Private,
            NotificationScope::Group,
            NotificationScope::Channel,
        ]
    }

    pub fn from_key(key: &str) -> Option<Self> {
        match key {
            "private" => Some(NotificationScope::Private),
            "group" => Some(NotificationScope::Group),
            "channel" => Some(NotificationScope::Channel),
            _ => None,
        }
    }

    pub fn from_type(value: &Value) -> Option<Self> {
        match value["@type"].as_str() {
            Some("notificationSettingsScopePrivateChats") => Some(NotificationScope::Private),
            Some("notificationSettingsScopeGroupChats") => Some(NotificationScope::Group),
            Some("notificationSettingsScopeChannelChats") => Some(NotificationScope::Channel),
            _ => None,
        }
    }
}

/// One scope default the UI can show. Other TDLib fields stay on
/// [`ScopeNotificationState`] and are written back unchanged.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScopeNotification {
    pub scope: NotificationScope,
    pub muted: bool,
    pub show_preview: bool,
}

/// `chatNotificationSettings` from TDLib 1.8.67, including the story fields
/// the schema requires. Those fields are copied through; this client does not
/// offer a Stories control.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChatNotificationSettings {
    pub use_default_mute_for: bool,
    pub mute_for: i32,
    pub use_default_sound: bool,
    pub sound_id: i64,
    pub use_default_show_preview: bool,
    pub show_preview: bool,
    pub use_default_mute_stories: bool,
    pub mute_stories: bool,
    pub use_default_story_sound: bool,
    pub story_sound_id: i64,
    pub use_default_show_story_poster: bool,
    pub show_story_poster: bool,
    pub use_default_disable_pinned_message_notifications: bool,
    pub disable_pinned_message_notifications: bool,
    pub use_default_disable_mention_notifications: bool,
    pub disable_mention_notifications: bool,
}

impl ChatNotificationSettings {
    /// Non-mute fields stay on the scope default (`use_default_*`).
    pub fn mute_only() -> Self {
        Self {
            use_default_mute_for: true,
            mute_for: 0,
            use_default_sound: true,
            sound_id: 0,
            use_default_show_preview: true,
            show_preview: true,
            use_default_mute_stories: true,
            mute_stories: false,
            use_default_story_sound: true,
            story_sound_id: 0,
            use_default_show_story_poster: true,
            show_story_poster: true,
            use_default_disable_pinned_message_notifications: true,
            disable_pinned_message_notifications: false,
            use_default_disable_mention_notifications: true,
            disable_mention_notifications: false,
        }
    }

    pub fn with_muted(mut self, muted: bool) -> Self {
        self.use_default_mute_for = false;
        self.mute_for = if muted { MUTE_FOREVER_SECONDS } else { 0 };
        self
    }

    pub fn to_json(&self) -> Value {
        json!({
            "@type": "chatNotificationSettings",
            "use_default_mute_for": self.use_default_mute_for,
            "mute_for": self.mute_for,
            "use_default_sound": self.use_default_sound,
            "sound_id": self.sound_id,
            "use_default_show_preview": self.use_default_show_preview,
            "show_preview": self.show_preview,
            "use_default_mute_stories": self.use_default_mute_stories,
            "mute_stories": self.mute_stories,
            "use_default_story_sound": self.use_default_story_sound,
            "story_sound_id": self.story_sound_id,
            "use_default_show_story_poster": self.use_default_show_story_poster,
            "show_story_poster": self.show_story_poster,
            "use_default_disable_pinned_message_notifications": self.use_default_disable_pinned_message_notifications,
            "disable_pinned_message_notifications": self.disable_pinned_message_notifications,
            "use_default_disable_mention_notifications": self.use_default_disable_mention_notifications,
            "disable_mention_notifications": self.disable_mention_notifications,
        })
    }
}

/// `scopeNotificationSettings` from TDLib 1.8.67.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScopeNotificationState {
    pub mute_for: i32,
    pub sound_id: i64,
    pub show_preview: bool,
    pub use_default_mute_stories: bool,
    pub mute_stories: bool,
    pub story_sound_id: i64,
    pub show_story_poster: bool,
    pub disable_pinned_message_notifications: bool,
    pub disable_mention_notifications: bool,
}

impl ScopeNotificationState {
    /// TDLib's own default: unmuted, previews shown, app-default sound (`-1`).
    pub fn tdlib_default() -> Self {
        Self {
            mute_for: 0,
            sound_id: -1,
            show_preview: true,
            use_default_mute_stories: true,
            mute_stories: false,
            story_sound_id: -1,
            show_story_poster: true,
            disable_pinned_message_notifications: false,
            disable_mention_notifications: false,
        }
    }

    pub fn muted(&self) -> bool {
        self.mute_for != 0
    }

    pub fn to_public(&self, scope: NotificationScope) -> ScopeNotification {
        ScopeNotification {
            scope,
            muted: self.muted(),
            show_preview: self.show_preview,
        }
    }

    pub fn to_json(&self) -> Value {
        json!({
            "@type": "scopeNotificationSettings",
            "mute_for": self.mute_for,
            "sound_id": self.sound_id,
            "show_preview": self.show_preview,
            "use_default_mute_stories": self.use_default_mute_stories,
            "mute_stories": self.mute_stories,
            "story_sound_id": self.story_sound_id,
            "show_story_poster": self.show_story_poster,
            "disable_pinned_message_notifications": self.disable_pinned_message_notifications,
            "disable_mention_notifications": self.disable_mention_notifications,
        })
    }
}

pub fn mute_forever(muted: bool) -> i32 {
    if muted {
        MUTE_FOREVER_SECONDS
    } else {
        0
    }
}

pub fn is_muted(mute_for: i32) -> bool {
    mute_for != 0
}

/// Effective mute for a chat. `use_default_mute_for` follows the scope.
/// Unknown settings are unmuted, which is TDLib's default.
pub fn chat_is_muted(
    settings: Option<&ChatNotificationSettings>,
    scope: Option<NotificationScope>,
    scopes: &HashMap<NotificationScope, ScopeNotificationState>,
) -> bool {
    let Some(settings) = settings else {
        return false;
    };
    if settings.use_default_mute_for {
        let Some(scope) = scope else {
            return false;
        };
        return scopes.get(&scope).is_some_and(|scope| scope.muted());
    }
    is_muted(settings.mute_for)
}

pub fn parse_chat_notification_settings(value: &Value) -> Option<ChatNotificationSettings> {
    if !value.is_object() {
        return None;
    }
    Some(ChatNotificationSettings {
        use_default_mute_for: json_bool(&value["use_default_mute_for"], true),
        mute_for: json_i32(&value["mute_for"]).unwrap_or(0),
        use_default_sound: json_bool(&value["use_default_sound"], true),
        sound_id: json_i64(&value["sound_id"]).unwrap_or(0),
        use_default_show_preview: json_bool(&value["use_default_show_preview"], true),
        show_preview: json_bool(&value["show_preview"], true),
        use_default_mute_stories: json_bool(&value["use_default_mute_stories"], true),
        mute_stories: json_bool(&value["mute_stories"], false),
        use_default_story_sound: json_bool(&value["use_default_story_sound"], true),
        story_sound_id: json_i64(&value["story_sound_id"]).unwrap_or(0),
        use_default_show_story_poster: json_bool(&value["use_default_show_story_poster"], true),
        show_story_poster: json_bool(&value["show_story_poster"], true),
        use_default_disable_pinned_message_notifications: json_bool(
            &value["use_default_disable_pinned_message_notifications"],
            true,
        ),
        disable_pinned_message_notifications: json_bool(
            &value["disable_pinned_message_notifications"],
            false,
        ),
        use_default_disable_mention_notifications: json_bool(
            &value["use_default_disable_mention_notifications"],
            true,
        ),
        disable_mention_notifications: json_bool(&value["disable_mention_notifications"], false),
    })
}

pub fn parse_scope_notification_settings(value: &Value) -> Option<ScopeNotificationState> {
    if !value.is_object() {
        return None;
    }
    Some(ScopeNotificationState {
        mute_for: json_i32(&value["mute_for"]).unwrap_or(0),
        sound_id: json_i64(&value["sound_id"]).unwrap_or(-1),
        show_preview: json_bool(&value["show_preview"], true),
        use_default_mute_stories: json_bool(&value["use_default_mute_stories"], true),
        mute_stories: json_bool(&value["mute_stories"], false),
        story_sound_id: json_i64(&value["story_sound_id"]).unwrap_or(-1),
        show_story_poster: json_bool(&value["show_story_poster"], true),
        disable_pinned_message_notifications: json_bool(
            &value["disable_pinned_message_notifications"],
            false,
        ),
        disable_mention_notifications: json_bool(&value["disable_mention_notifications"], false),
    })
}

pub fn scope_fetch_requests() -> Vec<Value> {
    NotificationScope::all()
        .into_iter()
        .map(get_scope_notification_settings)
        .collect()
}

pub fn get_scope_notification_settings(scope: NotificationScope) -> Value {
    json!({
        "@type": "getScopeNotificationSettings",
        "scope": {"@type": scope.type_name()},
        "@extra": format!("getScopeNotificationSettings:{}", scope.key()),
    })
}

pub fn set_scope_notification_settings(
    scope: NotificationScope,
    settings: &ScopeNotificationState,
) -> Value {
    json!({
        "@type": "setScopeNotificationSettings",
        "scope": {"@type": scope.type_name()},
        "notification_settings": settings.to_json(),
        "@extra": format!("setScopeNotificationSettings:{}", scope.key()),
    })
}

pub fn set_chat_notification_settings(chat_id: i64, settings: &ChatNotificationSettings) -> Value {
    json!({
        "@type": "setChatNotificationSettings",
        "chat_id": chat_id,
        "notification_settings": settings.to_json(),
        "@extra": format!("setChatNotificationSettings:{chat_id}"),
    })
}

pub fn scope_from_extra(extra: &str) -> Option<NotificationScope> {
    extra
        .rsplit_once(':')
        .and_then(|(_, key)| NotificationScope::from_key(key))
}

fn json_bool(value: &Value, default: bool) -> bool {
    value.as_bool().unwrap_or(default)
}

fn json_i64(value: &Value) -> Option<i64> {
    if let Some(number) = value.as_i64() {
        return Some(number);
    }
    if let Some(number) = value.as_u64() {
        return i64::try_from(number).ok();
    }
    value.as_str().and_then(|text| text.parse().ok())
}

fn json_i32(value: &Value) -> Option<i32> {
    i32::try_from(json_i64(value)?).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mute_longer_than_366_days_is_the_forever_value() {
        assert!(MUTE_FOREVER_SECONDS > 366 * 24 * 60 * 60);
        assert!(is_muted(MUTE_FOREVER_SECONDS));
        assert!(is_muted(1));
        assert!(!is_muted(0));
    }

    #[test]
    fn default_mute_follows_the_scope_and_an_explicit_unmute_wins() {
        let mut scopes = HashMap::new();
        scopes.insert(
            NotificationScope::Group,
            ScopeNotificationState {
                mute_for: MUTE_FOREVER_SECONDS,
                ..ScopeNotificationState::tdlib_default()
            },
        );
        let inherited = ChatNotificationSettings {
            use_default_mute_for: true,
            mute_for: 0,
            ..ChatNotificationSettings::mute_only()
        };
        assert!(chat_is_muted(
            Some(&inherited),
            Some(NotificationScope::Group),
            &scopes
        ));
        assert!(!chat_is_muted(
            Some(&inherited),
            Some(NotificationScope::Private),
            &scopes
        ));
        assert!(!chat_is_muted(Some(&inherited), None, &scopes));
        let explicit = inherited.with_muted(false);
        assert!(!chat_is_muted(
            Some(&explicit),
            Some(NotificationScope::Group),
            &scopes
        ));
        assert!(!chat_is_muted(
            None,
            Some(NotificationScope::Group),
            &scopes
        ));
    }

    #[test]
    fn chat_mute_request_keeps_the_other_fields() {
        let current = ChatNotificationSettings {
            use_default_mute_for: true,
            mute_for: 0,
            use_default_sound: false,
            sound_id: 7,
            use_default_show_preview: false,
            show_preview: false,
            use_default_mute_stories: false,
            mute_stories: true,
            use_default_story_sound: false,
            story_sound_id: 9,
            use_default_show_story_poster: false,
            show_story_poster: false,
            use_default_disable_pinned_message_notifications: false,
            disable_pinned_message_notifications: true,
            use_default_disable_mention_notifications: false,
            disable_mention_notifications: true,
        };
        let request = set_chat_notification_settings(15, &current.clone().with_muted(true));
        assert_eq!(request["@type"], "setChatNotificationSettings");
        assert_eq!(request["chat_id"], 15);
        assert_eq!(request["@extra"], "setChatNotificationSettings:15");
        let settings = &request["notification_settings"];
        assert_eq!(settings["@type"], "chatNotificationSettings");
        assert_eq!(settings["use_default_mute_for"], false);
        assert_eq!(settings["mute_for"], MUTE_FOREVER_SECONDS);
        assert_eq!(settings["use_default_sound"], false);
        assert_eq!(settings["sound_id"], 7);
        assert_eq!(settings["show_preview"], false);
        assert_eq!(settings["mute_stories"], true);
        assert_eq!(settings["story_sound_id"], 9);
        assert_eq!(settings["disable_pinned_message_notifications"], true);
        assert_eq!(settings["disable_mention_notifications"], true);
        let parsed = parse_chat_notification_settings(settings).unwrap();
        assert_eq!(parsed, current.with_muted(true));
    }

    #[test]
    fn scope_requests_use_the_1_8_67_type_names() {
        let fetch = scope_fetch_requests();
        assert_eq!(fetch.len(), 3);
        assert_eq!(fetch[0]["@type"], "getScopeNotificationSettings");
        assert_eq!(
            fetch[0]["scope"]["@type"],
            "notificationSettingsScopePrivateChats"
        );
        assert_eq!(fetch[0]["@extra"], "getScopeNotificationSettings:private");
        assert_eq!(
            fetch[1]["scope"]["@type"],
            "notificationSettingsScopeGroupChats"
        );
        assert_eq!(
            fetch[2]["scope"]["@type"],
            "notificationSettingsScopeChannelChats"
        );
        assert!(fetch
            .iter()
            .all(|request| request["@type"] != "getChatFolders" && request["@type"] != "logOut"));

        let mut state = ScopeNotificationState {
            sound_id: 4,
            disable_mention_notifications: true,
            ..ScopeNotificationState::tdlib_default()
        };
        state.mute_for = mute_forever(true);
        state.show_preview = false;
        let request = set_scope_notification_settings(NotificationScope::Channel, &state);
        assert_eq!(request["@type"], "setScopeNotificationSettings");
        assert_eq!(
            request["scope"]["@type"],
            "notificationSettingsScopeChannelChats"
        );
        assert_eq!(request["@extra"], "setScopeNotificationSettings:channel");
        assert_eq!(
            request["notification_settings"]["mute_for"],
            MUTE_FOREVER_SECONDS
        );
        assert_eq!(request["notification_settings"]["sound_id"], 4);
        assert_eq!(request["notification_settings"]["show_preview"], false);
        assert_eq!(
            request["notification_settings"]["disable_mention_notifications"],
            true
        );
        assert_eq!(
            parse_scope_notification_settings(&request["notification_settings"]).unwrap(),
            state
        );
        assert_eq!(
            NotificationScope::from_type(&request["scope"]),
            Some(NotificationScope::Channel)
        );
        assert_eq!(
            scope_from_extra("getScopeNotificationSettings:group"),
            Some(NotificationScope::Group)
        );
    }

    #[test]
    fn string_sound_ids_parse() {
        let parsed = parse_scope_notification_settings(&json!({
            "@type": "scopeNotificationSettings",
            "mute_for": "0",
            "sound_id": "12",
            "show_preview": true
        }))
        .unwrap();
        assert_eq!(parsed.sound_id, 12);
        assert!(!parsed.muted());
        assert!(parsed.show_preview);
    }
}
