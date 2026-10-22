use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ShortcutMode {
    Enabled,
    Disabled,
}
impl ShortcutMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Enabled => "enabled",
            Self::Disabled => "disabled",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct UserPreferences {
    pub workspace_id: Uuid,
    pub shortcut_mode: ShortcutMode,
    pub version: String,
    pub etag: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateUserPreferences {
    pub shortcut_mode: ShortcutMode,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn preferences_accept_only_explicit_known_modes() {
        for invalid in [
            json!({}),
            json!({"shortcut_mode":null}),
            json!({"shortcut_mode":"custom"}),
            json!({"shortcut_mode":"disabled","user_id":"someone"}),
        ] {
            assert!(serde_json::from_value::<UpdateUserPreferences>(invalid).is_err());
        }
        assert_eq!(
            serde_json::from_value::<UpdateUserPreferences>(json!({"shortcut_mode":"disabled"}))
                .unwrap()
                .shortcut_mode,
            ShortcutMode::Disabled
        );
    }
}
