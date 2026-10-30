//! The Linux x86-64 process personality.
//!
//! Callbacks run on the thread that runs the guest. Registering [`Context::on_module_load`] immediately replays the
//! callback for every module that is already mapped, in address order, so a late registration still sees the whole
//! module list.

use std::cell::{Cell, RefCell};
use std::marker::PhantomData;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::str;
use std::sync::{Arc, OnceLock};

use cxx::{CxxVector, UniquePtr};
use sogen_sys::common::ffi::{CallbackHandle, Machine};
use sogen_sys::linux::ffi;

use self::debug::Debugger;
use self::symbols::SymbolHooks;
use crate::error::Error;
use crate::machine::{
    CallContinuation, ContextPtr, Guest, HookContinuation, HookId, LogLevel, MemoryPermission,
    MemoryViolation, MemoryViolationContinuation, Owned, PendingError, Sealed, StopHandle,
    StopReason, StopTarget, callback, log_callback, native_path, native_str,
};

pub use self::debug::{Instruction, RegisterValue, Registers, StackFrame, ThreadRegisters};
pub use self::file_descriptor::{FileDescriptor, FileDescriptorKind, OpenFlags, Socket};
pub use self::info::{
    AuxiliaryEntry, AuxiliaryKind, ExportedSymbol, Module, ProcessInfo, Section, Thread,
    ThreadWaitState,
};
pub use self::memory::{Allocation, MemoryRegion, MemoryStats};
pub use self::settings::{Application, EnvironmentVariable, Mount, Settings};
pub use self::symbols::{Parameter, SymbolCall, Value};

mod debug;
mod file_descriptor;
mod info;
mod memory;
mod settings;
mod symbols;

pub struct Emulator {
    context: Owned<Context>,
}

impl Emulator {
    /// An emulator without a loaded program.
    pub fn empty(settings: &Settings) -> Result<Self, Error> {
        Self::create(settings.to_native(None)?)
    }

    /// An emulator with `application` loaded, ready to run from its entry point.
    pub fn application(settings: &Settings, application: &Application) -> Result<Self, Error> {
        Self::create(settings.to_native(Some(application))?)
    }

    fn create(settings: ffi::LinuxSettings) -> Result<Self, Error> {
        let native = ffi::create_linux_emulator(&settings)?;
        let emulator = Self {
            context: Owned::new(Context {
                native,
                stop: OnceLock::new(),
                pending_error: PendingError::default(),
                snapshot: RefCell::new(None),
                symbols: SymbolHooks::default(),
                debugger: Debugger::default(),
                not_sync: PhantomData,
            }),
        };

        let observer = callback(&*emulator.context, |context, args| {
            if let Some(module) = context.module_at(args.values[0])
                && let Err(error) = context.symbols.install(context, module)
            {
                context.fail(error);
            }
            0
        });
        emulator.context.native.set_module_observer(observer);
        Ok(emulator)
    }

    pub fn run(&mut self) -> Result<(), Error> {
        self.context.start(0)
    }

    /// Runs at most `count` instructions; zero means no limit.
    pub fn run_instructions(&mut self, count: u64) -> Result<(), Error> {
        self.context.start(count)
    }

    pub fn restore_snapshot(&mut self) -> Result<(), Error> {
        let snapshot = self.context.snapshot.borrow().clone();
        let snapshot = snapshot.ok_or(Error::NoSnapshot)?;
        self.restore(snapshot.as_slice())
    }

    pub fn deserialise_state(&mut self, state: impl AsRef<[u8]>) -> Result<(), Error> {
        self.restore(state.as_ref())
    }

    /// Makes the thread active; returns false if it does not exist or has terminated.
    pub fn activate_thread(&mut self, id: u32) -> Result<bool, Error> {
        self.context
            .pending_error
            .finish(self.context.native.activate_thread(id))
    }

    /// Switches to the next ready thread; returns false if there is none.
    pub fn perform_thread_switch(&mut self) -> Result<bool, Error> {
        self.context
            .pending_error
            .finish(self.context.native.perform_thread_switch())
    }

    /// Switches to the next ready thread and stops the guest if a switch happened.
    pub fn yield_thread(&mut self) -> Result<(), Error> {
        self.context
            .pending_error
            .finish(self.context.native.yield_thread())
    }

    /// Runs one instruction, stepping over a breakpoint at the current address.
    pub fn step_into(&mut self) -> Result<(), Error> {
        self.context.debugger.step_into(&self.context)
    }

