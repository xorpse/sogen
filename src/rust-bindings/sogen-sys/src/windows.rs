use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6};

#[cxx::bridge(namespace = "sogen::ffi")]
pub mod ffi {
    #[namespace = "sogen"]
    #[cxx_name = "machine_type"]
    #[derive(Debug, Hash)]
    #[repr(u8)]
    enum Architecture {
        #[cxx_name = "x86_64"]
        X86_64 = 0,
        #[cxx_name = "arm64"]
        Arm64 = 1,
    }

    #[namespace = "sogen"]
    #[cxx_name = "filesystem_mode"]
    #[derive(Debug, Hash)]
    #[repr(i32)]
    enum FilesystemMode {
        #[cxx_name = "read_only"]
        ReadOnly = 0,
        #[cxx_name = "read_write"]
        ReadWrite = 1,
    }

    #[namespace = "sogen::network"]
    #[cxx_name = "network_mode"]
    #[derive(Debug, Hash)]
    #[repr(u8)]
    enum NetworkMode {
        #[cxx_name = "offline"]
        Offline = 0,
        #[cxx_name = "live"]
        Live = 1,
    }

    #[namespace = "sogen::network"]
    #[cxx_name = "socket_activity"]
    #[derive(Debug, Hash)]
    #[repr(u8)]
    enum SocketActivity {
        #[cxx_name = "connect"]
        Connect = 0,
        #[cxx_name = "bind"]
        Bind = 1,
        #[cxx_name = "sendto"]
        SendTo = 2,
    }

    #[namespace = "sogen"]
    #[cxx_name = "function_calling_convention"]
    #[derive(Debug, Hash)]
    #[repr(i32)]
    enum CallingConvention {
        #[cxx_name = "x86_cdecl"]
        Cdecl = 0,
        #[cxx_name = "x86_stdcall"]
        Stdcall = 1,
        #[cxx_name = "x64_fastcall"]
        Fastcall = 2,
        #[cxx_name = "x64_syscall"]
        Syscall = 3,
    }

    #[namespace = "sogen"]
    #[cxx_name = "EVENT_TYPE"]
    #[derive(Debug, Hash)]
    #[repr(i32)]
    enum EventType {
        #[cxx_name = "NotificationEvent"]
        Notification = 0,
        #[cxx_name = "SynchronizationEvent"]
        Synchronization = 1,
    }

    /// Mirrors `sogen::handle_types::type`, which the shim checks value by value.
    #[derive(Debug, Hash)]
    #[repr(u16)]
    enum HandleType {
        Reserved = 0,
        File = 1,
        Device = 2,
        Event = 3,
        Section = 4,
        Symlink = 5,
        Directory = 6,
        Semaphore = 7,
        Port = 8,
        Thread = 9,
        Registry = 10,
        Mutant = 11,
        Token = 12,
        Window = 13,
        Menu = 14,
        Timer = 15,
        UserTimer = 16,
        Monitor = 17,
        Desktop = 18,
        IoCompletion = 19,
        WaitCompletionPacket = 20,
        WorkerFactory = 21,
        PrivateNamespace = 22,
        Process = 23,
        AcceleratorTable = 24,
        ManagedThread = 25,
        Job = 26,
        DeferredWindowPositions = 27,
    }

    #[derive(Debug)]
    #[repr(u8)]
    enum Event {
        ModuleLoad,
        ModuleUnload,
        Stdout,
        Syscall,
        GenericAccess,
        GenericActivity,
        SuspiciousActivity,
        SocketActivity,
        Exception,
        Instruction,
        MemoryProtect,
        MemoryAllocate,
        MemoryViolate,
        Rdtsc,
        Rdtscp,
        Ioctrl,
        DebugString,
        ThreadCreate,
        ThreadTerminated,
        ThreadSetName,
        ThreadSwitch,
        FastFail,
        Log,
        SectionFirstExecution,
    }

    #[derive(Debug)]
    #[repr(u8)]
    enum RegistryLookup {
        Found,
        MissingKey,
        MissingValue,
    }

    #[derive(Debug)]
    #[repr(u8)]
    enum CodeMode {
        X64,
        X86,
        Arm64,
    }

    struct MountSpec {
        host_path: String,
        guest_path: String,
        mode: FilesystemMode,
    }

    struct PortMapping {
        emulator_port: u16,
        host_port: u16,
    }

    struct EmulatorSettings {
        has_backend: bool,
        backend: BackendType,
        logging: bool,
        use_relative_time: bool,
        use_instruction_precision: bool,
        intercept_rdtsc: bool,
        intercept_kusd: bool,
        emulation_root: String,
        registry_directory: String,
        load_registry: bool,
        port_mappings: Vec<PortMapping>,
        root_mode: FilesystemMode,
        mounts: Vec<MountSpec>,
        number_of_processors: u32,
        nt_product_type: u8,
        network: NetworkMode,
        spoof_gpu_features: bool,
        vcpu_count: u32,
        child_executable: String,
        child_arguments: Vec<String>,
    }

    struct EnvironmentVariable {
        name: String,
        value: String,
    }

    struct ApplicationSettings {
        application: String,
        argument0: String,
        working_directory: String,
        arguments: Vec<String>,
        has_command_line: bool,
        command_line: String,
        environment: Vec<EnvironmentVariable>,
        has_process_id: bool,
        process_id: u32,
        has_thread_id: bool,
        thread_id: u32,
    }

