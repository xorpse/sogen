use std::fs;
use std::io::{self, Read, Seek, SeekFrom, Write};
#[cfg(unix)]
use std::os::unix::fs::MetadataExt;
use std::path::PathBuf;
use std::time::SystemTime;

use sogen_sys::windows::{FileBridge, FilesystemProviderBridge, ffi};

use super::clock::{system_time, unix_nanoseconds};

/// Metadata of a guest file as its mount reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileInfo {
    is_directory: bool,
    size: u64,
    accessed: SystemTime,
    modified: SystemTime,
    changed: SystemTime,
}

impl FileInfo {
    /// A file of `size` bytes whose times are the Unix epoch.
    pub fn file(size: u64) -> Self {
        Self {
            is_directory: false,
            size,
            accessed: SystemTime::UNIX_EPOCH,
            modified: SystemTime::UNIX_EPOCH,
            changed: SystemTime::UNIX_EPOCH,
        }
    }

    /// A directory whose times are the Unix epoch.
    pub fn directory() -> Self {
        Self {
            is_directory: true,
            ..Self::file(0)
        }
    }

    pub(super) fn from_native(stat: &ffi::FileStat) -> Self {
        Self {
            is_directory: stat.is_directory,
            size: stat.size,
            accessed: system_time(stat.accessed_nanoseconds),
            modified: system_time(stat.modified_nanoseconds),
            changed: system_time(stat.changed_nanoseconds),
        }
    }

    fn from_metadata(metadata: &fs::Metadata) -> io::Result<Self> {
        #[cfg(unix)]
        let changed = system_time(metadata.ctime() * 1_000_000_000 + metadata.ctime_nsec());
        #[cfg(not(unix))]
        let changed = metadata.created()?;

        Ok(Self {
            is_directory: metadata.is_dir(),
            size: metadata.len(),
            accessed: metadata.accessed()?,
            modified: metadata.modified()?,
            changed,
        })
    }

    fn to_native(self) -> ffi::FileStat {
        ffi::FileStat {
            is_directory: self.is_directory,
            size: self.size,
            accessed_nanoseconds: unix_nanoseconds(self.accessed),
            modified_nanoseconds: unix_nanoseconds(self.modified),
            changed_nanoseconds: unix_nanoseconds(self.changed),
        }
    }

    pub fn is_directory(&self) -> bool {
        self.is_directory
    }

    pub fn size(&self) -> u64 {
        self.size
    }

    pub fn accessed(&self) -> SystemTime {
        self.accessed
    }

    pub fn set_accessed(&mut self, accessed: SystemTime) {
        self.accessed = accessed;
    }

    pub fn with_accessed(mut self, accessed: SystemTime) -> Self {
        self.set_accessed(accessed);
        self
    }

    pub fn modified(&self) -> SystemTime {
        self.modified
    }

    pub fn set_modified(&mut self, modified: SystemTime) {
        self.modified = modified;
    }

    pub fn with_modified(mut self, modified: SystemTime) -> Self {
        self.set_modified(modified);
        self
    }

    /// The last change of the file's metadata.
    pub fn changed(&self) -> SystemTime {
        self.changed
    }

    pub fn set_changed(&mut self, changed: SystemTime) {
        self.changed = changed;
    }

