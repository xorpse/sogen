use std::cell::{Cell, RefCell};
use std::marker::PhantomData;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV6};
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use cxx::UniquePtr;
use sogen_sys::common::CallbackArgs;
use sogen_sys::common::ffi::Machine;
use sogen_sys::windows::ffi::Event;
use sogen_sys::windows::{
    FilesystemProvider as NativeFilesystemProvider, MmioDevice as NativeMmioDevice, ffi,
};

use self::api::ApiHooks;
use self::filesystem::ProviderAdapter;
use self::memory::MmioDeviceAdapter;
use crate::error::Error;
use crate::machine::{
    CallContinuation, ContextPtr, Guest, HookContinuation, LogLevel, MemoryPermission,
    MemoryViolation, Owned, PendingError, Sealed, StopHandle, StopReason, StopTarget, callback,
    log_callback, native_path, native_str,
};

mod api;
mod clock;
mod filesystem;
mod handle;
mod managed;
mod memory;
mod module;
mod network;
mod process;
mod registry;
mod settings;
mod version;

pub use sogen_sys::windows::ffi::{CallingConvention, SocketActivity};

pub use self::api::{ApiCall, ApiSignature};
pub use self::clock::Clock;
pub use self::filesystem::{DirectoryEntry, File, FileInfo, FilesystemProvider, OpenMode};
pub use self::handle::{EventType, HandleInfo, HandleType, ObjectInfo};
pub use self::managed::ManagedProcess;
pub use self::memory::{Allocation, HostMemory, MemoryRegion, MemoryStats, MmioDevice};
pub use self::module::{ExportedSymbol, Import, Module, ModuleOptions, Section};
pub use self::network::{Connection, Datagram, Readiness, Socket, SocketFactory, SocketKind};
pub use self::process::{ProcessInfo, Thread, ThreadStack, WaitState};
pub use self::registry::{RegistryEntry, RegistryValue};
pub use self::settings::{
    AddressFamily, Application, Architecture, ChildProcesses, DnsResolver, EnvironmentVariable,
    FilesystemMode, Mount, NetworkMode, Settings,
};
pub use self::version::WindowsVersion;

pub struct Emulator {
    context: Owned<Context>,
}

impl Emulator {
    /// An emulator without a process, for working with guest memory and state directly.
    pub fn empty(settings: &Settings, architecture: Architecture) -> Result<Self, Error> {
        Self::create(settings, |native, unsupported_vcpu_count| {
            ffi::create_empty_windows_emulator(
                native,
                architecture,
                settings.native_dns_resolver(),
                settings.native_clock(),
                settings.native_socket_factory(),
                unsupported_vcpu_count,
            )
        })
    }

    /// An emulator for `application`, on a guest CPU matching the executable's architecture.
    pub fn application(settings: &Settings, application: &Application) -> Result<Self, Error> {
        let application = application.to_native()?;
        Self::create(settings, |native, unsupported_vcpu_count| {
            ffi::create_windows_application_emulator(
                native,
                &application,
                settings.native_dns_resolver(),
                settings.native_clock(),
                settings.native_socket_factory(),
                unsupported_vcpu_count,
            )
        })
    }

    /// An emulator restored from a snapshot file that [`Context::write_snapshot`] wrote, on a guest CPU matching the
    /// snapshot. Fails if `settings` describe a different host setup (emulation root, root mode, registry, network,
    /// GPU feature spoofing, mounts) than the emulator that wrote it.
    pub fn snapshot(settings: &Settings, path: impl AsRef<Path>) -> Result<Self, Error> {
        let path = native_path(path.as_ref())?;
        Self::create(settings, |native, unsupported_vcpu_count| {
            ffi::create_windows_snapshot_emulator(
                native,
                &path,
                settings.native_dns_resolver(),
                settings.native_clock(),
                settings.native_socket_factory(),
                unsupported_vcpu_count,
            )
        })
    }

    /// An emulator holding the process a Windows minidump captured, on a guest CPU matching the dump.
    pub fn minidump(settings: &Settings, path: impl AsRef<Path>) -> Result<Self, Error> {
        let path = native_path(path.as_ref())?;
        Self::create(settings, |native, unsupported_vcpu_count| {
            ffi::create_windows_minidump_emulator(
                native,
                &path,
                settings.native_dns_resolver(),
                settings.native_clock(),
                settings.native_socket_factory(),
                unsupported_vcpu_count,
            )
        })
    }

    /// An emulator for the guest child process `process` runs, with [`ManagedProcess::application`]. The process is
    /// set up and its inherited pipes installed before this returns, so callbacks registered afterwards miss the
    /// initial module loads. The first run waits until the parent resumes the process, and a run that ends with the
    /// guest exiting reports the exit status to the parent. The parent's requests on the process's memory, such as
    /// `NtReadVirtualMemory`, are served only during a run: between runs they wait, and the parent gives up on one
    /// after five seconds. Snapshots do not capture the connection to the parent.
    pub fn managed_process(settings: &Settings, process: ManagedProcess) -> Result<Self, Error> {
        let process = process.into_native();
        Self::create(settings, |native, unsupported_vcpu_count| {
            ffi::create_windows_managed_process_emulator(
                native,
                process,
                settings.native_dns_resolver(),
                settings.native_clock(),
                settings.native_socket_factory(),
                unsupported_vcpu_count,
            )
        })
    }