    /// Runs one instruction, or a whole call until it returns to the next instruction of the current frame.
    pub fn step_over(&mut self) -> Result<(), Error> {
        self.context.debugger.step_over(&self.context)
    }

    /// Runs until the current function returns to its caller, found through the frame pointer. Without frame
    /// pointers, use [`Emulator::run_to`] with the return address.
    pub fn step_out(&mut self) -> Result<(), Error> {
        self.context.debugger.step_out(&self.context)
    }

    /// Runs until the guest reaches `address`, stopping there with [`StopReason::Breakpoint`], or stops otherwise.
    pub fn run_to(&mut self, address: u64) -> Result<(), Error> {
        self.context
            .debugger
            .run_until(&self.context, address, None)
    }

    /// Runs until a breakpoint or another stop, stepping over a breakpoint at the current address.
    pub fn continue_execution(&mut self) -> Result<(), Error> {
        self.context.debugger.resume(&self.context)
    }

    fn restore(&mut self, state: &[u8]) -> Result<(), Error> {
        self.context
            .pending_error
            .finish(self.context.native.deserialize_state(state))?;
        self.context.symbols.rebuild(&self.context)
    }
}

impl Deref for Emulator {
    type Target = Context;

    fn deref(&self) -> &Context {
        &self.context
    }
}

pub struct Context {
    native: UniquePtr<ffi::LinuxEmulator>,
    stop: OnceLock<Arc<StopTarget>>,
    pending_error: PendingError,
    snapshot: RefCell<Option<Arc<UniquePtr<CxxVector<u8>>>>>,
    symbols: SymbolHooks,
    debugger: Debugger,
    not_sync: PhantomData<Cell<()>>,
}

impl Context {
    /// Stops the guest from a callback, recording [`StopReason::ExplicitStop`] unless another reason was recorded.
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

    pub fn emulation_root(&self) -> PathBuf {
        PathBuf::from(self.native.emulation_root())
    }

    /// Whether the emulator's own log output reaches the host's standard output.
    pub fn logging(&self) -> bool {
        self.native.logging()
    }

    pub fn set_logging(&self, logging: bool) {
        self.native.set_logging(logging);
    }

    /// Whether all log output to the host is muted, errors included. [`Context::on_log`] still receives every line.
    pub fn silent(&self) -> bool {
        self.native.silent()
    }

    pub fn set_silent(&self, silent: bool) {
        self.native.set_silent(silent);
    }

    /// Records the current state for [`Emulator::restore_snapshot`], replacing any earlier snapshot.
    pub fn save_snapshot(&self) -> Result<(), Error> {
        let snapshot = self.native.serialize_state()?;
        *self.snapshot.borrow_mut() = Some(Arc::new(snapshot));
        Ok(())
    }

    pub fn serialise_state(&self) -> Result<Vec<u8>, Error> {
        Ok(self.native.serialize_state()?.as_slice().to_vec())
    }

    /// Redirects guest connections to `emulator_port` to `host_port`, replacing earlier mappings of either port.
    pub fn map_port(&self, emulator_port: u16, host_port: u16) -> Result<(), Error> {
        if emulator_port == 0 || host_port == 0 {
            return Err(Error::ZeroPort);
        }

        Ok(self.native.map_port(emulator_port, host_port)?)
    }

    pub fn host_port(&self, emulator_port: u16) -> Option<u16> {
        Some(self.native.host_port(emulator_port)).filter(|&port| port != 0)
    }

    pub fn emulator_port(&self, host_port: u16) -> Option<u16> {
        Some(self.native.emulator_port(host_port)).filter(|&port| port != 0)
    }

    pub fn process(&self) -> ProcessInfo {
        ProcessInfo::from_native(self.native.process())
    }

    /// The threads that have not terminated, ordered by thread id.
    pub fn threads(&self) -> Vec<Thread> {
        self.native
            .threads()
            .into_iter()
            .map(Thread::from_native)
            .collect()
    }

    /// A snapshot of the thread taken now, terminated threads included.
    pub fn thread(&self, id: u32) -> Option<Thread> {
        self.native
            .thread(id)
            .into_iter()
            .next()
            .map(Thread::from_native)
    }

    /// The active thread, unless it terminated.
    pub fn current_thread(&self) -> Option<Thread> {
        self.native
            .current_thread()
            .into_iter()
            .next()
            .map(Thread::from_native)
    }

    pub fn current_thread_id(&self) -> Option<u32> {
        self.current_thread().map(|thread| thread.id())
    }

