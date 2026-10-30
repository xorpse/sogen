use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use sogen_sys::linux::ffi;

use crate::error::Error;
use crate::machine::{Backend, native_path, native_string};

const DEFAULT_ENVIRONMENT: [(&str, &str); 3] = [
    ("PATH", "/usr/bin:/bin"),
    ("HOME", "/root"),
    ("TERM", "xterm"),
];

/// A host directory the guest sees at `guest_path`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mount {
    host_path: PathBuf,
    guest_path: PathBuf,
    read_only: bool,
}

impl Mount {
    /// A writable mount of `host_path` at `guest_path`.
    pub fn new(host_path: impl Into<PathBuf>, guest_path: impl Into<PathBuf>) -> Self {
        Self {
            host_path: host_path.into(),
            guest_path: guest_path.into(),
            read_only: false,
        }
    }

    pub fn host_path(&self) -> &Path {
        &self.host_path
    }

    pub fn set_host_path(&mut self, host_path: impl Into<PathBuf>) {
        self.host_path = host_path.into();
    }

    pub fn with_host_path(mut self, host_path: impl Into<PathBuf>) -> Self {
        self.set_host_path(host_path);
        self
    }

    pub fn guest_path(&self) -> &Path {
        &self.guest_path
    }

    pub fn set_guest_path(&mut self, guest_path: impl Into<PathBuf>) {
        self.guest_path = guest_path.into();
    }

    pub fn with_guest_path(mut self, guest_path: impl Into<PathBuf>) -> Self {
        self.set_guest_path(guest_path);
        self
    }

    pub fn read_only(&self) -> bool {
        self.read_only
    }

    pub fn set_read_only(&mut self, read_only: bool) {
        self.read_only = read_only;
    }

    pub fn with_read_only(mut self, read_only: bool) -> Self {
        self.set_read_only(read_only);
        self
    }

    pub(super) fn to_native(&self) -> Result<ffi::LinuxPathMapping, Error> {
        Ok(ffi::LinuxPathMapping {
            guest: native_path(&self.guest_path)?,
            host: native_path(&self.host_path)?,
            read_only: self.read_only,
        })
    }
}

#[derive(Debug, Clone, Default)]
pub struct Settings {
    backend: Option<Backend>,
    emulation_root: PathBuf,
    mounts: Vec<Mount>,
    port_mappings: BTreeMap<u16, u16>,
    library_paths: Vec<PathBuf>,
    logging: bool,
}

impl Settings {
    pub fn new() -> Self {
        Self::default()
    }

    /// Without a backend, the native default selection applies (Unicorn unless the environment selects another).
    pub fn backend(&self) -> Option<Backend> {
        self.backend
    }

    pub fn set_backend(&mut self, backend: Backend) {
        self.backend = Some(backend);
    }

    pub fn with_backend(mut self, backend: Backend) -> Self {
        self.set_backend(backend);
        self
    }

    /// The host directory the guest sees as `/`. Empty means guest paths are host paths.
    pub fn emulation_root(&self) -> &Path {
        &self.emulation_root
    }

    pub fn set_emulation_root(&mut self, emulation_root: impl Into<PathBuf>) {
        self.emulation_root = emulation_root.into();
    }

    pub fn with_emulation_root(mut self, emulation_root: impl Into<PathBuf>) -> Self {
        self.set_emulation_root(emulation_root);
        self
    }

    pub fn mounts(&self) -> &[Mount] {
        &self.mounts
    }

    pub fn add_mount(&mut self, mount: Mount) {
        self.mounts.push(mount);
    }

    pub fn with_mount(mut self, mount: Mount) -> Self {
        self.add_mount(mount);
        self
    }

    /// Host ports keyed by emulator port.
    pub fn port_mappings(&self) -> &BTreeMap<u16, u16> {
        &self.port_mappings
    }

    /// Redirects guest sockets on `emulator_port` to `host_port`. Both must be non-zero.
    pub fn add_port_mapping(&mut self, emulator_port: u16, host_port: u16) {
        self.port_mappings.insert(emulator_port, host_port);
    }

    pub fn with_port_mapping(mut self, emulator_port: u16, host_port: u16) -> Self {
        self.add_port_mapping(emulator_port, host_port);
        self
    }

    /// Host directories searched for libraries after a module's `DT_RPATH` and before its `DT_RUNPATH`.
    pub fn library_paths(&self) -> &[PathBuf] {
        &self.library_paths
    }

    pub fn add_library_path(&mut self, library_path: impl Into<PathBuf>) {
        self.library_paths.push(library_path.into());
    }

    pub fn with_library_path(mut self, library_path: impl Into<PathBuf>) -> Self {
        self.add_library_path(library_path);
        self
    }

    /// The emulator's own log output, off by default.
    pub fn logging(&self) -> bool {
        self.logging
    }

