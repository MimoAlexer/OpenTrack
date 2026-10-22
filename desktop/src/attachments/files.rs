use super::{MAX_ATTACHMENT_BYTES, NativeError, file_error, hex, valid_name};
use sha2::{Digest, Sha256};
use std::{
    ffi::{CStr, CString},
    fs::{File, OpenOptions},
    io::Seek,
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::{
            ffi::OsStrExt,
            fs::{FileExt, MetadataExt, OpenOptionsExt},
        },
    },
    path::{Component, PathBuf},
};
use uuid::Uuid;

pub(super) fn pick(path: PathBuf) -> Result<(File, String, u64, String), NativeError> {
    if !path.is_absolute() {
        return Err(file_error());
    }
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| valid_name(name))
        .ok_or_else(file_error)?
        .to_owned();
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
        .open(path)
        .map_err(|_| file_error())?;
    let metadata = file.metadata().map_err(|_| file_error())?;
    if !metadata.is_file()
        || metadata.len() == 0
        || metadata.len() > MAX_ATTACHMENT_BYTES
        || metadata.nlink() != 1
    {
        return Err(file_error());
    }
    let digest = hash(&mut file, metadata.len())?;
    let after = file.metadata().map_err(|_| file_error())?;
    if after.len() != metadata.len() || after.modified().ok() != metadata.modified().ok() {
        return Err(file_error());
    }
    file.rewind().map_err(|_| file_error())?;
    Ok((file, name, metadata.len(), digest))
}
pub(super) fn clone_unchanged(file: &File, size: u64, digest: &str) -> Result<File, NativeError> {
    let mut copy = file.try_clone().map_err(|_| file_error())?;
    copy.rewind().map_err(|_| file_error())?;
    let metadata = copy.metadata().map_err(|_| file_error())?;
    if !metadata.is_file()
        || metadata.len() != size
        || metadata.nlink() != 1
        || hash(&mut copy, size)? != digest
    {
        return Err(NativeError::new(
            "selected_file_changed",
            "The selected file changed. Select it again before uploading.",
            false,
        ));
    }
    copy.rewind().map_err(|_| file_error())?;
    Ok(copy)
}
fn hash(file: &mut File, expected: u64) -> Result<String, NativeError> {
    let mut buffer = [0_u8; 64 * 1024];
    let mut size = 0_u64;
    let mut hash = Sha256::new();
    loop {
        // File clones share an OS cursor. Hash without moving that cursor so a
        // cancelled hashing task cannot disturb a later upload using this handle.
        let count = file.read_at(&mut buffer, size).map_err(|_| file_error())?;
        if count == 0 {
            break;
        }
        size += count as u64;
        if size > expected {
            return Err(file_error());
        }
        hash.update(&buffer[..count]);
    }
    if size != expected {
        return Err(file_error());
    }
    Ok(hex(&hash.finalize()))
}

