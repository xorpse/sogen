use std::ops::Deref;
use std::path::Path;
use std::ptr::NonNull;
use std::sync::{Arc, Mutex, PoisonError};

use bitflags::bitflags;
use cxx::UniquePtr;
use sogen_sys::common::{Callback, CallbackArgs, ffi};

use crate::error::Error;

pub use sogen_sys::common::ffi::{
    Arm64HookableInstruction, Arm64Register, BackendType as Backend,
    InstructionHookContinuation as HookContinuation, LogLevel, MemoryExecutionHookMode,
    MemoryRegionKind, MemoryViolationContinuation, MemoryViolationType, StopReason,
    X86HookableInstruction, X86Register,
};

pub(crate) use self::sealed::Sealed;

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct MemoryPermission: u8 {
        const EXEC = ffi::MemoryPermission::Exec.repr;
        const GUARD = ffi::MemoryPermission::Guard.repr;
        const READ = ffi::MemoryPermission::Read.repr;
        const WRITE = ffi::MemoryPermission::Write.repr;
    }
}

impl From<MemoryPermission> for ffi::MemoryPermission {
    fn from(permission: MemoryPermission) -> Self {
        Self {
            repr: permission.bits(),
        }
    }
}

impl From<ffi::MemoryPermission> for MemoryPermission {
    fn from(permission: ffi::MemoryPermission) -> Self {
        Self::from_bits_retain(permission.repr)
    }
}

/// Identifies a hook for [`Guest::remove_hook`]. Ids are never reused within an emulator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HookId(pub(crate) u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum CallContinuation {
    /// Returns to the caller with the call's return value without running the function.
    Intercept,
    #[default]
    RunOriginal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BasicBlock {
    address: u64,
    instruction_count: u64,
    size: u64,
}

impl BasicBlock {
    pub fn address(&self) -> u64 {
        self.address
    }

    pub fn instruction_count(&self) -> u64 {
        self.instruction_count
    }