    struct ProcessInfo {
        is_wow64: bool,
        has_exit_status: bool,
        exit_status: u32,
        live_thread_count: u64,
        spawned_thread_count: u32,
        process_id: u32,
        initial_thread_id: u32,
        peb_address: u64,
        has_compatibility_peb: bool,
        compatibility_peb_address: u64,
        process_parameters_address: u64,
        hard_error_mode: u32,
    }

    #[derive(Default)]
    struct ThreadInfo {
        id: u32,
        name: String,
        start_address: u64,
        argument: u64,
        executed_instructions: u64,
        executed_blocks: u64,
        current_ip: u64,
        previous_ip: u64,
        setup_done: bool,
        has_exit_status: bool,
        exit_status: u32,
        stack_base: u64,
        stack_size: u64,
        has_wow64_stack: bool,
        wow64_stack_base: u64,
        wow64_stack_size: u64,
        has_teb: bool,
        teb_address: u64,
        has_compatibility_teb: bool,
        compatibility_teb_address: u64,
        suspended: u32,
        debugger_hide: bool,
        create_flags: u32,
        waiting: bool,
        waited_handles: Vec<u64>,
        wait_any: bool,
        has_timeout: bool,
        timeout_nanoseconds: u64,
        alertable: bool,
        pending_apc_count: u64,
    }

    struct RegionInfo {
        start: u64,
        length: u64,
        permissions: MemoryPermission,
        allocation_base: u64,
        allocation_length: u64,
        is_reserved: bool,
        is_committed: bool,
        initial_permissions: MemoryPermission,
        kind: MemoryRegionKind,
    }

    struct MemoryStats {
        reserved_memory: u64,
        committed_memory: u64,
    }

    #[derive(Default)]
    struct ExportedSymbol {
        name: String,
        ordinal: u64,
        rva: u64,
        address: u64,
    }

    #[derive(Default)]
    struct AddressName {
        address: u64,
        name: String,
    }

    #[derive(Default)]
    struct ModuleInfo {
        name: String,
        path: String,
        module_path: String,
        image_base: u64,
        image_base_file: u64,
        size_of_image: u64,
        entry_point: u64,
        is_static: bool,
        exports: Vec<ExportedSymbol>,
        address_names: Vec<AddressName>,
        machine: u16,
        characteristics: u16,
        dll_characteristics: u16,
        size_of_stack_reserve: u64,
        size_of_stack_commit: u64,
        size_of_heap_reserve: u64,
        size_of_heap_commit: u64,
        sections: Vec<SectionInfo>,
        imports: Vec<ImportInfo>,
        imported_modules: Vec<String>,
    }

    struct SectionInfo {
        name: String,
        start: u64,
        length: u64,
        permissions: MemoryPermission,
        has_first_execution: bool,
        first_execution: u64,
    }

    struct ImportInfo {
        module: String,
        name: String,
        slot: u64,
    }

    struct HandleRecord {
        value: u64,
        kind: HandleType,
        name: String,
        path: String,
        id: u32,
        signalled: bool,
        event_type: EventType,
        is_directory: bool,
        current_count: u32,
        maximum_count: u32,
        abandoned: bool,
        has_due_time: bool,
        due_in_nanoseconds: u64,
        period_nanoseconds: u64,
        limit_flags: u32,
        maximum_size: u64,
        page_protection: u32,
        allocation_attributes: u32,
    }

    struct MappingOptions {
        is_static: bool,
        allow_duplicate: bool,
        relocation_base: u64,
    }

    #[derive(Default)]
    struct RegistryData {
        name: String,
        kind: u32,
        data: Vec<u8>,
    }

    #[derive(Default)]
    struct FileStat {
        is_directory: bool,
        size: u64,
        accessed_nanoseconds: i64,
        modified_nanoseconds: i64,
        changed_nanoseconds: i64,
    }

    struct DirectoryEntryInfo {
        name: String,
        short_name: String,
        is_directory: bool,
        has_stat: bool,
        stat: FileStat,
    }

    struct WindowsVersionInfo {
        system_root: String,
        major: u32,
        minor: u32,
        build: u32,
        update_build_revision: u32,
    }

    struct NamedAddress {
        image_base: u64,
        module: String,
        address: u64,
    }

    struct IpAddress {
        ipv6: bool,
        octets: [u8; 16],
    }

    struct DnsAnswer {
        defer: bool,
        addresses: Vec<IpAddress>,
    }

    struct SocketAddress {
        ip: IpAddress,
        port: u16,
        flow_info: u32,
        scope_id: u32,
    }

    #[derive(Debug)]
    #[repr(u8)]
    enum SocketType {
        Datagram,
        Stream,
    }

    #[derive(Debug)]
    #[repr(u8)]
    enum SocketErrorKind {
        Success,
        Os,
        WouldBlock,
        ConnectionRefused,
        ConnectionReset,
        AddressInUse,
        AddressNotAvailable,
        NotConnected,
        InvalidInput,
        Other,
    }

    #[derive(Clone, Copy)]
    struct SocketStatus {
        error: SocketErrorKind,
        os_error: i32,
        count: u64,
    }

