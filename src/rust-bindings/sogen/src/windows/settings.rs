use std::collections::BTreeMap;
use std::net::IpAddr;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use sogen_sys::windows::{
    Clock as NativeClock, DnsResolver as NativeDnsResolver, SocketFactory as NativeSocketFactory,
    ffi,
};

use super::clock::{Clock, ClockAdapter};
use super::network::{SocketFactory, SocketFactoryAdapter};
use crate::error::Error;
use crate::machine::{Backend, native_path, native_string};

pub use sogen_sys::windows::ffi::{Architecture, FilesystemMode, NetworkMode};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AddressFamily {
    Ipv4,
    Ipv6,
}

/// Resolves a guest host name. Returning `None` defers to the resolver of the emulator's [`NetworkMode`]; an empty
/// list answers that the host has no addresses.
pub type DnsResolver = dyn Fn(&str, Option<AddressFamily>) -> Option<Vec<IpAddr>> + Send + Sync;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mount {
    host_path: PathBuf,
    guest_path: String,
    mode: FilesystemMode,
}

impl Mount {
    /// A read-only mount of `host_path` at the Windows path `guest_path`, such as `C:\data`.
    pub fn new(host_path: impl Into<PathBuf>, guest_path: impl Into<String>) -> Self {
        Self {
            host_path: host_path.into(),
            guest_path: guest_path.into(),
            mode: FilesystemMode::ReadOnly,
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

    pub fn guest_path(&self) -> &str {
        &self.guest_path
    }

    pub fn set_guest_path(&mut self, guest_path: impl Into<String>) {
        self.guest_path = guest_path.into();
    }

    pub fn with_guest_path(mut self, guest_path: impl Into<String>) -> Self {
        self.set_guest_path(guest_path);
        self
    }

    pub fn mode(&self) -> FilesystemMode {
        self.mode
    }

    pub fn set_mode(&mut self, mode: FilesystemMode) {
        self.mode = mode;
    }

    pub fn with_mode(mut self, mode: FilesystemMode) -> Self {
        self.set_mode(mode);
        self
    }

    pub(super) fn to_native(&self) -> Result<ffi::MountSpec, Error> {
        Ok(ffi::MountSpec {
            host_path: native_path(&self.host_path)?,
            guest_path: native_string(&self.guest_path)?,
            mode: self.mode,
        })
    }
}

#[derive(Clone)]
pub struct Settings {
    backend: Option<Backend>,
    logging: bool,
    use_relative_time: bool,
    use_instruction_precision: bool,
    intercept_rdtsc: bool,
    intercept_kusd: bool,
    emulation_root: PathBuf,
    registry_directory: PathBuf,
    load_registry: bool,
    port_mappings: BTreeMap<u16, u16>,
    root_mode: FilesystemMode,
    mounts: Vec<Mount>,
    number_of_processors: u32,
    nt_product_type: u8,
    network: NetworkMode,
    spoof_gpu_features: bool,
    dns_resolver: Option<Arc<DnsResolver>>,
    clock: Option<Arc<dyn Clock>>,
    socket_factory: Option<Arc<dyn SocketFactory>>,
    vcpu_count: NonZeroU32,
    child_processes: Option<ChildProcesses>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            backend: None,
            logging: true,
            use_relative_time: false,
            use_instruction_precision: true,
            intercept_rdtsc: true,
            intercept_kusd: true,
            emulation_root: PathBuf::new(),
            registry_directory: PathBuf::from("./registry"),
            load_registry: true,
            port_mappings: BTreeMap::new(),
            root_mode: FilesystemMode::ReadOnly,
            mounts: Vec::new(),
            number_of_processors: 4,
            nt_product_type: 1,
            network: NetworkMode::Offline,
            spoof_gpu_features: false,
            dns_resolver: None,
            clock: None,
            socket_factory: None,
            vcpu_count: NonZeroU32::MIN,
            child_processes: None,
        }
    }
}

impl Settings {
    pub fn new() -> Self {
        Self::default()
    }

    /// Without a backend, the native default for the guest architecture is used.
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

    /// Whether the emulator writes its own log output, which it does by default.
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

