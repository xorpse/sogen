#include "linux.hpp"

#include <sogen-sys/linux.rs.h>

#include <algorithm>
#include <cstdio>
#include <stdexcept>
#include <utility>
#include <vector>

namespace sogen::ffi
{
    namespace
    {
        ::rust::String to_rust_string(const std::string_view text)
        {
            return ::rust::String::lossy(text.data(), text.size());
        }

        ::rust::String path_to_rust_string(const std::filesystem::path& path)
        {
            const auto text = path.u8string();
            return ::rust::String::lossy(reinterpret_cast<const char*>(text.data()), text.size());
        }

        std::filesystem::path to_path(const ::rust::Str text)
        {
            return {std::u8string(reinterpret_cast<const char8_t*>(text.data()), text.size())};
        }

        std::string_view to_string_view(const ::rust::Str text)
        {
            return {text.data(), text.size()};
        }

        ::rust::Vec<::rust::String> to_rust_strings(const std::vector<std::string>& values)
        {
            ::rust::Vec<::rust::String> result{};
            result.reserve(values.size());
            for (const auto& value : values)
            {
                result.push_back(to_rust_string(value));
            }

            return result;
        }

        void write_to_host(FILE* target, const std::string_view data)
        {
            std::fwrite(data.data(), 1, data.size(), target);
            std::fflush(target);
        }

        uint64_t signed_argument(const int value)
        {
            return static_cast<uint64_t>(static_cast<uint32_t>(value));
        }

        LinuxMemoryRegion to_region(const linux_memory_region_info& region)
        {
            return {
                .start = region.start,
                .length = region.length,
                .permissions = region.permissions,
                .allocation_base = region.allocation_base,
                .allocation_length = region.allocation_length,
                .reserved = region.is_reserved,
                .committed = region.is_committed,
                .initial_permissions = region.initial_permissions,
                .kind = region.kind,
            };
        }

        LinuxThread to_thread(linux_emulator& emulator, const linux_thread& thread)
        {
            const auto active = emulator.process.active_thread == &thread;
            return {
                .tid = thread.tid,
                .stack_base = thread.stack_base,
                .stack_size = thread.stack_size,
                .fs_base = thread.fs_base,
                .current_ip = active ? emulator.emu().reg<uint64_t>(x86_register::rip) : thread.saved_regs.rip,
                .wait_state = thread.wait_state,
                .terminated = thread.terminated,
                .exit_code = thread.exit_code,
                .executed_instructions = thread.executed_instructions,
                .active = active,
                .signal_mask = thread.signal_mask,
                .clear_child_tid = thread.clear_child_tid,
                .robust_list_head = thread.robust_list_head,
            };
        }

        LinuxThreadRegisters to_thread_registers(const linux_saved_registers& registers)
        {
            return {
                .rax = registers.rax,
                .rbx = registers.rbx,
                .rcx = registers.rcx,
                .rdx = registers.rdx,
                .rsi = registers.rsi,
                .rdi = registers.rdi,
                .rbp = registers.rbp,
                .rsp = registers.rsp,
                .r8 = registers.r8,
                .r9 = registers.r9,
                .r10 = registers.r10,
                .r11 = registers.r11,
                .r12 = registers.r12,
                .r13 = registers.r13,
                .r14 = registers.r14,
                .r15 = registers.r15,
                .rip = registers.rip,
                .rflags = registers.rflags,
                .fs_base = registers.fs_base,
                .gs_base = registers.gs_base,
            };
        }

        linux_saved_registers to_saved_registers(const LinuxThreadRegisters& registers)
        {
            return {
                .rax = registers.rax,
                .rbx = registers.rbx,
                .rcx = registers.rcx,
                .rdx = registers.rdx,
                .rsi = registers.rsi,
                .rdi = registers.rdi,
                .rbp = registers.rbp,
                .rsp = registers.rsp,
                .r8 = registers.r8,
                .r9 = registers.r9,
                .r10 = registers.r10,
                .r11 = registers.r11,
                .r12 = registers.r12,
                .r13 = registers.r13,
                .r14 = registers.r14,
                .r15 = registers.r15,
                .rip = registers.rip,
                .rflags = registers.rflags,
                .fs_base = registers.fs_base,
                .gs_base = registers.gs_base,
            };
        }