    struct SocketReadiness {
        error: bool,
        hang_up: bool,
        readable: bool,
        writable: bool,
    }

    #[derive(Debug)]
    #[repr(u8)]
    enum OpenMode {
        Append,
        AppendRead,
        Read,
        ReadWrite,
        Write,
        WriteRead,
    }

    #[derive(Debug)]
    #[repr(u8)]
    enum FileError {
        Success,
        NotFound,
        PermissionDenied,
        AlreadyExists,
        IsADirectory,
        NotADirectory,
        DirectoryNotEmpty,
        Other,
    }

    #[derive(Debug)]
    #[repr(u8)]
    enum SeekOrigin {
        Start,
        Current,
        End,
    }

    extern "Rust" {
        type DnsResolver;

        fn is_active(self: &DnsResolver) -> bool;
        fn resolve(self: &DnsResolver, hostname: &[u8], family: u8) -> DnsAnswer;
    }

    extern "Rust" {
        type MmioDevice;

        fn read(self: &MmioDevice, offset: u64, data: &mut [u8]);
        fn write(self: &MmioDevice, offset: u64, data: &[u8]);
    }

    extern "Rust" {
        type Clock;

        fn is_active(self: &Clock) -> bool;
        fn system_time_nanoseconds(self: &Clock) -> i64;
        fn steady_time_nanoseconds(self: &Clock) -> u64;
        fn timestamp_counter(self: &Clock) -> u64;
    }

    extern "Rust" {
        type SocketFactory;

        fn is_active(self: &SocketFactory) -> bool;
        fn create_socket(self: &SocketFactory, family: u8, kind: SocketType) -> Box<Socket>;
    }

    extern "Rust" {
        type Socket;

        fn is_active(self: &Socket) -> bool;
        fn accept(
            self: &mut Socket,
            peer: &mut SocketAddress,
            status: &mut SocketStatus,
        ) -> Box<Socket>;
        fn bind(self: &mut Socket, address: &SocketAddress) -> SocketStatus;
        fn connect(self: &mut Socket, address: &SocketAddress) -> SocketStatus;
        fn is_listening(self: &Socket) -> bool;
        fn listen(self: &mut Socket, backlog: u32) -> SocketStatus;
        fn local_address(self: &Socket, address: &mut SocketAddress) -> bool;
        fn readiness(self: &mut Socket) -> SocketReadiness;
        fn receive(self: &mut Socket, data: &mut [u8]) -> SocketStatus;
        fn receive_from(
            self: &mut Socket,
            data: &mut [u8],
            source: &mut SocketAddress,
        ) -> SocketStatus;
        fn send(self: &mut Socket, data: &[u8]) -> SocketStatus;
        fn send_to(self: &mut Socket, data: &[u8], destination: &SocketAddress) -> SocketStatus;
        fn take_error(self: &mut Socket, pending: &mut SocketStatus) -> bool;
    }

    extern "Rust" {
        type FilesystemProvider;

        fn file_info(self: &FilesystemProvider, path: &str, stat: &mut FileStat) -> bool;
        fn read_directory(self: &FilesystemProvider, path: &str) -> Vec<DirectoryEntryInfo>;
        fn open(
            self: &FilesystemProvider,
            path: &str,
            mode: OpenMode,
            error: &mut FileError,
        ) -> Box<File>;
        fn create_directory(self: &FilesystemProvider, path: &str) -> FileError;
        fn rename(self: &FilesystemProvider, from: &str, to: &str) -> FileError;
        fn remove(self: &FilesystemProvider, path: &str) -> FileError;
        fn host_path(self: &FilesystemProvider, path: &str, host_path: &mut String) -> bool;
    }

    extern "Rust" {
        type File;

        fn is_open(self: &File) -> bool;
        fn read(self: &mut File, data: &mut [u8]) -> usize;
        fn write(self: &mut File, data: &[u8]) -> usize;
        fn flush(self: &mut File) -> bool;
        fn seek(self: &mut File, offset: i64, origin: SeekOrigin) -> bool;
        fn tell(self: &mut File) -> i64;
        fn size(self: &mut File) -> i64;
        fn resize(self: &mut File, size: u64) -> bool;
        fn stat(self: &mut File, stat: &mut FileStat) -> bool;
    }

