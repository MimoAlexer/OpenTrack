use crate::{Error, Result, Session, bounded_text};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use utoipa::ToSchema;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum SessionAudience {
    #[default]
    Web,
    Desktop,
}
impl SessionAudience {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Web => "web",
            Self::Desktop => "desktop",
        }
    }
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "web" => Some(Self::Web),
            "desktop" => Some(Self::Desktop),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ClientCapabilities {
    pub api_version: String,
    pub native_auth_version: u32,
    pub desktop_min_protocol: u32,
}
#[derive(Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct NativeAuthStart {
    #[schema(min_length = 43, max_length = 43)]
    pub code_challenge: String,
    #[schema(max_length = 256)]
    pub callback_uri: String,
    #[schema(min_length = 64, max_length = 64)]
    pub state: String,
    #[schema(min_length = 1, max_length = 80)]
    pub device_name: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct NativeAuthStartReceipt {
    pub id: Uuid,
    pub authorization_path: String,
    pub confirmation_code: String,
    pub expires_at: DateTime<Utc>,
}
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct NativeAuthConsent {
    pub id: Uuid,
    pub device_name: String,
    pub confirmation_code: String,
    pub expires_at: DateTime<Utc>,
    pub reauthentication: bool,
}
#[derive(Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct NativeAuthApprove {
    #[schema(min_length = 8, max_length = 8)]
    pub confirmation_code: String,
}
#[derive(Serialize, Deserialize, ToSchema)]
pub struct NativeAuthApproval {
    pub callback_uri: String,
}
#[derive(Serialize, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct NativeAuthExchange {
    pub request_id: Uuid,
    #[schema(min_length = 64, max_length = 64)]
    pub code: String,
    #[schema(min_length = 43, max_length = 43)]
    pub code_verifier: String,
}
#[derive(Serialize, Deserialize, ToSchema)]
pub struct NativeSessionGrant {
    pub token_type: String,
    pub access_token: String,
    pub session_id: Uuid,
    pub expires_at: DateTime<Utc>,
    pub session: Session,
}
macro_rules! redacted_debug {
    ($($name:ty),+ $(,)?) => {$ (
        impl std::fmt::Debug for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.debug_struct(stringify!($name)).finish_non_exhaustive()
            }
        }
    )+};
}
redacted_debug!(
    NativeAuthStart,
    NativeAuthApprove,
    NativeAuthApproval,
    NativeAuthExchange,
    NativeSessionGrant
);

pub fn native_hex_secret(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}
fn canonical_base64_32(value: &str) -> bool {
    value.len() == 43
        && URL_SAFE_NO_PAD
            .decode(value)
            .is_ok_and(|bytes| bytes.len() == 32 && URL_SAFE_NO_PAD.encode(bytes) == value)
}
pub fn native_code_challenge(verifier: &str) -> Result<String> {
    if !canonical_base64_32(verifier) {
        return Err(Error::InvalidNativeAuth);
    }
    Ok(URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes())))
}
/// Exact construction, rather than URL normalization, excludes alternate host,
/// port and path spellings before a browser can be redirected to them.
pub fn valid_native_callback(value: &str) -> bool {
    if value.len() > 256 {
        return false;
    }
    let port_path = value
        .strip_prefix("http://127.0.0.1:")
        .or_else(|| value.strip_prefix("http://[::1]:"));
    let Some((port, path)) = port_path.and_then(|rest| rest.split_once('/')) else {
        return false;
    };
    path == "opentrack/native-callback"
        && port
            .parse::<u16>()
            .is_ok_and(|p| p >= 1024 && p.to_string() == port)
}
impl NativeAuthStart {
    pub fn validate(&mut self) -> Result<()> {
        if !canonical_base64_32(&self.code_challenge)
            || !native_hex_secret(&self.state)
            || !valid_native_callback(&self.callback_uri)
        {
            return Err(Error::Invalid(
                "Invalid native authentication request".into(),
            ));
        }
        self.device_name = bounded_text(&self.device_name, "Device name", 80)?;
        if self.device_name.chars().any(char::is_control) {
            return Err(Error::Invalid(
                "Device name must not contain control characters".into(),
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn callback_validation_rejects_normalized_alternatives() {
        for good in [
            "http://127.0.0.1:1024/opentrack/native-callback",
            "http://[::1]:65535/opentrack/native-callback",
        ] {
            assert!(valid_native_callback(good));
        }
        for bad in [
            "http://localhost:8080/opentrack/native-callback",
            "http://127.1:8080/opentrack/native-callback",
            "http://2130706433:8080/opentrack/native-callback",
            "http://127.0.0.1:08080/opentrack/native-callback",
            "http://127.0.0.1:80/opentrack/native-callback",
            "http://[0:0:0:0:0:0:0:1]:8080/opentrack/native-callback",
            "http://127.0.0.1:8080/opentrack/%6eative-callback",
            "http://127.0.0.1:8080/opentrack/native-callback?x=1",
            "http://127.0.0.1:8080/opentrack/native-callback#x",
            "http://127.0.0.1:8080/a/../opentrack/native-callback",
            "http://user@127.0.0.1:8080/opentrack/native-callback",
            "https://127.0.0.1:8080/opentrack/native-callback",
        ] {
            assert!(!valid_native_callback(bad), "{bad}");
        }
    }
    #[test]
    fn pkce_is_canonical_and_matches_the_published_vector() {
        assert_eq!(
            native_code_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk").unwrap(),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
        assert!(native_code_challenge(&"A".repeat(42)).is_err());
        assert!(native_code_challenge(&format!("{}B", "A".repeat(42))).is_err());
        let request = NativeAuthExchange {
            request_id: Uuid::nil(),
            code: "private_code".into(),
            code_verifier: "private_verifier".into(),
        };
        let debug = format!("{request:?}");
        assert!(!debug.contains("private_"));
    }
}