    pub fn size(&self) -> u64 {
        self.size
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemoryViolation {
    address: u64,
    size: u64,
    operation: MemoryPermission,
    kind: MemoryViolationType,
}

impl MemoryViolation {
    pub(crate) fn from_args(args: &CallbackArgs<'_>) -> Self {
        Self {
            address: args.values[0],
            size: args.values[1],
            operation: MemoryPermission::from_bits_retain(args.values[2] as u8),
            kind: MemoryViolationType {
                repr: args.values[3] as u8,
            },
        }
    }

    pub fn address(&self) -> u64 {
        self.address
    }

    pub fn size(&self) -> u64 {
        self.size
    }

    pub fn operation(&self) -> MemoryPermission {
        self.operation
    }

    pub fn kind(&self) -> MemoryViolationType {
        self.kind
    }
}

/// The x86 segments whose base every backend can access; long mode ignores the bases of the others.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum X86Segment {
    Fs,
    Gs,
}

impl From<X86Segment> for X86Register {
    fn from(segment: X86Segment) -> Self {
        match segment {
            X86Segment::Fs => Self::Fs,
            X86Segment::Gs => Self::Gs,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DescriptorTable {
    Gdt,
    Idt,
}

impl From<DescriptorTable> for X86Register {
    fn from(table: DescriptorTable) -> Self {
        match table {
            DescriptorTable::Gdt => Self::Gdtr,
            DescriptorTable::Idt => Self::Idtr,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DescriptorTableRegister {
    base: u64,
    limit: u32,
}

impl DescriptorTableRegister {
    pub fn base(&self) -> u64 {
        self.base
    }

    pub fn limit(&self) -> u32 {
        self.limit
    }
}

impl From<ffi::DescriptorTableRegister> for DescriptorTableRegister {
    fn from(table: ffi::DescriptorTableRegister) -> Self {
        Self {
            base: table.base,
            limit: table.limit,
        }
    }
}

mod sealed {
    use sogen_sys::common::ffi;

    pub trait Sealed {
        fn native_machine(&self) -> &ffi::Machine;
    }

    pub trait RegisterId {
        fn read(self, machine: &ffi::Machine, value: &mut [u8]) -> Result<usize, cxx::Exception>;
        fn write(self, machine: &ffi::Machine, value: &[u8]) -> Result<usize, cxx::Exception>;
    }

    pub trait InstructionId {
        fn id(self) -> i32;
    }
}

pub trait Register: sealed::RegisterId + Copy {}

impl sealed::RegisterId for X86Register {
    fn read(self, machine: &ffi::Machine, value: &mut [u8]) -> Result<usize, cxx::Exception> {
        machine.read_x86_register(self, value)
    }

    fn write(self, machine: &ffi::Machine, value: &[u8]) -> Result<usize, cxx::Exception> {
        machine.write_x86_register(self, value)
    }
}

impl Register for X86Register {}

impl sealed::RegisterId for Arm64Register {
    fn read(self, machine: &ffi::Machine, value: &mut [u8]) -> Result<usize, cxx::Exception> {
        machine.read_arm64_register(self, value)
    }

    fn write(self, machine: &ffi::Machine, value: &[u8]) -> Result<usize, cxx::Exception> {
        machine.write_arm64_register(self, value)
    }
}

impl Register for Arm64Register {}

pub trait HookableInstruction: sealed::InstructionId + Copy {}

impl sealed::InstructionId for X86HookableInstruction {
    fn id(self) -> i32 {
        self.repr
    }
}

impl HookableInstruction for X86HookableInstruction {}

impl sealed::InstructionId for Arm64HookableInstruction {
    fn id(self) -> i32 {
        self.repr
    }
}

impl HookableInstruction for Arm64HookableInstruction {}

/// Pointer to an emulator's context, handed to callbacks.
///
/// The context lives at a fixed heap address for the emulator's whole lifetime, and the native emulator destroys
/// every callback before the context is freed, so callbacks never observe a dangling context. Callbacks never run
/// concurrently: Linux and raw ARM64 emulators run them on the one thread running the guest, and a Windows emulator
/// runs every callback holding its kernel lock, whichever virtual CPU or host thread raised it. The lock orders each
/// callback after the previous one, and outside a run `&mut Emulator` excludes callbacks, so the context is only ever
/// used by one thread at a time.
pub(crate) struct ContextPtr<T>(NonNull<T>);

unsafe impl<T> Send for ContextPtr<T> {}

unsafe impl<T> Sync for ContextPtr<T> {}

impl<T> Clone for ContextPtr<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for ContextPtr<T> {}

impl<T> ContextPtr<T> {
    pub(crate) fn new(context: &T) -> Self {
        Self(NonNull::from(context))
    }

    pub(crate) fn get(&self) -> &T {
        unsafe { self.0.as_ref() }
    }
}

pub(crate) fn callback<G: Guest>(
    guest: &G,
    function: impl Fn(&G, &CallbackArgs<'_>) -> u64 + Send + Sync + 'static,
) -> UniquePtr<ffi::CallbackHandle> {
    let context = ContextPtr::new(guest);
    Callback::handle(move |args| function(context.get(), args))
}

/// Native log sinks pass the level as the first value and the formatted line as the first byte slice.
pub(crate) fn log_callback<G: Guest>(
    guest: &G,
    function: impl Fn(&G, LogLevel, &str) + Send + Sync + 'static,
) -> UniquePtr<ffi::CallbackHandle> {
    callback(guest, move |guest, args| {
        let level = LogLevel {
            repr: args.values[0] as i32,
        };
        function(guest, level, &String::from_utf8_lossy(args.first));
        0
    })
}

/// Splits a hook key, `name` or `module!name`, into its module filter and name.
pub(crate) fn parse_hook_key(key: &str) -> Result<(Option<&str>, &str), Error> {
    let (module, name) = match key.split_once('!') {
        Some((module, name)) => (Some(module), name),
        None => (None, key),
    };
    if name.is_empty() || module.is_some_and(str::is_empty) {
        return Err(Error::hook_key(key));
    }

    Ok((module, name))
}

pub(crate) fn native_path(path: &Path) -> Result<String, Error> {
    native_string(path.to_str().ok_or_else(|| Error::non_utf8_path(path))?)
}

/// Native code passes these strings on NUL-terminated, so an embedded NUL would silently cut them short.
pub(crate) fn native_str(text: &str) -> Result<&str, Error> {
    if text.contains('\0') {
        return Err(Error::nul_byte(text));
    }

    Ok(text)
}

pub(crate) fn native_string(text: &str) -> Result<String, Error> {
    native_str(text).map(str::to_owned)
}

/// Memory, register and hook access shared by every personality.
///
/// Callbacks receive the guest they were registered on and may be re-entered, for example when a callback causes
/// the event it observes. A panic inside a callback aborts the process.
pub trait Guest: Sealed + Sized + 'static {
    fn backend_name(&self) -> String {
        self.native_machine().backend_name()
    }

    fn supports_instruction_counting(&self) -> bool {
        self.native_machine().supports_instruction_counting()
    }

    /// Whether the backend supports [`Guest::hook_memory_execution`], which hooks every executed instruction.
    fn supports_global_memory_execution_hooks(&self) -> bool {
        self.native_machine()
            .supports_global_memory_execution_hooks()
    }

    fn supports_multiple_vcpus(&self) -> bool {
        self.native_machine().supports_multiple_vcpus()
    }

    fn set_memory_execution_hook_mode(&self, mode: MemoryExecutionHookMode) -> Result<(), Error> {
        Ok(self.native_machine().set_memory_execution_hook_mode(mode)?)
    }

    fn read_memory(&self, address: u64, data: &mut [u8]) -> Result<(), Error> {
        Ok(self.native_machine().read_memory(address, data)?)
    }

    fn try_read_memory(&self, address: u64, data: &mut [u8]) -> bool {
        self.native_machine().try_read_memory(address, data)
    }

    fn write_memory(&self, address: u64, data: impl AsRef<[u8]>) -> Result<(), Error> {
        Ok(self.native_machine().write_memory(address, data.as_ref())?)
    }

    fn try_write_memory(&self, address: u64, data: impl AsRef<[u8]>) -> bool {
        self.native_machine()
            .try_write_memory(address, data.as_ref())
    }

    /// Reads up to the low 64 bits of a register, zero-extended.
    fn read_register(&self, register: impl Register) -> Result<u64, Error> {
        let mut value = [0; 8];
        self.read_register_bytes(register, &mut value)?;
        Ok(u64::from_le_bytes(value))
    }

    /// Reads a register of any width into `value`, returning the register's size in bytes.
    fn read_register_bytes(
        &self,
        register: impl Register,
        value: &mut [u8],
    ) -> Result<usize, Error> {
        Ok(register.read(self.native_machine(), value)?)
    }

    fn write_register(&self, register: impl Register, value: u64) -> Result<(), Error> {
        self.write_register_bytes(register, value.to_le_bytes())
            .map(drop)
    }

    fn write_register_bytes(
        &self,
        register: impl Register,
        value: impl AsRef<[u8]>,
    ) -> Result<usize, Error> {
        Ok(register.write(self.native_machine(), value.as_ref())?)
    }

    fn save_registers(&self) -> Result<Vec<u8>, Error> {
        Ok(self.native_machine().save_registers()?)
    }

    fn restore_registers(&self, data: impl AsRef<[u8]>) -> Result<(), Error> {
        Ok(self.native_machine().restore_registers(data.as_ref())?)
    }

    fn segment_base(&self, segment: X86Segment) -> Result<u64, Error> {
        Ok(self.native_machine().segment_base(segment.into())?)
    }

    fn set_segment_base(&self, segment: X86Segment, base: u64) -> Result<(), Error> {
        Ok(self
            .native_machine()
            .set_segment_base(segment.into(), base)?)
    }

    fn descriptor_table(&self, table: DescriptorTable) -> Result<DescriptorTableRegister, Error> {
        Ok(self.native_machine().descriptor_table(table.into())?.into())
    }

    fn load_gdt(&self, address: u64, limit: u32) -> Result<(), Error> {
        Ok(self.native_machine().load_gdt(address, limit)?)
    }

    fn hook_memory_execution(
        &self,
        function: impl Fn(&Self, u64) + Send + Sync + 'static,
    ) -> Result<HookId, Error> {
        let handle = callback(self, move |guest, args| {
            function(guest, args.values[0]);
            0
        });
        Ok(HookId(self.native_machine().hook_memory_execution(handle)?))
    }

    fn hook_memory_execution_at(
        &self,
        address: u64,
        function: impl Fn(&Self, u64) + Send + Sync + 'static,
    ) -> Result<HookId, Error> {
        let handle = callback(self, move |guest, args| {
            function(guest, args.values[0]);
            0
        });
        Ok(HookId(
            self.native_machine()
                .hook_memory_execution_at(address, handle)?,
        ))
    }

    fn hook_memory_range_execution(
        &self,
        address: u64,
        size: u64,
        function: impl Fn(&Self, u64) + Send + Sync + 'static,
    ) -> Result<HookId, Error> {
        let handle = callback(self, move |guest, args| {
            function(guest, args.values[0]);
            0
        });
        Ok(HookId(
            self.native_machine()
                .hook_memory_range_execution(address, size, handle)?,
        ))
    }

    /// Calls `function` with the address and the bytes of every read in the range.
    fn hook_memory_read(
        &self,
        address: u64,
        size: u64,
        function: impl Fn(&Self, u64, &[u8]) + Send + Sync + 'static,
    ) -> Result<HookId, Error> {
        let handle = callback(self, move |guest, args| {
            function(guest, args.values[0], args.first);
            0
        });
        Ok(HookId(
            self.native_machine()
                .hook_memory_read(address, size, handle)?,
        ))
    }

    /// Calls `function` with the address and the bytes of every write in the range.
    fn hook_memory_write(
        &self,
        address: u64,
        size: u64,
        function: impl Fn(&Self, u64, &[u8]) + Send + Sync + 'static,
    ) -> Result<HookId, Error> {
        let handle = callback(self, move |guest, args| {
            function(guest, args.values[0], args.first);
            0
        });
        Ok(HookId(
            self.native_machine()
                .hook_memory_write(address, size, handle)?,
        ))
    }

    /// The `u64` passed to `function` is instruction specific, for example the immediate of an ARM64 SVC.
    fn hook_instruction(
        &self,
        instruction: impl HookableInstruction,
        function: impl Fn(&Self, u64) -> HookContinuation + Send + Sync + 'static,
    ) -> Result<HookId, Error> {
        let handle = callback(self, move |guest, args| {
            u64::from(function(guest, args.values[0]).repr)
        });
        Ok(HookId(
            self.native_machine()
                .hook_instruction(instruction.id(), handle)?,
        ))
    }

    fn hook_interrupt(
        &self,
        function: impl Fn(&Self, u32) + Send + Sync + 'static,
    ) -> Result<HookId, Error> {
        let handle = callback(self, move |guest, args| {
            function(guest, args.values[0] as u32);
            0
        });
        Ok(HookId(self.native_machine().hook_interrupt(handle)?))
    }

    fn hook_memory_violation(
        &self,
        function: impl Fn(&Self, &MemoryViolation) -> MemoryViolationContinuation
        + Send
        + Sync
        + 'static,
    ) -> Result<HookId, Error> {
        let handle = callback(self, move |guest, args| {
            u64::from(function(guest, &MemoryViolation::from_args(args)).repr)
        });
        Ok(HookId(self.native_machine().hook_memory_violation(handle)?))
    }

    fn hook_basic_block(
        &self,
        function: impl Fn(&Self, &BasicBlock) + Send + Sync + 'static,
    ) -> Result<HookId, Error> {
        let handle = callback(self, move |guest, args| {
            let block = BasicBlock {
                address: args.values[0],
                instruction_count: args.values[1],
                size: args.values[2],
            };
            function(guest, &block);
            0
        });
        Ok(HookId(self.native_machine().hook_basic_block(handle)?))
    }

    /// Returns false if the hook was already removed.
    fn remove_hook(&self, hook: HookId) -> bool {
        self.native_machine().remove_hook(hook.0)
    }
}

/// Owns an emulator context at a fixed heap address; see [`ContextPtr`].
pub(crate) struct Owned<T>(NonNull<T>);

unsafe impl<T: Send> Send for Owned<T> {}

impl<T> Owned<T> {
    pub(crate) fn new(value: T) -> Self {
        Self(NonNull::from(Box::leak(Box::new(value))))
    }
}

impl<T> Deref for Owned<T> {
    type Target = T;

    fn deref(&self) -> &T {
        unsafe { self.0.as_ref() }
    }
}

impl<T> Drop for Owned<T> {
    fn drop(&mut self) {
        drop(unsafe { Box::from_raw(self.0.as_ptr()) });
    }
}

type StopFunction = Box<dyn Fn() + Send>;

/// Stops an emulator from another thread. Stopping after the emulator was dropped does nothing.
#[derive(Clone)]
pub struct StopHandle(Arc<StopTarget>);

impl StopHandle {
    /// Returns false once the emulator is gone.
    pub fn stop(&self) -> bool {
        let target = self.0.0.lock().unwrap_or_else(PoisonError::into_inner);
        target.as_ref().map(|stop| stop()).is_some()
    }
}

pub(crate) struct StopTarget(Mutex<Option<StopFunction>>);

impl StopTarget {
    pub(crate) fn new(stop: impl Fn() + Send + 'static) -> Arc<Self> {
        Arc::new(Self(Mutex::new(Some(Box::new(stop)))))
    }

    pub(crate) fn handle(self: &Arc<Self>) -> StopHandle {
        StopHandle(self.clone())
    }

    /// Waits for a concurrent stop to finish, so the emulator can be destroyed afterwards.
    pub(crate) fn detach(&self) {
        self.0.lock().unwrap_or_else(PoisonError::into_inner).take();
    }
}

/// The first failure inside a callback, which the call that ran the guest returns once the guest has stopped.
#[derive(Default)]
pub(crate) struct PendingError(Mutex<Option<Error>>);

impl PendingError {
    pub(crate) fn record(&self, error: Error) {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get_or_insert(error);
    }

    pub(crate) fn finish<T>(&self, result: Result<T, cxx::Exception>) -> Result<T, Error> {
        let pending = self.0.lock().unwrap_or_else(PoisonError::into_inner).take();
        match pending {
            Some(error) => Err(error),
            None => Ok(result?),
        }
    }
}
