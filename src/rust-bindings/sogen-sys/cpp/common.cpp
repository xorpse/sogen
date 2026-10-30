#include "common.hpp"

#include <sogen-sys/common.rs.h>

#include <stdexcept>
#include <utility>
#include <vector>

namespace sogen::ffi
{
    namespace
    {
        template <typename Cpu>
        Cpu& cpu_as(guest_cpu& cpu, const char* architecture)
        {
            auto* typed = dynamic_cast<Cpu*>(&cpu);
            if (!typed)
            {
                throw std::runtime_error(std::string("The guest CPU is not ") + architecture);
            }

            return *typed;
        }

        template <typename... Args>
        uint64_t dispatch_callback(const Machine::hook_dispatcher& dispatch, cpu_interface& cpu, const CallbackHandle& callback,
                                   const Args... args)
        {
            uint64_t result{};
            dispatch(cpu, [&] { result = callback(args...); });
            return result;
        }
    }

    CallbackHandle::CallbackHandle(::rust::Box<Callback> callback)
        : callback_(std::move(callback))
    {
    }

    uint64_t CallbackHandle::operator()(const uint64_t a, const uint64_t b, const uint64_t c, const uint64_t d,
                                        const std::span<const std::byte> first, const std::span<const std::byte> second) const
    {
        return invoke_callback(*this->callback_, a, b, c, d, to_slice(first), to_slice(second));
    }

    std::unique_ptr<CallbackHandle> new_callback(::rust::Box<Callback> callback)
    {
        return std::make_unique<CallbackHandle>(std::move(callback));
    }

    shared_callback share_callback(std::unique_ptr<CallbackHandle> callback)
    {
        if (!callback)
        {
            throw std::invalid_argument("Missing callback");
        }

        return {std::move(callback)};
    }

    std::span<const std::byte> as_bytes(const std::string_view text)
    {
        return std::as_bytes(std::span(text.data(), text.size()));
    }

    ::rust::Slice<const uint8_t> to_slice(const std::span<const std::byte> data)
    {
        return {reinterpret_cast<const uint8_t*>(data.data()), data.size()};
    }

    Machine::Machine(guest_machine& machine, cpu_resolver cpu, hook_dispatcher dispatch)
        : machine_(&machine),
          cpu_(std::move(cpu)),
          dispatch_(std::make_shared<const hook_dispatcher>(std::move(dispatch)))
    {
    }

    Machine::~Machine()
    {
        for (const auto& [id, hook] : this->hooks_)
        {
            this->machine_->delete_hook(hook);
        }

        for (const auto& [id, remove] : this->removals_)
        {
            remove();
        }
    }

    guest_machine& Machine::machine() const
    {
        return *this->machine_;
    }

    guest_cpu& Machine::cpu() const
    {
        return this->cpu_();
    }

    ::rust::String Machine::backend_name() const
    {
        return this->machine_->get_name();
    }

    bool Machine::supports_instruction_counting() const
    {
        return this->cpu().supports_instruction_counting();
    }

    bool Machine::is_stop_thread_safe() const
    {
        return this->cpu().is_stop_thread_safe();
    }

    void Machine::stop() const
    {
        this->cpu().stop();
    }

    bool Machine::supports_global_memory_execution_hooks() const
    {
        return this->machine_->supports_global_memory_execution_hooks();
    }

    bool Machine::supports_multiple_vcpus() const
    {
        return this->machine_->supports_multiple_vcpus();
    }

    void Machine::set_memory_execution_hook_mode(const memory_execution_hook_mode mode) const
    {
        this->machine_->set_memory_execution_hook_mode(mode);
    }

    void Machine::read_memory(const uint64_t address, const ::rust::Slice<uint8_t> data) const
    {
        this->machine_->read_memory(address, data.data(), data.size());
    }

    bool Machine::try_read_memory(const uint64_t address, const ::rust::Slice<uint8_t> data) const
    {
        return this->machine_->try_read_memory(address, data.data(), data.size());
    }

    void Machine::write_memory(const uint64_t address, const ::rust::Slice<const uint8_t> data) const
    {
        this->machine_->write_memory(address, data.data(), data.size());
    }

    bool Machine::try_write_memory(const uint64_t address, const ::rust::Slice<const uint8_t> data) const
    {
        return this->machine_->try_write_memory(address, data.data(), data.size());
    }

