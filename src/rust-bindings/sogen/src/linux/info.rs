use std::path::{Path, PathBuf};

use sogen_sys::linux::ffi;

use super::settings::EnvironmentVariable;
use crate::machine::MemoryPermission;

pub use sogen_sys::linux::ffi::ThreadWaitState;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportedSymbol {
    name: String,
    rva: u64,
    address: u64,
}

impl ExportedSymbol {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn rva(&self) -> u64 {
        self.rva
    }

    /// Zero for symbols the module imports rather than defines.
    pub fn address(&self) -> u64 {
        self.address
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    name: String,
    start: u64,
    length: u64,
    permissions: MemoryPermission,
}

impl Section {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn start(&self) -> u64 {
        self.start
    }

    pub fn length(&self) -> u64 {
        self.length
    }

    pub fn permissions(&self) -> MemoryPermission {
        self.permissions
    }
}

/// A mapped ELF image, copied at the time it was requested.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Module {
    name: String,
    path: PathBuf,
    image_base: u64,
    size_of_image: u64,
    entry_point: u64,
    exports: Vec<ExportedSymbol>,
    needed_libraries: Vec<String>,
    sections: Vec<Section>,
    rpath: String,
    runpath: String,
}

impl Module {
    pub(super) fn from_native(module: ffi::LinuxModule) -> Self {
        Self {
            name: module.name,
            path: PathBuf::from(module.path),
            image_base: module.image_base,
            size_of_image: module.size_of_image,
            entry_point: module.entry_point,
            exports: module
                .exports
                .into_iter()
                .map(|symbol| ExportedSymbol {
                    name: symbol.name,
                    rva: symbol.rva,
                    address: symbol.address,
                })
                .collect(),
            needed_libraries: module.needed_libraries,
            sections: module
                .sections
                .into_iter()
                .map(|section| Section {
                    name: section.name,
                    start: section.start,
                    length: section.length,
                    permissions: MemoryPermission::from(section.permissions),
                })
                .collect(),
            rpath: module.rpath,
            runpath: module.runpath,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// The host path the image was loaded from.
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn image_base(&self) -> u64 {
        self.image_base
    }

    pub fn size_of_image(&self) -> u64 {
        self.size_of_image
    }

    pub fn entry_point(&self) -> u64 {
        self.entry_point
    }

    pub fn exports(&self) -> &[ExportedSymbol] {
        &self.exports
    }

    /// The `DT_NEEDED` entries.
    pub fn needed_libraries(&self) -> &[String] {
        &self.needed_libraries
    }

    pub fn sections(&self) -> &[Section] {
        &self.sections
    }

    /// The `DT_RPATH` search path, empty when absent.
    pub fn rpath(&self) -> &str {
        &self.rpath
    }

    /// The `DT_RUNPATH` search path, empty when absent.
    pub fn runpath(&self) -> &str {
        &self.runpath
    }

    pub fn contains(&self, address: u64) -> bool {
        address.wrapping_sub(self.image_base) < self.size_of_image
    }
}

/// A thread's state at the moment it was requested; request it again to observe later changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Thread {
    id: u32,
    stack_base: u64,
    stack_size: u64,
    fs_base: u64,
    current_ip: u64,
    wait_state: ThreadWaitState,
    terminated: bool,
    exit_code: i32,
    executed_instructions: u64,
    active: bool,
    signal_mask: u64,
    clear_child_tid: u64,
    robust_list_head: u64,
}

impl Thread {
    pub(super) fn from_native(thread: ffi::LinuxThread) -> Self {
        Self {
            id: thread.tid,
            stack_base: thread.stack_base,
            stack_size: thread.stack_size,
            fs_base: thread.fs_base,
            current_ip: thread.current_ip,
            wait_state: thread.wait_state,
            terminated: thread.terminated,
            exit_code: thread.exit_code,
            executed_instructions: thread.executed_instructions,
            active: thread.active,
            signal_mask: thread.signal_mask,
            clear_child_tid: thread.clear_child_tid,
            robust_list_head: thread.robust_list_head,
        }
    }

    pub fn id(&self) -> u32 {
        self.id
    }

    pub fn stack_base(&self) -> u64 {
        self.stack_base
    }

    pub fn stack_size(&self) -> u64 {
        self.stack_size
    }

    pub fn fs_base(&self) -> u64 {
        self.fs_base
    }

    /// The live instruction pointer for the active thread, the saved one otherwise.
    pub fn current_ip(&self) -> u64 {
        self.current_ip
    }

    pub fn wait_state(&self) -> ThreadWaitState {
        self.wait_state
    }

    pub fn is_terminated(&self) -> bool {
        self.terminated
    }

    pub fn exit_code(&self) -> i32 {
        self.exit_code
    }

