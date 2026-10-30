#[cxx::bridge(namespace = "sogen::ffi")]
pub mod ffi {
    #[namespace = "sogen"]
    #[cxx_name = "thread_wait_state"]
    #[derive(Debug, Hash)]
    #[repr(u8)]
    enum ThreadWaitState {
        #[cxx_name = "running"]
        Running = 0,
        #[cxx_name = "futex_wait"]
        FutexWait = 1,
        #[cxx_name = "sleeping"]
        Sleeping = 2,
    }

    #[namespace = "sogen"]
    #[cxx_name = "fd_type"]
    #[derive(Debug, Hash)]
    #[repr(i32)]
    enum FileDescriptorType {
        #[cxx_name = "file"]
        File = 0,
        #[cxx_name = "memory_file"]
        MemoryFile = 1,
        #[cxx_name = "pipe_read"]
        PipeRead = 2,
        #[cxx_name = "pipe_write"]
        PipeWrite = 3,
        #[cxx_name = "socket"]
        Socket = 4,
        #[cxx_name = "eventfd"]
        Event = 5,
        #[cxx_name = "directory"]
        Directory = 6,
        #[cxx_name = "epoll"]
        Epoll = 7,
    }

    struct LinuxPathMapping {
        guest: String,
        host: String,
        read_only: bool,
    }

    struct LinuxPortMapping {
        emulator: u16,
        host: u16,
    }

    struct LinuxSettings {
        has_backend: bool,
        backend: BackendType,
        emulation_root: String,
        path_mappings: Vec<LinuxPathMapping>,
        port_mappings: Vec<LinuxPortMapping>,
        library_paths: Vec<String>,
        logging: bool,
        has_application: bool,
        application: String,
        arguments: Vec<String>,
        environment: Vec<String>,
        working_directory: String,
    }

    struct LinuxMemoryRegion {
        start: u64,
        length: u64,
        permissions: MemoryPermission,
        allocation_base: u64,
        allocation_length: u64,
        reserved: bool,
        committed: bool,
        initial_permissions: MemoryPermission,
        kind: MemoryRegionKind,
    }

    struct LinuxMemoryStats {
        region_count: u64,
        mapped_bytes: u64,
        executable_bytes: u64,
    }

    struct LinuxProcess {
        pid: u32,
        ppid: u32,
        uid: u32,
        gid: u32,
        euid: u32,
        egid: u32,
        has_exit_status: bool,
        exit_status: i32,
        thread_count: usize,
        brk_base: u64,
        brk_current: u64,
        working_directory: String,
        executable_path: String,
        arguments: Vec<String>,
        environment: Vec<String>,
    }

    struct LinuxAuxiliaryEntry {
        kind: u64,
        value: u64,
    }

    struct LinuxThread {
        tid: u32,
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

    struct LinuxThreadRegisters {
        rax: u64,
        rbx: u64,
        rcx: u64,
        rdx: u64,
        rsi: u64,
        rdi: u64,
        rbp: u64,
        rsp: u64,
        r8: u64,
        r9: u64,
        r10: u64,
        r11: u64,
        r12: u64,
        r13: u64,
        r14: u64,
        r15: u64,
        rip: u64,
        rflags: u64,
        fs_base: u64,
        gs_base: u64,
    }

    struct LinuxSocket {
        domain: i32,
        socket_type: i32,
        protocol: i32,
        listening: bool,
        connected: bool,
        bound_port: u16,
        bound_address: u32,
        peer_port: u16,
        peer_address: u32,
    }

    struct LinuxFileDescriptor {
        number: i32,
        kind: FileDescriptorType,
        host_path: String,
        guest_path: String,
        flags: i32,
        close_on_exec: bool,
        socket: LinuxSocket,
    }

    struct LinuxExport {
        name: String,
        rva: u64,
        address: u64,
    }

    struct LinuxSection {
        name: String,
        start: u64,
        length: u64,
        permissions: MemoryPermission,
    }

    struct LinuxModule {
        name: String,
        path: String,
        image_base: u64,
        size_of_image: u64,
        entry_point: u64,
        exports: Vec<LinuxExport>,
        needed_libraries: Vec<String>,
        sections: Vec<LinuxSection>,
        rpath: String,
        runpath: String,
    }