        LinuxFileDescriptor to_file_descriptor(const int number, const linux_fd& fd)
        {
            LinuxFileDescriptor result{
                .number = number,
                .kind = fd.type,
                .host_path = to_rust_string(fd.host_path),
                .guest_path = to_rust_string(fd.guest_path),
                .flags = fd.flags,
                .close_on_exec = fd.close_on_exec,
                .socket = {},
            };

            if (const auto& socket = fd.socket_state)
            {
                result.socket = {
                    .domain = socket->domain,
                    .socket_type = socket->type,
                    .protocol = socket->protocol,
                    .listening = socket->listening,
                    .connected = socket->connected,
                    .bound_port = socket->bound_port,
                    .bound_address = socket->bound_addr,
                    .peer_port = socket->peer_port,
                    .peer_address = socket->peer_addr,
                };
            }

            return result;
        }

        LinuxModule to_module(const linux_mapped_module& module)
        {
            LinuxModule result{
                .name = to_rust_string(module.name),
                .path = path_to_rust_string(module.path),
                .image_base = module.image_base,
                .size_of_image = module.size_of_image,
                .entry_point = module.entry_point,
                .exports = {},
                .needed_libraries = {},
                .sections = {},
                .rpath = to_rust_string(module.rpath),
                .runpath = to_rust_string(module.runpath),
            };

            result.exports.reserve(module.exports.size());
            for (const auto& symbol : module.exports)
            {
                result.exports.push_back({.name = to_rust_string(symbol.name), .rva = symbol.rva, .address = symbol.address});
            }

            result.needed_libraries = to_rust_strings(module.needed_libraries);

            result.sections.reserve(module.sections.size());
            for (const auto& section : module.sections)
            {
                result.sections.push_back({
                    .name = to_rust_string(section.name),
                    .start = section.start,
                    .length = section.length,
                    .permissions = section.permissions,
                });
            }

            return result;
        }

        ::rust::Vec<LinuxModule> optional_module(const linux_mapped_module* module)
        {
            ::rust::Vec<LinuxModule> result{};
            if (module)
            {
                result.push_back(to_module(*module));
            }

            return result;
        }

        bool is_call(const cs_insn& instruction)
        {
            const auto* detail = instruction.detail;
            if (!detail)
            {
                return false;
            }

            const auto groups = std::span(detail->groups, detail->groups_count);
            return std::ranges::find(groups, static_cast<uint8_t>(CS_GRP_CALL)) != groups.end();
        }
    }

    LinuxEmulator::LinuxEmulator(std::unique_ptr<linux_emulator> emulator)
        : emulator_(std::move(emulator)),
          machine_(
              this->emulator_->emu(), [this]() -> guest_cpu& { return this->emulator_->emu(); },
              [](cpu_interface&, const std::function<void()>& callback) { callback(); })
    {
        this->install_callbacks();
    }

    LinuxEmulator::~LinuxEmulator() = default;

