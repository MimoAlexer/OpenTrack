//! Local NTFS storage. Pinned handles deny deletion/renaming of path ancestors.
use super::{
    BlobError, BlobKey, CleanupOutcome, CleanupRequest, CleanupState, LocalBlobConfig, Result,
};
use std::{
    ffi::c_void,
    fs::{self, File, OpenOptions, TryLockError},
    io::{Read, Seek, SeekFrom, Write},
    mem::size_of,
    os::windows::{
        ffi::OsStrExt,
        fs::OpenOptionsExt,
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
    },
    path::{Component, Path, PathBuf, Prefix},
    ptr::{null, null_mut},
    sync::Arc,
};
use windows_sys::Win32::{
    Foundation::{GENERIC_ALL, GENERIC_READ, GENERIC_WRITE, HANDLE, LocalFree},
    Security::{
        ACCESS_ALLOWED_ACE, ACE_HEADER, ACL,
        Authorization::{
            ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
            GetSecurityInfo, SE_FILE_OBJECT, SetSecurityInfo,
        },
        DACL_SECURITY_INFORMATION, EqualSid, GetAce, GetLengthSid, GetSecurityDescriptorDacl,
        GetTokenInformation, IsWellKnownSid, OWNER_SECURITY_INFORMATION,
        PROTECTED_DACL_SECURITY_INFORMATION, PSID, TOKEN_QUERY, TOKEN_USER, TokenUser,
        WinBuiltinAdministratorsSid, WinLocalSystemSid,
    },
    Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, DELETE, FILE_APPEND_DATA, FILE_ATTRIBUTE_DIRECTORY,
        FILE_ATTRIBUTE_REPARSE_POINT, FILE_DISPOSITION_FLAG_DELETE,
        FILE_DISPOSITION_FLAG_POSIX_SEMANTICS, FILE_DISPOSITION_INFO_EX,
        FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_FLAG_WRITE_THROUGH,
        FILE_GENERIC_READ, FILE_RENAME_INFO, FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_WRITE_DATA,
        FileDispositionInfoEx, FileRenameInfo, GetFileInformationByHandle,
        GetFinalPathNameByHandleW, READ_CONTROL, SetFileInformationByHandle, WRITE_DAC,
    },
    System::{
        SystemServices::{ACCESS_ALLOWED_ACE_TYPE, ACCESS_DENIED_ACE_TYPE},
        Threading::{GetCurrentProcess, OpenProcessToken},
    },
};

pub(super) struct Directories {
    // Every component remains pinned, including the volume root. Without
    // FILE_SHARE_DELETE neither a directory nor a junction can replace it.
    _ancestors: Vec<File>,
    keys: Directory,
    staging: Directory,
    objects: Directory,
}
struct Directory {
    file: File,
    path: PathBuf,
}
pub(super) struct KeyLease {
    pub key: BlobKey,
    file: File,
}
pub(super) struct Stage {
    directories: Arc<Directories>,
    lease: KeyLease,
    file: File,
    remove_on_drop: bool,
}