    /// The registers of a thread: live for the active thread, the saved ones for any other.
    pub fn thread_registers(&self, id: u32) -> Option<ThreadRegisters> {
        self.native
            .thread_registers(id)
            .into_iter()
            .next()
            .map(ThreadRegisters::from_native)
    }

    /// Replaces the registers of a thread, the live ones for the active thread; returns false if there is no such
    /// thread.
    pub fn set_thread_registers(
        &self,
        id: u32,
        registers: &ThreadRegisters,
    ) -> Result<bool, Error> {
        Ok(self
            .native
            .set_thread_registers(id, &registers.to_native())?)
    }

    pub fn auxiliary_vector(&self) -> Vec<AuxiliaryEntry> {
        self.native
            .auxiliary_vector()
            .into_iter()
            .map(AuxiliaryEntry::from_native)
            .collect()
    }

    /// The open file descriptors, ordered by number.
    pub fn file_descriptors(&self) -> Vec<FileDescriptor> {
        self.native
            .file_descriptors()
            .into_iter()
            .map(FileDescriptor::from_native)
            .collect()
    }

    pub fn file_descriptor(&self, number: u32) -> Option<FileDescriptor> {
        let number = i32::try_from(number).ok()?;
        self.native
            .file_descriptor(number)
            .into_iter()
            .next()
            .map(FileDescriptor::from_native)
    }

    /// The host path the guest `path` resolves to; a relative `path` is taken relative to `/`.
    pub fn translate_path(&self, path: &str) -> Result<PathBuf, Error> {
        Ok(PathBuf::from(self.native.translate_path(native_str(path)?)))
    }

    /// The host path the guest reaches through `path`, resolved against the guest directory `base` when relative.
    pub fn translate_relative_path(&self, base: &str, path: &str) -> Result<PathBuf, Error> {
        Ok(PathBuf::from(self.native.translate_relative_path(
            native_str(base)?,
            native_str(path)?,
        )))
    }

    /// Whether the guest `path` lies in a read-only mount.
    pub fn is_path_read_only(&self, path: &str) -> bool {
        self.native.is_path_read_only(path)
    }

    /// Mounts a host directory, replacing an existing mount at the same guest path.
    pub fn add_mount(&self, mount: &Mount) -> Result<(), Error> {
        Ok(self.native.add_mount(&mount.to_native()?)?)
    }

    /// Lets guest paths inside the host directory `path` resolve to themselves instead of into the emulation root.
    pub fn add_passthrough_prefix(&self, path: &Path) -> Result<(), Error> {
        Ok(self.native.add_passthrough_prefix(&native_path(path)?)?)
    }

    /// The mapped modules, ordered by image base.
    pub fn modules(&self) -> Vec<Module> {
        self.native
            .modules()
            .into_iter()
            .map(Module::from_native)
            .collect()
    }

    pub fn find_module_by_address(&self, address: u64) -> Option<Module> {
        self.native
            .find_module_by_address(address)
            .into_iter()
            .next()
            .map(Module::from_native)
    }

    pub fn find_module_by_name(&self, name: &str) -> Option<Module> {
        self.native
            .find_module_by_name(name)
            .into_iter()
            .next()
            .map(Module::from_native)
    }

    /// Maps the ELF image at the host `path`, without relocating it or loading its dependencies.
    pub fn map_local_module(&self, path: &Path) -> Result<Module, Error> {
        let module = self.native.map_local_module(&native_path(path)?);
        Ok(Module::from_native(self.pending_error.finish(module)?))
    }

    /// Maps every `DT_NEEDED` library of the mapped modules that is not mapped yet, transitively. Libraries that
    /// cannot be found are skipped.
    pub fn load_module_dependencies(&self) -> Result<(), Error> {
        self.pending_error
            .finish(self.native.load_module_dependencies())
    }

    /// The host path of the library `name` as `from` would load it, searching its `DT_RPATH`, the configured library
    /// paths, its `DT_RUNPATH`, then the standard directories in the emulation root.
    pub fn resolve_library(&self, name: &str, from: &Module) -> Option<PathBuf> {
        let path = self.native.resolve_library(name, from.image_base());
        (!path.is_empty()).then(|| PathBuf::from(path))
    }

    /// Maps memory at the first free address at or above the allocation's start, or above the mmap base without one.
    /// Returns `None` when no free range is large enough.
    pub fn allocate_memory(&self, allocation: &Allocation) -> Result<Option<u64>, Error> {
        let address = self.native.allocate_memory(
            allocation.size(),
            allocation.permissions().into(),
            allocation.start().unwrap_or(0),
        )?;
        Ok(Some(address).filter(|&address| address != 0))
    }