    void LinuxEmulator::install_callbacks() const
    {
        auto& emulator = *this->emulator_;
        auto* slots = &this->slots_;

        emulator.on_stdout = [slots](const std::string_view data) {
            if (const auto cb = slots->stdout_callback)
            {
                (*cb)(0, 0, 0, 0, as_bytes(data));
            }
            else
            {
                write_to_host(stdout, data);
            }
        };

        emulator.on_stderr = [slots](const std::string_view data) {
            if (const auto cb = slots->stderr_callback)
            {
                (*cb)(0, 0, 0, 0, as_bytes(data));
            }
            else
            {
                write_to_host(stderr, data);
            }
        };

        emulator.on_syscall = [slots](const uint64_t syscall_id, const std::string_view syscall_name) {
            const auto cb = slots->syscall;
            if (!cb)
            {
                return instruction_hook_continuation::run_instruction;
            }

            return static_cast<instruction_hook_continuation>((*cb)(syscall_id, 0, 0, 0, as_bytes(syscall_name)));
        };

        emulator.on_signal.add([slots](const int signum, const uint64_t fault_address, const int code) {
            if (const auto cb = slots->signal)
            {
                (*cb)(signed_argument(signum), fault_address, signed_argument(code));
            }
        });

        emulator.memory.add_memory_allocate_callback(
            [slots](const uint64_t address, const size_t size, const memory_permission permissions, const bool committed) {
                if (const auto cb = slots->memory_allocate)
                {
                    (*cb)(address, size, static_cast<uint64_t>(permissions), committed ? 1 : 0);
                }
            });

        emulator.memory.add_memory_protect_callback(
            [slots](const uint64_t address, const size_t size, const memory_permission permissions) {
                if (const auto cb = slots->memory_protect)
                {
                    (*cb)(address, size, static_cast<uint64_t>(permissions));
                }
            });

        emulator.memory.add_memory_release_callback([slots](const uint64_t address, const size_t size) {
            if (const auto cb = slots->memory_release)
            {
                (*cb)(address, size);
            }
        });

        emulator.on_module_load.add([slots](const linux_mapped_module& module) {
            if (const auto cb = slots->module_observer)
            {
                (*cb)(module.image_base);
            }

            if (const auto cb = slots->module_load)
            {
                (*cb)(module.image_base);
            }
        });

        emulator.on_thread_create.add([slots](const linux_thread& thread) {
            if (const auto cb = slots->thread_create)
            {
                (*cb)(thread.tid);
            }
        });

        emulator.on_thread_terminated.add([slots](const linux_thread& thread) {
            if (const auto cb = slots->thread_terminated)
            {
                (*cb)(thread.tid);
            }
        });

        emulator.on_thread_switch.add([slots](const uint32_t old_tid, const uint32_t new_tid) {
            if (const auto cb = slots->thread_switch)
            {
                (*cb)(old_tid, new_tid);
            }
        });

        emulator.on_periodic_event = [slots] {
            if (const auto cb = slots->periodic_event)
            {
                (*cb)();
            }
        };

        emulator.log.set_sink([slots](const color c, const std::string_view message) {
            if (const auto cb = slots->log)
            {
                (*cb)(static_cast<uint64_t>(c), 0, 0, 0, as_bytes(message));
            }
        });
    }

    const Machine& LinuxEmulator::machine() const
    {
        return this->machine_;
    }

    void LinuxEmulator::start(const uint64_t count) const
    {
        this->emulator_->start(static_cast<size_t>(count));
    }

    void LinuxEmulator::stop() const
    {
        this->emulator_->stop();
    }

    void LinuxEmulator::record_stop(const stop_reason reason, const ::rust::Str detail) const
    {
        this->emulator_->record_stop(reason, std::string(detail));
    }

    stop_reason LinuxEmulator::last_stop_reason() const
    {
        return this->emulator_->last_stop_reason();
    }

    ::rust::String LinuxEmulator::last_stop_detail() const
    {
        return to_rust_string(this->emulator_->last_stop_detail());
    }

    uint64_t LinuxEmulator::executed_instructions() const
    {
        return this->emulator_->get_executed_instructions();
    }

    ::rust::String LinuxEmulator::emulation_root() const
    {
        return path_to_rust_string(this->emulator_->emulation_root);
    }

    std::unique_ptr<std::vector<uint8_t>> LinuxEmulator::serialize_state() const
    {
        utils::buffer_serializer serializer{};
        this->emulator_->serialize(serializer, false);
        const auto data = serializer.move_buffer();
        const auto bytes = std::span(reinterpret_cast<const uint8_t*>(data.data()), data.size());
        return std::make_unique<std::vector<uint8_t>>(bytes.begin(), bytes.end());
    }

    void LinuxEmulator::deserialize_state(const ::rust::Slice<const uint8_t> data) const
    {
        utils::buffer_deserializer deserializer{std::span(data.data(), data.size())};
        this->emulator_->deserialize(deserializer, false);
    }

    bool LinuxEmulator::activate_thread(const uint32_t tid) const
    {
        return this->emulator_->activate_thread(tid);
    }

    bool LinuxEmulator::perform_thread_switch() const
    {
        return this->emulator_->perform_thread_switch();
    }

    void LinuxEmulator::yield_thread() const
    {
        this->emulator_->yield_thread();
    }

    void LinuxEmulator::map_port(const uint16_t emulator_port, const uint16_t host_port) const
    {
        this->emulator_->port_mapper.map_port(emulator_port, host_port);
    }

    uint16_t LinuxEmulator::host_port(const uint16_t emulator_port) const
    {
        return this->emulator_->port_mapper.get_host_port(emulator_port);
    }

    uint16_t LinuxEmulator::emulator_port(const uint16_t host_port) const
    {
        return this->emulator_->port_mapper.get_emulator_port(host_port);
    }