    size_t Machine::read_x86_register(const x86_register reg, const ::rust::Slice<uint8_t> value) const
    {
        return cpu_as<x86_64_cpu>(this->cpu(), "x86-64").read_register(reg, value.data(), value.size());
    }

    size_t Machine::write_x86_register(const x86_register reg, const ::rust::Slice<const uint8_t> value) const
    {
        return cpu_as<x86_64_cpu>(this->cpu(), "x86-64").write_register(reg, value.data(), value.size());
    }

    size_t Machine::read_arm64_register(const arm64_register reg, const ::rust::Slice<uint8_t> value) const
    {
        return cpu_as<arm64_cpu>(this->cpu(), "ARM64").read_register(reg, value.data(), value.size());
    }

    size_t Machine::write_arm64_register(const arm64_register reg, const ::rust::Slice<const uint8_t> value) const
    {
        return cpu_as<arm64_cpu>(this->cpu(), "ARM64").write_register(reg, value.data(), value.size());
    }

    ::rust::Vec<uint8_t> Machine::save_registers() const
    {
        const auto data = this->cpu().save_registers();

        ::rust::Vec<uint8_t> result{};
        result.reserve(data.size());
        for (const auto value : data)
        {
            result.push_back(static_cast<uint8_t>(value));
        }

        return result;
    }

    void Machine::restore_registers(const ::rust::Slice<const uint8_t> data) const
    {
        const auto bytes = std::as_bytes(std::span(data.data(), data.size()));
        this->cpu().restore_registers(std::vector(bytes.begin(), bytes.end()));
    }

    uint64_t Machine::segment_base(const x86_register segment) const
    {
        return cpu_as<x86_64_cpu>(this->cpu(), "x86-64").get_segment_base(segment);
    }

    void Machine::set_segment_base(const x86_register segment, const uint64_t base) const
    {
        cpu_as<x86_64_cpu>(this->cpu(), "x86-64").set_segment_base(segment, base);
    }

    DescriptorTableRegister Machine::descriptor_table(const x86_register table) const
    {
        x86_64_cpu::descriptor_table_register native{};
        if (!cpu_as<x86_64_cpu>(this->cpu(), "x86-64").read_descriptor_table(static_cast<int>(table), native))
        {
            throw std::runtime_error("The backend cannot read the descriptor table");
        }

        return {.base = native.base, .limit = native.limit};
    }

    void Machine::load_gdt(const uint64_t address, const uint32_t limit) const
    {
        cpu_as<x86_64_cpu>(this->cpu(), "x86-64").load_gdt(address, limit);
    }

    uint64_t Machine::hook_memory_execution(std::unique_ptr<CallbackHandle> callback) const
    {
        auto cb = share_callback(std::move(callback));
        return this->track_hook(
            this->machine_->hook_memory_execution([cb, dispatch = this->dispatch_](cpu_interface& cpu, const uint64_t address) {
                dispatch_callback(*dispatch, cpu, *cb, address);
            }));
    }

    uint64_t Machine::hook_memory_execution_at(const uint64_t address, std::unique_ptr<CallbackHandle> callback) const
    {
        auto cb = share_callback(std::move(callback));
        return this->track_hook(
            this->machine_->hook_memory_execution(address, [cb, dispatch = this->dispatch_](cpu_interface& cpu, const uint64_t executed) {
                dispatch_callback(*dispatch, cpu, *cb, executed);
            }));
    }

    uint64_t Machine::hook_memory_range_execution(const uint64_t address, const uint64_t size,
                                                  std::unique_ptr<CallbackHandle> callback) const
    {
        auto cb = share_callback(std::move(callback));
        return this->track_hook(this->machine_->hook_memory_range_execution(
            address, size, [cb, dispatch = this->dispatch_](cpu_interface& cpu, const uint64_t executed) {
                dispatch_callback(*dispatch, cpu, *cb, executed);
            }));
    }

    uint64_t Machine::hook_memory_read(const uint64_t address, const uint64_t size, std::unique_ptr<CallbackHandle> callback) const
    {
        auto cb = share_callback(std::move(callback));
        return this->track_hook(this->machine_->hook_memory_read(
            address, size,
            [cb, dispatch = this->dispatch_](cpu_interface& cpu, const uint64_t accessed, const void* data, const size_t length) {
                dispatch_callback(*dispatch, cpu, *cb, accessed, static_cast<uint64_t>(length), uint64_t{}, uint64_t{},
                                  std::span(static_cast<const std::byte*>(data), length));
            }));
    }