    struct LinuxInstruction {
        address: u64,
        bytes: Vec<u8>,
        mnemonic: String,
        operands: String,
        call: bool,
    }

    unsafe extern "C++" {
        include!("cpp/linux.hpp");

        #[namespace = "sogen"]
        #[cxx_name = "backend_type"]
        type BackendType = crate::common::ffi::BackendType;
        #[namespace = "sogen"]
        #[cxx_name = "memory_permission"]
        type MemoryPermission = crate::common::ffi::MemoryPermission;
        #[namespace = "sogen"]
        #[cxx_name = "memory_region_kind"]
        type MemoryRegionKind = crate::common::ffi::MemoryRegionKind;
        #[namespace = "sogen"]
        #[cxx_name = "stop_reason"]
        type StopReason = crate::common::ffi::StopReason;
        #[namespace = "sogen"]
        #[cxx_name = "thread_wait_state"]
        type ThreadWaitState;
        #[namespace = "sogen"]
        #[cxx_name = "fd_type"]
        type FileDescriptorType;
        type Machine = crate::common::ffi::Machine;
        type CallbackHandle = crate::common::ffi::CallbackHandle;

        type LinuxEmulator;

        fn create_linux_emulator(settings: &LinuxSettings) -> Result<UniquePtr<LinuxEmulator>>;

        fn machine(self: &LinuxEmulator) -> &Machine;

        fn start(self: &LinuxEmulator, count: u64) -> Result<()>;
        fn stop(self: &LinuxEmulator);
        fn record_stop(self: &LinuxEmulator, reason: StopReason, detail: &str);
        fn last_stop_reason(self: &LinuxEmulator) -> StopReason;
        fn last_stop_detail(self: &LinuxEmulator) -> String;
        fn executed_instructions(self: &LinuxEmulator) -> u64;
        fn emulation_root(self: &LinuxEmulator) -> String;

        fn serialize_state(self: &LinuxEmulator) -> Result<UniquePtr<CxxVector<u8>>>;
        fn deserialize_state(self: &LinuxEmulator, data: &[u8]) -> Result<()>;

        fn activate_thread(self: &LinuxEmulator, tid: u32) -> Result<bool>;
        fn perform_thread_switch(self: &LinuxEmulator) -> Result<bool>;
        fn yield_thread(self: &LinuxEmulator) -> Result<()>;

        fn map_port(self: &LinuxEmulator, emulator_port: u16, host_port: u16) -> Result<()>;
        fn host_port(self: &LinuxEmulator, emulator_port: u16) -> u16;
        fn emulator_port(self: &LinuxEmulator, host_port: u16) -> u16;

        fn allocate_memory(
            self: &LinuxEmulator,
            size: u64,
            permissions: MemoryPermission,
            start: u64,
        ) -> Result<u64>;
        fn allocate_memory_at(
            self: &LinuxEmulator,
            address: u64,
            size: u64,
            permissions: MemoryPermission,
        ) -> Result<bool>;
        fn protect_memory(
            self: &LinuxEmulator,
            address: u64,
            size: u64,
            permissions: MemoryPermission,
        ) -> Result<bool>;
        fn release_memory(self: &LinuxEmulator, address: u64, size: u64) -> Result<bool>;
        fn find_free_allocation_base(self: &LinuxEmulator, size: u64, start: u64) -> u64;
        fn memory_region(self: &LinuxEmulator, address: u64) -> Vec<LinuxMemoryRegion>;
        fn memory_regions(self: &LinuxEmulator) -> Vec<LinuxMemoryRegion>;
        fn memory_stats(self: &LinuxEmulator) -> LinuxMemoryStats;
        fn mmap_base(self: &LinuxEmulator) -> u64;
        fn set_mmap_base(self: &LinuxEmulator, address: u64);

        fn process(self: &LinuxEmulator) -> LinuxProcess;
        fn threads(self: &LinuxEmulator) -> Vec<LinuxThread>;
        fn thread(self: &LinuxEmulator, tid: u32) -> Vec<LinuxThread>;
        fn current_thread(self: &LinuxEmulator) -> Vec<LinuxThread>;
        fn thread_registers(self: &LinuxEmulator, tid: u32) -> Vec<LinuxThreadRegisters>;
        fn set_thread_registers(
            self: &LinuxEmulator,
            tid: u32,
            registers: &LinuxThreadRegisters,
        ) -> Result<bool>;
        fn auxiliary_vector(self: &LinuxEmulator) -> Vec<LinuxAuxiliaryEntry>;

        fn file_descriptors(self: &LinuxEmulator) -> Vec<LinuxFileDescriptor>;
        fn file_descriptor(self: &LinuxEmulator, number: i32) -> Vec<LinuxFileDescriptor>;

        fn translate_path(self: &LinuxEmulator, path: &str) -> String;
        fn translate_relative_path(self: &LinuxEmulator, base: &str, path: &str) -> String;
        fn is_path_read_only(self: &LinuxEmulator, path: &str) -> bool;
        fn add_mount(self: &LinuxEmulator, mount: &LinuxPathMapping) -> Result<()>;
        fn add_passthrough_prefix(self: &LinuxEmulator, path: &str) -> Result<()>;

        fn modules(self: &LinuxEmulator) -> Vec<LinuxModule>;
        fn module_at(self: &LinuxEmulator, image_base: u64) -> Vec<LinuxModule>;
        fn find_module_by_address(self: &LinuxEmulator, address: u64) -> Vec<LinuxModule>;
        fn find_module_by_name(self: &LinuxEmulator, name: &str) -> Vec<LinuxModule>;
        fn map_local_module(self: &LinuxEmulator, path: &str) -> Result<LinuxModule>;
        fn load_module_dependencies(self: &LinuxEmulator) -> Result<()>;
        fn resolve_library(self: &LinuxEmulator, name: &str, image_base: u64) -> String;

        fn logging(self: &LinuxEmulator) -> bool;
        fn set_logging(self: &LinuxEmulator, logging: bool);
        fn silent(self: &LinuxEmulator) -> bool;
        fn set_silent(self: &LinuxEmulator, silent: bool);

        fn disassemble(
            self: &LinuxEmulator,
            address: u64,
            data: &[u8],
            count: u64,
        ) -> Result<Vec<LinuxInstruction>>;

        fn set_stdout_callback(self: &LinuxEmulator, callback: UniquePtr<CallbackHandle>);
        fn set_stderr_callback(self: &LinuxEmulator, callback: UniquePtr<CallbackHandle>);
        fn set_syscall_callback(self: &LinuxEmulator, callback: UniquePtr<CallbackHandle>);
        fn set_signal_callback(self: &LinuxEmulator, callback: UniquePtr<CallbackHandle>);
        fn set_memory_allocate_callback(self: &LinuxEmulator, callback: UniquePtr<CallbackHandle>);
        fn set_memory_protect_callback(self: &LinuxEmulator, callback: UniquePtr<CallbackHandle>);
        fn set_memory_release_callback(self: &LinuxEmulator, callback: UniquePtr<CallbackHandle>);
        fn set_module_load_callback(self: &LinuxEmulator, callback: UniquePtr<CallbackHandle>);
        fn set_module_observer(self: &LinuxEmulator, callback: UniquePtr<CallbackHandle>);
        fn set_thread_create_callback(self: &LinuxEmulator, callback: UniquePtr<CallbackHandle>);
        fn set_thread_terminated_callback(
            self: &LinuxEmulator,
            callback: UniquePtr<CallbackHandle>,
        );
        fn set_thread_switch_callback(self: &LinuxEmulator, callback: UniquePtr<CallbackHandle>);
        fn set_memory_violation_callback(self: &LinuxEmulator, callback: UniquePtr<CallbackHandle>);
        fn set_periodic_event_callback(self: &LinuxEmulator, callback: UniquePtr<CallbackHandle>);
        fn set_log_callback(self: &LinuxEmulator, callback: UniquePtr<CallbackHandle>);

        fn hook_interrupt(self: &LinuxEmulator, callback: UniquePtr<CallbackHandle>)
        -> Result<u64>;
        fn hook_memory_violation(
            self: &LinuxEmulator,
            callback: UniquePtr<CallbackHandle>,
        ) -> Result<u64>;
    }
}

unsafe impl Send for ffi::LinuxEmulator {}