    uint64_t LinuxEmulator::allocate_memory(const uint64_t size, const memory_permission permissions, const uint64_t start) const
    {
        return this->emulator_->memory.allocate_memory(static_cast<size_t>(size), permissions, start);
    }

    bool LinuxEmulator::allocate_memory_at(const uint64_t address, const uint64_t size, const memory_permission permissions) const
    {
        return this->emulator_->memory.allocate_memory(address, static_cast<size_t>(size), permissions);
    }

    bool LinuxEmulator::protect_memory(const uint64_t address, const uint64_t size, const memory_permission permissions) const
    {
        return this->emulator_->memory.protect_memory(address, static_cast<size_t>(size), permissions);
    }

    bool LinuxEmulator::release_memory(const uint64_t address, const uint64_t size) const
    {
        return this->emulator_->memory.release_memory(address, static_cast<size_t>(size));
    }

    uint64_t LinuxEmulator::find_free_allocation_base(const uint64_t size, const uint64_t start) const
    {
        return this->emulator_->memory.find_free_allocation_base(static_cast<size_t>(size), start);
    }

    ::rust::Vec<LinuxMemoryRegion> LinuxEmulator::memory_region(const uint64_t address) const
    {
        ::rust::Vec<LinuxMemoryRegion> result{};
        if (const auto region = this->emulator_->memory.get_region_info(address))
        {
            result.push_back(to_region(*region));
        }

        return result;
    }

    ::rust::Vec<LinuxMemoryRegion> LinuxEmulator::memory_regions() const
    {
        const auto regions = this->emulator_->memory.get_mapped_region_infos();

        ::rust::Vec<LinuxMemoryRegion> result{};
        result.reserve(regions.size());
        for (const auto& region : regions)
        {
            result.push_back(to_region(region));
        }

        return result;
    }

    LinuxMemoryStats LinuxEmulator::memory_stats() const
    {
        const auto stats = this->emulator_->memory.compute_memory_stats();
        return {
            .region_count = stats.region_count,
            .mapped_bytes = stats.mapped_bytes,
            .executable_bytes = stats.executable_bytes,
        };
    }

    uint64_t LinuxEmulator::mmap_base() const
    {
        return this->emulator_->memory.get_mmap_base();
    }

    void LinuxEmulator::set_mmap_base(const uint64_t address) const
    {
        this->emulator_->memory.set_mmap_base(address);
    }

    LinuxProcess LinuxEmulator::process() const
    {
        const auto& process = this->emulator_->process;
        return {
            .pid = process.pid,
            .ppid = process.ppid,
            .uid = process.uid,
            .gid = process.gid,
            .euid = process.euid,
            .egid = process.egid,
            .has_exit_status = process.exit_status.has_value(),
            .exit_status = process.exit_status.value_or(0),
            .thread_count = process.threads.size(),
            .brk_base = process.brk_base,
            .brk_current = process.brk_current,
            .working_directory = to_rust_string(process.current_working_directory),
            .executable_path = to_rust_string(process.executable_path),
            .arguments = to_rust_strings(process.argv),
            .environment = to_rust_strings(process.envp),
        };
    }

    ::rust::Vec<LinuxThread> LinuxEmulator::threads() const
    {
        ::rust::Vec<LinuxThread> result{};
        for (const auto& [tid, thread] : this->emulator_->process.threads)
        {
            if (!thread.terminated)
            {
                result.push_back(to_thread(*this->emulator_, thread));
            }
        }

        return result;
    }

    ::rust::Vec<LinuxThread> LinuxEmulator::thread(const uint32_t tid) const
    {
        ::rust::Vec<LinuxThread> result{};
        const auto& threads = this->emulator_->process.threads;
        if (const auto entry = threads.find(tid); entry != threads.end())
        {
            result.push_back(to_thread(*this->emulator_, entry->second));
        }

        return result;
    }

    ::rust::Vec<LinuxThread> LinuxEmulator::current_thread() const
    {
        ::rust::Vec<LinuxThread> result{};
        if (const auto* thread = this->emulator_->current_thread())
        {
            result.push_back(to_thread(*this->emulator_, *thread));
        }

        return result;
    }