impl Directories {
    pub fn open(config: &LocalBlobConfig) -> Result<Self> {
        if !config.root.is_absolute() || config.forbidden_roots.is_empty() {
            return Err(BlobError::Configuration(
                "absolute root and checkout/webroot exclusions are required",
            ));
        }
        let mut ancestors = open_directory_chain(&config.root, true)?;
        let root_file = ancestors.pop().ok_or(BlobError::UnsafeEntry)?;
        private(&root_file, false)?;
        let root = Directory {
            path: final_path(&root_file)?,
            file: root_file,
        };
        let location = folded(&root.path);
        for excluded in &config.forbidden_roots {
            if !excluded.is_absolute() {
                return Err(BlobError::Configuration("exclusions must be absolute"));
            }
            let excluded = folded(&fs::canonicalize(excluded)?);
            if location.starts_with(&excluded) || excluded.starts_with(&location) {
                return Err(BlobError::Configuration(
                    "blob root overlaps checkout or webroot",
                ));
            }
        }
        if root
            .path
            .ancestors()
            .any(|parent| parent.join(".git").exists())
        {
            return Err(BlobError::Configuration(
                "blob root is inside a Git checkout",
            ));
        }
        let keys = private_child(&root, "keys")?;
        let staging = private_child(&root, "staging")?;
        let objects = private_child(&root, "objects")?;
        root.file.sync_all()?;
        ancestors.push(root.file);
        Ok(Self {
            _ancestors: ancestors,
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
        let path = self.keys.path.join(key.to_string());
        let mut created = false;
        let access = GENERIC_READ | GENERIC_WRITE | READ_CONTROL;
        let file = if create {
            match open_file(&path, access, FILE_SHARE_READ | FILE_SHARE_WRITE, true) {
                Ok(file) => {
                    created = true;
                    file
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    open_file(&path, access, FILE_SHARE_READ | FILE_SHARE_WRITE, false)?
                }
                Err(error) => return Err(error.into()),
            }
        } else {
            match open_file(&path, access, FILE_SHARE_READ | FILE_SHARE_WRITE, false) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    return Err(BlobError::IntegrityMissing);
                }
                result => result?,
            }
        };
        regular(&file, false)?;
        let result = if exclusive {
            file.try_lock()
        } else {
            file.try_lock_shared()
        };
        match result {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => return Err(BlobError::Busy),
            Err(TryLockError::Error(error)) => return Err(error.into()),
        }
        same_entry(&path, &file)?;
        if created {
            file.sync_all()?;
            self.keys.file.sync_all()?;
        }
        Ok(KeyLease { key, file })
    }
    pub fn open_object(&self, key: BlobKey) -> Result<Option<File>> {
        optional_regular(&self.objects.path.join(key.to_string()), true, false)
    }
    pub fn confirm_durable(&self, key: BlobKey, file: &File) -> Result<()> {
        same_entry(&self.objects.path.join(key.to_string()), file)?;
        // Bytes were flushed before the immutable rename. Windows requires
        // write access for FlushFileBuffers, which published objects deny.
        // Complete the potentially interrupted directory flushes on retry.
        self.objects.file.sync_all()?;
        self.staging.file.sync_all()?;
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
        lease.retired()?;
        let name = request.key.to_string();
        let stage = optional_regular(&self.staging.path.join(&name), false, true)?;
        let object = optional_regular(&self.objects.path.join(&name), true, true)?;
        for file in [&stage, &object].into_iter().flatten() {
            if file.metadata()?.modified()? > request.older_than {
                return Ok(CleanupOutcome {
                    key: request.key,
                    state: CleanupState::TooRecent,
                });
            }
        }
        lease.file.seek(SeekFrom::Start(0))?;
        lease.file.write_all(b"R")?;
        lease.file.set_len(1)?;
        lease.file.sync_all()?;
        self.keys.file.sync_all()?;
        let existed = stage.is_some() || object.is_some();
        for file in [stage, object].into_iter().flatten() {
            delete(&file)?;
            drop(file);
        }
        self.staging.file.sync_all()?;
        self.objects.file.sync_all()?;
        Ok(CleanupOutcome {
            key: request.key,
            state: if existed {
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
        let path = directories.staging.path.join(lease.key.to_string());
        if let Some(old) = optional_regular(&path, false, true)? {
            delete(&old)?;
            drop(old);
        }
        let file = open_file(
            &path,
            GENERIC_READ | GENERIC_WRITE | READ_CONTROL | WRITE_DAC | DELETE,
            FILE_SHARE_READ,
            true,
        )?;
        regular(&file, false)?;
        let retained = file.try_clone()?;
        Ok((
            Self {
                directories,
                lease,
                file: retained,
                remove_on_drop: true,
            },
            file,
        ))
    }
    pub fn publish(mut self, file: File) -> Result<()> {
        regular(&file, false)?;
        same_entry(
            &self
                .directories
                .staging
                .path
                .join(self.lease.key.to_string()),
            &file,
        )?;
        file.sync_all()?;
        make_immutable(&file)?;
        file.sync_all()?;
        rename(
            &file,
            &self
                .directories
                .objects
                .path
                .join(self.lease.key.to_string()),
        )?;
        self.remove_on_drop = false;
        self.directories.objects.file.sync_all()?;
        self.directories.staging.file.sync_all()?;
        Ok(())
    }
}
impl Drop for Stage {
    fn drop(&mut self) {
        if self.remove_on_drop {
            // A retained duplicate identifies exactly this staged file even
            // while a cancelled Tokio write is still holding its other handle.
            let _ = delete(&self.file);
        }
    }
}

fn open_file(path: &Path, access: u32, sharing: u32, create: bool) -> std::io::Result<File> {
    OpenOptions::new()
        .write(create)
        .access_mode(access)
        .share_mode(sharing)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_WRITE_THROUGH)
        .create_new(create)
        .open(path)
}
fn directory(path: &Path, writable: bool) -> Result<File> {
    let access = GENERIC_READ | READ_CONTROL | if writable { GENERIC_WRITE } else { 0 };
    let file = OpenOptions::new()
        .access_mode(access)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .custom_flags(
            FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_WRITE_THROUGH,
        )
        .open(path)?;
    let info = information(&file)?;
    if info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
        || info.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY == 0
    {
        return Err(BlobError::UnsafeEntry);
    }
    Ok(file)
}
fn private_child(parent: &Directory, name: &str) -> Result<Directory> {
    let path = parent.path.join(name);
    match fs::create_dir(&path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error.into()),
    }
    let file = directory(&path, true)?;
    private(&file, false)?;
    Ok(Directory { file, path })
}
fn open_directory_chain(path: &Path, writable_last: bool) -> Result<Vec<File>> {
    let components: Vec<_> = path.components().collect();
    if !matches!(components.first(), Some(Component::Prefix(prefix)) if matches!(prefix.kind(), Prefix::Disk(_) | Prefix::VerbatimDisk(_)))
    {
        return Err(BlobError::Configuration(
            "Windows blob storage requires a local drive path",
        ));
    }
    let mut current = PathBuf::new();
    let mut handles = Vec::new();
    for (index, component) in components.iter().enumerate() {
        match component {
            Component::Prefix(_) => {
                current.push(component.as_os_str());
                continue;
            }
            Component::RootDir | Component::Normal(_) => current.push(component.as_os_str()),
            _ => {
                return Err(BlobError::Configuration(
                    "root path must not traverse parent directories",
                ));
            }
        }
        handles.push(directory(
            &current,
            writable_last && index + 1 == components.len(),
        )?);
    }
    Ok(handles)
}
pub(crate) fn open_private_file(path: &Path) -> Result<File> {
    if !path.is_absolute()
        || path
            .components()
            .any(|part| matches!(part, Component::ParentDir))
    {
        return Err(BlobError::UnsafeEntry);
    }
    let parent = path.parent().ok_or(BlobError::UnsafeEntry)?;
    let name = path.file_name().ok_or(BlobError::UnsafeEntry)?;
    let handles = open_directory_chain(parent, false)?;
    let parent = final_path(handles.last().ok_or(BlobError::UnsafeEntry)?)?;
    let path = parent.join(name);
    let file = open_file(&path, GENERIC_READ | READ_CONTROL, FILE_SHARE_READ, false)?;
    regular(&file, false)?;
    same_entry(&path, &file)?;
    Ok(file)
}
fn optional_regular(path: &Path, immutable: bool, deleting: bool) -> Result<Option<File>> {
    let access = GENERIC_READ | READ_CONTROL | if deleting { DELETE } else { 0 };
    match open_file(path, access, FILE_SHARE_READ, false) {
        Ok(file) => {
            regular(&file, immutable)?;
            same_entry(path, &file)?;
            Ok(Some(file))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}
fn information(file: &File) -> Result<BY_HANDLE_FILE_INFORMATION> {
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: live file handle and an initialized correctly sized output.
    if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(info)
}
fn regular(file: &File, immutable: bool) -> Result<()> {
    let info = information(file)?;
    if info.dwFileAttributes & (FILE_ATTRIBUTE_DIRECTORY | FILE_ATTRIBUTE_REPARSE_POINT) != 0
        || info.nNumberOfLinks != 1
    {
        return Err(BlobError::UnsafeEntry);
    }
    private(file, immutable)
}
fn same_entry(path: &Path, file: &File) -> Result<()> {
    if folded(&final_path(file)?) != folded(path) || information(file)?.nNumberOfLinks != 1 {
        return Err(BlobError::UnsafeEntry);
    }
    Ok(())
}
fn folded(path: &Path) -> PathBuf {
    PathBuf::from(path.as_os_str().to_string_lossy().to_lowercase())
}
fn final_path(file: &File) -> Result<PathBuf> {
    let mut buffer = vec![0_u16; 32768];
    // SAFETY: buffer spans its reported length and the handle remains open.
    let count = unsafe {
        GetFinalPathNameByHandleW(
            file.as_raw_handle(),
            buffer.as_mut_ptr(),
            buffer.len() as u32,
            0,
        )
    } as usize;
    if count == 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    if count >= buffer.len() {
        return Err(BlobError::UnsafeEntry);
    }
    use std::os::windows::ffi::OsStringExt;
    Ok(PathBuf::from(std::ffi::OsString::from_wide(
        &buffer[..count],
    )))
}
fn delete(file: &File) -> Result<()> {
    let info = FILE_DISPOSITION_INFO_EX {
        Flags: FILE_DISPOSITION_FLAG_DELETE | FILE_DISPOSITION_FLAG_POSIX_SEMANTICS,
    };
    // SAFETY: the live handle has DELETE access; this acts on its exact inode.
    if unsafe {
        SetFileInformationByHandle(
            file.as_raw_handle(),
            FileDispositionInfoEx,
            (&info as *const FILE_DISPOSITION_INFO_EX).cast(),
            size_of::<FILE_DISPOSITION_INFO_EX>() as u32,
        )
    } == 0
    {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(())
}
fn rename(file: &File, target: &Path) -> Result<()> {
    let name: Vec<u16> = target.as_os_str().encode_wide().collect();
    let bytes = size_of::<FILE_RENAME_INFO>() + name.len() * 2;
    let mut buffer = vec![0_usize; bytes.div_ceil(size_of::<usize>())];
    let info = buffer.as_mut_ptr().cast::<FILE_RENAME_INFO>();
    // SAFETY: usize storage supplies sufficient alignment/space for this
    // variable-length Windows structure. The target's directory and all its
    // ancestors are pinned against rename; the source is the exact open file.
    unsafe {
        (*info).Anonymous.ReplaceIfExists = false;
        (*info).RootDirectory = null_mut();
        (*info).FileNameLength = (name.len() * 2) as u32;
        std::ptr::copy_nonoverlapping(name.as_ptr(), (*info).FileName.as_mut_ptr(), name.len());
        if SetFileInformationByHandle(
            file.as_raw_handle(),
            FileRenameInfo,
            info.cast(),
            bytes as u32,
        ) == 0
        {
            return Err(std::io::Error::last_os_error().into());
        }
    }
    Ok(())
}

struct LocalAllocation(*mut c_void);
impl Drop for LocalAllocation {
    fn drop(&mut self) {
        // SAFETY: all instances wrap an allocation explicitly returned by a
        // Windows API documented to transfer LocalFree-compatible ownership.
        unsafe {
            LocalFree(self.0);
        }
    }
}
struct Identity {
    sid: Vec<u32>,
    text: String,
}
impl Identity {
    fn current() -> Result<Self> {
        let mut token: HANDLE = null_mut();
        // SAFETY: APIs receive live process/token handles and sized buffers.
        unsafe {
            if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
                return Err(std::io::Error::last_os_error().into());
            }
            let token = OwnedHandle::from_raw_handle(token);
            let mut length = 0;
            GetTokenInformation(token.as_raw_handle(), TokenUser, null_mut(), 0, &mut length);
            let mut buffer = vec![0_usize; (length as usize).div_ceil(size_of::<usize>())];
            if GetTokenInformation(
                token.as_raw_handle(),
                TokenUser,
                buffer.as_mut_ptr().cast(),
                length,
                &mut length,
            ) == 0
            {
                return Err(std::io::Error::last_os_error().into());
            }
            let user = &*buffer.as_ptr().cast::<TOKEN_USER>();
            let length = GetLengthSid(user.User.Sid) as usize;
            let mut sid = vec![0_u32; length.div_ceil(4)];
            std::ptr::copy_nonoverlapping(
                user.User.Sid.cast::<u8>(),
                sid.as_mut_ptr().cast(),
                length,
            );
            let mut text = null_mut();
            if ConvertSidToStringSidW(sid.as_mut_ptr().cast(), &mut text) == 0 {
                return Err(std::io::Error::last_os_error().into());
            }
            let _allocation = LocalAllocation(text.cast());
            let mut count = 0;
            while *text.add(count) != 0 {
                count += 1;
            }
            Ok(Self {
                sid,
                text: String::from_utf16_lossy(std::slice::from_raw_parts(text, count)),
            })
        }
    }
    fn pointer(&self) -> PSID {
        self.sid.as_ptr().cast_mut().cast()
    }
}
fn private(file: &File, immutable: bool) -> Result<()> {
    let identity = Identity::current()?;
    let mut owner = null_mut();
    let mut acl: *mut ACL = null_mut();
    let mut descriptor = null_mut();
    // SAFETY: returned pointers belong to descriptor and are used before its
    // LocalFree guard is dropped. The OS validates the self-relative descriptor.
    unsafe {
        let status = GetSecurityInfo(
            file.as_raw_handle(),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            &mut owner,
            null_mut(),
            &mut acl,
            null_mut(),
            &mut descriptor,
        );
        if status != 0 {
            return Err(std::io::Error::from_raw_os_error(status as i32).into());
        }
        let _allocation = LocalAllocation(descriptor);
        if owner.is_null() || acl.is_null() || EqualSid(owner, identity.pointer()) == 0 {
            return Err(BlobError::Configuration(
                "Windows blob entries must have a private service-owned DACL",
            ));
        }
        for index in 0..(*acl).AceCount {
            let mut ace = null_mut();
            if GetAce(acl, u32::from(index), &mut ace) == 0 {
                return Err(BlobError::UnsafeEntry);
            }
            let header = &*ace.cast::<ACE_HEADER>();
            if u32::from(header.AceType) == ACCESS_DENIED_ACE_TYPE {
                continue;
            }
            if u32::from(header.AceType) != ACCESS_ALLOWED_ACE_TYPE {
                return Err(BlobError::UnsafeEntry);
            }
            let allowed = &*ace.cast::<ACCESS_ALLOWED_ACE>();
            let sid = (&allowed.SidStart as *const u32).cast_mut().cast();
            let trusted = IsWellKnownSid(sid, WinLocalSystemSid) != 0
                || IsWellKnownSid(sid, WinBuiltinAdministratorsSid) != 0;
            if !trusted
                && (EqualSid(sid, identity.pointer()) == 0
                    || (immutable
                        && allowed.Mask
                            & (FILE_WRITE_DATA | FILE_APPEND_DATA | GENERIC_WRITE | GENERIC_ALL)
                            != 0))
            {
                return Err(BlobError::Configuration(
                    "Windows blob entries must have a private service-owned DACL",
                ));
            }
        }
    }
    Ok(())
}
fn make_immutable(file: &File) -> Result<()> {
    let identity = Identity::current()?;
    let rights = FILE_GENERIC_READ | READ_CONTROL | WRITE_DAC | DELETE;
    let sddl = format!(
        "D:P(A;;0x{rights:08x};;;{})(A;;FA;;;SY)(A;;FA;;;BA)",
        identity.text
    );
    let text: Vec<u16> = sddl.encode_utf16().chain(Some(0)).collect();
    let mut descriptor = null_mut();
    // SAFETY: a NUL-terminated SDDL and correctly typed output parameters. The
    // DACL is copied by SetSecurityInfo before its owning descriptor is freed.
    unsafe {
        if ConvertStringSecurityDescriptorToSecurityDescriptorW(
            text.as_ptr(),
            1,
            &mut descriptor,
            null_mut(),
        ) == 0
        {
            return Err(std::io::Error::last_os_error().into());
        }
        let _allocation = LocalAllocation(descriptor);
        let mut present = 0;
        let mut defaulted = 0;
        let mut acl = null_mut();
        if GetSecurityDescriptorDacl(descriptor, &mut present, &mut acl, &mut defaulted) == 0
            || present == 0
            || acl.is_null()
        {
            return Err(BlobError::UnsafeEntry);
        }
        let status = SetSecurityInfo(
            file.as_raw_handle(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            null_mut(),
            null_mut(),
            acl,
            null(),
        );
        if status != 0 {
            return Err(std::io::Error::from_raw_os_error(status as i32).into());
        }
    }
    Ok(())
}