    fn create(
        settings: &Settings,
        construct: impl FnOnce(
            &ffi::EmulatorSettings,
            &mut bool,
        ) -> Result<UniquePtr<ffi::WindowsEmulator>, cxx::Exception>,
    ) -> Result<Self, Error> {
        let mut unsupported_vcpu_count = false;
        let native = construct(&settings.to_native()?, &mut unsupported_vcpu_count)?;
        if unsupported_vcpu_count {
            return Err(Error::UnsupportedVcpuCount(settings.vcpu_count()));
        }

        let emulator = Self {
            context: Owned::new(Context {
                native,
                stop: OnceLock::new(),
                api_hooks: ApiHooks::default(),
                pending_error: PendingError::default(),
                host_memory: RefCell::default(),
                not_sync: PhantomData,
            }),
        };
        let context = &*emulator.context;
        let load = callback(context, |context, _| {
            if let Err(error) = context.api_hooks.refresh(context, None) {
                context.fail(error);
            }
            0
        });
        let unload = callback(context, |context, args| {
            if let Err(error) = context.api_hooks.refresh(context, Some(args.values[0])) {
                context.fail(error);
            }
            0
        });
        context.native.observe_modules(load, unload);
        Ok(emulator)
    }

    pub fn run(&mut self) -> Result<(), Error> {
        self.run_instructions(0)
    }

    /// Runs at most `count` instructions; zero means no limit.
    pub fn run_instructions(&mut self, count: u64) -> Result<(), Error> {
        self.context
            .pending_error
            .finish(self.context.native.start(count))
    }

    /// Maps the executable and its dependencies and creates the main thread, unless that already happened.
    pub fn setup_process_if_necessary(&mut self) -> Result<(), Error> {
        self.context
            .pending_error
            .finish(self.context.native.setup_process_if_necessary())
    }

    /// Makes the next run switch the first virtual CPU away from its current thread.
    pub fn yield_thread(&mut self, alertable: bool) -> Result<(), Error> {
        self.context
            .pending_error
            .finish(self.context.native.yield_thread(alertable))
    }

    /// Switches the first virtual CPU's thread. Returns false if no thread is ready to run.
    pub fn perform_thread_switch(&mut self) -> Result<bool, Error> {
        self.context
            .pending_error
            .finish(self.context.native.perform_thread_switch())
    }

    /// Schedules the thread on the first virtual CPU. Returns false if no thread has the id.
    pub fn activate_thread(&mut self, id: u32) -> Result<bool, Error> {
        self.context
            .pending_error
            .finish(self.context.native.activate_thread(id))
    }

    /// Drops every device and host memory mapping; map them again afterwards.
    pub fn restore_snapshot(&mut self) -> Result<(), Error> {
        self.context
            .pending_error
            .finish(self.context.native.restore_snapshot())?;
        self.context.api_hooks.refresh(&self.context, None)
    }

    /// Drops every device and host memory mapping; map them again afterwards.
    pub fn deserialise_state(&mut self, data: impl AsRef<[u8]>) -> Result<(), Error> {
        self.context
            .pending_error
            .finish(self.context.native.deserialize_state(data.as_ref()))?;
        self.context.api_hooks.refresh(&self.context, None)
    }
}

impl Deref for Emulator {
    type Target = Context;

    fn deref(&self) -> &Context {
        &self.context
    }
}

/// Each event has one callback; setting another replaces it, also from inside the callback itself. Callbacks run one at
/// a time holding the emulator's kernel lock, so a callback that blocks stalls every virtual CPU.
pub struct Context {
    native: UniquePtr<ffi::WindowsEmulator>,
    stop: OnceLock<Arc<StopTarget>>,
    api_hooks: ApiHooks,
    pending_error: PendingError,
    /// Declared after `native`, so mapped host memory outlives every mapping of it.
    host_memory: RefCell<Vec<HostMemory>>,
    not_sync: PhantomData<Cell<()>>,
}

impl Context {
    pub fn stop(&self) {
        self.native.stop();
    }

    /// A handle that stops the guest from another thread, where the backend supports that.
    pub fn stop_handle(&self) -> Result<StopHandle, Error> {
        if !self.native.machine().is_stop_thread_safe() {
            return Err(Error::StopFromAnotherThread);
        }

        let context = ContextPtr::new(self);
        let target = self
            .stop
            .get_or_init(|| StopTarget::new(move || context.get().native.stop()));
        Ok(target.handle())
    }

    pub fn last_stop_reason(&self) -> StopReason {
        self.native.last_stop_reason()
    }

    pub fn last_stop_detail(&self) -> String {
        self.native.last_stop_detail()
    }

    pub fn executed_instructions(&self) -> u64 {
        self.native.executed_instructions()
    }

    /// The absolute emulation root, empty if none was configured.
    pub fn emulation_root(&self) -> PathBuf {
        PathBuf::from(self.native.emulation_root())
    }

    /// Keeps a state that [`Emulator::restore_snapshot`] returns to.
    pub fn save_snapshot(&self) -> Result<(), Error> {
        Ok(self.native.save_snapshot()?)
    }

    pub fn serialise_state(&self) -> Result<Vec<u8>, Error> {
        Ok(self.native.serialize_state()?)
    }

    /// Writes a compressed snapshot file that [`Emulator::snapshot`] restores, also in another process.
    pub fn write_snapshot(&self, path: impl AsRef<Path>) -> Result<(), Error> {
        Ok(self.native.write_snapshot(&native_path(path.as_ref())?)?)
    }

    /// Mapping a port to itself removes its mapping.
    pub fn map_port(&self, emulator_port: u16, host_port: u16) -> Result<(), Error> {
        if emulator_port == 0 || host_port == 0 {
            return Err(Error::ZeroPort);
        }

        self.native.map_port(emulator_port, host_port);
        Ok(())
    }