    pub fn use_relative_time(&self) -> bool {
        self.use_relative_time
    }

    pub fn set_use_relative_time(&mut self, use_relative_time: bool) {
        self.use_relative_time = use_relative_time;
    }

    pub fn with_use_relative_time(mut self, use_relative_time: bool) -> Self {
        self.set_use_relative_time(use_relative_time);
        self
    }

    pub fn use_instruction_precision(&self) -> bool {
        self.use_instruction_precision
    }

    pub fn set_use_instruction_precision(&mut self, use_instruction_precision: bool) {
        self.use_instruction_precision = use_instruction_precision;
    }

    pub fn with_use_instruction_precision(mut self, use_instruction_precision: bool) -> Self {
        self.set_use_instruction_precision(use_instruction_precision);
        self
    }

    pub fn intercept_rdtsc(&self) -> bool {
        self.intercept_rdtsc
    }

    pub fn set_intercept_rdtsc(&mut self, intercept_rdtsc: bool) {
        self.intercept_rdtsc = intercept_rdtsc;
    }

    pub fn with_intercept_rdtsc(mut self, intercept_rdtsc: bool) -> Self {
        self.set_intercept_rdtsc(intercept_rdtsc);
        self
    }

    pub fn intercept_kusd(&self) -> bool {
        self.intercept_kusd
    }

    pub fn set_intercept_kusd(&mut self, intercept_kusd: bool) {
        self.intercept_kusd = intercept_kusd;
    }

    pub fn with_intercept_kusd(mut self, intercept_kusd: bool) -> Self {
        self.set_intercept_kusd(intercept_kusd);
        self
    }

    /// The directory holding `filesys` and `registry`.
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

    /// Only used without an emulation root.
    pub fn registry_directory(&self) -> &Path {
        &self.registry_directory
    }

    pub fn set_registry_directory(&mut self, registry_directory: impl Into<PathBuf>) {
        self.registry_directory = registry_directory.into();
    }

    pub fn with_registry_directory(mut self, registry_directory: impl Into<PathBuf>) -> Self {
        self.set_registry_directory(registry_directory);
        self
    }

    pub fn load_registry(&self) -> bool {
        self.load_registry
    }

    pub fn set_load_registry(&mut self, load_registry: bool) {
        self.load_registry = load_registry;
    }

    pub fn with_load_registry(mut self, load_registry: bool) -> Self {
        self.set_load_registry(load_registry);
        self
    }

    /// Emulator ports and the host ports they map to.
    pub fn port_mappings(&self) -> &BTreeMap<u16, u16> {
        &self.port_mappings
    }

    /// Guest sockets using `emulator_port` use `host_port` on the host. Both must be non-zero.
    pub fn add_port_mapping(&mut self, emulator_port: u16, host_port: u16) {
        self.port_mappings.insert(emulator_port, host_port);
    }

    pub fn with_port_mapping(mut self, emulator_port: u16, host_port: u16) -> Self {
        self.add_port_mapping(emulator_port, host_port);
        self
    }

    pub fn root_mode(&self) -> FilesystemMode {
        self.root_mode
    }

    pub fn set_root_mode(&mut self, root_mode: FilesystemMode) {
        self.root_mode = root_mode;
    }

