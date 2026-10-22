//! OS credential storage only; disk state contains nonsecret references.
use crate::protocol::NativeError;
use keyring_core::api::CredentialStoreApi;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;
use zeroize::Zeroizing;

#[derive(Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct CredentialRef {
    pub user_id: Uuid,
    pub session_id: Uuid,
}
impl CredentialRef {
    pub fn key(&self, origin: &str) -> String {
        let hash = Sha256::digest(origin.as_bytes());
        format!("{:x}:{}:{}", hash, self.user_id, self.session_id)
    }
}
pub trait Vault: Send + Sync {
    fn get(
        &self,
        origin: &str,
        reference: &CredentialRef,
    ) -> Result<Option<Zeroizing<String>>, NativeError>;
    fn set(&self, origin: &str, reference: &CredentialRef, secret: &str)
    -> Result<(), NativeError>;
    fn delete(&self, origin: &str, reference: &CredentialRef) -> Result<(), NativeError>;
}
pub struct SystemVault;
impl SystemVault {
    fn entry(origin: &str, reference: &CredentialRef) -> Result<keyring_core::Entry, NativeError> {
        let store =
            zbus_secret_service_keyring_store::Store::new().map_err(|_| NativeError::storage())?;
        store
            .build("app.opentrack.desktop", &reference.key(origin), None)
            .map_err(|_| NativeError::storage())
    }
}
impl Vault for SystemVault {
    fn get(
        &self,
        origin: &str,
        reference: &CredentialRef,
    ) -> Result<Option<Zeroizing<String>>, NativeError> {
        match Self::entry(origin, reference)?.get_password() {
            Ok(value) if valid_token(&value) => Ok(Some(Zeroizing::new(value))),
            Err(keyring_core::Error::NoEntry) => Ok(None),
            _ => Err(NativeError::storage()),
        }
    }
    fn set(
        &self,
        origin: &str,
        reference: &CredentialRef,
        secret: &str,
    ) -> Result<(), NativeError> {
        if !valid_token(secret) {
            return Err(NativeError::server());
        }
        Self::entry(origin, reference)?
            .set_password(secret)
            .map_err(|_| NativeError::storage())?;
        if self.get(origin, reference)?.as_deref().map(|v| v.as_str()) != Some(secret) {
            return Err(NativeError::storage());
        }
        Ok(())
    }
    fn delete(&self, origin: &str, reference: &CredentialRef) -> Result<(), NativeError> {
        match Self::entry(origin, reference)?.delete_credential() {
            Ok(()) | Err(keyring_core::Error::NoEntry) => {}
            Err(_) => return Err(NativeError::storage()),
        }
        if self.get(origin, reference)?.is_some() {
            return Err(NativeError::storage());
        }
        Ok(())
    }
}
pub fn valid_token(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