    unsafe extern "C++" {
        include!("cpp/windows.hpp");

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
        type Machine = crate::common::ffi::Machine;
        type CallbackHandle = crate::common::ffi::CallbackHandle;

        #[namespace = "sogen"]
        #[cxx_name = "machine_type"]
        type Architecture;
        #[namespace = "sogen"]
        #[cxx_name = "filesystem_mode"]
        type FilesystemMode;
        #[namespace = "sogen::network"]
        #[cxx_name = "network_mode"]
        type NetworkMode;
        #[namespace = "sogen::network"]
        #[cxx_name = "socket_activity"]
        type SocketActivity;
        #[namespace = "sogen"]
        #[cxx_name = "function_calling_convention"]
        type CallingConvention;
        #[namespace = "sogen"]
        #[cxx_name = "EVENT_TYPE"]
        type EventType;

        type WindowsEmulator;
        type ManagedProcess;

        fn create_empty_windows_emulator(
            settings: &EmulatorSettings,
            architecture: Architecture,
            resolver: Box<DnsResolver>,
            clock: Box<Clock>,
            socket_factory: Box<SocketFactory>,
            unsupported_vcpu_count: &mut bool,
        ) -> Result<UniquePtr<WindowsEmulator>>;
        fn create_windows_application_emulator(
            settings: &EmulatorSettings,
            application: &ApplicationSettings,
            resolver: Box<DnsResolver>,
            clock: Box<Clock>,
            socket_factory: Box<SocketFactory>,
            unsupported_vcpu_count: &mut bool,
        ) -> Result<UniquePtr<WindowsEmulator>>;
        fn create_windows_snapshot_emulator(
            settings: &EmulatorSettings,
            path: &str,
            resolver: Box<DnsResolver>,
            clock: Box<Clock>,
            socket_factory: Box<SocketFactory>,
            unsupported_vcpu_count: &mut bool,
        ) -> Result<UniquePtr<WindowsEmulator>>;
        fn create_windows_minidump_emulator(
            settings: &EmulatorSettings,
            path: &str,
            resolver: Box<DnsResolver>,
            clock: Box<Clock>,
            socket_factory: Box<SocketFactory>,
            unsupported_vcpu_count: &mut bool,
        ) -> Result<UniquePtr<WindowsEmulator>>;
        fn create_windows_managed_process_emulator(
            settings: &EmulatorSettings,
            process: UniquePtr<ManagedProcess>,
            resolver: Box<DnsResolver>,
            clock: Box<Clock>,
            socket_factory: Box<SocketFactory>,
            unsupported_vcpu_count: &mut bool,
        ) -> Result<UniquePtr<WindowsEmulator>>;

        fn connect_managed_process(arguments: &[String]) -> Result<UniquePtr<ManagedProcess>>;
        fn application(self: &ManagedProcess) -> ApplicationSettings;

        fn machine(self: &WindowsEmulator) -> &Machine;

        fn start(self: &WindowsEmulator, count: u64) -> Result<()>;
        fn stop(self: &WindowsEmulator);
        fn setup_process_if_necessary(self: &WindowsEmulator) -> Result<()>;
        fn yield_thread(self: &WindowsEmulator, alertable: bool) -> Result<()>;
        fn perform_thread_switch(self: &WindowsEmulator) -> Result<bool>;
        fn activate_thread(self: &WindowsEmulator, id: u32) -> Result<bool>;
        fn save_snapshot(self: &WindowsEmulator) -> Result<()>;
        fn restore_snapshot(self: &WindowsEmulator) -> Result<()>;
        fn serialize_state(self: &WindowsEmulator) -> Result<Vec<u8>>;
        fn deserialize_state(self: &WindowsEmulator, data: &[u8]) -> Result<()>;
        fn write_snapshot(self: &WindowsEmulator, path: &str) -> Result<()>;

        fn executed_instructions(self: &WindowsEmulator) -> u64;
        fn last_stop_reason(self: &WindowsEmulator) -> StopReason;
        fn last_stop_detail(self: &WindowsEmulator) -> String;
        fn emulation_root(self: &WindowsEmulator) -> String;
        fn host_port(self: &WindowsEmulator, emulator_port: u16) -> u16;
        fn emulator_port(self: &WindowsEmulator, host_port: u16) -> u16;
        fn map_port(self: &WindowsEmulator, emulator_port: u16, host_port: u16);

        fn process_info(self: &WindowsEmulator) -> ProcessInfo;
        fn active_thread(self: &WindowsEmulator, thread: &mut ThreadInfo) -> bool;
        fn current_thread_id(self: &WindowsEmulator, id: &mut u32) -> bool;
        fn vcpu_count(self: &WindowsEmulator) -> u32;
        fn current_vcpu(self: &WindowsEmulator) -> u32;
        fn threads(self: &WindowsEmulator) -> Vec<ThreadInfo>;
        fn thread(self: &WindowsEmulator, id: u32, thread: &mut ThreadInfo) -> bool;
        fn thread_registers(
            self: &WindowsEmulator,
            id: u32,
            registers: &mut Vec<u8>,
        ) -> Result<bool>;
        fn set_thread_registers(self: &WindowsEmulator, id: u32, registers: &[u8]) -> Result<bool>;
        fn create_thread(
            self: &WindowsEmulator,
            start_address: u64,
            argument: u64,
            suspended: bool,
        ) -> Result<u32>;
        fn terminate_thread(self: &WindowsEmulator, id: u32, exit_status: u32) -> Result<bool>;

        fn handles(self: &WindowsEmulator) -> Vec<HandleRecord>;
        fn handle_info(self: &WindowsEmulator, value: u64, handle: &mut HandleRecord) -> bool;
        fn atom_name(self: &WindowsEmulator, id: u16, name: &mut String) -> bool;
        fn find_atom(self: &WindowsEmulator, name: &str, id: &mut u16) -> bool;
        fn add_atom(self: &WindowsEmulator, name: &str) -> Result<u16>;
        fn delete_atom(self: &WindowsEmulator, id: u16) -> bool;

        fn allocate_memory(
            self: &WindowsEmulator,
            size: u64,
            permissions: MemoryPermission,
            reserve_only: bool,
            start: u64,
            kind: MemoryRegionKind,
            highest_address: u64,
        ) -> Result<u64>;
        fn allocate_memory_at(
            self: &WindowsEmulator,
            address: u64,
            size: u64,
            permissions: MemoryPermission,
            reserve_only: bool,
            kind: MemoryRegionKind,
        ) -> Result<bool>;
        fn protect_memory(
            self: &WindowsEmulator,
            address: u64,
            size: u64,
            permissions: MemoryPermission,
        ) -> Result<bool>;
        fn commit_memory(
            self: &WindowsEmulator,
            address: u64,
            size: u64,
            permissions: MemoryPermission,
        ) -> Result<bool>;
        fn decommit_memory(self: &WindowsEmulator, address: u64, size: u64) -> Result<bool>;
        fn release_memory(self: &WindowsEmulator, address: u64, size: u64) -> Result<bool>;
        fn find_free_allocation_base(self: &WindowsEmulator, size: u64, start: u64) -> u64;
        fn region_info(self: &WindowsEmulator, address: u64, region: &mut RegionInfo) -> bool;
        fn memory_stats(self: &WindowsEmulator) -> MemoryStats;
        fn default_allocation_address(self: &WindowsEmulator) -> u64;
        fn set_default_allocation_address(self: &WindowsEmulator, address: u64);
        fn memory_regions(self: &WindowsEmulator) -> Vec<RegionInfo>;
        fn maximum_allocation_address(self: &WindowsEmulator) -> u64;
        fn dep_enabled(self: &WindowsEmulator) -> bool;
        fn set_dep_enabled(self: &WindowsEmulator, enabled: bool);
        fn mapped_filename(self: &WindowsEmulator, address: u64, filename: &mut String) -> bool;
        fn map_mmio(
            self: &WindowsEmulator,
            address: u64,
            size: u64,
            device: Box<MmioDevice>,
        ) -> Result<bool>;
        /// # Safety
        ///
        /// `host_memory` must stay valid for `size` bytes until the emulator is destroyed.
        #[allow(clippy::missing_safety_doc)]
        unsafe fn map_host_memory_at(
            self: &WindowsEmulator,
            address: u64,
            size: u64,
            host_memory: *mut u8,
            permissions: MemoryPermission,
        ) -> Result<bool>;
        /// # Safety
        ///
        /// `host_memory` must stay valid for `size` bytes until the emulator is destroyed.
        #[allow(clippy::missing_safety_doc)]
        unsafe fn map_host_memory(
            self: &WindowsEmulator,
            size: u64,
            host_memory: *mut u8,
            permissions: MemoryPermission,
        ) -> Result<u64>;

        fn registry_value(
            self: &WindowsEmulator,
            key: &str,
            name: &str,
            value: &mut RegistryData,
        ) -> Result<RegistryLookup>;
        fn registry_values(
            self: &WindowsEmulator,
            key: &str,
            values: &mut Vec<RegistryData>,
        ) -> Result<bool>;
        fn registry_subkeys(
            self: &WindowsEmulator,
            key: &str,
            names: &mut Vec<String>,
        ) -> Result<bool>;
        fn create_registry_key(self: &WindowsEmulator, key: &str) -> Result<bool>;
        fn set_registry_value(
            self: &WindowsEmulator,
            key: &str,
            name: &str,
            kind: u32,
            data: &[u8],
        ) -> Result<bool>;
        fn import_registry_file(self: &WindowsEmulator, path: &str) -> Result<()>;
        fn import_registry(self: &WindowsEmulator, contents: &[u8]) -> Result<()>;

        fn translate_path(self: &WindowsEmulator, path: &str) -> String;
        fn drives(self: &WindowsEmulator) -> Vec<u8>;
        fn is_path_writable(self: &WindowsEmulator, path: &str) -> bool;
        fn file_info(self: &WindowsEmulator, path: &str, stat: &mut FileStat) -> bool;
        fn read_directory(self: &WindowsEmulator, path: &str) -> Result<Vec<DirectoryEntryInfo>>;
        fn add_mount(self: &WindowsEmulator, mount: &MountSpec) -> Result<()>;
        fn mount_provider(
            self: &WindowsEmulator,
            guest_path: &str,
            mode: FilesystemMode,
            provider: Box<FilesystemProvider>,
        ) -> Result<()>;

        fn windows_version(self: &WindowsEmulator) -> WindowsVersionInfo;
        fn logging(self: &WindowsEmulator) -> bool;
        fn set_logging(self: &WindowsEmulator, logging: bool);
        fn silent(self: &WindowsEmulator) -> bool;
        fn set_silent(self: &WindowsEmulator, silent: bool);

        fn modules(self: &WindowsEmulator) -> Vec<ModuleInfo>;
        fn find_module_by_address(
            self: &WindowsEmulator,
            address: u64,
            module: &mut ModuleInfo,
        ) -> bool;
        fn find_module_by_name(self: &WindowsEmulator, name: &str, module: &mut ModuleInfo)
        -> bool;
        fn named_addresses(self: &WindowsEmulator, name: &str) -> Vec<NamedAddress>;
        fn map_module(
            self: &WindowsEmulator,
            path: &str,
            options: &MappingOptions,
            module: &mut ModuleInfo,
        ) -> Result<()>;
        fn map_local_module(
            self: &WindowsEmulator,
            host_path: &str,
            module_path: &str,
            options: &MappingOptions,
            module: &mut ModuleInfo,
        ) -> Result<()>;
        fn map_memory_module(
            self: &WindowsEmulator,
            address: u64,
            size: u64,
            module_path: &str,
            options: &MappingOptions,
            module: &mut ModuleInfo,
        ) -> Result<()>;
        fn unmap_module(self: &WindowsEmulator, image_base: u64) -> Result<bool>;
        fn module_load_count(self: &WindowsEmulator, path: &str, count: &mut u64) -> bool;

        fn code_mode(self: &WindowsEmulator) -> Result<CodeMode>;
        fn function_argument(
            self: &WindowsEmulator,
            convention: CallingConvention,
            index: u64,
        ) -> Result<u64>;
        fn stack_pointer(self: &WindowsEmulator) -> Result<u64>;
        fn set_stack_pointer(self: &WindowsEmulator, value: u64) -> Result<()>;
        fn set_return_value(self: &WindowsEmulator, value: u64) -> Result<()>;
        fn set_instruction_pointer(self: &WindowsEmulator, value: u64) -> Result<()>;

        fn set_callback(self: &WindowsEmulator, event: Event, callback: UniquePtr<CallbackHandle>);
        fn observe_modules(
            self: &WindowsEmulator,
            load: UniquePtr<CallbackHandle>,
            unload: UniquePtr<CallbackHandle>,
        );
    }
}

