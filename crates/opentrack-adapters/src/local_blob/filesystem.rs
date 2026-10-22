use super::{
    BlobError, BlobKey, CleanupOutcome, CleanupRequest, CleanupState, LocalBlobConfig, Result,
};
use std::{
    ffi::{CStr, CString},
    fs::{self, File},
    io::{Read, Seek, SeekFrom, Write},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::fs::{MetadataExt, PermissionsExt},
    },
    path::{Component, Path},
    sync::Arc,
};

pub(super) struct Directories {
    keys: File,
    staging: File,
    objects: File,
}
pub(super) struct KeyLease {
    pub key: BlobKey,
    file: File,
}
pub(super) struct Stage {
    directories: Arc<Directories>,
    lease: KeyLease,
    remove_on_drop: bool,
}

impl Directories {
    pub fn open(config: &LocalBlobConfig) -> Result<Self> {
        if !config.root.is_absolute() || config.forbidden_roots.is_empty() {
            return Err(BlobError::Configuration(
                "absolute root and checkout/webroot exclusions are required",
            ));
        }
        let root = open_directory_chain(&config.root)?;
        private_directory(&root)?;
        // Read the pinned descriptor's actual location, not a second resolution
        // of the caller's path after it may have been renamed.
        let location = fs::read_link(format!("/proc/self/fd/{}", root.as_raw_fd()))?;
        for excluded in &config.forbidden_roots {
            if !excluded.is_absolute() {
                return Err(BlobError::Configuration("exclusions must be absolute"));
            }
            let excluded = fs::canonicalize(excluded)?;
            if location.starts_with(&excluded) || excluded.starts_with(&location) {
                return Err(BlobError::Configuration(
                    "blob root overlaps checkout or webroot",
                ));
            }
        }
        if location
            .ancestors()
            .any(|parent| parent.join(".git").exists())
        {
            return Err(BlobError::Configuration(
                "blob root is inside a Git checkout",
            ));
        }
        let keys = private_child(&root, c"keys")?;
        let staging = private_child(&root, c"staging")?;
        let objects = private_child(&root, c"objects")?;
        root.sync_all()?;
        Ok(Self {
            keys,
            staging,
            objects,
        })
    }
    pub fn lease(&self, key: BlobKey, exclusive: bool, create: bool) -> Result<KeyLease> {
        let lease = self.acquire(key, exclusive, create)?;
        if lease.retired()? {
            return Err(BlobError::Retired);
        }
        Ok(lease)
    }
    fn acquire(&self, key: BlobKey, exclusive: bool, create: bool) -> Result<KeyLease> {
        let name = key_name(key);
        let mut created = false;
        let file = if create {
            match open_at(
                &self.keys,
                &name,
                libc::O_RDWR | libc::O_CREAT | libc::O_EXCL,
                0o600,
            ) {
                Ok(file) => {
                    created = true;
                    file
                }
                Err(BlobError::Io(error)) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    open_at(&self.keys, &name, libc::O_RDWR, 0)?
                }
                Err(error) => return Err(error),
            }
        } else {
            match open_at(&self.keys, &name, libc::O_RDWR, 0) {
                Err(BlobError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
                    return Err(BlobError::IntegrityMissing);
                }
                result => result?,
            }
        };
        regular(&file, &[0o600])?;
        let operation = if exclusive {
            libc::LOCK_EX
        } else {
            libc::LOCK_SH
        };
        // SAFETY: flock receives a live descriptor; a separate open file
        // description is retained for the complete operation/stream lifetime.
        if unsafe { libc::flock(file.as_raw_fd(), operation | libc::LOCK_NB) } != 0 {
            let error = std::io::Error::last_os_error();
            return if error.kind() == std::io::ErrorKind::WouldBlock {
                Err(BlobError::Busy)
            } else {
                Err(error.into())
            };
        }
        same_entry(&self.keys, &name, &file)?;
        if created {
            file.sync_all()?;
            self.keys.sync_all()?;
        }
        Ok(KeyLease { key, file })
    }
    pub fn open_object(&self, key: BlobKey) -> Result<Option<File>> {
        optional_regular(&self.objects, &key_name(key), &[0o400])
    }
    pub fn confirm_durable(&self, key: BlobKey, file: &File) -> Result<()> {
        same_entry(&self.objects, &key_name(key), file)?;
        file.sync_all()?;
        self.objects.sync_all()?;
        self.staging.sync_all()?;
        Ok(())
    }
    pub fn cleanup(&self, request: CleanupRequest) -> Result<CleanupOutcome> {
        let mut lease = match self.acquire(request.key, true, true) {
            Err(BlobError::Busy) => {
                return Ok(CleanupOutcome {
                    key: request.key,
                    state: CleanupState::Busy,
                });
            }
            result => result?,
        };
        let name = key_name(request.key);
        lease.retired()?;
        let stage = optional_regular(&self.staging, &name, &[0o600, 0o400])?;
        let object = self.open_object(request.key)?;
        for file in [&stage, &object].into_iter().flatten() {
            if file.metadata()?.modified()? > request.older_than {
                return Ok(CleanupOutcome {
                    key: request.key,
                    state: CleanupState::TooRecent,
                });
            }
        }
        // This durable tombstone is deliberately retained: a caller whose task
        // starts late cannot recreate a physically deleted key after GC ack.
        lease.file.seek(SeekFrom::Start(0))?;
        lease.file.write_all(b"R")?;
        lease.file.set_len(1)?;
        lease.file.sync_all()?;
        self.keys.sync_all()?;
        if let Some(file) = &stage {
            same_entry(&self.staging, &name, file)?;
            unlink(&self.staging, &name)?;
        }
        if let Some(file) = &object {
            same_entry(&self.objects, &name, file)?;
            unlink(&self.objects, &name)?;
        }
        self.staging.sync_all()?;
        self.objects.sync_all()?;
        Ok(CleanupOutcome {
            key: request.key,
            state: if stage.is_some() || object.is_some() {
                CleanupState::Removed
            } else {
                CleanupState::Missing
            },
        })
    }
}
impl KeyLease {
    fn retired(&self) -> Result<bool> {
        let mut bytes = [0_u8; 2];
        let mut file = &self.file;
        file.seek(SeekFrom::Start(0))?;
        let count = file.read(&mut bytes)?;
        match &bytes[..count] {
            [] => Ok(false),
            b"R" => Ok(true),
            _ => Err(BlobError::UnsafeEntry),
        }
    }
}
impl Stage {
    pub fn create(directories: Arc<Directories>, lease: KeyLease) -> Result<(Self, File)> {
        let name = key_name(lease.key);
        // An exclusive key lease proves no live adapter owns this old staging
        // inode. A killed process may leave it; never follow or modify links.
        if let Some(old) = optional_regular(&directories.staging, &name, &[0o600, 0o400])? {
            same_entry(&directories.staging, &name, &old)?;
            unlink(&directories.staging, &name)?;
        }
        let file = open_at(
            &directories.staging,
            &name,
            libc::O_RDWR | libc::O_CREAT | libc::O_EXCL,
            0o600,
        )?;
        regular(&file, &[0o600])?;
        Ok((
            Self {
                directories,
                lease,
                remove_on_drop: true,
            },
            file,
        ))
    }
    pub fn publish(mut self, file: File) -> Result<()> {
        let name = key_name(self.lease.key);
        regular(&file, &[0o600])?;
        same_entry(&self.directories.staging, &name, &file)?;
        file.sync_all()?;
        file.set_permissions(fs::Permissions::from_mode(0o400))?;
        file.sync_all()?;
        // Linux no-replace rename publishes one name atomically; unlike linkat
        // publication this never creates a second hard link to accepted bytes.
        // SAFETY: both descriptors and NUL-terminated generated names are live.
        let result = unsafe {
            libc::renameat2(
                self.directories.staging.as_raw_fd(),
                name.as_ptr(),
                self.directories.objects.as_raw_fd(),
                name.as_ptr(),
                libc::RENAME_NOREPLACE,
            )
        };
        if result != 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        self.remove_on_drop = false;
        // If either sync fails the outcome remains uncertain. The immutable
        // orphan is preserved and a retry must verify it, never overwrite it.
        self.directories.objects.sync_all()?;
        self.directories.staging.sync_all()?;
        Ok(())
    }
}
impl Drop for Stage {
    fn drop(&mut self) {
        if self.remove_on_drop {
            // Private directory + held exclusive key lease makes unlink safe;
            // unlinks never follow the final component. Crash leftovers remain
            // known by the reservation and are reclaimed by authorized cleanup.
            let _ = unlink(&self.directories.staging, &key_name(self.lease.key));
        }
    }
}