    ::rust::Vec<LinuxThreadRegisters> LinuxEmulator::thread_registers(const uint32_t tid) const
    {
        auto& emulator = *this->emulator_;
        ::rust::Vec<LinuxThreadRegisters> result{};
        const auto entry = emulator.process.threads.find(tid);
        if (entry == emulator.process.threads.end())
        {
            return result;
        }

        if (emulator.process.active_thread != &entry->second)
        {
            result.push_back(to_thread_registers(entry->second.saved_regs));
            return result;
        }

        linux_thread live{};
        live.save(emulator.emu());
        result.push_back(to_thread_registers(live.saved_regs));
        return result;
    }

    bool LinuxEmulator::set_thread_registers(const uint32_t tid, const LinuxThreadRegisters& registers) const
    {
        auto& emulator = *this->emulator_;
        const auto entry = emulator.process.threads.find(tid);
        if (entry == emulator.process.threads.end())
        {
            return false;
        }

        if (emulator.process.active_thread != &entry->second)
        {
            entry->second.saved_regs = to_saved_registers(registers);
            return true;
        }

        linux_thread live{};
        live.saved_regs = to_saved_registers(registers);
        live.restore(emulator.emu());
        return true;
    }

    ::rust::Vec<LinuxAuxiliaryEntry> LinuxEmulator::auxiliary_vector() const
    {
        const auto& auxv = this->emulator_->process.auxv;

        ::rust::Vec<LinuxAuxiliaryEntry> result{};
        result.reserve(auxv.size());
        for (const auto& entry : auxv)
        {
            if (entry.type != elf::AT_NULL)
            {
                result.push_back({.kind = entry.type, .value = entry.value});
            }
        }

        return result;
    }

    ::rust::Vec<LinuxFileDescriptor> LinuxEmulator::file_descriptors() const
    {
        const auto& fds = this->emulator_->process.fds.get_fds();

        ::rust::Vec<LinuxFileDescriptor> result{};
        result.reserve(fds.size());
        for (const auto& [number, fd] : fds)
        {
            result.push_back(to_file_descriptor(number, fd));
        }

        return result;
    }

    ::rust::Vec<LinuxFileDescriptor> LinuxEmulator::file_descriptor(const int32_t number) const
    {
        ::rust::Vec<LinuxFileDescriptor> result{};
        if (const auto* fd = this->emulator_->process.fds.get(number))
        {
            result.push_back(to_file_descriptor(number, *fd));
        }

        return result;
    }

    ::rust::String LinuxEmulator::translate_path(const ::rust::Str path) const
    {
        return path_to_rust_string(this->emulator_->file_sys.translate(to_string_view(path)));
    }

    ::rust::String LinuxEmulator::translate_relative_path(const ::rust::Str base, const ::rust::Str path) const
    {
        return path_to_rust_string(this->emulator_->file_sys.translate_guest_relative_to(to_string_view(base), to_string_view(path)));
    }

    bool LinuxEmulator::is_path_read_only(const ::rust::Str path) const
    {
        return this->emulator_->file_sys.is_read_only_guest_path(to_string_view(path));
    }

    void LinuxEmulator::add_mount(const LinuxPathMapping& mount) const
    {
        this->emulator_->file_sys.add_path_mapping(to_path(mount.guest), to_path(mount.host), mount.read_only);
    }

    void LinuxEmulator::add_passthrough_prefix(const ::rust::Str path) const
    {
        this->emulator_->file_sys.add_passthrough_prefix(to_path(path));
    }

    ::rust::Vec<LinuxModule> LinuxEmulator::modules() const
    {
        const auto& modules = this->emulator_->mod_manager.get_modules();

        ::rust::Vec<LinuxModule> result{};
        result.reserve(modules.size());
        for (const auto& [base, module] : modules)
        {
            result.push_back(to_module(module));
        }

        return result;
    }

    ::rust::Vec<LinuxModule> LinuxEmulator::module_at(const uint64_t image_base) const
    {
        const auto& modules = this->emulator_->mod_manager.get_modules();
        const auto entry = modules.find(image_base);
        return optional_module(entry == modules.end() ? nullptr : &entry->second);
    }

    ::rust::Vec<LinuxModule> LinuxEmulator::find_module_by_address(const uint64_t address) const
    {
        return optional_module(this->emulator_->mod_manager.find_by_address(address));
    }

    ::rust::Vec<LinuxModule> LinuxEmulator::find_module_by_name(const ::rust::Str name) const
    {
        return optional_module(this->emulator_->mod_manager.find_by_name(to_string_view(name)));
    }

    LinuxModule LinuxEmulator::map_local_module(const ::rust::Str path) const
    {
        return to_module(*this->emulator_->mod_manager.map_module(to_path(path)));
    }