    pub fn host_port(&self, emulator_port: u16) -> Option<u16> {
        let host_port = self.native.host_port(emulator_port);
        (host_port != emulator_port).then_some(host_port)
    }

    pub fn emulator_port(&self, host_port: u16) -> Option<u16> {
        let emulator_port = self.native.emulator_port(host_port);
        (emulator_port != host_port).then_some(emulator_port)
    }

    pub fn process(&self) -> ProcessInfo {
        ProcessInfo::from_native(self.native.process_info())
    }

    pub fn vcpu_count(&self) -> u32 {
        self.native.vcpu_count()
    }

    /// The virtual CPU that raised the running callback; 0 outside callbacks.
    pub fn current_vcpu(&self) -> u32 {
        self.native.current_vcpu()
    }

    /// The thread scheduled on [`Context::current_vcpu`].
    pub fn current_thread(&self) -> Option<Thread> {
        let mut thread = ffi::ThreadInfo::default();
        self.native
            .active_thread(&mut thread)
            .then(|| Thread::from_native(thread))
    }

    /// The id of the thread scheduled on [`Context::current_vcpu`].
    pub fn current_thread_id(&self) -> Option<u32> {
        let mut id = 0;
        self.native.current_thread_id(&mut id).then_some(id)
    }

    /// Every thread the process still holds, terminated ones included until their last handle closes.
    pub fn threads(&self) -> Vec<Thread> {
        self.native
            .threads()
            .into_iter()
            .map(Thread::from_native)
            .collect()
    }

    pub fn thread(&self, id: u32) -> Option<Thread> {
        let mut thread = ffi::ThreadInfo::default();
        self.native
            .thread(id, &mut thread)
            .then(|| Thread::from_native(thread))
    }

    /// The registers of a thread in the format of [`Guest::save_registers`]: live for a thread scheduled on a virtual
    /// CPU, as saved at its last switch for any other. `None` if no thread has the id. Inside a callback, a thread
    /// scheduled on another virtual CPU is running there, and reading its registers fails; a thread another virtual
    /// CPU is switching in or out is waited for, and callbacks on other virtual CPUs may run meanwhile.
    pub fn thread_registers(&self, id: u32) -> Result<Option<Vec<u8>>, Error> {
        let mut registers = Vec::new();
        Ok(self
            .native
            .thread_registers(id, &mut registers)?
            .then_some(registers))
    }

    /// Replaces the registers [`Context::thread_registers`] returns. Returns false if no thread has the id.
    pub fn set_thread_registers(
        &self,
        id: u32,
        registers: impl AsRef<[u8]>,
    ) -> Result<bool, Error> {
        Ok(self.native.set_thread_registers(id, registers.as_ref())?)
    }

    /// Starts a guest thread at `start_address` with `argument`, with the executable's stack reserve. Returns its id.
    pub fn create_thread(
        &self,
        start_address: u64,
        argument: u64,
        suspended: bool,
    ) -> Result<u32, Error> {
        Ok(self
            .native
            .create_thread(start_address, argument, suspended)?)
    }

    /// Ends a thread with the NTSTATUS `exit_status`, as `NtTerminateThread` does. Returns false if no thread has the
    /// id.
    pub fn terminate_thread(&self, id: u32, exit_status: u32) -> Result<bool, Error> {
        Ok(self.native.terminate_thread(id, exit_status)?)
    }

    /// The open kernel object handles of the process.
    pub fn handles(&self) -> Vec<HandleInfo> {
        self.native
            .handles()
            .into_iter()
            .map(HandleInfo::from_native)
            .collect()
    }

    /// `None` for pseudo handles and handles that are not open.
    pub fn handle(&self, value: u64) -> Option<HandleInfo> {
        let mut handle = ffi::HandleRecord::default();
        self.native
            .handle_info(value, &mut handle)
            .then(|| HandleInfo::from_native(handle))
    }

    /// Integer atoms below `0xC000` are named `#` and their value.
    pub fn atom_name(&self, id: u16) -> Option<String> {
        let mut name = String::new();
        self.native.atom_name(id, &mut name).then_some(name)
    }

    /// Compares names ignoring case.
    pub fn find_atom(&self, name: &str) -> Option<u16> {
        let mut id = 0;
        self.native.find_atom(name, &mut id).then_some(id)
    }

    /// Adds a reference to the atom named `name`, creating it if necessary.
    pub fn add_atom(&self, name: &str) -> Result<u16, Error> {
        Ok(self.native.add_atom(name)?)
    }

    /// Drops a reference; the atom goes once none remain. Returns false if no atom has the id.
    pub fn delete_atom(&self, id: u16) -> bool {
        self.native.delete_atom(id)
    }

    pub fn modules(&self) -> Vec<Module> {
        self.native
            .modules()
            .into_iter()
            .map(Module::from_native)
            .collect()
    }

    pub fn find_module_by_address(&self, address: u64) -> Option<Module> {
        let mut module = ffi::ModuleInfo::default();
        self.native
            .find_module_by_address(address, &mut module)
            .then(|| Module::from_native(module))
    }

    /// Compares `name` with [`Module::name`] exactly.
    pub fn find_module_by_name(&self, name: &str) -> Option<Module> {
        let mut module = ffi::ModuleInfo::default();
        self.native
            .find_module_by_name(name, &mut module)
            .then(|| Module::from_native(module))
    }