fn key_name(key: BlobKey) -> CString {
    CString::new(key.to_string()).expect("UUID has no NUL")
}
fn open_at(parent: &File, name: &CStr, flags: i32, mode: u32) -> Result<File> {
    // SAFETY: parent is open and name is NUL terminated; ownership of a returned
    // descriptor is transferred exactly once into File.
    let fd = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            flags | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK,
            mode as libc::mode_t,
        )
    };
    if fd < 0 {
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::ELOOP) {
            return Err(BlobError::UnsafeEntry);
        }
        return Err(error.into());
    }
    Ok(unsafe { File::from_raw_fd(fd) })
}
fn optional_regular(parent: &File, name: &CStr, modes: &[u32]) -> Result<Option<File>> {
    match open_at(parent, name, libc::O_RDONLY, 0) {
        Ok(file) => {
            regular(&file, modes)?;
            same_entry(parent, name, &file)?;
            Ok(Some(file))
        }
        Err(BlobError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}
fn regular(file: &File, modes: &[u32]) -> Result<()> {
    let metadata = file.metadata()?;
    // SAFETY: geteuid has no pointer arguments or side effects.
    let uid = unsafe { libc::geteuid() };
    if !metadata.is_file()
        || metadata.nlink() != 1
        || metadata.uid() != uid
        || !modes.contains(&(metadata.mode() & 0o7777))
    {
        return Err(BlobError::UnsafeEntry);
    }
    Ok(())
}
fn same_entry(parent: &File, name: &CStr, file: &File) -> Result<()> {
    let metadata = file.metadata()?;
    let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
    // SAFETY: fstatat initializes stat on success; AT_SYMLINK_NOFOLLOW prevents
    // resolving a substituted symlink. Only initialized fields are read.
    if unsafe {
        libc::fstatat(
            parent.as_raw_fd(),
            name.as_ptr(),
            stat.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    } != 0
    {
        return Err(std::io::Error::last_os_error().into());
    }
    let stat = unsafe { stat.assume_init() };
    if stat.st_dev != metadata.dev()
        || stat.st_ino != metadata.ino()
        || stat.st_nlink != 1
        || stat.st_mode & libc::S_IFMT != libc::S_IFREG
    {
        return Err(BlobError::UnsafeEntry);
    }
    Ok(())
}
fn unlink(parent: &File, name: &CStr) -> Result<()> {
    // SAFETY: live directory descriptor and generated NUL-terminated filename.
    if unsafe { libc::unlinkat(parent.as_raw_fd(), name.as_ptr(), 0) } != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(())
}
fn private_directory(file: &File) -> Result<()> {
    let metadata = file.metadata()?;
    // SAFETY: geteuid has no pointer arguments.
    if !metadata.is_dir()
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.mode() & 0o7777 != 0o700
    {
        return Err(BlobError::Configuration(
            "blob directories must be owned by this UID with mode 0700",
        ));
    }
    Ok(())
}
fn private_child(parent: &File, name: &CStr) -> Result<File> {
    // SAFETY: live descriptor, fixed constant filename, and valid permission bits.
    if unsafe { libc::mkdirat(parent.as_raw_fd(), name.as_ptr(), 0o700) } != 0 {
        let error = std::io::Error::last_os_error();
        if error.kind() != std::io::ErrorKind::AlreadyExists {
            return Err(error.into());
        }
    }
    let file = open_at(parent, name, libc::O_RDONLY | libc::O_DIRECTORY, 0)?;
    private_directory(&file)?;
    Ok(file)
}
fn open_directory_chain(path: &Path) -> Result<File> {
    let mut directory = File::open("/")?;
    for component in path.components() {
        match component {
            Component::RootDir => continue,
            Component::Normal(name) => {
                use std::os::unix::ffi::OsStrExt;
                let name = CString::new(name.as_bytes())
                    .map_err(|_| BlobError::Configuration("NUL in root path"))?;
                directory = open_at(&directory, &name, libc::O_RDONLY | libc::O_DIRECTORY, 0)?;
                let metadata = directory.metadata()?;
                // Root-owned sticky /tmp is safe to traverse: the private final
                // directory is owned by the serving UID and pinned by descriptor.
                let root_sticky = metadata.uid() == 0 && metadata.mode() & 0o1000 != 0;
                let trusted_owner =
                    metadata.uid() == 0 || metadata.uid() == unsafe { libc::geteuid() };
                if !trusted_owner || (metadata.mode() & 0o022 != 0 && !root_sticky) {
                    return Err(BlobError::Configuration(
                        "unsafe storage ancestor ownership or permissions",
                    ));
                }
            }
            _ => {
                return Err(BlobError::Configuration(
                    "root path must not traverse parent directories",
                ));
            }
        }
    }
    Ok(directory)
}