    /// Returns false if the range overlaps a mapping or is invalid.
    pub fn allocate_memory_at(
        &self,
        address: u64,
        size: u64,
        permissions: MemoryPermission,
    ) -> Result<bool, Error> {
        Ok(self
            .native
            .allocate_memory_at(address, size, permissions.into())?)
    }

    /// Returns false if the range is not mapped.
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

    /// Returns false if nothing was unmapped.
    pub fn release_memory(&self, address: u64, size: u64) -> Result<bool, Error> {
        Ok(self.native.release_memory(address, size)?)
    }

    pub fn find_free_allocation_base(&self, size: u64, start: Option<u64>) -> Option<u64> {
        Some(
            self.native
                .find_free_allocation_base(size, start.unwrap_or(0)),
        )
        .filter(|&address| address != 0)
    }

    pub fn memory_region(&self, address: u64) -> Option<MemoryRegion> {
        self.native
            .memory_region(address)
            .into_iter()
            .next()
            .map(MemoryRegion::from_native)
    }

    pub fn memory_regions(&self) -> Vec<MemoryRegion> {
        self.native
            .memory_regions()
            .into_iter()
            .map(MemoryRegion::from_native)
            .collect()
    }

    pub fn memory_stats(&self) -> MemoryStats {
        MemoryStats::from_native(self.native.memory_stats())
    }

    /// Where allocations without an explicit address start searching.
    pub fn mmap_base(&self) -> u64 {
        self.native.mmap_base()
    }

    pub fn set_mmap_base(&self, address: u64) {
        self.native.set_mmap_base(address);
    }

    /// Receives everything the guest writes to standard output. Without a callback the output goes to the host's.
    pub fn on_stdout(&self, function: impl Fn(&Context, &[u8]) + Send + Sync + 'static) {
        self.native
            .set_stdout_callback(callback(self, move |context, args| {
                function(context, args.first);
                0
            }));
    }

    pub fn clear_on_stdout(&self) {
        self.native.set_stdout_callback(UniquePtr::null());
    }

    /// Receives everything the guest writes to standard error. Without a callback the output goes to the host's.
    pub fn on_stderr(&self, function: impl Fn(&Context, &[u8]) + Send + Sync + 'static) {
        self.native
            .set_stderr_callback(callback(self, move |context, args| {
                function(context, args.first);
                0
            }));
    }

    pub fn clear_on_stderr(&self) {
        self.native.set_stderr_callback(UniquePtr::null());
    }

    /// Runs before every system call with its number and name (`<unknown>` for unknown numbers).
    /// [`HookContinuation::SkipInstruction`] skips the system call and
    /// [`HookContinuation::FinalizedInstructionPointer`] keeps an instruction pointer the callback wrote; both bypass
    /// the emulator's implementation.
    pub fn on_syscall(
        &self,
        function: impl Fn(&Context, u64, &str) -> HookContinuation + Send + Sync + 'static,
    ) {
        self.native
            .set_syscall_callback(callback(self, move |context, args| {
                let name = str::from_utf8(args.first).unwrap_or("<unknown>");
                u64::from(function(context, args.values[0], name).repr)
            }));
    }

    pub fn clear_on_syscall(&self) {
        self.native.set_syscall_callback(UniquePtr::null());
    }

    /// Runs when a signal is delivered, with the signal number, fault address and `si_code`.
    pub fn on_signal(&self, function: impl Fn(&Context, i32, u64, i32) + Send + Sync + 'static) {
        self.native
            .set_signal_callback(callback(self, move |context, args| {
                function(
                    context,
                    args.values[0] as u32 as i32,
                    args.values[1],
                    args.values[2] as u32 as i32,
                );
                0
            }));
    }

    pub fn clear_on_signal(&self) {
        self.native.set_signal_callback(UniquePtr::null());
    }

    /// Runs after memory was mapped, with its address, size, permissions and whether it is committed.
    pub fn on_memory_allocate(
        &self,
        function: impl Fn(&Context, u64, u64, MemoryPermission, bool) + Send + Sync + 'static,
    ) {
        self.native
            .set_memory_allocate_callback(callback(self, move |context, args| {
                function(
                    context,
                    args.values[0],
                    args.values[1],
                    MemoryPermission::from_bits_retain(args.values[2] as u8),
                    args.values[3] != 0,
                );
                0
            }));
    }