    pub fn with_changed(mut self, changed: SystemTime) -> Self {
        self.set_changed(changed);
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectoryEntry {
    name: String,
    short_name: Option<String>,
    is_directory: bool,
    info: Option<FileInfo>,
}

impl DirectoryEntry {
    pub fn new(name: impl Into<String>, is_directory: bool) -> Self {
        Self {
            name: name.into(),
            short_name: None,
            is_directory,
            info: None,
        }
    }

    pub(super) fn from_native(entry: ffi::DirectoryEntryInfo) -> Self {
        Self {
            name: entry.name,
            short_name: (!entry.short_name.is_empty()).then_some(entry.short_name),
            is_directory: entry.is_directory,
            info: entry.has_stat.then(|| FileInfo::from_native(&entry.stat)),
        }
    }

    fn to_native(&self) -> ffi::DirectoryEntryInfo {
        ffi::DirectoryEntryInfo {
            name: self.name.clone(),
            short_name: self.short_name.clone().unwrap_or_default(),
            is_directory: self.is_directory,
            has_stat: self.info.is_some(),
            stat: self.info.map(FileInfo::to_native).unwrap_or_default(),
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// The 8.3 name, where the mount has one.
    pub fn short_name(&self) -> Option<&str> {
        self.short_name.as_deref()
    }

    pub fn set_short_name(&mut self, short_name: impl Into<String>) {
        self.short_name = Some(short_name.into());
    }

    pub fn with_short_name(mut self, short_name: impl Into<String>) -> Self {
        self.set_short_name(short_name);
        self
    }

    pub fn is_directory(&self) -> bool {
        self.is_directory
    }

    pub fn info(&self) -> Option<&FileInfo> {
        self.info.as_ref()
    }

    pub fn set_info(&mut self, info: FileInfo) {
        self.info = Some(info);
    }

    pub fn with_info(mut self, info: FileInfo) -> Self {
        self.set_info(info);
        self
    }
}

/// A guest file system mounted with [`Context::mount_provider`](super::Context::mount_provider). Paths are relative to
/// the mount point, lower case, `\`-separated and without `.` or `..` components; the mount point itself is the empty
/// path. The provider is called with the emulator's kernel lock held, or while the emulator is stopped, and must not
/// use the emulator.
pub trait FilesystemProvider: Send + Sync + 'static {
    fn file_info(&self, path: &str) -> Option<FileInfo>;
    /// An error lists the directory as empty.
    fn read_directory(&self, path: &str) -> io::Result<Vec<DirectoryEntry>>;
    fn open(&self, path: &str, mode: OpenMode) -> io::Result<Box<dyn File>>;
    fn create_directory(&self, path: &str) -> io::Result<()>;
    fn rename(&self, from: &str, to: &str) -> io::Result<()>;
    fn remove(&self, path: &str) -> io::Result<()>;
    /// Executables, DLLs and other files the loader maps need a host path, which must be valid UTF-8.
    fn host_path(&self, path: &str) -> Option<PathBuf> {
        let _ = path;
        None
    }
}

/// An open guest file. Read and write errors reach the guest as short transfers.
pub trait File: Read + Write + Seek + Send + 'static {
    fn set_len(&mut self, size: u64) -> io::Result<()>;
    fn info(&mut self) -> io::Result<FileInfo>;
}

impl File for fs::File {
    fn set_len(&mut self, size: u64) -> io::Result<()> {
        fs::File::set_len(self, size)
    }

    fn info(&mut self) -> io::Result<FileInfo> {
        FileInfo::from_metadata(&self.metadata()?)
    }
}

/// The C `fopen` modes the guest file layer uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OpenMode {
    /// `a`: writes append, creating the file.
    Append,
    /// `a+`: reads anywhere, writes append, creating the file.
    AppendRead,
    /// `r`: reads an existing file.
    Read,
    /// `r+`: reads and writes an existing file.
    ReadWrite,
    /// `w`: writes, creating or truncating the file.
    Write,
    /// `w+`: reads and writes, creating or truncating the file.
    WriteRead,
}

pub(super) struct ProviderAdapter<P>(pub(super) P);

impl<P: FilesystemProvider> FilesystemProviderBridge for ProviderAdapter<P> {
    fn file_info(&self, path: &str, stat: &mut ffi::FileStat) -> bool {
        let Some(info) = self.0.file_info(path) else {
            return false;
        };
        *stat = info.to_native();
        true
    }

    fn read_directory(&self, path: &str) -> Vec<ffi::DirectoryEntryInfo> {
        self.0
            .read_directory(path)
            .map(|entries| entries.iter().map(DirectoryEntry::to_native).collect())
            .unwrap_or_default()
    }

