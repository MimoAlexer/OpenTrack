//! Atomic private TOML configuration. No bearer material is serializable here.
use crate::{credentials::CredentialRef, links::ServerOrigin, protocol::NativeError};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
};
use uuid::Uuid;
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SavedServer {
    pub origin: String,
    pub development_http: bool,
    pub credential: Option<CredentialRef>,
}
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Configuration {
    pub selected: Option<String>,
    #[serde(default)]
    pub servers: Vec<SavedServer>,
}
pub struct Persistence {
    directory: PathBuf,
}
impl Persistence {
    pub fn new(directory: PathBuf) -> Result<Self, NativeError> {
        fs::create_dir_all(&directory).map_err(|_| config_error())?;
        let metadata = fs::symlink_metadata(&directory).map_err(|_| config_error())?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(config_error());
        }
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))
            .map_err(|_| config_error())?;
        Ok(Self { directory })
    }
    pub fn load(&self) -> Result<Configuration, NativeError> {
        let path = self.directory.join("connections.toml");
        if !path.try_exists().map_err(|_| config_error())? {
            return Ok(Configuration::default());
        }
        validate_file(&path)?;
        let mut bytes = String::new();
        OpenOptions::new()
            .read(true)
            .custom_flags(0x20000)
            .open(&path)
            .map_err(|_| config_error())?
            .take(65537)
            .read_to_string(&mut bytes)
            .map_err(|_| config_error())?;
        if bytes.len() > 65536 {
            return Err(config_error());
        }
        let configuration: Configuration = toml::from_str(&bytes).map_err(|_| config_error())?;
        if configuration.servers.len() > 32 {
            return Err(config_error());
        }
        let mut seen = std::collections::BTreeSet::new();
        for server in &configuration.servers {
            let origin = ServerOrigin::parse(&server.origin).map_err(|_| config_error())?;
            if origin.as_str() != server.origin
                || (origin.as_str().starts_with("http:") && !server.development_http)
                || !seen.insert(&server.origin)
            {
                return Err(config_error());
            }
        }
        if configuration
            .selected
            .as_ref()
            .is_some_and(|s| !seen.contains(s))
        {
            return Err(config_error());
        }
        Ok(configuration)
    }
    pub fn save(&self, configuration: &Configuration) -> Result<(), NativeError> {
        let value = toml::to_string(configuration).map_err(|_| config_error())?;
        let temporary = self
            .directory
            .join(format!(".connections-{}.tmp", Uuid::new_v4()));
        let result = (|| {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&temporary)
                .map_err(|_| config_error())?;
            file.write_all(value.as_bytes())
                .and_then(|_| file.sync_all())
                .map_err(|_| config_error())?;
            fs::rename(&temporary, self.directory.join("connections.toml"))
                .map_err(|_| config_error())?;
            fs::File::open(&self.directory)
                .and_then(|f| f.sync_all())
                .map_err(|_| config_error())?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(temporary);
        }
        result
    }
}
fn validate_file(path: &Path) -> Result<(), NativeError> {
    let metadata = fs::symlink_metadata(path).map_err(|_| config_error())?;
    let parent = path
        .parent()
        .and_then(|p| fs::metadata(p).ok())
        .ok_or_else(config_error)?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.mode() & 0o077 != 0
        || metadata.uid() != parent.uid()
    {
        return Err(config_error());
    }
    Ok(())
}
fn config_error() -> NativeError {
    NativeError::new(
        "configuration_error",
        "The private desktop configuration could not be read or saved.",
        false,
    )
}