    /// Maps the image at the guest path `path`, such as `C:\Windows\System32\version.dll`.
    /// Mapping a file that is already mapped returns that module unless [`ModuleOptions::allow_duplicate`] is set.
    pub fn map_module(&self, path: &str, options: &ModuleOptions) -> Result<Module, Error> {
        let mut module = ffi::ModuleInfo::default();
        self.native
            .map_module(native_str(path)?, &options.to_native(), &mut module)?;
        Ok(Module::from_native(module))
    }

    /// Maps the host file `host_path` as if the guest had loaded it from `module_path`.
    pub fn map_local_module(
        &self,
        host_path: impl AsRef<Path>,
        module_path: &str,
        options: &ModuleOptions,
    ) -> Result<Module, Error> {
        let mut module = ffi::ModuleInfo::default();
        self.native.map_local_module(
            &native_path(host_path.as_ref())?,
            native_str(module_path)?,
            &options.to_native(),
            &mut module,
        )?;
        Ok(Module::from_native(module))
    }

    /// Registers the image the guest already holds at `address` as a module named by `module_path`.
    pub fn map_memory_module(
        &self,
        address: u64,
        size: u64,
        module_path: &str,
        options: &ModuleOptions,
    ) -> Result<Module, Error> {
        let mut module = ffi::ModuleInfo::default();
        self.native.map_memory_module(
            address,
            size,
            native_str(module_path)?,
            &options.to_native(),
            &mut module,
        )?;
        Ok(Module::from_native(module))
    }

    /// Returns false if no module has the image base. Static modules stay mapped.
    pub fn unmap_module(&self, image_base: u64) -> Result<bool, Error> {
        Ok(self.native.unmap_module(image_base)?)
    }

    /// How often the file at the guest path `path` is currently mapped; `None` if it is not.
    pub fn module_load_count(&self, path: &str) -> Option<u64> {
        let mut count = 0;
        self.native
            .module_load_count(native_str(path).ok()?, &mut count)
            .then_some(count)
    }

    /// Returns the base address of the new region, or `None` if no suitable range is free.
    pub fn allocate_memory(&self, allocation: &Allocation) -> Result<Option<u64>, Error> {
        let base = self.native.allocate_memory(
            allocation.size(),
            allocation.permissions().into(),
            allocation.reserve_only(),
            allocation.start().unwrap_or(0),
            allocation.kind(),
            allocation.highest_address().unwrap_or(0),
        )?;
        Ok((base != 0).then_some(base))
    }

    /// Allocates exactly at `address`, ignoring the start and highest address of `allocation`. Returns false if the
    /// range is not free or not allocatable.
    pub fn allocate_memory_at(&self, address: u64, allocation: &Allocation) -> Result<bool, Error> {
        Ok(self.native.allocate_memory_at(
            address,
            allocation.size(),
            allocation.permissions().into(),
            allocation.reserve_only(),
            allocation.kind(),
        )?)
    }

    pub fn protect_memory(
        &self,
        address: u64,
        size: u64,
        permissions: MemoryPermission,
    ) -> Result<bool, Error> {
        Ok(self
            .native
            .protect_memory(address, size, permissions.into())?)
    }

    pub fn commit_memory(
        &self,
        address: u64,
        size: u64,
        permissions: MemoryPermission,
    ) -> Result<bool, Error> {
        Ok(self
            .native
            .commit_memory(address, size, permissions.into())?)
    }

    pub fn decommit_memory(&self, address: u64, size: u64) -> Result<bool, Error> {
        Ok(self.native.decommit_memory(address, size)?)
    }

    /// Releases the allocation at `address`; a `size` of zero releases all of it. Device and host memory mappings
    /// can only be released whole.
    pub fn release_memory(&self, address: u64, size: u64) -> Result<bool, Error> {
        Ok(self.native.release_memory(address, size)?)
    }

    /// The lowest free, suitably aligned base at or above `start`, or the default allocation address without one.
    pub fn find_free_allocation_base(&self, size: u64, start: Option<u64>) -> Option<u64> {
        let base = self
            .native
            .find_free_allocation_base(size, start.unwrap_or(0));
        (base != 0).then_some(base)
    }

    /// `None` outside the user-mode address range.
    pub fn memory_region(&self, address: u64) -> Option<MemoryRegion> {
        let mut region = ffi::RegionInfo::default();
        self.native
            .region_info(address, &mut region)
            .then(|| MemoryRegion::from_native(region))
    }

    pub fn memory_stats(&self) -> MemoryStats {
        MemoryStats::from_native(self.native.memory_stats())
    }

    pub fn default_allocation_address(&self) -> u64 {
        self.native.default_allocation_address()
    }

    pub fn set_default_allocation_address(&self, address: u64) {
        self.native.set_default_allocation_address(address);
    }

    /// The reserved and committed regions of guest memory, in address order, without the free ranges between them.
    pub fn memory_regions(&self) -> Vec<MemoryRegion> {
        self.native
            .memory_regions()
            .into_iter()
            .map(MemoryRegion::from_native)
            .collect()
    }

    /// Returns false if the range overlaps an allocation.
    pub fn map_mmio(
        &self,
        address: u64,
        size: u64,
        device: impl MmioDevice,
    ) -> Result<bool, Error> {
        Ok(self.native.map_mmio(
            address,
            size,
            NativeMmioDevice::new(MmioDeviceAdapter(device)),
        )?)
    }

