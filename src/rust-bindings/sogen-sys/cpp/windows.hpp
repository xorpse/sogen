#pragma once

#include "common.hpp"

#include <emulator_process_target.hpp>
#include <subprocess_process_manager.hpp>
#include <windows_emulator.hpp>

#include <array>
#include <condition_variable>
#include <mutex>
#include <string>

namespace sogen::ffi
{
    struct DnsResolver;
    struct MmioDevice;
    struct Clock;
    struct SocketFactory;
    struct FilesystemProvider;
    struct EmulatorSettings;
    struct ApplicationSettings;
    struct MountSpec;
    struct ProcessInfo;
    struct ThreadInfo;
    struct RegionInfo;
    struct MemoryStats;
    struct ModuleInfo;
    struct MappingOptions;
    struct NamedAddress;
    struct HandleRecord;
    struct RegistryData;
    struct FileStat;
    struct DirectoryEntryInfo;
    struct WindowsVersionInfo;
    enum class Event : uint8_t;
    enum class CodeMode : uint8_t;
    enum class RegistryLookup : uint8_t;

    inline constexpr size_t event_count = 24;

    struct ManagedProcess
    {
        managed_process_connection connection;

        ApplicationSettings application() const;
    };

    // Serves the parent's requests only while the owner runs the emulator: between runs the owner uses it without the
    // kernel lock, so the control thread must not touch it then.
    class gated_process_target final : public managed_process_target
    {
      public:
        explicit gated_process_target(windows_emulator& emulator);

        void open();
        void close();

        uint64_t native_environment() const override;
        uint64_t compatibility_environment() const override;
        uint64_t native_parameters() const override;
        uint64_t compatibility_parameters() const override;
        process_memory_result allocate_memory(uint64_t address, uint64_t size, memory_permission permission, bool reserve,
                                              bool commit) override;
        process_error free_memory(uint64_t address, uint64_t size, bool release) override;
        process_memory_result protect_memory(uint64_t address, uint64_t size, memory_permission permission) override;
        process_memory_read_result read_memory(uint64_t address, uint64_t size) const override;
        process_memory_result write_memory(uint64_t address, std::span<const uint8_t> data) override;

      private:
        emulator_process_target target_;
        mutable std::mutex mutex_{};
        mutable std::condition_variable opened_{};
        bool open_{};

        template <typename Function>
        decltype(auto) when_open(Function&& fn) const
        {
            std::unique_lock lock(this->mutex_);
            this->opened_.wait(lock, [this] { return this->open_; });
            return std::forward<Function>(fn)();
        }
    };

    struct ManagedChild
    {
        std::unique_ptr<ManagedProcess> process;
        std::unique_ptr<gated_process_target> target;
        bool awaiting_resume{true};
        bool exit_reported{};
    };

    class WindowsEmulator
    {
      public:
        WindowsEmulator(std::unique_ptr<process_manager> processes, std::unique_ptr<windows_emulator> emulator, std::string configuration,
                        std::unique_ptr<ManagedChild> managed_child = {});
        ~WindowsEmulator();

        WindowsEmulator(const WindowsEmulator&) = delete;
        WindowsEmulator& operator=(const WindowsEmulator&) = delete;
        WindowsEmulator(WindowsEmulator&&) = delete;
        WindowsEmulator& operator=(WindowsEmulator&&) = delete;

        const Machine& machine() const;

        void start(uint64_t count) const;
        void stop() const;
        void setup_process_if_necessary() const;
        void yield_thread(bool alertable) const;
        bool perform_thread_switch() const;
        bool activate_thread(uint32_t id) const;
        void save_snapshot() const;
        void restore_snapshot() const;
        ::rust::Vec<uint8_t> serialize_state() const;
        void deserialize_state(::rust::Slice<const uint8_t> data) const;
        void write_snapshot(::rust::Str path) const;

        uint64_t executed_instructions() const;
        stop_reason last_stop_reason() const;
        ::rust::String last_stop_detail() const;
        ::rust::String emulation_root() const;
        uint16_t host_port(uint16_t emulator_port) const;
        uint16_t emulator_port(uint16_t host_port) const;
        void map_port(uint16_t emulator_port, uint16_t host_port) const;