pub(super) struct DownloadTarget {
    parent: File,
    temporary: CString,
    destination: CString,
    published: bool,
}
impl DownloadTarget {
    pub fn create(path: PathBuf) -> Result<(Self, File), NativeError> {
        if !path.is_absolute() {
            return Err(file_error());
        }
        let name = path.file_name().ok_or_else(file_error)?;
        let destination = CString::new(name.as_bytes()).map_err(|_| file_error())?;
        let mut parent = File::open("/").map_err(|_| file_error())?;
        for component in path.parent().ok_or_else(file_error)?.components() {
            match component {
                Component::RootDir => continue,
                Component::Normal(name) => {
                    let name = CString::new(name.as_bytes()).map_err(|_| file_error())?;
                    parent = open_at(&parent, &name, libc::O_RDONLY | libc::O_DIRECTORY, 0)?;
                    let meta = parent.metadata().map_err(|_| file_error())?;
                    let owner = meta.uid() == 0 || meta.uid() == unsafe { libc::geteuid() };
                    let sticky = meta.uid() == 0 && meta.mode() & 0o1000 != 0;
                    if !owner || (meta.mode() & 0o022 != 0 && !sticky) {
                        return Err(file_error());
                    }
                }
                _ => return Err(file_error()),
            }
        }
        let temporary = CString::new(format!(".opentrack-download-{}.part", Uuid::new_v4()))
            .expect("UUID has no NUL");
        let file = open_at(
            &parent,
            &temporary,
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL,
            0o600,
        )?;
        Ok((
            Self {
                parent,
                temporary,
                destination,
                published: false,
            },
            file,
        ))
    }
    pub fn prepare(self, file: File) -> Result<Self, NativeError> {
        let metadata = file.metadata().map_err(|_| file_error())?;
        if !metadata.is_file() || metadata.nlink() != 1 || metadata.mode() & 0o7777 != 0o600 {
            return Err(file_error());
        }
        let entry = open_at(&self.parent, &self.temporary, libc::O_RDONLY, 0)?;
        if entry.metadata().map_err(|_| file_error())?.ino() != metadata.ino() {
            return Err(file_error());
        }
        file.sync_all().map_err(|_| file_error())?;
        Ok(self)
    }
    pub fn publish(mut self) -> Result<(), NativeError> {
        // No replacement of an existing destination, even after a chooser's
        // overwrite prompt: a concurrent file must never be silently destroyed.
        let result = unsafe {
            libc::renameat2(
                self.parent.as_raw_fd(),
                self.temporary.as_ptr(),
                self.parent.as_raw_fd(),
                self.destination.as_ptr(),
                libc::RENAME_NOREPLACE,
            )
        };
        if result != 0 {
            return Err(
                if std::io::Error::last_os_error().kind() == std::io::ErrorKind::AlreadyExists {
                    NativeError::new(
                        "destination_exists",
                        "A file already exists at that destination. Choose a new filename.",
                        false,
                    )
                } else {
                    file_error()
                },
            );
        }
        self.published = true;
        self.parent.sync_all().map_err(|_| NativeError::new("download_save_uncertain", "The file was written, but its durability could not be confirmed. Check the destination before retrying.", false))?;
        Ok(())
    }
}
impl Drop for DownloadTarget {
    fn drop(&mut self) {
        if !self.published {
            unsafe {
                libc::unlinkat(self.parent.as_raw_fd(), self.temporary.as_ptr(), 0);
            }
        }
    }
}
fn open_at(parent: &File, name: &CStr, flags: i32, mode: u32) -> Result<File, NativeError> {
    let descriptor = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            flags | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK,
            mode as libc::mode_t,
        )
    };
    if descriptor < 0 {
        return Err(file_error());
    }
    Ok(unsafe { File::from_raw_fd(descriptor) })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        io::Write,
        os::unix::fs::{DirBuilderExt, symlink},
    };
    #[test]
    fn private_destination_verifies_before_publish_and_never_overwrites() {
        let root = std::env::temp_dir().join(format!("ot-download-{}", Uuid::new_v4()));
        fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
        let output = root.join("chosen.txt");
        let (target, mut file) = DownloadTarget::create(output.clone()).unwrap();
        file.write_all(b"validated").unwrap();
        target.prepare(file).unwrap().publish().unwrap();
        assert_eq!(fs::read(&output).unwrap(), b"validated");
        assert_eq!(fs::metadata(&output).unwrap().mode() & 0o7777, 0o600);
        let (target, mut file) = DownloadTarget::create(output.clone()).unwrap();
        file.write_all(b"replacement").unwrap();
        assert_eq!(
            target.prepare(file).unwrap().publish().unwrap_err().code,
            "destination_exists"
        );
        assert_eq!(fs::read(&output).unwrap(), b"validated");
        assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
        let alias = root.join("alias");
        symlink(&root, &alias).unwrap();
        assert!(DownloadTarget::create(alias.join("bad")).is_err());
        fs::remove_dir_all(root).unwrap();
    }
}