    pub fn with_root_mode(mut self, root_mode: FilesystemMode) -> Self {
        self.set_root_mode(root_mode);
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

    pub fn number_of_processors(&self) -> u32 {
        self.number_of_processors
    }

    pub fn set_number_of_processors(&mut self, number_of_processors: u32) {
        self.number_of_processors = number_of_processors;
    }

    pub fn with_number_of_processors(mut self, number_of_processors: u32) -> Self {
        self.set_number_of_processors(number_of_processors);
        self
    }

    /// The `NtProductType` of `KUSER_SHARED_DATA`: 1 for a workstation, 2 for a domain controller, 3 for a server.
    pub fn nt_product_type(&self) -> u8 {
        self.nt_product_type
    }

    pub fn set_nt_product_type(&mut self, nt_product_type: u8) {
        self.nt_product_type = nt_product_type;
    }

    pub fn with_nt_product_type(mut self, nt_product_type: u8) -> Self {
        self.set_nt_product_type(nt_product_type);
        self
    }

    pub fn network(&self) -> NetworkMode {
        self.network
    }

    pub fn set_network(&mut self, network: NetworkMode) {
        self.network = network;
    }

    pub fn with_network(mut self, network: NetworkMode) -> Self {
        self.set_network(network);
        self
    }

    pub fn spoof_gpu_features(&self) -> bool {
        self.spoof_gpu_features
    }

    pub fn set_spoof_gpu_features(&mut self, spoof_gpu_features: bool) {
        self.spoof_gpu_features = spoof_gpu_features;
    }

    pub fn with_spoof_gpu_features(mut self, spoof_gpu_features: bool) -> Self {
        self.set_spoof_gpu_features(spoof_gpu_features);
        self
    }

    pub fn dns_resolver(&self) -> Option<&DnsResolver> {
        self.dns_resolver.as_deref()
    }

    pub fn set_dns_resolver(
        &mut self,
        resolver: impl Fn(&str, Option<AddressFamily>) -> Option<Vec<IpAddr>> + Send + Sync + 'static,
    ) {
        self.dns_resolver = Some(Arc::new(resolver));
    }

    pub fn with_dns_resolver(
        mut self,
        resolver: impl Fn(&str, Option<AddressFamily>) -> Option<Vec<IpAddr>> + Send + Sync + 'static,
    ) -> Self {
        self.set_dns_resolver(resolver);
        self
    }

    /// The guest's source of time in place of the host clocks; it takes precedence over relative time and makes the
    /// guest's timestamp counter come from the clock. Emulators built from these settings share it. Snapshots do not
    /// capture it: restoring one keeps the clock's current time.
    pub fn clock(&self) -> Option<&dyn Clock> {
        self.clock.as_deref()
    }

    pub fn set_clock(&mut self, clock: impl Clock) {
        self.clock = Some(Arc::new(clock));
    }

    pub fn with_clock(mut self, clock: impl Clock) -> Self {
        self.set_clock(clock);
        self
    }

    /// Creates the guest's IPv4 and IPv6 stream and datagram sockets; other sockets and those the factory declines
    /// come from the factory of the [`NetworkMode`]. Emulators built from these settings share it. Snapshots do not
    /// capture sockets: restoring one asks the factory for fresh sockets of the saved family and kind, without their
    /// addresses, connections or buffered data.
    pub fn socket_factory(&self) -> Option<&dyn SocketFactory> {
        self.socket_factory.as_deref()
    }

    pub fn set_socket_factory(&mut self, factory: impl SocketFactory) {
        self.socket_factory = Some(Arc::new(factory));
    }

    pub fn with_socket_factory(mut self, factory: impl SocketFactory) -> Self {
        self.set_socket_factory(factory);
        self
    }

    /// The virtual CPUs running guest threads in parallel. Only backends that support several accept more than one,
    /// and those reject instruction precision, relative time, instruction budgets and snapshots.
    pub fn vcpu_count(&self) -> NonZeroU32 {
        self.vcpu_count
    }

    pub fn set_vcpu_count(&mut self, vcpu_count: NonZeroU32) {
        self.vcpu_count = vcpu_count;
    }

    pub fn with_vcpu_count(mut self, vcpu_count: NonZeroU32) -> Self {
        self.set_vcpu_count(vcpu_count);
        self
    }

    /// Without child processes, guest attempts to create a process fail with `STATUS_NOT_SUPPORTED`.
    pub fn child_processes(&self) -> Option<&ChildProcesses> {
        self.child_processes.as_ref()
    }

    pub fn set_child_processes(&mut self, child_processes: ChildProcesses) {
        self.child_processes = Some(child_processes);
    }

    pub fn with_child_processes(mut self, child_processes: ChildProcesses) -> Self {
        self.set_child_processes(child_processes);
        self
    }

    pub(super) fn to_native(&self) -> Result<ffi::EmulatorSettings, Error> {
        let mounts = self
            .mounts
            .iter()
            .map(Mount::to_native)
            .collect::<Result<Vec<_>, Error>>()?;
        let port_mappings = self
            .port_mappings
            .iter()
            .map(|(&emulator_port, &host_port)| {
                if emulator_port == 0 || host_port == 0 {
                    return Err(Error::ZeroPort);
                }

                Ok(ffi::PortMapping {
                    emulator_port,
                    host_port,
                })
            })
            .collect::<Result<Vec<_>, Error>>()?;
        let (child_executable, child_arguments) = match &self.child_processes {
            Some(child_processes) => (
                native_path(&child_processes.executable)?,
                child_processes
                    .arguments
                    .iter()
                    .map(|argument| native_string(argument))
                    .collect::<Result<Vec<_>, Error>>()?,
            ),
            None => (String::new(), Vec::new()),
        };

        Ok(ffi::EmulatorSettings {
            has_backend: self.backend.is_some(),
            backend: self.backend.unwrap_or(Backend::Unicorn),
            logging: self.logging,
            use_relative_time: self.use_relative_time,
            use_instruction_precision: self.use_instruction_precision,
            intercept_rdtsc: self.intercept_rdtsc,
            intercept_kusd: self.intercept_kusd,
            emulation_root: native_path(&self.emulation_root)?,
            registry_directory: native_path(&self.registry_directory)?,
            load_registry: self.load_registry,
            port_mappings,
            root_mode: self.root_mode,
            mounts,
            number_of_processors: self.number_of_processors,
            nt_product_type: self.nt_product_type,
            network: self.network,
            spoof_gpu_features: self.spoof_gpu_features,
            vcpu_count: self.vcpu_count.get(),
            child_executable,
            child_arguments,
        })
    }

    pub(super) fn native_dns_resolver(&self) -> Box<NativeDnsResolver> {
        let Some(resolver) = self.dns_resolver.clone() else {
            return NativeDnsResolver::inactive();
        };

        NativeDnsResolver::new(move |hostname, family| {
            let family = match family {
                4 => Some(AddressFamily::Ipv4),
                6 => Some(AddressFamily::Ipv6),
                _ => None,
            };
            resolver(hostname, family)
        })
    }

    pub(super) fn native_clock(&self) -> Box<NativeClock> {
        match self.clock.clone() {
            Some(clock) => NativeClock::new(ClockAdapter(clock)),
            None => NativeClock::inactive(),
        }
    }

    pub(super) fn native_socket_factory(&self) -> Box<NativeSocketFactory> {
        match self.socket_factory.clone() {
            Some(factory) => NativeSocketFactory::new(SocketFactoryAdapter(factory)),
            None => NativeSocketFactory::inactive(),
        }
    }
}

/// Runs guest child processes as host processes of `executable`, usually [`std::env::current_exe`]. The child is
/// started with `arguments` followed by the managed-process arguments that
/// [`ManagedProcess::from_arguments`](super::ManagedProcess::from_arguments) recognises, and must hand them to it before
/// doing anything else.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChildProcesses {
    executable: PathBuf,
    arguments: Vec<String>,
}