    /// Returns false if the range overlaps an allocation. Fails where the backend cannot map host memory there, or
    /// at all (Icicle). The emulator keeps `memory` alive until it is dropped.
    pub fn map_host_memory_at(
        &self,
        address: u64,
        memory: &HostMemory,
        permissions: MemoryPermission,
    ) -> Result<bool, Error> {
        let mapped = unsafe {
            self.native.map_host_memory_at(
                address,
                memory.size(),
                memory.as_mut_ptr(),
                permissions.into(),
            )
        }?;
        if mapped {
            self.retain_host_memory(memory);
        }

        Ok(mapped)
    }

    /// Returns the guest address chosen, or `None` if no range is free. The emulator keeps `memory` alive until it is
    /// dropped.
    pub fn map_host_memory(
        &self,
        memory: &HostMemory,
        permissions: MemoryPermission,
    ) -> Result<Option<u64>, Error> {
        let address = unsafe {
            self.native
                .map_host_memory(memory.size(), memory.as_mut_ptr(), permissions.into())
        }?;
        if address == 0 {
            return Ok(None);
        }

        self.retain_host_memory(memory);
        Ok(Some(address))
    }

    /// The highest address the guest can allocate at, which the backend may lower below the end of the user address
    /// space.
    pub fn maximum_allocation_address(&self) -> u64 {
        self.native.maximum_allocation_address()
    }

    /// Whether data execution prevention keeps memory without [`MemoryPermission::EXEC`] from executing.
    pub fn dep_enabled(&self) -> bool {
        self.native.dep_enabled()
    }

    pub fn set_dep_enabled(&self, enabled: bool) {
        self.native.set_dep_enabled(enabled);
    }

    /// The guest path of the file mapped at `address`, for image and file views.
    pub fn mapped_filename(&self, address: u64) -> Option<String> {
        let mut filename = String::new();
        self.native
            .mapped_filename(address, &mut filename)
            .then_some(filename)
    }

    /// Reads a value of the key at the registry path `key`, such as `\Registry\Machine\Software\Microsoft`; `None` if
    /// the key has no value named `name`. An empty name reads the default value.
    pub fn registry_value(&self, key: &str, name: &str) -> Result<Option<RegistryValue>, Error> {
        let mut value = ffi::RegistryData::default();
        match self
            .native
            .registry_value(native_str(key)?, native_str(name)?, &mut value)?
        {
            ffi::RegistryLookup::Found => {
                Ok(Some(RegistryValue::from_native(value.kind, value.data)))
            }
            ffi::RegistryLookup::MissingValue => Ok(None),
            _ => Err(Error::registry_key_not_found(key)),
        }
    }

    /// The values of the key at the registry path `key`.
    pub fn registry_values(&self, key: &str) -> Result<Vec<RegistryEntry>, Error> {
        let mut values = Vec::new();
        if !self.native.registry_values(native_str(key)?, &mut values)? {
            return Err(Error::registry_key_not_found(key));
        }

        Ok(values.into_iter().map(RegistryEntry::from_native).collect())
    }

    /// The names of the subkeys of the key at the registry path `key`.
    pub fn registry_subkeys(&self, key: &str) -> Result<Vec<String>, Error> {
        let mut names = Vec::new();
        if !self.native.registry_subkeys(native_str(key)?, &mut names)? {
            return Err(Error::registry_key_not_found(key));
        }

        Ok(names)
    }

    /// Creates the key at the registry path `key` and any missing parents. Returns false if no loaded hive holds the
    /// path. The hives on disk stay unchanged.
    pub fn create_registry_key(&self, key: &str) -> Result<bool, Error> {
        Ok(self.native.create_registry_key(native_str(key)?)?)
    }

    /// Sets a value of an existing key. The hives on disk stay unchanged.
    pub fn set_registry_value(
        &self,
        key: &str,
        name: &str,
        value: &RegistryValue,
    ) -> Result<(), Error> {
        if !self.native.set_registry_value(
            native_str(key)?,
            native_str(name)?,
            value.kind(),
            &value.data(),
        )? {
            return Err(Error::registry_key_not_found(key));
        }

        Ok(())
    }

    /// Imports a `.reg` file into the registry the guest sees.
    pub fn import_registry_file(&self, path: impl AsRef<Path>) -> Result<(), Error> {
        Ok(self
            .native
            .import_registry_file(&native_path(path.as_ref())?)?)
    }

    /// Imports the contents of a `.reg` file, in UTF-8 or UTF-16 with a byte order mark.
    pub fn import_registry(&self, contents: impl AsRef<[u8]>) -> Result<(), Error> {
        Ok(self.native.import_registry(contents.as_ref())?)
    }

    /// The host path behind the guest path `path`; `None` if no mount resolves it.
    pub fn translate_path(&self, path: &str) -> Option<PathBuf> {
        let host_path = self.native.translate_path(native_str(path).ok()?);
        (!host_path.is_empty()).then(|| PathBuf::from(host_path))
    }

    /// The letters of the mounted drives.
    pub fn drives(&self) -> Vec<char> {
        self.native.drives().into_iter().map(char::from).collect()
    }

    /// Whether the mount resolving the guest path `path` is writable.
    pub fn is_path_writable(&self, path: &str) -> bool {
        native_str(path).is_ok_and(|path| self.native.is_path_writable(path))
    }

    pub fn file_info(&self, path: &str) -> Option<FileInfo> {
        let mut stat = ffi::FileStat::default();
        self.native
            .file_info(native_str(path).ok()?, &mut stat)
            .then(|| FileInfo::from_native(&stat))
    }

    /// The entries of the guest directory `path`, mount points within it included.
    pub fn read_directory(&self, path: &str) -> Result<Vec<DirectoryEntry>, Error> {
        Ok(self
            .native
            .read_directory(native_str(path)?)?
            .into_iter()
            .map(DirectoryEntry::from_native)
            .collect())
    }