unsafe impl Send for ffi::WindowsEmulator {}

type ResolveFunction = dyn Fn(&str, u8) -> Option<Vec<IpAddr>> + Send + Sync;

/// A guest DNS resolver. `family` is 4 or 6 for a lookup restricted to IPv4 or IPv6, 0 for any; returning `None`
/// defers to the resolver of the emulator's network mode.
pub struct DnsResolver(Option<Box<ResolveFunction>>);

impl DnsResolver {
    pub fn new(
        resolve: impl Fn(&str, u8) -> Option<Vec<IpAddr>> + Send + Sync + 'static,
    ) -> Box<Self> {
        Box::new(Self(Some(Box::new(resolve))))
    }

    pub fn inactive() -> Box<Self> {
        Box::new(Self(None))
    }

    fn is_active(&self) -> bool {
        self.0.is_some()
    }

    fn resolve(&self, hostname: &[u8], family: u8) -> ffi::DnsAnswer {
        let answer = self
            .0
            .as_ref()
            .and_then(|resolve| resolve(&String::from_utf8_lossy(hostname), family));
        ffi::DnsAnswer {
            defer: answer.is_none(),
            addresses: answer
                .into_iter()
                .flatten()
                .map(ffi::IpAddress::from)
                .collect(),
        }
    }
}