impl ChildProcesses {
    pub fn new(executable: impl Into<PathBuf>) -> Self {
        Self {
            executable: executable.into(),
            arguments: Vec::new(),
        }
    }

    pub fn executable(&self) -> &Path {
        &self.executable
    }

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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvironmentVariable {
    name: String,
    value: String,
}

impl EnvironmentVariable {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn value(&self) -> &str {
        &self.value
    }
}

/// A Windows executable to run, given by its guest path such as `C:\test-sample.exe`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Application {
    path: String,
    argument0: Option<String>,
    working_directory: Option<String>,
    arguments: Vec<String>,
    command_line: Option<String>,
    environment: Vec<EnvironmentVariable>,
    process_id: Option<u32>,
    thread_id: Option<u32>,
}

impl Application {
    pub fn new(path: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            argument0: None,
            working_directory: None,
            arguments: Vec::new(),
            command_line: None,
            environment: Vec::new(),
            process_id: None,
            thread_id: None,
        }
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    /// Replaces the program name the guest sees as its first argument.
    pub fn argument0(&self) -> Option<&str> {
        self.argument0.as_deref()
    }

    pub fn set_argument0(&mut self, argument0: impl Into<String>) {
        self.argument0 = Some(argument0.into());
    }

    pub fn with_argument0(mut self, argument0: impl Into<String>) -> Self {
        self.set_argument0(argument0);
        self
    }