    fn open(&self, path: &str, mode: ffi::OpenMode) -> Result<Box<dyn FileBridge>, ffi::FileError> {
        let mode = match mode {
            ffi::OpenMode::Append => OpenMode::Append,
            ffi::OpenMode::AppendRead => OpenMode::AppendRead,
            ffi::OpenMode::Read => OpenMode::Read,
            ffi::OpenMode::ReadWrite => OpenMode::ReadWrite,
            ffi::OpenMode::Write => OpenMode::Write,
            ffi::OpenMode::WriteRead => OpenMode::WriteRead,
            _ => unreachable!("the shim passes only declared open modes"),
        };
        match self.0.open(path, mode) {
            Ok(file) => Ok(Box::new(FileAdapter(file))),
            Err(error) => Err(file_error(&error)),
        }
    }

    fn create_directory(&self, path: &str) -> ffi::FileError {
        file_status(self.0.create_directory(path))
    }

    fn rename(&self, from: &str, to: &str) -> ffi::FileError {
        file_status(self.0.rename(from, to))
    }

    fn remove(&self, path: &str) -> ffi::FileError {
        file_status(self.0.remove(path))
    }

    fn host_path(&self, path: &str, host_path: &mut String) -> bool {
        let Some(Ok(path)) = self
            .0
            .host_path(path)
            .map(|path| path.into_os_string().into_string())
        else {
            return false;
        };
        *host_path = path;
        true
    }
}

struct FileAdapter(Box<dyn File>);

impl FileBridge for FileAdapter {
    fn read(&mut self, data: &mut [u8]) -> usize {
        let mut filled = 0;
        while filled < data.len() {
            match self.0.read(&mut data[filled..]) {
                Ok(0) => break,
                Ok(count) => filled += count,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(_) => break,
            }
        }
        filled
    }

    fn write(&mut self, data: &[u8]) -> usize {
        let mut written = 0;
        while written < data.len() {
            match self.0.write(&data[written..]) {
                Ok(0) => break,
                Ok(count) => written += count,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(_) => break,
            }
        }
        written
    }

    fn flush(&mut self) -> bool {
        self.0.flush().is_ok()
    }

    fn seek(&mut self, offset: i64, origin: ffi::SeekOrigin) -> bool {
        let position = match origin {
            ffi::SeekOrigin::Start => match u64::try_from(offset) {
                Ok(offset) => SeekFrom::Start(offset),
                Err(_) => return false,
            },
            ffi::SeekOrigin::Current => SeekFrom::Current(offset),
            ffi::SeekOrigin::End => SeekFrom::End(offset),
            _ => unreachable!("the shim passes only declared seek origins"),
        };
        self.0.seek(position).is_ok()
    }

    fn tell(&mut self) -> i64 {
        self.0
            .stream_position()
            .ok()
            .and_then(|position| i64::try_from(position).ok())
            .unwrap_or(-1)
    }

    fn size(&mut self) -> i64 {
        let mut measure = || -> io::Result<u64> {
            let position = self.0.stream_position()?;
            let size = self.0.seek(SeekFrom::End(0))?;
            self.0.seek(SeekFrom::Start(position))?;
            Ok(size)
        };
        measure()
            .ok()
            .and_then(|size| i64::try_from(size).ok())
            .unwrap_or(-1)
    }

    fn resize(&mut self, size: u64) -> bool {
        self.0.set_len(size).is_ok()
    }

    fn stat(&mut self, stat: &mut ffi::FileStat) -> bool {
        let Ok(info) = self.0.info() else {
            return false;
        };
        *stat = info.to_native();
        true
    }
}

fn file_status(result: io::Result<()>) -> ffi::FileError {
    match result {
        Ok(()) => ffi::FileError::Success,
        Err(error) => file_error(&error),
    }
}

fn file_error(error: &io::Error) -> ffi::FileError {
    match error.kind() {
        io::ErrorKind::NotFound => ffi::FileError::NotFound,
        io::ErrorKind::PermissionDenied => ffi::FileError::PermissionDenied,
        io::ErrorKind::AlreadyExists => ffi::FileError::AlreadyExists,
        io::ErrorKind::IsADirectory => ffi::FileError::IsADirectory,
        io::ErrorKind::NotADirectory => ffi::FileError::NotADirectory,
        io::ErrorKind::DirectoryNotEmpty => ffi::FileError::DirectoryNotEmpty,
        _ => ffi::FileError::Other,
    }
}
