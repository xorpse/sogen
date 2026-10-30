#pragma once

#include "common.hpp"

#include <disassembler.hpp>
#include <linux_emulator.hpp>

namespace sogen::ffi
{
    struct LinuxSettings;
    struct LinuxPathMapping;
    struct LinuxMemoryRegion;
    struct LinuxMemoryStats;
    struct LinuxProcess;
    struct LinuxAuxiliaryEntry;
    struct LinuxThread;
    struct LinuxThreadRegisters;
    struct LinuxFileDescriptor;
    struct LinuxModule;
    struct LinuxInstruction;

    class LinuxEmulator
    {
      public:
        explicit LinuxEmulator(std::unique_ptr<linux_emulator> emulator);
        ~LinuxEmulator();

        LinuxEmulator(const LinuxEmulator&) = delete;
        LinuxEmulator& operator=(const LinuxEmulator&) = delete;
        LinuxEmulator(LinuxEmulator&&) = delete;
        LinuxEmulator& operator=(LinuxEmulator&&) = delete;

        const Machine& machine() const;

        void start(uint64_t count) const;
        void stop() const;
        void record_stop(stop_reason reason, ::rust::Str detail) const;
        stop_reason last_stop_reason() const;
        ::rust::String last_stop_detail() const;
        uint64_t executed_instructions() const;
        ::rust::String emulation_root() const;

        std::unique_ptr<std::vector<uint8_t>> serialize_state() const;
        void deserialize_state(::rust::Slice<const uint8_t> data) const;

        bool activate_thread(uint32_t tid) const;
        bool perform_thread_switch() const;
        void yield_thread() const;

        void map_port(uint16_t emulator_port, uint16_t host_port) const;
        uint16_t host_port(uint16_t emulator_port) const;
        uint16_t emulator_port(uint16_t host_port) const;

        uint64_t allocate_memory(uint64_t size, memory_permission permissions, uint64_t start) const;
        bool allocate_memory_at(uint64_t address, uint64_t size, memory_permission permissions) const;
        bool protect_memory(uint64_t address, uint64_t size, memory_permission permissions) const;
        bool release_memory(uint64_t address, uint64_t size) const;
        uint64_t find_free_allocation_base(uint64_t size, uint64_t start) const;
        ::rust::Vec<LinuxMemoryRegion> memory_region(uint64_t address) const;
        ::rust::Vec<LinuxMemoryRegion> memory_regions() const;
        LinuxMemoryStats memory_stats() const;
        uint64_t mmap_base() const;
        void set_mmap_base(uint64_t address) const;

        LinuxProcess process() const;
        ::rust::Vec<LinuxThread> threads() const;
        ::rust::Vec<LinuxThread> thread(uint32_t tid) const;
        ::rust::Vec<LinuxThread> current_thread() const;
        ::rust::Vec<LinuxThreadRegisters> thread_registers(uint32_t tid) const;
        bool set_thread_registers(uint32_t tid, const LinuxThreadRegisters& registers) const;
        ::rust::Vec<LinuxAuxiliaryEntry> auxiliary_vector() const;

        ::rust::Vec<LinuxFileDescriptor> file_descriptors() const;
        ::rust::Vec<LinuxFileDescriptor> file_descriptor(int32_t number) const;

        ::rust::String translate_path(::rust::Str path) const;
        ::rust::String translate_relative_path(::rust::Str base, ::rust::Str path) const;
        bool is_path_read_only(::rust::Str path) const;
        void add_mount(const LinuxPathMapping& mount) const;
        void add_passthrough_prefix(::rust::Str path) const;

        ::rust::Vec<LinuxModule> modules() const;
        ::rust::Vec<LinuxModule> module_at(uint64_t image_base) const;
        ::rust::Vec<LinuxModule> find_module_by_address(uint64_t address) const;
        ::rust::Vec<LinuxModule> find_module_by_name(::rust::Str name) const;
        LinuxModule map_local_module(::rust::Str path) const;
        void load_module_dependencies() const;
        ::rust::String resolve_library(::rust::Str name, uint64_t image_base) const;

        bool logging() const;
        void set_logging(bool logging) const;
        bool silent() const;
        void set_silent(bool silent) const;

        ::rust::Vec<LinuxInstruction> disassemble(uint64_t address, ::rust::Slice<const uint8_t> data, uint64_t count) const;

        void set_stdout_callback(std::unique_ptr<CallbackHandle> callback) const;
        void set_stderr_callback(std::unique_ptr<CallbackHandle> callback) const;
        void set_syscall_callback(std::unique_ptr<CallbackHandle> callback) const;
        void set_signal_callback(std::unique_ptr<CallbackHandle> callback) const;
        void set_memory_allocate_callback(std::unique_ptr<CallbackHandle> callback) const;
        void set_memory_protect_callback(std::unique_ptr<CallbackHandle> callback) const;
        void set_memory_release_callback(std::unique_ptr<CallbackHandle> callback) const;
        void set_module_load_callback(std::unique_ptr<CallbackHandle> callback) const;
        void set_module_observer(std::unique_ptr<CallbackHandle> callback) const;
        void set_thread_create_callback(std::unique_ptr<CallbackHandle> callback) const;
        void set_thread_terminated_callback(std::unique_ptr<CallbackHandle> callback) const;
        void set_thread_switch_callback(std::unique_ptr<CallbackHandle> callback) const;
        void set_memory_violation_callback(std::unique_ptr<CallbackHandle> callback) const;
        void set_periodic_event_callback(std::unique_ptr<CallbackHandle> callback) const;
        void set_log_callback(std::unique_ptr<CallbackHandle> callback) const;

        uint64_t hook_interrupt(std::unique_ptr<CallbackHandle> callback) const;
        uint64_t hook_memory_violation(std::unique_ptr<CallbackHandle> callback) const;

      private:
        struct callback_slots
        {
            shared_callback stdout_callback{};
            shared_callback stderr_callback{};
            shared_callback syscall{};
            shared_callback signal{};
            shared_callback memory_allocate{};
            shared_callback memory_protect{};
            shared_callback memory_release{};
            shared_callback module_load{};
            shared_callback module_observer{};
            shared_callback thread_create{};
            shared_callback thread_terminated{};
            shared_callback thread_switch{};
            shared_callback memory_violation{};
            shared_callback periodic_event{};
            shared_callback log{};
            uint64_t memory_violation_observer{};
        };

        // Declared before the emulator so native callbacks still pointing at the slots stay valid while it is destroyed.
        mutable callback_slots slots_{};
        std::unique_ptr<linux_emulator> emulator_{};
        disassembler disassembler_{};
        Machine machine_;

        void install_callbacks() const;
    };

    std::unique_ptr<LinuxEmulator> create_linux_emulator(const LinuxSettings& settings);
}