    pub fn working_directory(&self) -> Option<&str> {
        self.working_directory.as_deref()
    }

    pub fn set_working_directory(&mut self, working_directory: impl Into<String>) {
        self.working_directory = Some(working_directory.into());
    }

    pub fn with_working_directory(mut self, working_directory: impl Into<String>) -> Self {
        self.set_working_directory(working_directory);
        self
    }

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

    /// Passed verbatim instead of a command line built from the program name and arguments.
    pub fn command_line(&self) -> Option<&str> {
        self.command_line.as_deref()
    }

    pub fn set_command_line(&mut self, command_line: impl Into<String>) {
        self.command_line = Some(command_line.into());
    }

    pub fn with_command_line(mut self, command_line: impl Into<String>) -> Self {
        self.set_command_line(command_line);
        self
    }

    pub fn environment(&self) -> &[EnvironmentVariable] {
        &self.environment
    }

    /// Names compare case-insensitively, as on Windows, so this replaces the value of a variable whose name differs
    /// only in case.
    pub fn set_environment_variable(&mut self, name: impl Into<String>, value: impl Into<String>) {
        let name = name.into();
        let value = value.into();
        match self
            .environment
            .iter_mut()
            .find(|variable| variable.name.eq_ignore_ascii_case(&name))
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

    /// The guest process id; without one, the emulator's default.
    pub fn process_id(&self) -> Option<u32> {
        self.process_id
    }

    pub fn set_process_id(&mut self, process_id: u32) {
        self.process_id = Some(process_id);
    }

    pub fn with_process_id(mut self, process_id: u32) -> Self {
        self.set_process_id(process_id);
        self
    }

    /// The id of the initial thread; without one, the emulator's default.
    pub fn thread_id(&self) -> Option<u32> {
        self.thread_id
    }

    pub fn set_thread_id(&mut self, thread_id: u32) {
        self.thread_id = Some(thread_id);
    }

    pub fn with_thread_id(mut self, thread_id: u32) -> Self {
        self.set_thread_id(thread_id);
        self
    }

    pub(super) fn to_native(&self) -> Result<ffi::ApplicationSettings, Error> {
        if self.path.is_empty() {
            return Err(Error::EmptyApplicationPath);
        }

        Ok(ffi::ApplicationSettings {
            application: native_string(&self.path)?,
            argument0: native_string(self.argument0.as_deref().unwrap_or_default())?,
            working_directory: native_string(
                self.working_directory.as_deref().unwrap_or_default(),
            )?,
            arguments: self
                .arguments
                .iter()
                .map(|argument| native_string(argument))
                .collect::<Result<Vec<_>, Error>>()?,
            has_command_line: self.command_line.is_some(),
            command_line: native_string(self.command_line.as_deref().unwrap_or_default())?,
            environment: self
                .environment
                .iter()
                .map(|variable| {
                    Ok(ffi::EnvironmentVariable {
                        name: native_string(&variable.name)?,
                        value: native_string(&variable.value)?,
                    })
                })
                .collect::<Result<Vec<_>, Error>>()?,
            has_process_id: self.process_id.is_some(),
            process_id: self.process_id.unwrap_or_default(),
            has_thread_id: self.thread_id.is_some(),
            thread_id: self.thread_id.unwrap_or_default(),
        })
    }

    pub(super) fn from_native(application: ffi::ApplicationSettings) -> Self {
        let present = |text: String| (!text.is_empty()).then_some(text);
        Self {
            path: application.application,
            argument0: present(application.argument0),
            working_directory: present(application.working_directory),
            arguments: application.arguments,
            command_line: application
                .has_command_line
                .then_some(application.command_line),
            environment: application
                .environment
                .into_iter()
                .map(|variable| EnvironmentVariable {
                    name: variable.name,
                    value: variable.value,
                })
                .collect(),
            process_id: application.has_process_id.then_some(application.process_id),
            thread_id: application.has_thread_id.then_some(application.thread_id),
        }
    }
}