        ProcessInfo process_info() const;
        bool active_thread(ThreadInfo& thread) const;
        bool current_thread_id(uint32_t& id) const;
        uint32_t vcpu_count() const;
        uint32_t current_vcpu() const;
        ::rust::Vec<ThreadInfo> threads() const;
        bool thread(uint32_t id, ThreadInfo& thread) const;
        bool thread_registers(uint32_t id, ::rust::Vec<uint8_t>& registers) const;
        bool set_thread_registers(uint32_t id, ::rust::Slice<const uint8_t> registers) const;
        uint32_t create_thread(uint64_t start_address, uint64_t argument, bool suspended) const;
        bool terminate_thread(uint32_t id, uint32_t exit_status) const;

        ::rust::Vec<HandleRecord> handles() const;
        bool handle_info(uint64_t value, HandleRecord& handle) const;
        bool atom_name(uint16_t id, ::rust::String& name) const;
        bool find_atom(::rust::Str name, uint16_t& id) const;
        uint16_t add_atom(::rust::Str name) const;
        bool delete_atom(uint16_t id) const;

        uint64_t allocate_memory(uint64_t size, memory_permission permissions, bool reserve_only, uint64_t start, memory_region_kind kind,
                                 uint64_t highest_address) const;
        bool allocate_memory_at(uint64_t address, uint64_t size, memory_permission permissions, bool reserve_only,
                                memory_region_kind kind) const;
        bool protect_memory(uint64_t address, uint64_t size, memory_permission permissions) const;
        bool commit_memory(uint64_t address, uint64_t size, memory_permission permissions) const;
        bool decommit_memory(uint64_t address, uint64_t size) const;
        bool release_memory(uint64_t address, uint64_t size) const;
        uint64_t find_free_allocation_base(uint64_t size, uint64_t start) const;
        bool region_info(uint64_t address, RegionInfo& region) const;
        MemoryStats memory_stats() const;
        uint64_t default_allocation_address() const;
        void set_default_allocation_address(uint64_t address) const;
        ::rust::Vec<RegionInfo> memory_regions() const;
        uint64_t maximum_allocation_address() const;
        bool dep_enabled() const;
        void set_dep_enabled(bool enabled) const;
        bool mapped_filename(uint64_t address, ::rust::String& filename) const;
        bool map_mmio(uint64_t address, uint64_t size, ::rust::Box<MmioDevice> device) const;
        bool map_host_memory_at(uint64_t address, uint64_t size, uint8_t* host_memory, memory_permission permissions) const;
        uint64_t map_host_memory(uint64_t size, uint8_t* host_memory, memory_permission permissions) const;

        RegistryLookup registry_value(::rust::Str key, ::rust::Str name, RegistryData& value) const;
        bool registry_values(::rust::Str key, ::rust::Vec<RegistryData>& values) const;
        bool registry_subkeys(::rust::Str key, ::rust::Vec<::rust::String>& names) const;
        bool create_registry_key(::rust::Str key) const;
        bool set_registry_value(::rust::Str key, ::rust::Str name, uint32_t kind, ::rust::Slice<const uint8_t> data) const;
        void import_registry_file(::rust::Str path) const;
        void import_registry(::rust::Slice<const uint8_t> contents) const;

        ::rust::String translate_path(::rust::Str path) const;
        ::rust::Vec<uint8_t> drives() const;
        bool is_path_writable(::rust::Str path) const;
        bool file_info(::rust::Str path, FileStat& stat) const;
        ::rust::Vec<DirectoryEntryInfo> read_directory(::rust::Str path) const;
        void add_mount(const MountSpec& mount) const;
        void mount_provider(::rust::Str guest_path, filesystem_mode mode, ::rust::Box<FilesystemProvider> provider) const;

        WindowsVersionInfo windows_version() const;
        bool logging() const;
        void set_logging(bool logging) const;
        bool silent() const;
        void set_silent(bool silent) const;