    pub fn set_logging(&mut self, logging: bool) {
        self.logging = logging;
    }

    pub fn with_logging(mut self, logging: bool) -> Self {
        self.set_logging(logging);
        self
    }

    pub(super) fn to_native(
        &self,
        application: Option<&Application>,
    ) -> Result<ffi::LinuxSettings, Error> {
        let path_mappings = self
            .mounts
            .iter()
            .map(Mount::to_native)
            .collect::<Result<Vec<_>, Error>>()?;

        let port_mappings = self
            .port_mappings
            .iter()
            .map(|(&emulator, &host)| {
                if emulator == 0 || host == 0 {
                    return Err(Error::ZeroPort);
                }
                Ok(ffi::LinuxPortMapping { emulator, host })
            })
            .collect::<Result<Vec<_>, Error>>()?;

        let library_paths = self
            .library_paths
            .iter()
            .map(|path| native_path(path))
            .collect::<Result<Vec<_>, Error>>()?;

        let mut native = ffi::LinuxSettings {
            has_backend: self.backend.is_some(),
            backend: self.backend.unwrap_or(Backend::Unicorn),
            emulation_root: native_path(&self.emulation_root)?,
            path_mappings,
            port_mappings,
            library_paths,
            logging: self.logging,
            has_application: false,
            application: String::new(),
            arguments: Vec::new(),
            environment: Vec::new(),
            working_directory: String::new(),
        };

        if let Some(application) = application {
            application.apply(&mut native)?;
        }

        Ok(native)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvironmentVariable {
    name: String,
    value: String,
}

impl EnvironmentVariable {
    pub(super) fn from_native(entry: String) -> Self {
        match entry.split_once('=') {
            Some((name, value)) => Self {
                name: name.to_owned(),
                value: value.to_owned(),
            },
            None => Self {
                name: entry,
                value: String::new(),
            },
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn value(&self) -> &str {
        &self.value
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Application {
    path: PathBuf,
    arguments: Vec<String>,
    environment: Vec<EnvironmentVariable>,
    working_directory: PathBuf,
}

impl Application {
    /// `path` is resolved inside the emulation root first, then on the host. It also becomes `argv[0]`.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            arguments: Vec::new(),
            environment: Vec::new(),
            working_directory: PathBuf::from("/"),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Arguments after `argv[0]`.
    pub fn arguments(&self) -> &[String] {
        &self.arguments
    }

    pub fn add_argument(&mut self, argument: impl Into<String>) {
        self.arguments.push(argument.into());
    }

    pub fn with_argument(mut self, argument: impl Into<String>) -> Self {
        self.add_argument(argument);
        self
    }

    /// The variables set so far. Without any, the process starts with `PATH=/usr/bin:/bin`, `HOME=/root` and
    /// `TERM=xterm`.
    pub fn environment(&self) -> &[EnvironmentVariable] {
        &self.environment
    }

    /// Replaces the value of a variable that was already set.
    pub fn set_environment_variable(&mut self, name: impl Into<String>, value: impl Into<String>) {
        let name = name.into();
        let value = value.into();
        match self
            .environment
            .iter_mut()
            .find(|variable| variable.name == name)
        {
            Some(variable) => variable.value = value,
            None => self.environment.push(EnvironmentVariable { name, value }),
        }
    }

    pub fn with_environment_variable(
        mut self,
        name: impl Into<String>,
        value: impl Into<String>,
    ) -> Self {
        self.set_environment_variable(name, value);
        self
    }

    /// The guest working directory, `/` by default.
    pub fn working_directory(&self) -> &Path {
        &self.working_directory
    }

    pub fn set_working_directory(&mut self, working_directory: impl Into<PathBuf>) {
        self.working_directory = working_directory.into();
    }

    pub fn with_working_directory(mut self, working_directory: impl Into<PathBuf>) -> Self {
        self.set_working_directory(working_directory);
        self
    }

    fn apply(&self, native: &mut ffi::LinuxSettings) -> Result<(), Error> {
        native.has_application = true;
        native.application = native_path(&self.path)?;
        native.working_directory = native_path(&self.working_directory)?;
        native.arguments = self
            .arguments
            .iter()
            .map(|argument| native_string(argument))
            .collect::<Result<Vec<_>, Error>>()?;
        native.environment = if self.environment.is_empty() {
            DEFAULT_ENVIRONMENT
                .iter()
                .map(|(name, value)| format!("{name}={value}"))
                .collect()
        } else {
            self.environment
                .iter()
                .map(|variable| {
                    if variable.name.is_empty() || variable.name.contains('=') {
                        return Err(Error::environment_variable_name(&variable.name));
                    }
                    native_string(&format!("{}={}", variable.name, variable.value))
                })
                .collect::<Result<Vec<_>, Error>>()?
        };
        Ok(())
    }
}