    /// Mounts a host path while the emulator exists. [`Emulator::snapshot`] then needs settings with the mount.
    pub fn add_mount(&self, mount: &Mount) -> Result<(), Error> {
        Ok(self.native.add_mount(&mount.to_native()?)?)
    }

    /// Mounts `provider` at the absolute guest path, replacing any mount there. Mount before the first run for the
    /// process setup to see it; the executable itself must be reachable through a host mount. Snapshots do not capture
    /// the provider: restoring one reopens the saved files through the provider mounted then and seeks to their saved
    /// positions without undoing the provider's changes, and [`Context::write_snapshot`] does not record the mount.
    pub fn mount_provider(
        &self,
        guest_path: &str,
        mode: FilesystemMode,
        provider: impl FilesystemProvider,
    ) -> Result<(), Error> {
        Ok(self.native.mount_provider(
            native_str(guest_path)?,
            mode,
            NativeFilesystemProvider::new(ProviderAdapter(provider)),
        )?)
    }

    pub fn windows_version(&self) -> WindowsVersion {
        WindowsVersion::from_native(self.native.windows_version())
    }

    /// Whether the emulator writes its own log output.
    pub fn logging(&self) -> bool {
        self.native.logging()
    }

    pub fn set_logging(&self, logging: bool) {
        self.native.set_logging(logging);
    }

    /// Whether the emulator writes nothing at all, not even the errors it prints without [`Context::logging`].
    pub fn silent(&self) -> bool {
        self.native.silent()
    }

    pub fn set_silent(&self, silent: bool) {
        self.native.set_silent(silent);
    }

    /// Calls `callback` whenever an export named like `key` starts executing, with the call and its first
    /// [`ApiSignature::parameter_count`] arguments. `key` is `name` for every module or `module!name`, where
    /// `module` matches a module name with or without its extension, ignoring case. Replaces any hook with the
    /// same key. Hooks at one address run in key order; once one intercepts the call, the rest do not run.
    pub fn hook_api(
        &self,
        key: &str,
        signature: ApiSignature,
        callback: impl Fn(&Context, &mut ApiCall, &[u64]) -> CallContinuation + Send + Sync + 'static,
    ) -> Result<(), Error> {
        self.api_hooks.insert(self, key, signature, callback)
    }

    /// Returns false if no hook had the key.
    pub fn unhook_api(&self, key: &str) -> bool {
        self.api_hooks.remove(self, key)
    }

    pub fn clear_api_hooks(&self) {
        self.api_hooks.clear(self);
    }

    pub fn on_module_load(&self, function: impl Fn(&Context, &Module) + Send + Sync + 'static) {
        let handle = callback(self, move |context, args| {
            if let Some(module) = context.find_module_by_address(args.values[0]) {
                function(context, &module);
            }
            0
        });
        self.native.set_callback(Event::ModuleLoad, handle);
    }

    pub fn clear_on_module_load(&self) {
        self.native
            .set_callback(Event::ModuleLoad, UniquePtr::null());
    }

    /// Runs before the module is unmapped.
    pub fn on_module_unload(&self, function: impl Fn(&Context, &Module) + Send + Sync + 'static) {
        let handle = callback(self, move |context, args| {
            if let Some(module) = context.find_module_by_address(args.values[0]) {
                function(context, &module);
            }
            0
        });
        self.native.set_callback(Event::ModuleUnload, handle);
    }

    pub fn clear_on_module_unload(&self) {
        self.native
            .set_callback(Event::ModuleUnload, UniquePtr::null());
    }

    pub fn on_stdout(&self, function: impl Fn(&Context, &[u8]) + Send + Sync + 'static) {
        let handle = callback(self, move |context, args| {
            function(context, args.first);
            0
        });
        self.native.set_callback(Event::Stdout, handle);
    }

    pub fn clear_on_stdout(&self) {
        self.native.set_callback(Event::Stdout, UniquePtr::null());
    }

    /// Receives the syscall id and name. [`HookContinuation::SkipInstruction`] skips the syscall; any other
    /// continuation runs it.
    pub fn on_syscall(
        &self,
        function: impl Fn(&Context, u32, &str) -> HookContinuation + Send + Sync + 'static,
    ) {
        let handle = callback(self, move |context, args| {
            let name = String::from_utf8_lossy(args.first);
            u64::from(function(context, args.values[0] as u32, &name).repr)
        });
        self.native.set_callback(Event::Syscall, handle);
    }

    pub fn clear_on_syscall(&self) {
        self.native.set_callback(Event::Syscall, UniquePtr::null());
    }

    /// Receives the kind of access, such as `Opening file`, and the name of the object.
    pub fn on_generic_access(
        &self,
        function: impl Fn(&Context, &str, &str) + Send + Sync + 'static,
    ) {
        let handle = callback(self, move |context, args| {
            function(
                context,
                &String::from_utf8_lossy(args.first),
                &String::from_utf8_lossy(args.second),
            );
            0
        });
        self.native.set_callback(Event::GenericAccess, handle);
    }

    pub fn clear_on_generic_access(&self) {
        self.native
            .set_callback(Event::GenericAccess, UniquePtr::null());
    }

    pub fn on_generic_activity(&self, function: impl Fn(&Context, &str) + Send + Sync + 'static) {
        let handle = callback(self, move |context, args| {
            function(context, &String::from_utf8_lossy(args.first));
            0
        });
        self.native.set_callback(Event::GenericActivity, handle);
    }