    void LinuxEmulator::load_module_dependencies() const
    {
        auto& emulator = *this->emulator_;
        emulator.mod_manager.load_dependencies(emulator.file_sys);
    }

    ::rust::String LinuxEmulator::resolve_library(const ::rust::Str name, const uint64_t image_base) const
    {
        auto& emulator = *this->emulator_;
        const auto& modules = emulator.mod_manager.get_modules();
        const auto entry = modules.find(image_base);
        if (entry == modules.end())
        {
            return {};
        }

        return path_to_rust_string(emulator.mod_manager.resolve_library(std::string(name), entry->second, emulator.file_sys));
    }

    bool LinuxEmulator::logging() const
    {
        return !this->emulator_->log.is_output_disabled();
    }

    void LinuxEmulator::set_logging(const bool logging) const
    {
        this->emulator_->log.disable_output(!logging);
    }

    bool LinuxEmulator::silent() const
    {
        return this->emulator_->log.is_silent();
    }

    void LinuxEmulator::set_silent(const bool silent) const
    {
        this->emulator_->log.set_silent(silent);
    }

    ::rust::Vec<LinuxInstruction> LinuxEmulator::disassemble(const uint64_t address, const ::rust::Slice<const uint8_t> data,
                                                             const uint64_t count) const
    {
        auto& cpu = this->emulator_->emu();
        const auto instructions = this->disassembler_.disassemble(cpu, cpu.reg<uint16_t>(x86_register::cs),
                                                                  std::span(data.data(), data.size()), static_cast<size_t>(count), address);

        ::rust::Vec<LinuxInstruction> result{};
        result.reserve(instructions.size());
        for (const auto& instruction : instructions)
        {
            LinuxInstruction entry{
                .address = instruction.address,
                .bytes = {},
                .mnemonic = to_rust_string(instruction.mnemonic),
                .operands = to_rust_string(instruction.op_str),
                .call = is_call(instruction),
            };

            entry.bytes.reserve(instruction.size);
            for (const auto value : std::span(instruction.bytes, instruction.size))
            {
                entry.bytes.push_back(value);
            }

            result.push_back(std::move(entry));
        }

        return result;
    }

    void LinuxEmulator::set_stdout_callback(std::unique_ptr<CallbackHandle> callback) const
    {
        this->slots_.stdout_callback = std::move(callback);
    }

    void LinuxEmulator::set_stderr_callback(std::unique_ptr<CallbackHandle> callback) const
    {
        this->slots_.stderr_callback = std::move(callback);
    }

    void LinuxEmulator::set_syscall_callback(std::unique_ptr<CallbackHandle> callback) const
    {
        this->slots_.syscall = std::move(callback);
    }

    void LinuxEmulator::set_signal_callback(std::unique_ptr<CallbackHandle> callback) const
    {
        this->slots_.signal = std::move(callback);
    }

    void LinuxEmulator::set_memory_allocate_callback(std::unique_ptr<CallbackHandle> callback) const
    {
        this->slots_.memory_allocate = std::move(callback);
    }

    void LinuxEmulator::set_memory_protect_callback(std::unique_ptr<CallbackHandle> callback) const
    {
        this->slots_.memory_protect = std::move(callback);
    }

    void LinuxEmulator::set_memory_release_callback(std::unique_ptr<CallbackHandle> callback) const
    {
        this->slots_.memory_release = std::move(callback);
    }

    void LinuxEmulator::set_module_load_callback(std::unique_ptr<CallbackHandle> callback) const
    {
        this->slots_.module_load = std::move(callback);

        const auto cb = this->slots_.module_load;
        if (!cb)
        {
            return;
        }

        std::vector<uint64_t> bases{};
        for (const auto& [base, module] : this->emulator_->mod_manager.get_modules())
        {
            bases.push_back(base);
        }

        for (const auto base : bases)
        {
            (*cb)(base);
        }
    }

    void LinuxEmulator::set_module_observer(std::unique_ptr<CallbackHandle> callback) const
    {
        this->slots_.module_observer = std::move(callback);
    }

    void LinuxEmulator::set_thread_create_callback(std::unique_ptr<CallbackHandle> callback) const
    {
        this->slots_.thread_create = std::move(callback);
    }

    void LinuxEmulator::set_thread_terminated_callback(std::unique_ptr<CallbackHandle> callback) const
    {
        this->slots_.thread_terminated = std::move(callback);
    }

