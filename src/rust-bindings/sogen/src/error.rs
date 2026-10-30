use std::num::NonZeroU32;
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("the application path is empty")]
    EmptyApplicationPath,
    #[error("{0:?} is not a valid environment variable name")]
    EnvironmentVariableName(String),
    #[error("{0:?} is not `name` or `module!name`")]
    HookKey(String),
    #[error("emulator error: {0}")]
    Native(#[from] cxx::Exception),
    #[error("no snapshot has been saved")]
    NoSnapshot,
    #[error("path is not valid UTF-8: {}", .0.display())]
    NonUtf8Path(PathBuf),
    #[error("{0:?} contains a NUL byte")]
    NulByte(String),
    #[error("the registry key {0:?} does not exist")]
    RegistryKeyNotFound(String),
    #[error("the saved return address at {0:#x} cannot be read")]
    ReturnAddressUnreadable(u64),
    #[error("the saved return address at {0:#x} is zero")]
    ReturnAddressZero(u64),
    #[error("the emulator cannot be stopped from another thread")]
    StopFromAnotherThread,
    #[error("the backend does not support {0} virtual CPUs")]
    UnsupportedVcpuCount(NonZeroU32),
    #[error("the frame pointer is zero, so the caller is unknown")]
    ZeroFramePointer,
    #[error("port 0 cannot be mapped")]
    ZeroPort,
}

impl Error {
    pub(crate) fn environment_variable_name(name: &str) -> Self {
        Self::EnvironmentVariableName(name.to_owned())
    }

    pub(crate) fn hook_key(key: &str) -> Self {
        Self::HookKey(key.to_owned())
    }

    pub(crate) fn non_utf8_path(path: &Path) -> Self {
        Self::NonUtf8Path(path.to_path_buf())
    }

    pub(crate) fn nul_byte(text: &str) -> Self {
        Self::NulByte(text.to_owned())
    }

    pub(crate) fn registry_key_not_found(key: &str) -> Self {
        Self::RegistryKeyNotFound(key.to_owned())
    }
}