/// The bridge side of a guest MMIO device, called with the offset into the mapped range.
pub trait MmioDeviceBridge: Send + Sync {
    fn read(&self, offset: u64, data: &mut [u8]);
    fn write(&self, offset: u64, data: &[u8]);
}

pub struct MmioDevice(Box<dyn MmioDeviceBridge>);

impl MmioDevice {
    pub fn new(device: impl MmioDeviceBridge + 'static) -> Box<Self> {
        Box::new(Self(Box::new(device)))
    }

    fn read(&self, offset: u64, data: &mut [u8]) {
        self.0.read(offset, data);
    }

    fn write(&self, offset: u64, data: &[u8]) {
        self.0.write(offset, data);
    }
}

/// The bridge side of a guest clock, called from several host threads.
pub trait ClockBridge: Send + Sync {
    /// Nanoseconds since the Unix epoch, negative before it.
    fn system_time_nanoseconds(&self) -> i64;
    fn steady_time_nanoseconds(&self) -> u64;
    fn timestamp_counter(&self) -> u64;
}

/// A guest clock replacing the host clocks; an inactive one keeps the emulator's own.
pub struct Clock(Option<Box<dyn ClockBridge>>);

impl Clock {
    pub fn new(clock: impl ClockBridge + 'static) -> Box<Self> {
        Box::new(Self(Some(Box::new(clock))))
    }

    pub fn inactive() -> Box<Self> {
        Box::new(Self(None))
    }

    fn is_active(&self) -> bool {
        self.0.is_some()
    }

    fn active(&self) -> &dyn ClockBridge {
        self.0
            .as_deref()
            .expect("native code only installs an active clock")
    }

    fn system_time_nanoseconds(&self) -> i64 {
        self.active().system_time_nanoseconds()
    }

    fn steady_time_nanoseconds(&self) -> u64 {
        self.active().steady_time_nanoseconds()
    }

    fn timestamp_counter(&self) -> u64 {
        self.active().timestamp_counter()
    }
}

/// The bridge side of a guest socket factory. `family` is 4 or 6; returning `None` defers to the socket factory of the
/// emulator's network mode.
pub trait SocketFactoryBridge: Send + Sync {
    fn create_socket(&self, family: u8, kind: ffi::SocketType) -> Option<Box<dyn SocketBridge>>;
}

/// A guest socket factory; an inactive one keeps the factory of the emulator's network mode.
pub struct SocketFactory(Option<Box<dyn SocketFactoryBridge>>);

impl SocketFactory {
    pub fn new(factory: impl SocketFactoryBridge + 'static) -> Box<Self> {
        Box::new(Self(Some(Box::new(factory))))
    }