    pub fn clear_on_generic_activity(&self) {
        self.native
            .set_callback(Event::GenericActivity, UniquePtr::null());
    }

    pub fn on_suspicious_activity(
        &self,
        function: impl Fn(&Context, &str) + Send + Sync + 'static,
    ) {
        let handle = callback(self, move |context, args| {
            function(context, &String::from_utf8_lossy(args.first));
            0
        });
        self.native.set_callback(Event::SuspiciousActivity, handle);
    }

    pub fn clear_on_suspicious_activity(&self) {
        self.native
            .set_callback(Event::SuspiciousActivity, UniquePtr::null());
    }

    /// Receives the address as the guest sees it, with its port mapped back to the emulator port.
    pub fn on_socket_activity(
        &self,
        function: impl Fn(&Context, SocketActivity, SocketAddr) + Send + Sync + 'static,
    ) {
        let handle = callback(self, move |context, args| {
            let activity = SocketActivity {
                repr: args.values[0] as u8,
            };
            function(context, activity, socket_address(args));
            0
        });
        self.native.set_callback(Event::SocketActivity, handle);
    }

    pub fn clear_on_socket_activity(&self) {
        self.native
            .set_callback(Event::SocketActivity, UniquePtr::null());
    }

    pub fn on_exception(&self, function: impl Fn(&Context) + Send + Sync + 'static) {
        let handle = callback(self, move |context, _| {
            function(context);
            0
        });
        self.native.set_callback(Event::Exception, handle);
    }

    pub fn clear_on_exception(&self) {
        self.native
            .set_callback(Event::Exception, UniquePtr::null());
    }

    /// Receives the address of every executed instruction; needs instruction precision.
    pub fn on_instruction(&self, function: impl Fn(&Context, u64) + Send + Sync + 'static) {
        let handle = callback(self, move |context, args| {
            function(context, args.values[0]);
            0
        });
        self.native.set_callback(Event::Instruction, handle);
    }

    pub fn clear_on_instruction(&self) {
        self.native
            .set_callback(Event::Instruction, UniquePtr::null());
    }

    /// Receives the address, length and requested permissions of a guest `NtProtectVirtualMemory`.
    pub fn on_memory_protect(
        &self,
        function: impl Fn(&Context, u64, u64, MemoryPermission) + Send + Sync + 'static,
    ) {
        let handle = callback(self, move |context, args| {
            function(
                context,
                args.values[0],
                args.values[1],
                MemoryPermission::from_bits_retain(args.values[2] as u8),
            );
            0
        });
        self.native.set_callback(Event::MemoryProtect, handle);
    }

    pub fn clear_on_memory_protect(&self) {
        self.native
            .set_callback(Event::MemoryProtect, UniquePtr::null());
    }

    /// Receives the address, length, permissions and whether the memory is committed of a guest
    /// `NtAllocateVirtualMemory`.
    pub fn on_memory_allocate(
        &self,
        function: impl Fn(&Context, u64, u64, MemoryPermission, bool) + Send + Sync + 'static,
    ) {
        let handle = callback(self, move |context, args| {
            function(
                context,
                args.values[0],
                args.values[1],
                MemoryPermission::from_bits_retain(args.values[2] as u8),
                args.values[3] != 0,
            );
            0
        });
        self.native.set_callback(Event::MemoryAllocate, handle);
    }

    pub fn clear_on_memory_allocate(&self) {
        self.native
            .set_callback(Event::MemoryAllocate, UniquePtr::null());
    }

    /// Observes a guest access violation before the emulator raises it in the guest. Use
    /// [`Guest::hook_memory_violation`] to decide how a violation continues.
    pub fn on_memory_violate(
        &self,
        function: impl Fn(&Context, &MemoryViolation) + Send + Sync + 'static,
    ) {
        let handle = callback(self, move |context, args| {
            function(context, &MemoryViolation::from_args(args));
            0
        });
        self.native.set_callback(Event::MemoryViolate, handle);
    }

    pub fn clear_on_memory_violate(&self) {
        self.native
            .set_callback(Event::MemoryViolate, UniquePtr::null());
    }

    pub fn on_rdtsc(&self, function: impl Fn(&Context) + Send + Sync + 'static) {
        let handle = callback(self, move |context, _| {
            function(context);
            0
        });
        self.native.set_callback(Event::Rdtsc, handle);
    }

    pub fn clear_on_rdtsc(&self) {
        self.native.set_callback(Event::Rdtsc, UniquePtr::null());
    }

    pub fn on_rdtscp(&self, function: impl Fn(&Context) + Send + Sync + 'static) {
        let handle = callback(self, move |context, _| {
            function(context);
            0
        });
        self.native.set_callback(Event::Rdtscp, handle);
    }

    pub fn clear_on_rdtscp(&self) {
        self.native.set_callback(Event::Rdtscp, UniquePtr::null());
    }

    /// Receives the device name and the control code of a guest `NtDeviceIoControlFile`.
    pub fn on_ioctrl(&self, function: impl Fn(&Context, &str, u32) + Send + Sync + 'static) {
        let handle = callback(self, move |context, args| {
            function(
                context,
                &String::from_utf8_lossy(args.first),
                args.values[0] as u32,
            );
            0
        });
        self.native.set_callback(Event::Ioctrl, handle);
    }

    pub fn clear_on_ioctrl(&self) {
        self.native.set_callback(Event::Ioctrl, UniquePtr::null());
    }

