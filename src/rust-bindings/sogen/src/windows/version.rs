use sogen_sys::windows::ffi;

/// The Windows release the emulator presents, as read from the registry of the emulation root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsVersion {
    system_root: String,
    major: u32,
    minor: u32,
    build: u32,
    update_build_revision: u32,
}

impl WindowsVersion {
    pub(super) fn from_native(version: ffi::WindowsVersionInfo) -> Self {
        Self {
            system_root: version.system_root,
            major: version.major,
            minor: version.minor,
            build: version.build,
            update_build_revision: version.update_build_revision,
        }
    }

    /// The guest path of the Windows directory, such as `C:\Windows`.
    pub fn system_root(&self) -> &str {
        &self.system_root
    }

    pub fn major(&self) -> u32 {
        self.major
    }

    pub fn minor(&self) -> u32 {
        self.minor
    }

    pub fn build(&self) -> u32 {
        self.build
    }

    pub fn update_build_revision(&self) -> u32 {
        self.update_build_revision
    }
}