    pub fn executed_instructions(&self) -> u64 {
        self.executed_instructions
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    /// The blocked signals; bit `n - 1` stands for signal `n`.
    pub fn signal_mask(&self) -> u64 {
        self.signal_mask
    }

    /// The address set through `set_tid_address` or `CLONE_CHILD_CLEARTID`, zero if none.
    pub fn clear_child_tid(&self) -> u64 {
        self.clear_child_tid
    }

    /// The address registered with `set_robust_list`, zero if none.
    pub fn robust_list_head(&self) -> u64 {
        self.robust_list_head
    }
}

/// The process state at the moment it was requested.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessInfo {
    pid: u32,
    ppid: u32,
    uid: u32,
    gid: u32,
    euid: u32,
    egid: u32,
    exit_status: Option<i32>,
    thread_count: usize,
    brk_base: u64,
    brk_current: u64,
    working_directory: PathBuf,
    executable_path: PathBuf,
    arguments: Vec<String>,
    environment: Vec<EnvironmentVariable>,
}

impl ProcessInfo {
    pub(super) fn from_native(process: ffi::LinuxProcess) -> Self {
        Self {
            pid: process.pid,
            ppid: process.ppid,
            uid: process.uid,
            gid: process.gid,
            euid: process.euid,
            egid: process.egid,
            exit_status: process.has_exit_status.then_some(process.exit_status),
            thread_count: process.thread_count,
            brk_base: process.brk_base,
            brk_current: process.brk_current,
            working_directory: PathBuf::from(process.working_directory),
            executable_path: PathBuf::from(process.executable_path),
            arguments: process.arguments,
            environment: process
                .environment
                .into_iter()
                .map(EnvironmentVariable::from_native)
                .collect(),
        }
    }

    pub fn pid(&self) -> u32 {
        self.pid
    }

    pub fn ppid(&self) -> u32 {
        self.ppid
    }

    pub fn uid(&self) -> u32 {
        self.uid
    }

    pub fn gid(&self) -> u32 {
        self.gid
    }

    pub fn euid(&self) -> u32 {
        self.euid
    }

    pub fn egid(&self) -> u32 {
        self.egid
    }

    /// The exit status once the process exited normally.
    pub fn exit_status(&self) -> Option<i32> {
        self.exit_status
    }

    /// The number of threads the process ever created, terminated ones included.
    pub fn thread_count(&self) -> usize {
        self.thread_count
    }

    /// Where the heap starts.
    pub fn brk_base(&self) -> u64 {
        self.brk_base
    }

    /// The current program break, the end of the heap.
    pub fn brk_current(&self) -> u64 {
        self.brk_current
    }

    /// The guest working directory.
    pub fn working_directory(&self) -> &Path {
        &self.working_directory
    }

    /// The guest path of the executable.
    pub fn executable_path(&self) -> &Path {
        &self.executable_path
    }

    /// The process's `argv`, starting with `argv[0]`.
    pub fn arguments(&self) -> &[String] {
        &self.arguments
    }

    pub fn environment(&self) -> &[EnvironmentVariable] {
        &self.environment
    }
}

/// The `AT_*` type of an auxiliary vector entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AuxiliaryKind {
    Base,
    BasePlatform,
    Clktck,
    Egid,
    Entry,
    Euid,
    Execfd,
    Execfn,
    Flags,
    Gid,
    Hwcap,
    Hwcap2,
    Ignore,
    Notelf,
    Other(u64),
    Pagesz,
    Phdr,
    Phent,
    Phnum,
    Platform,
    Random,
    Secure,
    SysinfoEhdr,
    Uid,
}

/// An entry of the auxiliary vector the process started with; the terminating `AT_NULL` is not included.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuxiliaryEntry {
    kind: AuxiliaryKind,
    value: u64,
}

impl AuxiliaryEntry {
    pub(super) fn from_native(entry: ffi::LinuxAuxiliaryEntry) -> Self {
        let kind = match entry.kind {
            1 => AuxiliaryKind::Ignore,
            2 => AuxiliaryKind::Execfd,
            3 => AuxiliaryKind::Phdr,
            4 => AuxiliaryKind::Phent,
            5 => AuxiliaryKind::Phnum,
            6 => AuxiliaryKind::Pagesz,
            7 => AuxiliaryKind::Base,
            8 => AuxiliaryKind::Flags,
            9 => AuxiliaryKind::Entry,
            10 => AuxiliaryKind::Notelf,
            11 => AuxiliaryKind::Uid,
            12 => AuxiliaryKind::Euid,
            13 => AuxiliaryKind::Gid,
            14 => AuxiliaryKind::Egid,
            15 => AuxiliaryKind::Platform,
            16 => AuxiliaryKind::Hwcap,
            17 => AuxiliaryKind::Clktck,
            23 => AuxiliaryKind::Secure,
            24 => AuxiliaryKind::BasePlatform,
            25 => AuxiliaryKind::Random,
            26 => AuxiliaryKind::Hwcap2,
            31 => AuxiliaryKind::Execfn,
            33 => AuxiliaryKind::SysinfoEhdr,
            other => AuxiliaryKind::Other(other),
        };
        Self {
            kind,
            value: entry.value,
        }
    }

    pub fn kind(&self) -> AuxiliaryKind {
        self.kind
    }

    pub fn value(&self) -> u64 {
        self.value
    }
}