    pub fn inactive() -> Box<Self> {
        Box::new(Self(None))
    }

    fn is_active(&self) -> bool {
        self.0.is_some()
    }

    fn create_socket(&self, family: u8, kind: ffi::SocketType) -> Box<Socket> {
        let factory = self
            .0
            .as_deref()
            .expect("native code only installs an active socket factory");
        Box::new(Socket(factory.create_socket(family, kind)))
    }
}

/// The bridge side of a non-blocking guest socket, used by one emulator thread at a time.
pub trait SocketBridge: Send {
    fn accept(
        &mut self,
        peer: &mut ffi::SocketAddress,
    ) -> Result<Box<dyn SocketBridge>, ffi::SocketStatus>;
    fn bind(&mut self, address: &ffi::SocketAddress) -> ffi::SocketStatus;
    fn connect(&mut self, address: &ffi::SocketAddress) -> ffi::SocketStatus;
    fn is_listening(&self) -> bool;
    fn listen(&mut self, backlog: u32) -> ffi::SocketStatus;
    fn local_address(&self, address: &mut ffi::SocketAddress) -> bool;
    fn readiness(&mut self) -> ffi::SocketReadiness;
    fn receive(&mut self, data: &mut [u8]) -> ffi::SocketStatus;
    fn receive_from(
        &mut self,
        data: &mut [u8],
        source: &mut ffi::SocketAddress,
    ) -> ffi::SocketStatus;
    fn send(&mut self, data: &[u8]) -> ffi::SocketStatus;
    fn send_to(&mut self, data: &[u8], destination: &ffi::SocketAddress) -> ffi::SocketStatus;
    /// Returns false if the pending error cannot be queried.
    fn take_error(&mut self, pending: &mut ffi::SocketStatus) -> bool;
}

/// A guest socket; an inactive one stands for a socket that was not created.
pub struct Socket(Option<Box<dyn SocketBridge>>);

impl Socket {
    fn is_active(&self) -> bool {
        self.0.is_some()
    }

    fn active(&self) -> &dyn SocketBridge {
        self.0
            .as_deref()
            .expect("native code only uses an active socket")
    }

    fn active_mut(&mut self) -> &mut dyn SocketBridge {
        self.0
            .as_deref_mut()
            .expect("native code only uses an active socket")
    }

    fn accept(
        &mut self,
        peer: &mut ffi::SocketAddress,
        status: &mut ffi::SocketStatus,
    ) -> Box<Socket> {
        let accepted = self.active_mut().accept(peer);
        *status = accepted
            .as_ref()
            .err()
            .copied()
            .unwrap_or(ffi::SocketStatus::success(0));
        Box::new(Socket(accepted.ok()))
    }

    fn bind(&mut self, address: &ffi::SocketAddress) -> ffi::SocketStatus {
        self.active_mut().bind(address)
    }

    fn connect(&mut self, address: &ffi::SocketAddress) -> ffi::SocketStatus {
        self.active_mut().connect(address)
    }

    fn is_listening(&self) -> bool {
        self.active().is_listening()
    }

    fn listen(&mut self, backlog: u32) -> ffi::SocketStatus {
        self.active_mut().listen(backlog)
    }

    fn local_address(&self, address: &mut ffi::SocketAddress) -> bool {
        self.active().local_address(address)
    }

    fn readiness(&mut self) -> ffi::SocketReadiness {
        self.active_mut().readiness()
    }

    fn receive(&mut self, data: &mut [u8]) -> ffi::SocketStatus {
        self.active_mut().receive(data)
    }

    fn receive_from(
        &mut self,
        data: &mut [u8],
        source: &mut ffi::SocketAddress,
    ) -> ffi::SocketStatus {
        self.active_mut().receive_from(data, source)
    }

    fn send(&mut self, data: &[u8]) -> ffi::SocketStatus {
        self.active_mut().send(data)
    }

    fn send_to(&mut self, data: &[u8], destination: &ffi::SocketAddress) -> ffi::SocketStatus {
        self.active_mut().send_to(data, destination)
    }

    fn take_error(&mut self, pending: &mut ffi::SocketStatus) -> bool {
        self.active_mut().take_error(pending)
    }
}

impl ffi::SocketStatus {
    pub fn success(count: u64) -> Self {
        Self {
            error: ffi::SocketErrorKind::Success,
            os_error: 0,
            count,
        }
    }
}

/// The bridge side of a mounted guest file system, over paths relative to the mount point.
pub trait FilesystemProviderBridge: Send + Sync {
    fn file_info(&self, path: &str, stat: &mut ffi::FileStat) -> bool;
    fn read_directory(&self, path: &str) -> Vec<ffi::DirectoryEntryInfo>;
    fn open(&self, path: &str, mode: ffi::OpenMode) -> Result<Box<dyn FileBridge>, ffi::FileError>;
    fn create_directory(&self, path: &str) -> ffi::FileError;
    fn rename(&self, from: &str, to: &str) -> ffi::FileError;
    fn remove(&self, path: &str) -> ffi::FileError;
    fn host_path(&self, path: &str, host_path: &mut String) -> bool;
}

pub struct FilesystemProvider(Box<dyn FilesystemProviderBridge>);

impl FilesystemProvider {
    pub fn new(provider: impl FilesystemProviderBridge + 'static) -> Box<Self> {
        Box::new(Self(Box::new(provider)))
    }