    pub fn clear_on_memory_allocate(&self) {
        self.native.set_memory_allocate_callback(UniquePtr::null());
    }

    pub fn on_memory_protect(
        &self,
        function: impl Fn(&Context, u64, u64, MemoryPermission) + Send + Sync + 'static,
    ) {
        self.native
            .set_memory_protect_callback(callback(self, move |context, args| {
                function(
                    context,
                    args.values[0],
                    args.values[1],
                    MemoryPermission::from_bits_retain(args.values[2] as u8),
                );
                0
            }));
    }

    pub fn clear_on_memory_protect(&self) {
        self.native.set_memory_protect_callback(UniquePtr::null());
    }

    pub fn on_memory_release(&self, function: impl Fn(&Context, u64, u64) + Send + Sync + 'static) {
        self.native
            .set_memory_release_callback(callback(self, move |context, args| {
                function(context, args.values[0], args.values[1]);
                0
            }));
    }

    pub fn clear_on_memory_release(&self) {
        self.native.set_memory_release_callback(UniquePtr::null());
    }

    /// Runs for every module mapped from now on, after first replaying every module already mapped.
    pub fn on_module_load(&self, function: impl Fn(&Context, &Module) + Send + Sync + 'static) {
        self.native
            .set_module_load_callback(callback(self, move |context, args| {
                if let Some(module) = context.module_at(args.values[0]) {
                    function(context, &module);
                }
                0
            }));
    }

    pub fn clear_on_module_load(&self) {
        self.native.set_module_load_callback(UniquePtr::null());
    }

    /// Runs after a thread was created, with its state at that moment.
    pub fn on_thread_create(&self, function: impl Fn(&Context, &Thread) + Send + Sync + 'static) {
        self.native
            .set_thread_create_callback(thread_callback(self, function));
    }

    pub fn clear_on_thread_create(&self) {
        self.native.set_thread_create_callback(UniquePtr::null());
    }

    /// Runs when a thread terminates, with its final state.
    pub fn on_thread_terminated(
        &self,
        function: impl Fn(&Context, &Thread) + Send + Sync + 'static,
    ) {
        self.native
            .set_thread_terminated_callback(thread_callback(self, function));
    }

    pub fn clear_on_thread_terminated(&self) {
        self.native
            .set_thread_terminated_callback(UniquePtr::null());
    }

    /// Runs when the active thread changes, with the previous thread id (zero if none) and the new one.
    pub fn on_thread_switch(&self, function: impl Fn(&Context, u32, u32) + Send + Sync + 'static) {
        self.native
            .set_thread_switch_callback(callback(self, move |context, args| {
                function(context, args.values[0] as u32, args.values[1] as u32);
                0
            }));
    }

    pub fn clear_on_thread_switch(&self) {
        self.native.set_thread_switch_callback(UniquePtr::null());
    }

    /// Decides how to continue after a memory violation. While set, it replaces the default `SIGSEGV` delivery, like
    /// [`Guest::hook_memory_violation`].
    pub fn on_memory_violate(
        &self,
        function: impl Fn(&Context, &MemoryViolation) -> MemoryViolationContinuation
        + Send
        + Sync
        + 'static,
    ) {
        self.native
            .set_memory_violation_callback(callback(self, move |context, args| {
                u64::from(function(context, &MemoryViolation::from_args(args)).repr)
            }));
    }

    pub fn clear_on_memory_violate(&self) {
        self.native.set_memory_violation_callback(UniquePtr::null());
    }

    /// Runs every 0x20000 executed instructions.
    pub fn on_periodic_event(&self, function: impl Fn(&Context) + Send + Sync + 'static) {
        self.native
            .set_periodic_event_callback(callback(self, move |context, _| {
                function(context);
                0
            }));
    }

    pub fn clear_on_periodic_event(&self) {
        self.native.set_periodic_event_callback(UniquePtr::null());
    }

    /// Receives every line the emulator logs, whether or not logging is enabled or silent.
    pub fn on_log(&self, function: impl Fn(&Context, LogLevel, &str) + Send + Sync + 'static) {
        self.native.set_log_callback(log_callback(self, function));
    }

    pub fn clear_on_log(&self) {
        self.native.set_log_callback(UniquePtr::null());
    }