    void LinuxEmulator::set_thread_switch_callback(std::unique_ptr<CallbackHandle> callback) const
    {
        this->slots_.thread_switch = std::move(callback);
    }

    // Any registered observer replaces the default SIGSEGV delivery, so the observer only exists while a callback is set.
    void LinuxEmulator::set_memory_violation_callback(std::unique_ptr<CallbackHandle> callback) const
    {
        auto& slots = this->slots_;
        slots.memory_violation = std::move(callback);

        if (!slots.memory_violation)
        {
            if (slots.memory_violation_observer != 0)
            {
                this->emulator_->remove_memory_violation_observer(slots.memory_violation_observer);
                slots.memory_violation_observer = 0;
            }

            return;
        }

        if (slots.memory_violation_observer != 0)
        {
            return;
        }

        slots.memory_violation_observer = this->emulator_->add_memory_violation_observer(
            [&slots](const uint64_t address, const size_t size, const memory_operation operation, const memory_violation_type type) {
                const auto cb = slots.memory_violation;
                if (!cb)
                {
                    return memory_violation_continuation::resume;
                }

                return static_cast<memory_violation_continuation>(
                    (*cb)(address, size, static_cast<uint64_t>(operation), static_cast<uint64_t>(type)));
            });
    }

    void LinuxEmulator::set_periodic_event_callback(std::unique_ptr<CallbackHandle> callback) const
    {
        this->slots_.periodic_event = std::move(callback);
    }

    void LinuxEmulator::set_log_callback(std::unique_ptr<CallbackHandle> callback) const
    {
        this->slots_.log = std::move(callback);
    }

    uint64_t LinuxEmulator::hook_interrupt(std::unique_ptr<CallbackHandle> callback) const
    {
        auto cb = share_callback(std::move(callback));
        auto& emulator = *this->emulator_;
        const auto id = emulator.add_interrupt_observer([cb](const int interrupt) { (*cb)(signed_argument(interrupt)); });
        return this->machine_.track_removal([&emulator, id] { emulator.remove_interrupt_observer(id); });
    }

    uint64_t LinuxEmulator::hook_memory_violation(std::unique_ptr<CallbackHandle> callback) const
    {
        auto cb = share_callback(std::move(callback));
        auto& emulator = *this->emulator_;
        const auto id = emulator.add_memory_violation_observer(
            [cb](const uint64_t address, const size_t size, const memory_operation operation, const memory_violation_type type) {
                return static_cast<memory_violation_continuation>(
                    (*cb)(address, size, static_cast<uint64_t>(operation), static_cast<uint64_t>(type)));
            });
        return this->machine_.track_removal([&emulator, id] { emulator.remove_memory_violation_observer(id); });
    }

    std::unique_ptr<LinuxEmulator> create_linux_emulator(const LinuxSettings& settings)
    {
        auto backend = settings.has_backend ? create_x86_64_emulator(settings.backend) : create_x86_64_emulator_from_environment();
        auto native = std::make_unique<linux_emulator>(std::move(backend), to_path(settings.emulation_root));
        native->log.disable_output(!settings.logging);

        for (const auto& mapping : settings.path_mappings)
        {
            native->file_sys.add_path_mapping(to_path(mapping.guest), to_path(mapping.host), mapping.read_only);
        }

        for (const auto& mapping : settings.port_mappings)
        {
            native->port_mapper.map_port(mapping.emulator, mapping.host);
        }

        for (const auto& path : settings.library_paths)
        {
            native->mod_manager.add_library_path(to_path(path));
        }

        auto& personality = *native;
        auto emulator = std::make_unique<LinuxEmulator>(std::move(native));

        if (settings.has_application)
        {
            personality.process.current_working_directory =
                linux_file_system::normalize_guest_path_string(std::string(settings.working_directory));

            std::vector<std::string> argv{};
            argv.reserve(settings.arguments.size() + 1);
            argv.emplace_back(settings.application);
            for (const auto& argument : settings.arguments)
            {
                argv.emplace_back(argument);
            }

            std::vector<std::string> envp{};
            envp.reserve(settings.environment.size());
            for (const auto& variable : settings.environment)
            {
                envp.emplace_back(variable);
            }

            personality.load_application(to_path(settings.application), std::move(argv), envp);
        }

        return emulator;
    }
}