    fn file_info(&self, path: &str, stat: &mut ffi::FileStat) -> bool {
        self.0.file_info(path, stat)
    }

    fn read_directory(&self, path: &str) -> Vec<ffi::DirectoryEntryInfo> {
        self.0.read_directory(path)
    }

    fn open(&self, path: &str, mode: ffi::OpenMode, error: &mut ffi::FileError) -> Box<File> {
        let opened = self.0.open(path, mode);
        *error = opened
            .as_ref()
            .err()
            .copied()
            .unwrap_or(ffi::FileError::Success);
        Box::new(File(opened.ok()))
    }

    fn create_directory(&self, path: &str) -> ffi::FileError {
        self.0.create_directory(path)
    }

    fn rename(&self, from: &str, to: &str) -> ffi::FileError {
        self.0.rename(from, to)
    }

    fn remove(&self, path: &str) -> ffi::FileError {
        self.0.remove(path)
    }

    fn host_path(&self, path: &str, host_path: &mut String) -> bool {
        self.0.host_path(path, host_path)
    }
}

/// The bridge side of an open guest file, with the semantics of the C stream functions: `read` and `write` transfer as
/// much as they can, `tell` and `size` return -1 on failure.
pub trait FileBridge: Send {
    fn read(&mut self, data: &mut [u8]) -> usize;
    fn write(&mut self, data: &[u8]) -> usize;
    fn flush(&mut self) -> bool;
    fn seek(&mut self, offset: i64, origin: ffi::SeekOrigin) -> bool;
    fn tell(&mut self) -> i64;
    fn size(&mut self) -> i64;
    fn resize(&mut self, size: u64) -> bool;
    fn stat(&mut self, stat: &mut ffi::FileStat) -> bool;
}

/// An open guest file; an inactive one stands for a file that failed to open.
pub struct File(Option<Box<dyn FileBridge>>);

impl File {
    fn is_open(&self) -> bool {
        self.0.is_some()
    }

    fn active(&mut self) -> &mut dyn FileBridge {
        self.0
            .as_deref_mut()
            .expect("native code only uses an open file")
    }

    fn read(&mut self, data: &mut [u8]) -> usize {
        self.active().read(data)
    }

    fn write(&mut self, data: &[u8]) -> usize {
        self.active().write(data)
    }

    fn flush(&mut self) -> bool {
        self.active().flush()
    }

    fn seek(&mut self, offset: i64, origin: ffi::SeekOrigin) -> bool {
        self.active().seek(offset, origin)
    }

    fn tell(&mut self) -> i64 {
        self.active().tell()
    }

    fn size(&mut self) -> i64 {
        self.active().size()
    }

    fn resize(&mut self, size: u64) -> bool {
        self.active().resize(size)
    }

    fn stat(&mut self, stat: &mut ffi::FileStat) -> bool {
        self.active().stat(stat)
    }
}

impl From<IpAddr> for ffi::IpAddress {
    fn from(address: IpAddr) -> Self {
        match address {
            IpAddr::V4(v4) => {
                let mut octets = [0; 16];
                octets[..4].copy_from_slice(&v4.octets());
                Self {
                    ipv6: false,
                    octets,
                }
            }
            IpAddr::V6(v6) => Self {
                ipv6: true,
                octets: v6.octets(),
            },
        }
    }
}

impl From<SocketAddr> for ffi::SocketAddress {
    fn from(address: SocketAddr) -> Self {
        let (flow_info, scope_id) = match address {
            SocketAddr::V4(_) => (0, 0),
            SocketAddr::V6(v6) => (v6.flowinfo(), v6.scope_id()),
        };
        Self {
            ip: address.ip().into(),
            port: address.port(),
            flow_info,
            scope_id,
        }
    }
}

impl From<&ffi::SocketAddress> for SocketAddr {
    fn from(address: &ffi::SocketAddress) -> Self {
        if address.ip.ipv6 {
            return SocketAddr::V6(SocketAddrV6::new(
                Ipv6Addr::from(address.ip.octets),
                address.port,
                address.flow_info,
                address.scope_id,
            ));
        }

        let [a, b, c, d, ..] = address.ip.octets;
        SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(a, b, c, d), address.port))
    }
}

impl Default for ffi::RegionInfo {
    fn default() -> Self {
        Self {
            start: 0,
            length: 0,
            permissions: ffi::MemoryPermission::None,
            allocation_base: 0,
            allocation_length: 0,
            is_reserved: false,
            is_committed: false,
            initial_permissions: ffi::MemoryPermission::None,
            kind: ffi::MemoryRegionKind::Free,
        }
    }
}

impl Default for ffi::HandleRecord {
    fn default() -> Self {
        Self {
            value: 0,
            kind: ffi::HandleType::Reserved,
            name: String::new(),
            path: String::new(),
            id: 0,
            signalled: false,
            event_type: ffi::EventType::Notification,
            is_directory: false,
            current_count: 0,
            maximum_count: 0,
            abandoned: false,
            has_due_time: false,
            due_in_nanoseconds: 0,
            period_nanoseconds: 0,
            limit_flags: 0,
            maximum_size: 0,
            page_protection: 0,
            allocation_attributes: 0,
        }
    }
}