    /// Runs `callback` whenever an exported function matching `key` is entered, replacing an existing hook with the
    /// same key.
    ///
    /// A key is either `name`, matching that export in every module, or `module!name`, matching only modules whose
    /// name or file stem equals `module` (case-sensitive). Export names match exactly; imports (address zero) never
    /// match. Hooks follow modules mapped later and survive state restores.
    ///
    /// The callback receives the call and the first `parameters.len()` arguments (RDI, RSI, RDX, RCX, R8, R9, then
    /// 8-byte stack slots from RSP+8). A call whose return address or stack arguments cannot be read is not reported.
    pub fn hook_symbol(
        &self,
        key: &str,
        parameters: &[Parameter],
        callback: impl Fn(&Context, &mut SymbolCall, &[Value]) -> CallContinuation
        + Send
        + Sync
        + 'static,
    ) -> Result<(), Error> {
        self.symbols.hook(self, key, parameters, callback)
    }

    /// Returns false if no hook has this key.
    pub fn unhook_symbol(&self, key: &str) -> bool {
        self.symbols.unhook(self, key)
    }

    pub fn clear_symbol_hooks(&self) {
        self.symbols.clear(self);
    }

    /// Stops the guest before the instruction at `address` runs, with [`StopReason::Breakpoint`]. Setting an existing
    /// breakpoint does nothing.
    pub fn set_breakpoint(&self, address: u64) -> Result<(), Error> {
        self.debugger.set_breakpoint(self, address)
    }

    /// Returns false if there is no breakpoint at `address`.
    pub fn clear_breakpoint(&self, address: u64) -> bool {
        self.debugger.clear_breakpoint(self, address)
    }

    /// Breakpoint addresses in ascending order.
    pub fn breakpoints(&self) -> Vec<u64> {
        self.debugger.breakpoints()
    }

    pub fn registers(&self) -> Result<Registers, Error> {
        Registers::read(self)
    }

    /// Decodes up to `count` instructions (at most 4096) from the readable bytes at `address`.
    pub fn disassemble(&self, address: u64, count: usize) -> Result<Vec<Instruction>, Error> {
        debug::disassemble(self, address, count)
    }

    /// The current frame followed by the callers found by walking the RBP chain. The walk ends at a zero return
    /// address or once the saved frame pointer no longer moves up the stack.
    pub fn call_stack(&self) -> Result<Vec<StackFrame>, Error> {
        debug::call_stack(self)
    }

    fn start(&self, count: u64) -> Result<(), Error> {
        self.pending_error.finish(self.native.start(count))
    }

    /// Records a failure inside a callback, which the call running the guest returns once the guest has stopped.
    fn fail(&self, error: Error) {
        self.pending_error.record(error);
        self.native.stop();
    }

    fn module_at(&self, image_base: u64) -> Option<Module> {
        self.native
            .module_at(image_base)
            .into_iter()
            .next()
            .map(Module::from_native)
    }

    fn read_stack_slot(&self, address: u64) -> Option<u64> {
        let mut slot = [0; 8];
        self.try_read_memory(address, &mut slot)
            .then(|| u64::from_le_bytes(slot))
    }
}

impl Sealed for Context {
    fn native_machine(&self) -> &Machine {
        self.native.machine()
    }
}

impl Guest for Context {
    /// Observes interrupts before the Linux personality handles them, for example to see an `int3` before it becomes
    /// a `SIGTRAP`.
    fn hook_interrupt(
        &self,
        function: impl Fn(&Self, u32) + Send + Sync + 'static,
    ) -> Result<HookId, Error> {
        let handle = callback(self, move |context, args| {
            function(context, args.values[0] as u32);
            0
        });
        Ok(HookId(self.native.hook_interrupt(handle)?))
    }

    /// While any violation hook or [`Context::on_memory_violate`] is registered, they replace the default `SIGSEGV`
    /// delivery; the guest stops as soon as one returns [`MemoryViolationContinuation::Stop`].
    fn hook_memory_violation(
        &self,
        function: impl Fn(&Self, &MemoryViolation) -> MemoryViolationContinuation
        + Send
        + Sync
        + 'static,
    ) -> Result<HookId, Error> {
        let handle = callback(self, move |context, args| {
            u64::from(function(context, &MemoryViolation::from_args(args)).repr)
        });
        Ok(HookId(self.native.hook_memory_violation(handle)?))
    }
}

impl Drop for Context {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.get() {
            stop.detach();
        }
    }
}

fn thread_callback(
    context: &Context,
    function: impl Fn(&Context, &Thread) + Send + Sync + 'static,
) -> UniquePtr<CallbackHandle> {
    callback(context, move |context, args| {
        if let Some(thread) = context.thread(args.values[0] as u32) {
            function(context, &thread);
        }
        0
    })
}