    uint64_t Machine::hook_memory_write(const uint64_t address, const uint64_t size, std::unique_ptr<CallbackHandle> callback) const
    {
        auto cb = share_callback(std::move(callback));
        return this->track_hook(this->machine_->hook_memory_write(
            address, size,
            [cb, dispatch = this->dispatch_](cpu_interface& cpu, const uint64_t accessed, const void* data, const size_t length) {
                dispatch_callback(*dispatch, cpu, *cb, accessed, static_cast<uint64_t>(length), uint64_t{}, uint64_t{},
                                  std::span(static_cast<const std::byte*>(data), length));
            }));
    }

    uint64_t Machine::hook_instruction(const int32_t instruction, std::unique_ptr<CallbackHandle> callback) const
    {
        auto cb = share_callback(std::move(callback));
        return this->track_hook(
            this->machine_->hook_instruction(instruction, [cb, dispatch = this->dispatch_](cpu_interface& cpu, const uint64_t data) {
                return static_cast<instruction_hook_continuation>(dispatch_callback(*dispatch, cpu, *cb, data));
            }));
    }

    uint64_t Machine::hook_interrupt(std::unique_ptr<CallbackHandle> callback) const
    {
        auto cb = share_callback(std::move(callback));
        return this->track_hook(this->machine_->hook_interrupt([cb, dispatch = this->dispatch_](cpu_interface& cpu, const int interrupt) {
            dispatch_callback(*dispatch, cpu, *cb, static_cast<uint64_t>(static_cast<uint32_t>(interrupt)));
        }));
    }

    uint64_t Machine::hook_memory_violation(std::unique_ptr<CallbackHandle> callback) const
    {
        auto cb = share_callback(std::move(callback));
        return this->track_hook(this->machine_->hook_memory_violation(
            [cb, dispatch = this->dispatch_](cpu_interface& cpu, const uint64_t address, const size_t size,
                                             const memory_operation operation, const memory_violation_type type) {
                return static_cast<memory_violation_continuation>(
                    dispatch_callback(*dispatch, cpu, *cb, address, static_cast<uint64_t>(size), static_cast<uint64_t>(operation),
                                      static_cast<uint64_t>(type)));
            }));
    }

    uint64_t Machine::hook_basic_block(std::unique_ptr<CallbackHandle> callback) const
    {
        auto cb = share_callback(std::move(callback));
        return this->track_hook(
            this->machine_->hook_basic_block([cb, dispatch = this->dispatch_](cpu_interface& cpu, const basic_block& block) {
                dispatch_callback(*dispatch, cpu, *cb, block.address, static_cast<uint64_t>(block.instruction_count),
                                  static_cast<uint64_t>(block.size));
            }));
    }

    bool Machine::remove_hook(const uint64_t id) const
    {
        emulator_hook* hook{};
        std::function<void()> remove{};

        {
            const std::scoped_lock lock(this->hooks_mutex_);
            if (const auto entry = this->hooks_.find(id); entry != this->hooks_.end())
            {
                hook = entry->second;
                this->hooks_.erase(entry);
            }
            else if (const auto removal = this->removals_.find(id); removal != this->removals_.end())
            {
                remove = std::move(removal->second);
                this->removals_.erase(removal);
            }
            else
            {
                return false;
            }
        }

        if (hook)
        {
            this->machine_->delete_hook(hook);
        }
        else
        {
            remove();
        }

        return true;
    }

    uint64_t Machine::track_hook(emulator_hook* hook) const
    {
        if (!hook)
        {
            throw std::runtime_error("The backend rejected the hook");
        }

        const std::scoped_lock lock(this->hooks_mutex_);
        const auto id = this->next_hook_id_++;
        this->hooks_.emplace(id, hook);
        return id;
    }

    uint64_t Machine::track_removal(std::function<void()> remove) const
    {
        const std::scoped_lock lock(this->hooks_mutex_);
        const auto id = this->next_hook_id_++;
        this->removals_.emplace(id, std::move(remove));
        return id;
    }
}