    /// Receives `OutputDebugString` messages as the guest wrote them.
    pub fn on_debug_string(&self, function: impl Fn(&Context, &[u8]) + Send + Sync + 'static) {
        let handle = callback(self, move |context, args| {
            function(context, args.first);
            0
        });
        self.native.set_callback(Event::DebugString, handle);
    }

    pub fn clear_on_debug_string(&self) {
        self.native
            .set_callback(Event::DebugString, UniquePtr::null());
    }

    /// Receives the thread handle, thread id, start address and argument.
    pub fn on_thread_create(
        &self,
        function: impl Fn(&Context, u64, u32, u64, u64) + Send + Sync + 'static,
    ) {
        let handle = callback(self, move |context, args| {
            function(
                context,
                args.values[0],
                args.values[1] as u32,
                args.values[2],
                args.values[3],
            );
            0
        });
        self.native.set_callback(Event::ThreadCreate, handle);
    }

    pub fn clear_on_thread_create(&self) {
        self.native
            .set_callback(Event::ThreadCreate, UniquePtr::null());
    }

    /// Receives the thread handle and thread id.
    pub fn on_thread_terminated(
        &self,
        function: impl Fn(&Context, u64, u32) + Send + Sync + 'static,
    ) {
        let handle = callback(self, move |context, args| {
            function(context, args.values[0], args.values[1] as u32);
            0
        });
        self.native.set_callback(Event::ThreadTerminated, handle);
    }

    pub fn clear_on_thread_terminated(&self) {
        self.native
            .set_callback(Event::ThreadTerminated, UniquePtr::null());
    }

    /// Receives the thread id and its new name.
    pub fn on_thread_set_name(
        &self,
        function: impl Fn(&Context, u32, &str) + Send + Sync + 'static,
    ) {
        let handle = callback(self, move |context, args| {
            function(
                context,
                args.values[0] as u32,
                &String::from_utf8_lossy(args.first),
            );
            0
        });
        self.native.set_callback(Event::ThreadSetName, handle);
    }

    pub fn clear_on_thread_set_name(&self) {
        self.native
            .set_callback(Event::ThreadSetName, UniquePtr::null());
    }

    /// Receives the ids of the outgoing and the incoming thread.
    pub fn on_thread_switch(&self, function: impl Fn(&Context, u32, u32) + Send + Sync + 'static) {
        let handle = callback(self, move |context, args| {
            function(context, args.values[0] as u32, args.values[1] as u32);
            0
        });
        self.native.set_callback(Event::ThreadSwitch, handle);
    }

    pub fn clear_on_thread_switch(&self) {
        self.native
            .set_callback(Event::ThreadSwitch, UniquePtr::null());
    }

    /// Receives the module, the section and the address the first time the guest executes in a section of a module.
    /// On backends with global execution hooks, this needs [`Settings::use_instruction_precision`].
    pub fn on_section_first_execution(
        &self,
        function: impl Fn(&Context, &Module, &Section, u64) + Send + Sync + 'static,
    ) {
        let handle = callback(self, move |context, args| {
            let section = usize::try_from(args.values[1]).expect("section index fits usize");
            if let Some(module) = context.find_module_by_address(args.values[0])
                && let Some(section) = module.sections().get(section)
            {
                function(context, &module, section, args.values[2]);
            }
            0
        });
        self.native
            .set_callback(Event::SectionFirstExecution, handle);
    }

    pub fn clear_on_section_first_execution(&self) {
        self.native
            .set_callback(Event::SectionFirstExecution, UniquePtr::null());
    }

    /// Receives the code of a guest `__fastfail`.
    pub fn on_fast_fail(&self, function: impl Fn(&Context, u32) + Send + Sync + 'static) {
        let handle = callback(self, move |context, args| {
            function(context, args.values[0] as u32);
            0
        });
        self.native.set_callback(Event::FastFail, handle);
    }

    pub fn clear_on_fast_fail(&self) {
        self.native.set_callback(Event::FastFail, UniquePtr::null());
    }

    /// Receives every line the emulator logs, also while [`Context::logging`] is off or [`Context::silent`] is on.
    pub fn on_log(&self, function: impl Fn(&Context, LogLevel, &str) + Send + Sync + 'static) {
        self.native
            .set_callback(Event::Log, log_callback(self, function));
    }

    pub fn clear_on_log(&self) {
        self.native.set_callback(Event::Log, UniquePtr::null());
    }

    /// Records a failure inside a callback, which the running call returns once the guest has stopped.
    fn fail(&self, error: Error) {
        self.pending_error.record(error);
        self.stop();
    }

    fn retain_host_memory(&self, memory: &HostMemory) {
        let mut retained = self.host_memory.borrow_mut();
        if !retained
            .iter()
            .any(|kept| kept.as_mut_ptr() == memory.as_mut_ptr())
        {
            retained.push(memory.clone());
        }
    }
}

impl Sealed for Context {
    fn native_machine(&self) -> &Machine {
        self.native.machine()
    }
}

impl Guest for Context {}

impl Drop for Context {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.get() {
            stop.detach();
        }
    }
}

fn socket_address(args: &CallbackArgs<'_>) -> SocketAddr {
    let port = args.values[1] as u16;
    match <[u8; 4]>::try_from(args.first) {
        Ok(octets) => SocketAddr::new(IpAddr::V4(Ipv4Addr::from(octets)), port),
        Err(_) => {
            let mut octets = [0; 16];
            octets.copy_from_slice(args.first);
            SocketAddr::V6(SocketAddrV6::new(
                Ipv6Addr::from(octets),
                port,
                0,
                args.values[2] as u32,
            ))
        }
    }
}
