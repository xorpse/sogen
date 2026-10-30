use std::time::Duration;

use sogen_sys::windows::ffi;

/// A snapshot of the emulated process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessInfo {
    is_wow64: bool,
    exit_status: Option<u32>,
    live_thread_count: u64,
    spawned_thread_count: u32,
    process_id: u32,
    initial_thread_id: u32,
    peb_address: u64,
    compatibility_peb_address: Option<u64>,
    process_parameters_address: u64,
    hard_error_mode: u32,
}

impl ProcessInfo {
    pub(super) fn from_native(process: ffi::ProcessInfo) -> Self {
        Self {
            is_wow64: process.is_wow64,
            exit_status: process.has_exit_status.then_some(process.exit_status),
            live_thread_count: process.live_thread_count,
            spawned_thread_count: process.spawned_thread_count,
            process_id: process.process_id,
            initial_thread_id: process.initial_thread_id,
            peb_address: process.peb_address,
            compatibility_peb_address: process
                .has_compatibility_peb
                .then_some(process.compatibility_peb_address),
            process_parameters_address: process.process_parameters_address,
            hard_error_mode: process.hard_error_mode,
        }
    }

    /// Whether a 32-bit executable runs on the 64-bit system.
    pub fn is_wow64(&self) -> bool {
        self.is_wow64
    }

    /// The NTSTATUS the process exited with, once it has.
    pub fn exit_status(&self) -> Option<u32> {
        self.exit_status
    }

    pub fn live_thread_count(&self) -> u64 {
        self.live_thread_count
    }

    pub fn spawned_thread_count(&self) -> u32 {
        self.spawned_thread_count
    }

    pub fn process_id(&self) -> u32 {
        self.process_id
    }

    pub fn initial_thread_id(&self) -> u32 {
        self.initial_thread_id
    }

    /// The 64-bit PEB.
    pub fn peb_address(&self) -> u64 {
        self.peb_address
    }

    /// The 32-bit PEB of a WoW64 process.
    pub fn compatibility_peb_address(&self) -> Option<u64> {
        self.compatibility_peb_address
    }

    /// The 64-bit `RTL_USER_PROCESS_PARAMETERS`.
    pub fn process_parameters_address(&self) -> u64 {
        self.process_parameters_address
    }

    /// The mode `NtSetInformationProcess(ProcessDefaultHardErrorMode)` set.
    pub fn hard_error_mode(&self) -> u32 {
        self.hard_error_mode
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThreadStack {
    base: u64,
    size: u64,
}

impl ThreadStack {
    /// The lowest address of the stack.
    pub fn base(&self) -> u64 {
        self.base
    }

    pub fn size(&self) -> u64 {
        self.size
    }
}

/// What a blocked thread waits for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WaitState {
    handles: Vec<u64>,
    waits_for_any: bool,
    timeout: Option<Duration>,
    alertable: bool,
}

impl WaitState {
    /// The objects waited for; empty for a wait without objects, such as a delay.
    pub fn handles(&self) -> &[u64] {
        &self.handles
    }

    /// Whether one signalled object ends the wait rather than all of them.
    pub fn waits_for_any(&self) -> bool {
        self.waits_for_any
    }

    /// The time left on the emulator clock until the wait times out; `None` for an unbounded wait.
    pub fn timeout(&self) -> Option<Duration> {
        self.timeout
    }

    /// Whether a queued user APC ends the wait.
    pub fn alertable(&self) -> bool {
        self.alertable
    }
}

/// A snapshot of a guest thread.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Thread {
    id: u32,
    name: String,
    start_address: u64,
    argument: u64,
    executed_instructions: u64,
    executed_blocks: u64,
    current_ip: u64,
    previous_ip: u64,
    setup_done: bool,
    exit_status: Option<u32>,
    stack: ThreadStack,
    wow64_stack: Option<ThreadStack>,
    teb_address: Option<u64>,
    compatibility_teb_address: Option<u64>,
    suspend_count: u32,
    debugger_hide: bool,
    create_flags: u32,
    wait_state: Option<WaitState>,
    pending_apc_count: u64,
}

impl Thread {
    pub(super) fn from_native(thread: ffi::ThreadInfo) -> Self {
        Self {
            id: thread.id,
            name: thread.name,
            start_address: thread.start_address,
            argument: thread.argument,
            executed_instructions: thread.executed_instructions,
            executed_blocks: thread.executed_blocks,
            current_ip: thread.current_ip,
            previous_ip: thread.previous_ip,
            setup_done: thread.setup_done,
            exit_status: thread.has_exit_status.then_some(thread.exit_status),
            stack: ThreadStack {
                base: thread.stack_base,
                size: thread.stack_size,
            },
            wow64_stack: thread.has_wow64_stack.then_some(ThreadStack {
                base: thread.wow64_stack_base,
                size: thread.wow64_stack_size,
            }),
            teb_address: thread.has_teb.then_some(thread.teb_address),
            compatibility_teb_address: thread
                .has_compatibility_teb
                .then_some(thread.compatibility_teb_address),
            suspend_count: thread.suspended,
            debugger_hide: thread.debugger_hide,
            create_flags: thread.create_flags,
            wait_state: thread.waiting.then(|| WaitState {
                handles: thread.waited_handles,
                waits_for_any: thread.wait_any,
                timeout: thread
                    .has_timeout
                    .then(|| Duration::from_nanos(thread.timeout_nanoseconds)),
                alertable: thread.alertable,
            }),
            pending_apc_count: thread.pending_apc_count,
        }
    }

    pub fn id(&self) -> u32 {
        self.id
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn start_address(&self) -> u64 {
        self.start_address
    }

    pub fn argument(&self) -> u64 {
        self.argument
    }

    pub fn executed_instructions(&self) -> u64 {
        self.executed_instructions
    }

    pub fn executed_blocks(&self) -> u64 {
        self.executed_blocks
    }

    /// The instruction pointer as of the last executed instruction the emulator tracked.
    pub fn current_ip(&self) -> u64 {
        self.current_ip
    }

    pub fn previous_ip(&self) -> u64 {
        self.previous_ip
    }

    pub fn setup_done(&self) -> bool {
        self.setup_done
    }

    pub fn exit_status(&self) -> Option<u32> {
        self.exit_status
    }

    /// The native 64-bit stack.
    pub fn stack(&self) -> ThreadStack {
        self.stack
    }

    /// The 32-bit stack of a WoW64 thread.
    pub fn wow64_stack(&self) -> Option<ThreadStack> {
        self.wow64_stack
    }

    /// The 64-bit TEB.
    pub fn teb_address(&self) -> Option<u64> {
        self.teb_address
    }

    /// The 32-bit TEB of a WoW64 thread.
    pub fn compatibility_teb_address(&self) -> Option<u64> {
        self.compatibility_teb_address
    }

    /// The thread runs once this is zero.
    pub fn suspend_count(&self) -> u32 {
        self.suspend_count
    }

    /// Whether the thread is hidden from debuggers (`ThreadHideFromDebugger`).
    pub fn debugger_hide(&self) -> bool {
        self.debugger_hide
    }

    /// The `THREAD_CREATE_FLAGS_*` the thread was created with.
    pub fn create_flags(&self) -> u32 {
        self.create_flags
    }

    /// `None` while the thread is not blocked.
    pub fn wait_state(&self) -> Option<&WaitState> {
        self.wait_state.as_ref()
    }

    pub fn pending_apc_count(&self) -> u64 {
        self.pending_apc_count
    }
}
