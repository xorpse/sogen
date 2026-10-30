#pragma once

#include <rust/cxx.h>

#include <arch_emulator.hpp>
#include <backend_selection.hpp>
#include <logger.hpp>
#include <memory_manager.hpp>
#include <stop_reason.hpp>

#include <cstddef>
#include <cstdint>
#include <functional>
#include <map>
#include <memory>
#include <mutex>
#include <span>

namespace sogen::ffi
{
    struct DescriptorTableRegister;

    using memory_execution_hook_mode = hook_interface::memory_execution_hook_mode;

    struct Callback;

    class CallbackHandle
    {
      public:
        explicit CallbackHandle(::rust::Box<Callback> callback);

        uint64_t operator()(uint64_t a = 0, uint64_t b = 0, uint64_t c = 0, uint64_t d = 0, std::span<const std::byte> first = {},
                            std::span<const std::byte> second = {}) const;

      private:
        ::rust::Box<Callback> callback_;
    };

    using shared_callback = std::shared_ptr<const CallbackHandle>;

    std::unique_ptr<CallbackHandle> new_callback(::rust::Box<Callback> callback);
    shared_callback share_callback(std::unique_ptr<CallbackHandle> callback);

    std::span<const std::byte> as_bytes(std::string_view text);
    ::rust::Slice<const uint8_t> to_slice(std::span<const std::byte> data);

    // Hooks registered through the bindings, keyed by ids that stay invalid once removed, so a stale id from Rust can
    // never reach delete_hook twice.
    class Machine
    {
      public:
        using cpu_resolver = std::function<guest_cpu&()>;
        using hook_dispatcher = std::function<void(cpu_interface&, const std::function<void()>&)>;

        Machine(guest_machine& machine, cpu_resolver cpu, hook_dispatcher dispatch);
        ~Machine();

        Machine(const Machine&) = delete;
        Machine& operator=(const Machine&) = delete;
        Machine(Machine&&) = delete;
        Machine& operator=(Machine&&) = delete;

        guest_machine& machine() const;
        guest_cpu& cpu() const;

        ::rust::String backend_name() const;
        bool supports_instruction_counting() const;
        bool is_stop_thread_safe() const;
        void stop() const;
        bool supports_global_memory_execution_hooks() const;
        bool supports_multiple_vcpus() const;
        void set_memory_execution_hook_mode(memory_execution_hook_mode mode) const;

        void read_memory(uint64_t address, ::rust::Slice<uint8_t> data) const;
        bool try_read_memory(uint64_t address, ::rust::Slice<uint8_t> data) const;
        void write_memory(uint64_t address, ::rust::Slice<const uint8_t> data) const;
        bool try_write_memory(uint64_t address, ::rust::Slice<const uint8_t> data) const;

        size_t read_x86_register(x86_register reg, ::rust::Slice<uint8_t> value) const;
        size_t write_x86_register(x86_register reg, ::rust::Slice<const uint8_t> value) const;
        size_t read_arm64_register(arm64_register reg, ::rust::Slice<uint8_t> value) const;
        size_t write_arm64_register(arm64_register reg, ::rust::Slice<const uint8_t> value) const;
        ::rust::Vec<uint8_t> save_registers() const;
        void restore_registers(::rust::Slice<const uint8_t> data) const;
        uint64_t segment_base(x86_register segment) const;
        void set_segment_base(x86_register segment, uint64_t base) const;
        DescriptorTableRegister descriptor_table(x86_register table) const;
        void load_gdt(uint64_t address, uint32_t limit) const;

        uint64_t hook_memory_execution(std::unique_ptr<CallbackHandle> callback) const;
        uint64_t hook_memory_execution_at(uint64_t address, std::unique_ptr<CallbackHandle> callback) const;
        uint64_t hook_memory_range_execution(uint64_t address, uint64_t size, std::unique_ptr<CallbackHandle> callback) const;
        uint64_t hook_memory_read(uint64_t address, uint64_t size, std::unique_ptr<CallbackHandle> callback) const;
        uint64_t hook_memory_write(uint64_t address, uint64_t size, std::unique_ptr<CallbackHandle> callback) const;
        uint64_t hook_instruction(int32_t instruction, std::unique_ptr<CallbackHandle> callback) const;
        uint64_t hook_interrupt(std::unique_ptr<CallbackHandle> callback) const;
        uint64_t hook_memory_violation(std::unique_ptr<CallbackHandle> callback) const;
        uint64_t hook_basic_block(std::unique_ptr<CallbackHandle> callback) const;
        bool remove_hook(uint64_t id) const;

        uint64_t track_hook(emulator_hook* hook) const;
        uint64_t track_removal(std::function<void()> remove) const;

      private:
        guest_machine* machine_{};
        cpu_resolver cpu_{};
        std::shared_ptr<const hook_dispatcher> dispatch_{};

        mutable std::mutex hooks_mutex_{};
        mutable std::map<uint64_t, emulator_hook*> hooks_{};
        mutable std::map<uint64_t, std::function<void()>> removals_{};
        mutable uint64_t next_hook_id_{1};
    };
}