        ::rust::Vec<ModuleInfo> modules() const;
        bool find_module_by_address(uint64_t address, ModuleInfo& module) const;
        bool find_module_by_name(::rust::Str name, ModuleInfo& module) const;
        ::rust::Vec<NamedAddress> named_addresses(::rust::Str name) const;
        void map_module(::rust::Str path, const MappingOptions& options, ModuleInfo& module) const;
        void map_local_module(::rust::Str host_path, ::rust::Str module_path, const MappingOptions& options, ModuleInfo& module) const;
        void map_memory_module(uint64_t address, uint64_t size, ::rust::Str module_path, const MappingOptions& options,
                               ModuleInfo& module) const;
        bool unmap_module(uint64_t image_base) const;
        bool module_load_count(::rust::Str path, uint64_t& count) const;

        CodeMode code_mode() const;
        uint64_t function_argument(function_calling_convention convention, uint64_t index) const;
        uint64_t stack_pointer() const;
        void set_stack_pointer(uint64_t value) const;
        void set_return_value(uint64_t value) const;
        void set_instruction_pointer(uint64_t value) const;

        void set_callback(Event event, std::unique_ptr<CallbackHandle> callback) const;
        void observe_modules(std::unique_ptr<CallbackHandle> load, std::unique_ptr<CallbackHandle> unload) const;

      private:
        // Declared before the emulator so native events raised while it is destroyed still find (empty) slots.
        mutable std::array<shared_callback, event_count> callbacks_{};
        mutable shared_callback module_load_observer_{};
        mutable shared_callback module_unload_observer_{};

        // Declared before the emulator, which holds a raw pointer to it; destroying it kills the child processes.
        std::unique_ptr<process_manager> processes_{};
        std::unique_ptr<windows_emulator> emulator_{};
        Machine machine_;

        // The host setup a snapshot file depends on; loading one under a different setup fails.
        mutable std::string configuration_{};

        // Declared after the emulator: its control thread borrows the emulator until the destructor disconnects it.
        std::unique_ptr<ManagedChild> managed_child_{};

        void install_callbacks();
        uint64_t emit(Event event, uint64_t a = 0, uint64_t b = 0, uint64_t c = 0, uint64_t d = 0, std::span<const std::byte> first = {},
                      std::span<const std::byte> second = {}) const;

        template <typename Function>
        decltype(auto) run_serialised(Function&& fn) const
        {
            auto& emulator = *this->emulator_;
            return emulator.kernel_lock_held() ? fn() : emulator.synchronize(std::forward<Function>(fn));
        }
    };

    std::unique_ptr<WindowsEmulator> create_empty_windows_emulator(const EmulatorSettings& settings, machine_type architecture,
                                                                   ::rust::Box<DnsResolver> resolver, ::rust::Box<Clock> clock,
                                                                   ::rust::Box<SocketFactory> socket_factory, bool& unsupported_vcpu_count);
    std::unique_ptr<WindowsEmulator> create_windows_application_emulator(const EmulatorSettings& settings,
                                                                         const ApplicationSettings& application,
                                                                         ::rust::Box<DnsResolver> resolver, ::rust::Box<Clock> clock,
                                                                         ::rust::Box<SocketFactory> socket_factory,
                                                                         bool& unsupported_vcpu_count);
    std::unique_ptr<WindowsEmulator> create_windows_snapshot_emulator(const EmulatorSettings& settings, ::rust::Str path,
                                                                      ::rust::Box<DnsResolver> resolver, ::rust::Box<Clock> clock,
                                                                      ::rust::Box<SocketFactory> socket_factory,
                                                                      bool& unsupported_vcpu_count);
    std::unique_ptr<WindowsEmulator> create_windows_minidump_emulator(const EmulatorSettings& settings, ::rust::Str path,
                                                                      ::rust::Box<DnsResolver> resolver, ::rust::Box<Clock> clock,
                                                                      ::rust::Box<SocketFactory> socket_factory,
                                                                      bool& unsupported_vcpu_count);
    std::unique_ptr<WindowsEmulator> create_windows_managed_process_emulator(const EmulatorSettings& settings,
                                                                             std::unique_ptr<ManagedProcess> process,
                                                                             ::rust::Box<DnsResolver> resolver, ::rust::Box<Clock> clock,
                                                                             ::rust::Box<SocketFactory> socket_factory,
                                                                             bool& unsupported_vcpu_count);
    std::unique_ptr<ManagedProcess> connect_managed_process(::rust::Slice<const ::rust::String> arguments);
}
